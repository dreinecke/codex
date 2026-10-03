//! Herdr agent reporting (fork-owned).
//!
//! Panes managed by Herdr inherit `HERDR_ENV=1`, `HERDR_PANE_ID`, `HERDR_BIN_PATH`, and
//! `HERDR_SOCKET_PATH`. When those are present, Hushdex self-reports as an agent so pane-level
//! features work: sidebar state, `herdr agent get/prompt/wait`, notifications, and session
//! restore after a Herdr restart. Outside Herdr every call here is a no-op.
//!
//! Reports go through the Herdr CLI in the background with a short timeout and failures are
//! ignored, so reporting never blocks or breaks the TUI. State changes coalesce to the latest
//! observation while a report is in flight. The report sequence number is a wall-clock
//! nanosecond count, which keeps increasing across sessions and restarts so late reports from
//! a previous process cannot overwrite newer state.

use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// Herdr drops reports whose sequence number is not higher than the last accepted one.
const REPORT_TIMEOUT: Duration = Duration::from_secs(/*secs*/ 2);
const POLL_INTERVAL: Duration = Duration::from_millis(/*millis*/ 25);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HerdrState {
    Working,
    Idle,
    Blocked,
}

impl HerdrState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Idle => "idle",
            Self::Blocked => "blocked",
        }
    }
}

#[derive(Debug)]
enum Job {
    Report {
        state: HerdrState,
        message: Option<String>,
        session_id: Option<String>,
    },
    Release,
}

#[derive(Default)]
struct Shared {
    pending: Option<Job>,
    /// The last state/session pair already sent (or queued) to Herdr.
    sent: Option<(HerdrState, Option<String>)>,
    shutdown: bool,
}

pub(crate) struct HerdrAgent {
    pane_id: String,
    herdr_bin: PathBuf,
    shared: Arc<(Mutex<Shared>, Condvar)>,
}

impl HerdrAgent {
    /// Reads the Herdr pane environment through `lookup` so tests can inject values without
    /// mutating process environment.
    pub(crate) fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Option<Self> {
        if lookup("HERDR_ENV").as_deref() != Some("1") {
            return None;
        }
        let pane_id = lookup("HERDR_PANE_ID").filter(|id| !id.is_empty())?;
        let herdr_bin = PathBuf::from(lookup("HERDR_BIN_PATH").filter(|p| !p.is_empty())?);
        let agent = Self {
            pane_id,
            herdr_bin,
            shared: Arc::new((Mutex::new(Shared::default()), Condvar::new())),
        };
        agent.spawn_worker();
        Some(agent)
    }
}

static HERDR: OnceLock<Option<HerdrAgent>> = OnceLock::new();

fn agent() -> Option<&'static HerdrAgent> {
    #[cfg(test)]
    // Test binaries must never report to a real Herdr server: panes inherit the
    // environment, so tests running inside Herdr would otherwise hijack pane state.
    {
        None
    }
    #[cfg(not(test))]
    {
        HERDR
            .get_or_init(|| HerdrAgent::from_lookup(|key| std::env::var(key).ok()))
            .as_ref()
    }
}

/// Derives the reportable state: a pending decision outranks a running turn, which outranks
/// readiness. Reports only when the derived state or session changes.
pub(crate) fn note_observed(
    running: bool,
    blocked: bool,
    message: Option<&str>,
    session_id: Option<&str>,
) {
    let Some(agent) = agent() else {
        return;
    };
    let state = if blocked {
        HerdrState::Blocked
    } else if running {
        HerdrState::Working
    } else {
        HerdrState::Idle
    };
    let session_id = session_id.map(str::to_string);
    let message = message.map(str::to_string);
    let (lock, signal) = &*agent.shared;
    let mut shared = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if shared.sent.as_ref() == Some(&(state, session_id.clone()))
        && !(state == HerdrState::Blocked && message.is_some())
    {
        return;
    }
    shared.sent = Some((state, session_id.clone()));
    shared.pending = Some(Job::Report {
        state,
        message,
        session_id,
    });
    signal.notify_all();
}

/// Flushes the latest state, releases the pane, and stops reporting. Call once when the
/// process is actually exiting, not for in-process session switches.
pub(crate) fn release() {
    let Some(agent) = HERDR.get().and_then(Option::as_ref) else {
        return;
    };
    let (lock, signal) = &*agent.shared;
    let mut shared = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if shared.shutdown {
        return;
    }
    shared.shutdown = true;
    if shared.pending.is_none() {
        shared.pending = Some(Job::Release);
    }
    signal.notify_all();
}

impl HerdrAgent {
    fn spawn_worker(&self) {
        let pane_id = self.pane_id.clone();
        let herdr_bin = self.herdr_bin.clone();
        let shared = Arc::clone(&self.shared);
        std::thread::Builder::new()
            .name("hushdex-herdr".to_string())
            .spawn(move || worker(pane_id, herdr_bin, shared))
            .ok();
    }
}

fn worker(pane_id: String, herdr_bin: PathBuf, shared: Arc<(Mutex<Shared>, Condvar)>) {
    let (lock, signal) = &*shared;
    loop {
        let mut guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while guard.pending.is_none() {
            if guard.shutdown {
                return;
            }
            guard = signal
                .wait(guard)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        let job = guard.pending.take();
        drop(guard);
        match job {
            Some(Job::Report {
                state,
                message,
                session_id,
            }) => report_state(&pane_id, &herdr_bin, state, message.as_deref(), session_id),
            Some(Job::Release) => {
                report_release(&pane_id, &herdr_bin);
                return;
            }
            None => {}
        }
    }
}

fn next_seq() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_nanos())
        .unwrap_or_default()
}

/// Herdr rejects resume commands containing apostrophes or control characters; session ids
/// are UUID-shaped, so anything else simply skips the resume argv instead of failing.
fn resume_argv_safe(session_id: &str) -> bool {
    !session_id.contains('\'')
        && !session_id.chars().any(char::is_control)
        && session_id.len() <= 256
}

fn report_state(
    pane_id: &str,
    herdr_bin: &PathBuf,
    state: HerdrState,
    message: Option<&str>,
    session_id: Option<String>,
) {
    let mut command = Command::new(herdr_bin);
    command
        .arg("pane")
        .arg("report-agent")
        .arg(pane_id)
        .arg("--source")
        .arg("hushdex")
        .arg("--agent")
        .arg("hushdex")
        .arg("--state")
        .arg(state.as_str())
        .arg("--seq")
        .arg(next_seq().to_string());
    if let Some(message) = message {
        command.arg("--message").arg(message);
    }
    if let Some(session_id) = session_id.as_deref() {
        command.arg("--agent-session-id").arg(session_id);
        if resume_argv_safe(session_id) {
            command
                .arg("--")
                .arg("hushdex")
                .arg("resume")
                .arg("--yolo")
                .arg(session_id);
        }
    }
    run_with_timeout(command);
}

fn report_release(pane_id: &str, herdr_bin: &PathBuf) {
    let mut command = Command::new(herdr_bin);
    command
        .arg("pane")
        .arg("release-agent")
        .arg(pane_id)
        .arg("--source")
        .arg("hushdex")
        .arg("--agent")
        .arg("hushdex")
        .arg("--seq")
        .arg(next_seq().to_string());
    run_with_timeout(command);
}

fn run_with_timeout(mut command: Command) {
    let Ok(mut child) = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return;
    };
    let deadline = Instant::now() + REPORT_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) | Err(_) => return,
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return;
            }
            Ok(None) => std::thread::sleep(POLL_INTERVAL),
        }
    }
}

#[cfg(test)]
#[path = "herdr_tests.rs"]
mod tests;
