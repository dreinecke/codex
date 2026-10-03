//! Herdr pane reporting from the real binary (fork-owned).
//!
//! Launches the TUI in a pseudo-terminal with a fake Herdr CLI on `HERDR_BIN_PATH` and
//! verifies the pane-agent protocol end to end: state reports with the resume argv while
//! running, a release on exit, and silence when the Herdr environment is absent.

use super::focus_palette::PtyCodex;
use super::focus_palette::write_test_config;
use anyhow::Result;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;
use std::time::Instant;

struct FakeHerdr {
    #[allow(dead_code)]
    dir: tempfile::TempDir,
    bin: PathBuf,
    log: PathBuf,
}

impl FakeHerdr {
    fn install() -> Result<Self> {
        let dir = tempfile::tempdir()?;
        let log = dir.path().join("reports.log");
        let bin = dir.path().join("herdr");
        let mut script = std::fs::File::create(&bin)?;
        writeln!(
            script,
            "#!/bin/sh\nprintf '%s ' \"$@\" >> {log}\nprintf '\\n' >> {log}\n",
            log = log.display()
        )?;
        drop(script);
        std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755))?;
        Ok(Self { dir, bin, log })
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("HERDR_ENV", "1".to_string()),
            ("HERDR_PANE_ID", "pty:p1".to_string()),
            ("HERDR_BIN_PATH", self.bin.display().to_string()),
        ]
    }

    fn reports(&self) -> Vec<String> {
        std::fs::read_to_string(&self.log)
            .map(|log| log.lines().map(str::to_string).collect())
            .unwrap_or_default()
    }

    fn wait_for(&self, needle: &str, timeout: Duration) -> Option<String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(found) = self.reports().into_iter().find(|r| r.contains(needle)) {
                return Some(found);
            }
            std::thread::sleep(Duration::from_millis(/*millis*/ 100));
        }
        None
    }
}

#[test]
fn herdr_pane_receives_state_and_release_reports() -> Result<()> {
    let repo_root = codex_utils_cargo_bin::repo_root()?;
    let home = tempfile::tempdir()?;
    write_test_config(home.path(), &repo_root)?;
    let herdr = FakeHerdr::install()?;
    let codex = std::env::var("HERDR_TEST_RELEASE_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            codex_utils_cargo_bin::cargo_bin("codex-tui")
                .or_else(|_| codex_utils_cargo_bin::cargo_bin("codex"))
                .expect("codex binary")
        });

    let env = herdr.env();
    let env_refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
    let mut terminal = PtyCodex::start_binary_with_env(
        &codex,
        &repo_root,
        home,
        /*extra_args*/ &[],
        /*editor*/ None,
        &env_refs,
    )?;
    terminal.wait_for_startup()?;

    let state = herdr
        .wait_for("report-agent pty:p1", Duration::from_secs(/*secs*/ 10))
        .unwrap_or_else(|| {
            panic!(
                "a state report reaches the Herdr CLI; screen:\n{}",
                terminal.screen_contents()
            )
        });
    assert!(
        state.contains("--source hushdex --agent hushdex"),
        "{state}"
    );
    assert!(
        state.contains("--state idle") || state.contains("--state working"),
        "the first report carries an observed state: {state}"
    );
    // The first report now arrives during the startup splash; wait for the composer
    // before quitting so the exit key reaches the running app.
    terminal.wait_for_screen("Ask Codex to do anything")?;

    terminal.write_input(b"\x04")?;
    std::thread::sleep(Duration::from_secs(/*secs*/ 3));
    let status = terminal.exit_status()?;
    let release = herdr.wait_for("release-agent pty:p1", Duration::from_secs(/*secs*/ 10));
    assert!(
        release.is_some(),
        "the pane is released on exit; status={status:?}; log: {:?}; screen:\n{}",
        herdr.reports(),
        terminal.screen_contents()
    );
    Ok(())
}

#[test]
fn herdr_environment_absent_means_no_reports() -> Result<()> {
    let repo_root = codex_utils_cargo_bin::repo_root()?;
    let home = tempfile::tempdir()?;
    write_test_config(home.path(), &repo_root)?;
    let herdr = FakeHerdr::install()?;
    let codex = codex_utils_cargo_bin::cargo_bin("codex-tui")
        .or_else(|_| codex_utils_cargo_bin::cargo_bin("codex"))?;

    let mut terminal = PtyCodex::start_binary_with_env(
        &codex,
        &repo_root,
        home,
        /*extra_args*/ &[],
        /*editor*/ None,
        /*extra_env*/ &[],
    )?;
    terminal.wait_for_startup()?;
    std::thread::sleep(Duration::from_millis(/*millis*/ 500));
    assert!(
        herdr.reports().is_empty(),
        "no reports without the Herdr pane environment"
    );
    terminal.write_input(b"\x04")?;
    std::thread::sleep(Duration::from_millis(/*millis*/ 300));
    assert!(
        !herdr.reports().iter().any(|r| r.contains("release-agent")),
        "no release without the Herdr pane environment"
    );
    Ok(())
}
