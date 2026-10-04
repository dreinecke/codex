use super::*;
use codex_app_server_protocol::CommandExecutionSource;
use ratatui::style::Modifier;

fn spans_are_dimmed(line: &Line<'static>) -> bool {
    !line.spans.is_empty()
        && line
            .spans
            .iter()
            .all(|span| span.style.add_modifier.contains(Modifier::DIM))
}

#[test]
fn aggregate_lines_render_at_the_working_status_level() {
    let counts = FocusActivityCounts {
        reads: 3,
        shells: 1,
        ..FocusActivityCounts::default()
    };
    let line = aggregate_line(&counts);
    assert_eq!(line.to_string(), "Read 3 files, ran 1 shell command");
    assert!(
        spans_are_dimmed(&line),
        "aggregate spans carry the dim modifier: {line:?}"
    );
}

#[test]
fn exec_summary_lines_render_at_the_working_status_level() {
    let command = vec!["cargo".to_owned(), "build".to_owned()];
    let parsed = codex_shell_command::parse_command::parse_command(&command);
    let cell = crate::exec_cell::ExecCell::new(
        crate::exec_cell::ExecCall {
            call_id: "call-1".to_owned(),
            command,
            parsed,
            output: Some(crate::exec_cell::CommandOutput::new(
                /*exit_code*/ 0,
                String::new(),
            )),
            source: CommandExecutionSource::UnifiedExecStartup,
            start_time: None,
            duration: Some(std::time::Duration::from_millis(/*millis*/ 5)),
            interaction_input: None,
        },
        /*animations_enabled*/ false,
    );
    let lines = exec_focus_lines(&cell, /*width*/ 80);
    assert_eq!(lines.len(), 1);
    assert!(
        spans_are_dimmed(&lines[0]),
        "exec summary spans carry the dim modifier: {lines:?}"
    );
}

#[test]
fn label_lines_render_at_the_working_status_level() {
    let lines = label_line(
        "•".green().bold(),
        "Called",
        "github.search_code".to_string(),
        /*width*/ 80,
    );
    assert_eq!(lines.len(), 1);
    assert!(
        spans_are_dimmed(&lines[0]),
        "label spans carry the dim modifier: {lines:?}"
    );
    // Muting keeps the marker's hue; it only adds the dim modifier.
    assert_eq!(
        lines[0].spans[0].style.fg,
        Some(ratatui::style::Color::Green)
    );
}

#[test]
fn failed_multiline_commands_clamp_their_source_echo() {
    let command = vec![format!(
        "python - <<'PY'\nfrom pathlib import Path\nimport json\nprint(json.dumps({{}}))\nPY"
    )];
    let parsed = codex_shell_command::parse_command::parse_command(&command);
    let output_text = (1..=20)
        .map(|i| format!("phase=step{i} result=passed"))
        .chain(std::iter::once(
            "result=failed status=1 cleanup=complete log=/tmp/run.log".to_owned(),
        ))
        .collect::<Vec<_>>()
        .join("\n");
    let cell = crate::exec_cell::ExecCell::new(
        crate::exec_cell::ExecCall {
            call_id: "call-ml".to_owned(),
            command,
            parsed,
            output: Some(crate::exec_cell::CommandOutput::new(
                /*exit_code*/ 1,
                output_text,
            )),
            source: CommandExecutionSource::UnifiedExecStartup,
            start_time: None,
            duration: Some(std::time::Duration::from_millis(/*millis*/ 827)),
            interaction_input: None,
        },
        /*animations_enabled*/ false,
    );
    let lines = exec_focus_lines(&cell, /*width*/ 80);
    let rendered: Vec<String> = lines.iter().map(ToString::to_string).collect();
    let joined = rendered.join("\n");
    assert!(
        joined.contains("python - <<'PY'"),
        "the command's first line stays visible: {joined}"
    );
    assert!(
        joined.contains("⋯ +4 source lines"),
        "the heredoc body collapses to a count: {joined}"
    );
    assert!(
        !joined.contains("from pathlib"),
        "heredoc source never echoes: {joined}"
    );
    assert!(
        joined.contains("⋯ +15 output lines"),
        "long output clamps to its tail with a count: {joined}"
    );
    assert!(
        joined.contains("result=failed status=1"),
        "the trailing failure summary stays visible: {joined}"
    );
    assert!(
        !joined.contains("phase=step1 "),
        "early bookkeeping output is dropped: {joined}"
    );
    assert!(
        joined.contains("✗ (1)"),
        "the exit footer stays visible: {joined}"
    );
}
