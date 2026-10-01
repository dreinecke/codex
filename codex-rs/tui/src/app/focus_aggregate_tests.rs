//! Tests for focus-mode run aggregation.

use super::*;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use crate::focus_summaries::exec_activity_counts;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::McpInvocation;
use crate::history_cell::ReasoningSummaryCell;
use crate::history_cell::new_active_mcp_tool_call;
use crate::history_cell::new_patch_event;
use crate::history_cell::new_user_prompt;
use codex_app_server_protocol::CommandExecutionSource;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::parse_command::ParsedCommand;
use pretty_assertions::assert_eq;
use ratatui::text::Line;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

fn exec_cell(call_id: &str, command: &str, parsed: Vec<ParsedCommand>, exit_code: i32) -> ExecCell {
    let mut cell = new_active_exec_command(
        call_id.to_string(),
        vec!["bash".into(), "-lc".into(), command.into()],
        parsed,
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    cell.complete_call(
        call_id,
        CommandOutput::new(exit_code, String::new()),
        Duration::from_millis(10),
    )
    .then_some(())
    .expect("complete call");
    cell
}

fn pending_text(tui: &tui::Tui) -> String {
    tui.pending_history_lines_for_test()
        .iter()
        .map(|line| line.line.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// The aggregate line currently owning the scrollback tail, if any.
fn tail_text(app: &App) -> String {
    app.last_rendered_history_tail
        .as_ref()
        .map(|tail| {
            tail.lines
                .iter()
                .map(|line| line.line.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn lines_to_string(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn aggregate_line_phrases_match_claude_code_style() {
    let counts = FocusActivityCounts {
        patterns: 1,
        reads: 2,
        edits: 0,
        images: 0,
        tools: 0,
        shells: 9,
    };
    assert_eq!(
        aggregate_line(&counts).to_string(),
        "Searched for 1 pattern, read 2 files, ran 9 shell commands"
    );

    let counts = FocusActivityCounts {
        patterns: 0,
        reads: 1,
        edits: 3,
        images: 2,
        tools: 2,
        shells: 1,
    };
    assert_eq!(
        aggregate_line(&counts).to_string(),
        "Read 1 file, edited 3 files, viewed 2 images, called 2 tools, ran 1 shell command"
    );

    let counts = FocusActivityCounts {
        tools: 1,
        ..FocusActivityCounts::default()
    };
    assert_eq!(aggregate_line(&counts).to_string(), "Called 1 tool");
}

#[test]
fn exec_counts_split_reads_searches_and_shell_commands() {
    let read = exec_cell(
        "read",
        "sed -n 1p a.rs",
        vec![ParsedCommand::Read {
            cmd: "sed".to_string(),
            name: "a.rs".to_string(),
            path: PathBuf::from("a.rs"),
        }],
        /*exit_code*/ 0,
    );
    assert_eq!(
        exec_activity_counts(&read),
        Some(FocusActivityCounts {
            reads: 1,
            ..FocusActivityCounts::default()
        })
    );

    let search = exec_cell(
        "search",
        "rg foo",
        vec![ParsedCommand::Search {
            cmd: "rg foo".to_string(),
            query: None,
            path: None,
        }],
        /*exit_code*/ 0,
    );
    assert_eq!(
        exec_activity_counts(&search),
        Some(FocusActivityCounts {
            patterns: 1,
            ..FocusActivityCounts::default()
        })
    );

    let shell = exec_cell(
        "shell",
        "cargo build",
        vec![ParsedCommand::Unknown {
            cmd: "cargo build".to_string(),
        }],
        /*exit_code*/ 0,
    );
    assert_eq!(
        exec_activity_counts(&shell),
        Some(FocusActivityCounts {
            shells: 1,
            ..FocusActivityCounts::default()
        })
    );

    assert_eq!(
        exec_activity_counts(&exec_cell(
            "fail",
            "cargo test",
            vec![ParsedCommand::Unknown {
                cmd: "cargo test".to_string(),
            }],
            /*exit_code*/ 2,
        )),
        None
    );

    // Exit 1 from a read-only command (rg with no matches at the end of a pipeline) is
    // benign and stays absorbable; from anything else it is a real failure.
    let benign = exec_cell(
        "benign",
        "cat a.json; rg -n x a.rb | tail -15; rg -n y b.erb",
        vec![ParsedCommand::Unknown {
            cmd: "cat a.json".to_string(),
        }],
        /*exit_code*/ 1,
    );
    assert_eq!(
        exec_activity_counts(&benign),
        Some(FocusActivityCounts {
            shells: 1,
            ..FocusActivityCounts::default()
        })
    );

    let real_failure = exec_cell(
        "real-failure",
        "pytest -q",
        vec![ParsedCommand::Unknown {
            cmd: "pytest -q".to_string(),
        }],
        /*exit_code*/ 1,
    );
    assert_eq!(exec_activity_counts(&real_failure), None);
}

#[test]
fn aggregate_renders_one_dim_line_for_a_run() {
    let cells: Vec<Arc<dyn HistoryCell>> = vec![
        Arc::new(exec_cell(
            "one",
            "cargo build",
            vec![ParsedCommand::Unknown {
                cmd: "cargo build".to_string(),
            }],
            /*exit_code*/ 0,
        )),
        Arc::new(exec_cell(
            "two",
            "cargo test",
            vec![ParsedCommand::Unknown {
                cmd: "cargo test".to_string(),
            }],
            /*exit_code*/ 0,
        )),
    ];
    let lines = FocusAggregateCell::new(cells.clone()).focus_lines(/*width*/ 80);
    assert_eq!(lines.len(), 1, "one line per run");
    assert_eq!(
        lines_to_string(&lines),
        "Ran 2 shell commands",
        "aggregate phrasing"
    );

    // Full presentation (focus off) expands everything the run absorbed.
    let aggregate = FocusAggregateCell::new(cells);
    assert!(
        !aggregate.display_lines(/*width*/ 80).is_empty(),
        "focus off expands the aggregate"
    );
    assert!(
        !aggregate.transcript_lines(/*width*/ 80).is_empty(),
        "pager keeps full transcript content"
    );
}

#[tokio::test]
async fn consecutive_tool_cells_collapse_into_one_scrollback_line() {
    let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");
    app.chat_widget.set_focus_mode(true);

    app.insert_history_cell(
        &mut tui,
        Box::new(exec_cell(
            "one",
            "cargo build",
            vec![ParsedCommand::Unknown {
                cmd: "cargo build".to_string(),
            }],
            /*exit_code*/ 0,
        )),
    );
    assert_eq!(pending_text(&tui), "Ran 1 shell command");

    app.insert_history_cell(
        &mut tui,
        Box::new(ReasoningSummaryCell::new(
            "h".to_string(),
            "thinking".to_string(),
            Path::new("/repo"),
            /*transcript_only*/ false,
        )),
    );
    app.insert_history_cell(
        &mut tui,
        Box::new(exec_cell(
            "two",
            "cargo test",
            vec![ParsedCommand::Unknown {
                cmd: "cargo test".to_string(),
            }],
            /*exit_code*/ 0,
        )),
    );

    // The rewrite goes straight to the terminal backend, so pending is flushed and the tail
    // bookkeeping now owns the merged line.
    assert_eq!(
        tail_text(&app),
        "Ran 2 shell commands",
        "aggregate counts both commands and rewrites in place"
    );
    assert!(
        !pending_text(&tui).contains("Ran 1 shell command"),
        "the earlier single-command line must not linger in pending output"
    );
    assert!(
        tail_text(&app).matches("thinking").count() == 0,
        "hidden cells between tools do not break the run or leak"
    );
    assert_eq!(
        app.transcript_cells
            .iter()
            .filter(|cell| cell.as_any().downcast_ref::<FocusAggregateCell>().is_some())
            .count(),
        1,
        "one aggregate cell in the transcript"
    );
}

#[tokio::test]
async fn agent_messages_and_failures_close_runs_and_stay_full() {
    let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");
    app.chat_widget.set_focus_mode(true);

    app.insert_history_cell(
        &mut tui,
        Box::new(exec_cell(
            "ok",
            "cargo build",
            vec![ParsedCommand::Unknown {
                cmd: "cargo build".to_string(),
            }],
            /*exit_code*/ 0,
        )),
    );
    app.insert_history_cell(
        &mut tui,
        Box::new(AgentMessageCell::new(
            vec![Line::from("Build finished.")],
            /*is_first_line*/ true,
        )),
    );
    app.insert_history_cell(
        &mut tui,
        Box::new(exec_cell(
            "fail",
            "cargo test",
            vec![ParsedCommand::Unknown {
                cmd: "cargo test".to_string(),
            }],
            /*exit_code*/ 2,
        )),
    );

    let text = pending_text(&tui);
    assert!(
        text.contains("Build finished."),
        "agent messages render in full and close runs:\n{text}"
    );
    assert!(
        !text.contains("Ran 2"),
        "a failed command after a message joins no counted run:\n{text}"
    );
    assert!(
        text.contains("cargo test"),
        "the failed command itself renders in full:\n{text}"
    );

    // A new successful command after the failure opens a fresh run.
    app.insert_history_cell(
        &mut tui,
        Box::new(exec_cell(
            "next",
            "ls",
            vec![ParsedCommand::ListFiles {
                cmd: "ls".to_string(),
                path: None,
            }],
            /*exit_code*/ 0,
        )),
    );
    assert_eq!(
        tail_text(&app),
        "Ran 1 shell command",
        "run after the failure aggregates separately"
    );
}

#[tokio::test]
async fn prompts_and_patches_aggregate_by_category() {
    let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");
    app.chat_widget.set_focus_mode(true);

    app.insert_history_cell(
        &mut tui,
        Box::new(new_user_prompt(
            "Ship it.".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )),
    );
    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("src/a.rs"),
        crate::diff_model::FileChange::Add {
            content: "x".to_string(),
        },
    );
    changes.insert(
        PathBuf::from("src/b.rs"),
        crate::diff_model::FileChange::Add {
            content: "y".to_string(),
        },
    );
    app.insert_history_cell(
        &mut tui,
        Box::new(new_patch_event(changes, Path::new("/repo"))),
    );
    let text = pending_text(&tui);
    assert!(text.contains("Ship it."), "prompts render in full:\n{text}");
    let mut mcp = new_active_mcp_tool_call(
        "mcp".to_string(),
        McpInvocation {
            server: "github".to_string(),
            tool: "search_code".to_string(),
            arguments: None,
        },
        /*animations_enabled*/ false,
    );
    mcp.complete(
        Duration::from_millis(5),
        Ok(CallToolResult {
            content: Vec::new(),
            structured_content: None,
            is_error: None,
            meta: None,
        }),
    );
    app.insert_history_cell(&mut tui, Box::new(mcp));

    assert_eq!(
        tail_text(&app),
        "Edited 2 files, called 1 tool",
        "patches and MCP calls aggregate by category"
    );
    let aggregate_cells: Vec<_> = app
        .transcript_cells
        .iter()
        .filter(|cell| cell.as_any().downcast_ref::<FocusAggregateCell>().is_some())
        .collect();
    assert_eq!(aggregate_cells.len(), 1, "one run between prompt and now");
    let aggregate_text = aggregate_cells
        .iter()
        .filter_map(|cell| cell.as_any().downcast_ref::<FocusAggregateCell>())
        .map(|cell| lines_to_string(&cell.focus_lines(/*width*/ 80)))
        .collect::<String>();
    assert_eq!(aggregate_text, "Edited 2 files, called 1 tool");
}

#[tokio::test]
async fn hidden_lifecycle_and_stream_fragments_do_not_break_runs() {
    let (mut app, _events, _ops) = crate::app::tests::make_test_app_with_channels().await;
    let mut tui = crate::tui::test_support::make_test_tui().expect("test tui");
    app.chat_widget.set_focus_mode(true);

    app.insert_history_cell(
        &mut tui,
        Box::new(exec_cell(
            "one",
            "cargo build",
            vec![ParsedCommand::Unknown {
                cmd: "cargo build".to_string(),
            }],
            /*exit_code*/ 0,
        )),
    );
    // Sub-agent lifecycle telemetry (Started/Interacted/Completed) is hidden in focus mode.
    app.insert_history_cell(
        &mut tui,
        Box::new(crate::focus::FocusHiddenHistoryCell(
            crate::history_cell::PlainHistoryCell::new(vec![Line::from("Started `/root/s279_a`")]),
        )),
    );
    // A stream-continuation fragment of an in-flight agent message also does not close a run.
    app.insert_history_cell(
        &mut tui,
        Box::new(AgentMessageCell::new(
            vec![Line::from("mid-stream fragment")],
            /*is_first_line*/ false,
        )),
    );
    app.insert_history_cell(
        &mut tui,
        Box::new(exec_cell(
            "two",
            "cargo test",
            vec![ParsedCommand::Unknown {
                cmd: "cargo test".to_string(),
            }],
            /*exit_code*/ 0,
        )),
    );

    assert_eq!(
        tail_text(&app),
        "Ran 2 shell commands",
        "hidden telemetry and stream fragments keep the run open"
    );
    let text = pending_text(&tui) + &tail_text(&app);
    assert!(
        !text.contains("Started"),
        "lifecycle telemetry stays hidden in focus mode:\n{text}"
    );
}

#[test]
fn hidden_lifecycle_wrapper_delegates_full_presentations() {
    let wrapper =
        crate::focus::FocusHiddenHistoryCell(crate::history_cell::PlainHistoryCell::new(vec![
            Line::from("Completed `/root/x`"),
        ]));
    assert!(wrapper.focus_lines(/*width*/ 80).is_empty());
    assert!(
        !wrapper.display_lines(/*width*/ 80).is_empty(),
        "focus off renders the event"
    );
    assert!(
        !wrapper.transcript_lines(/*width*/ 80).is_empty(),
        "the pager keeps the event"
    );
}
