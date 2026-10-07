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
fn failed_calls_condense_to_one_muted_row_with_exit_code() {
    let heredoc = vec![format!(
        "python - <<'PY'\nfrom pathlib import Path\nimport json\nprint(json.dumps({{}}))\nPY"
    )];
    let parsed = codex_shell_command::parse_command::parse_command(&heredoc);
    let bookkeeping = (1..=20)
        .map(|i| format!("phase=step{i} result=passed"))
        .chain(std::iter::once(
            "sed: can't read docs/testing/source-freeze.md: No such file or directory".to_owned(),
        ))
        .collect::<Vec<_>>()
        .join("\n");
    let cell = crate::exec_cell::ExecCell::new(
        crate::exec_cell::ExecCall {
            call_id: "heredoc".to_owned(),
            command: heredoc,
            parsed,
            output: Some(crate::exec_cell::CommandOutput::new(
                /*exit_code*/ 1,
                bookkeeping,
            )),
            source: CommandExecutionSource::UnifiedExecStartup,
            start_time: None,
            duration: Some(std::time::Duration::from_millis(/*millis*/ 827)),
            interaction_input: None,
        },
        /*animations_enabled*/ false,
    );
    let lines = exec_focus_lines(&cell, /*width*/ 80);
    assert_eq!(lines.len(), 1, "a failed call costs exactly one row");
    let text = lines[0].to_string();
    assert!(
        text.contains("python - <<'PY'"),
        "the command's first line stays visible: {text}"
    );
    assert!(
        text.contains("(exit 1)"),
        "the exit code rides the row: {text}"
    );
    assert!(
        !text.contains("from pathlib"),
        "heredoc source never renders: {text}"
    );
    assert!(
        !text.contains("phase=step"),
        "output never renders in focus mode: {text}"
    );
    assert!(
        lines
            .iter()
            .all(|line| crate::line_truncation::line_width(line) <= 80),
        "the row fits the viewport width"
    );
    assert!(
        lines[0].spans.iter().all(|span| span
            .style
            .add_modifier
            .contains(ratatui::style::Modifier::DIM)),
        "failure rows render at the working-status level"
    );
}

fn exec_call(
    call_id: &str,
    command: &str,
    exit_code: i32,
    output: String,
) -> crate::exec_cell::ExecCall {
    let command = vec![command.to_owned()];
    let parsed = codex_shell_command::parse_command::parse_command(&command);
    crate::exec_cell::ExecCall {
        call_id: call_id.to_owned(),
        command,
        parsed,
        output: Some(crate::exec_cell::CommandOutput::new(exit_code, output)),
        source: CommandExecutionSource::UnifiedExecStartup,
        start_time: None,
        duration: Some(std::time::Duration::from_millis(/*millis*/ 5)),
        interaction_input: None,
    }
}

#[test]
fn mixed_cells_render_one_row_per_call() {
    let cell = crate::exec_cell::ExecCell::new(
        exec_call(
            "cat-ok",
            "cat test/order_matrix/test_suite_candidates.py",
            /*exit_code*/ 0,
            "file body".to_owned(),
        ),
        /*animations_enabled*/ false,
    );
    let mut cell = cell;
    cell.group.calls.push(exec_call(
        "pytest-fail",
        "bin/isolated-full-test",
        /*exit_code*/ 1,
        "result=failed status=1".to_owned(),
    ));
    let lines = exec_focus_lines(&cell, /*width*/ 100);
    assert_eq!(lines.len(), 2, "one row per call in a mixed cell");
    let joined = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("Ran 'cat test/order_matrix/test_suite_candidates.py'"),
        "the successful call keeps its one-liner: {joined}"
    );
    assert!(
        joined.contains("✗") && joined.contains("bin/isolated-full-test"),
        "the failed call keeps its one-liner with the failure marker: {joined}"
    );
    assert!(
        !joined.contains("result=failed"),
        "no output renders for either call: {joined}"
    );
}
