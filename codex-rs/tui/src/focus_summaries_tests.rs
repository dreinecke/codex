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
        joined.contains("⋯ +20 output lines"),
        "long output clamps to its error-anchored tail with a count: {joined}"
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
fn mixed_cells_condense_successful_siblings_of_failures() {
    let cell = crate::exec_cell::ExecCell::new(
        exec_call(
            "cat-ok",
            "cat test/order_matrix/test_suite_candidates.py",
            /*exit_code*/ 0,
            (1..=90)
                .map(|i| format!("source line {i} of the file"))
                .collect::<Vec<_>>()
                .join("\n"),
        ),
        /*animations_enabled*/ false,
    );
    // Attach a failed sibling to the same group the way multi-call cells are built.
    let mut cell = cell;
    cell.group.calls.push(exec_call(
        "pytest-fail",
        "bin/isolated-full-test",
        /*exit_code*/ 1,
        "phase=preflight result=passed\nresult=failed status=1\n".to_owned(),
    ));
    let lines = exec_focus_lines(&cell, /*width*/ 80);
    let joined = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("Ran 'cat test/order_matrix/test_suite_candidates.py'"),
        "the successful sibling condenses to its one-liner: {joined}"
    );
    assert!(
        !joined.contains("source line 1 of the file"),
        "successful sibling output never renders: {joined}"
    );
    assert!(
        joined.contains("$ bin/isolated-full-test"),
        "the failed call keeps its command line: {joined}"
    );
    assert!(
        joined.contains("✗ (1)"),
        "the failed call keeps its exit footer: {joined}"
    );
}

#[test]
fn failed_chain_output_anchors_to_the_error_line() {
    let output = [
        "useful tail of the successful first command".to_owned(),
        "    print(rendered, end='')".to_owned(),
        String::new(),
        "if __name__ == '__main__':".to_owned(),
        "sed: can't read docs/testing/order-matrix/source-freeze.md: No such file or directory"
            .to_owned(),
    ]
    .join("\n");
    let cell = crate::exec_cell::ExecCell::new(
        exec_call(
            "chain-fail",
            "tail -n 65 test/order_matrix/pin_applications.py; sed -n '1,150p' docs/testing/order-matrix/source-freeze.md",
            /*exit_code*/ 2,
            output,
        ),
        /*animations_enabled*/ false,
    );
    let lines = exec_focus_lines(&cell, /*width*/ 100);
    let joined = lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("sed: can't read docs/testing/order-matrix/source-freeze.md"),
        "the error line stays visible: {joined}"
    );
    assert!(
        joined.contains("⋯ +4 output lines"),
        "successful sibling output is counted, not shown: {joined}"
    );
    assert!(
        !joined.contains("print(rendered"),
        "the successful command's output never renders: {joined}"
    );
    assert!(joined.contains("✗ (2)"), "the exit footer stays: {joined}");
}
