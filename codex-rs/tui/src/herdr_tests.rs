use super::*;

use pretty_assertions::assert_eq;

fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let vars: Vec<(String, String)> = vars
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    move |key| {
        vars.iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| value.clone())
    }
}

#[test]
fn agent_requires_the_herdr_pane_environment() {
    assert!(HerdrAgent::from_lookup(lookup(&[])).is_none());
    assert!(HerdrAgent::from_lookup(lookup(&[("HERDR_ENV", "1")])).is_none());
    assert!(
        HerdrAgent::from_lookup(lookup(&[
            ("HERDR_ENV", "1"),
            ("HERDR_PANE_ID", ""),
            ("HERDR_BIN_PATH", "/bin/herdr"),
        ]))
        .is_none()
    );
    assert!(
        HerdrAgent::from_lookup(lookup(&[
            ("HERDR_ENV", "0"),
            ("HERDR_PANE_ID", "pane-7"),
            ("HERDR_BIN_PATH", "/bin/herdr"),
        ]))
        .is_none()
    );
    assert!(
        HerdrAgent::from_lookup(lookup(&[
            ("HERDR_ENV", "1"),
            ("HERDR_PANE_ID", "pane-7"),
            ("HERDR_BIN_PATH", "/bin/herdr"),
        ]))
        .is_some()
    );
}

#[test]
fn derived_state_ranks_blocked_over_working_over_idle() {
    let cases = [
        (true, true, HerdrState::Blocked),
        (false, true, HerdrState::Blocked),
        (true, false, HerdrState::Working),
        (false, false, HerdrState::Idle),
    ];
    for (running, blocked, expected) in cases {
        let state = if blocked {
            HerdrState::Blocked
        } else if running {
            HerdrState::Working
        } else {
            HerdrState::Idle
        };
        assert_eq!(state, expected, "running={running} blocked={blocked}");
    }
}

#[test]
fn state_strings_match_the_herdr_protocol() {
    assert_eq!(HerdrState::Working.as_str(), "working");
    assert_eq!(HerdrState::Idle.as_str(), "idle");
    assert_eq!(HerdrState::Blocked.as_str(), "blocked");
}

#[test]
fn resume_argv_only_accepts_safe_session_ids() {
    assert!(resume_argv_safe("9f0e6d52-1111-4bbb-8ccc-2ddd0000eeee"));
    assert!(!resume_argv_safe("it's-not"));
    assert!(!resume_argv_safe("bad\u{1b}id"));
    assert!(!resume_argv_safe(&"x".repeat(257)));
}

#[cfg(unix)]
#[test]
fn reports_reach_the_herdr_cli_with_protocol_arguments() {
    use std::io::Write;
    let dir = tempfile::tempdir().expect("tempdir");
    let log = dir.path().join("reports.log");
    let bin = dir.path().join("herdr");
    let mut script = std::fs::File::create(&bin).expect("create script");
    write!(
        script,
        "#!/bin/sh\nprintf '%s ' \"$@\" >> {log}\nprintf '\\n' >> {log}\n",
        log = log.display()
    )
    .expect("write script");
    drop(script);
    std::fs::set_permissions(&bin, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("chmod");

    let agent = HerdrAgent::from_lookup(lookup(&[
        ("HERDR_ENV", "1"),
        ("HERDR_PANE_ID", "pane-42"),
        ("HERDR_BIN_PATH", bin.to_str().expect("utf8 path")),
    ]))
    .expect("agent");
    report_state(
        &agent.pane_id,
        &agent.herdr_bin,
        HerdrState::Blocked,
        Some("Approval needed"),
        Some("9f0e6d52-1111-4bbb-8ccc-2ddd0000eeee".to_string()),
    );
    report_state(
        &agent.pane_id,
        &agent.herdr_bin,
        HerdrState::Idle,
        None,
        None,
    );
    report_release(&agent.pane_id, &agent.herdr_bin);

    let logged = std::fs::read_to_string(&log).expect("read log");
    let lines: Vec<&str> = logged.lines().collect();
    assert_eq!(lines.len(), 3, "one argument list per report: {logged:?}");
    let blocked = lines[0];
    assert!(
        blocked.starts_with(
            "pane report-agent pane-42 --source hushdex --agent hushdex --state blocked"
        )
    );
    assert!(blocked.contains("--message Approval needed"));
    assert!(
        blocked.contains(
            "--agent-session-id 9f0e6d52-1111-4bbb-8ccc-2ddd0000eeee -- \
             hushdex resume --yolo 9f0e6d52-1111-4bbb-8ccc-2ddd0000eeee"
        ),
        "blocked report carries the resume argv: {blocked}"
    );
    assert!(lines[1].contains("--state idle"));
    assert!(!lines[1].contains("-- hushdex resume"));
    assert_eq!(
        lines[2].split(" --seq ").next(),
        Some("pane release-agent pane-42 --source hushdex --agent hushdex")
    );
    let first = seq_from(lines[0]).parse::<u128>();
    let last = seq_from(lines[2]).parse::<u128>();
    assert!(
        first.is_ok() && last.is_ok() && last.unwrap() >= first.unwrap(),
        "sequence numbers strictly increase: {logged:?}"
    );
}

#[cfg(unix)]
fn seq_from(line: &str) -> String {
    line.split(" --seq ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .unwrap_or_default()
        .to_string()
}
