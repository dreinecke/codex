//! Hushdex focus mode in the owned (fullscreen) transcript.
//!
//! The owned transcript is the default TUI surface, so focus must condense committed cells here
//! exactly as it condenses terminal scrollback: prompts and agent text render in full, tool
//! activity renders one-line summaries, reasoning is omitted, and failures stay in full. The
//! live tail and the Ctrl+T overlay are not covered here.

use super::*;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::new_active_exec_command;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::ReasoningSummaryCell;
use crate::history_cell::new_user_prompt;
use codex_app_server_protocol::CommandExecutionSource;
use codex_protocol::parse_command::ParsedCommand;
use pretty_assertions::assert_eq;
use ratatui::layout::Rect;
use std::path::Path;
use std::sync::Arc;

const WIDTH: u16 = 80;

fn view_with(focus: bool) -> TranscriptView {
    let mut view = TranscriptView {
        focus,
        ..TranscriptView::default()
    };
    view.area = Rect::new(/*x*/ 0, /*y*/ 0, WIDTH, /*height*/ 24);
    view
}

fn fixture_cells() -> Vec<Arc<dyn HistoryCell>> {
    let mut succeeded = new_active_exec_command(
        "exec-ok".to_string(),
        vec!["bash".into(), "-lc".into(), "cargo build --quiet".into()],
        vec![ParsedCommand::Unknown {
            cmd: "cargo build --quiet".to_string(),
        }],
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    );
    succeeded
        .complete_call(
            "exec-ok",
            CommandOutput::new(
                /*exit_code*/ 0,
                "   Compiling hushdex\n    Finished `dev`".to_string(),
            ),
            std::time::Duration::from_millis(1_000),
        )
        .then_some(())
        .expect("complete exec-ok");

    vec![
        Arc::new(new_user_prompt(
            "Summarize the build setup.".to_string(),
            Vec::new(),
            Vec::new(),
            Vec::new(),
        )),
        Arc::new(ReasoningSummaryCell::new(
            "header".to_string(),
            "reasoning that must not appear in the focused transcript".to_string(),
            Path::new("/repo"),
            /*transcript_only*/ false,
        )),
        Arc::new(AgentMessageCell::new(
            vec![ratatui::text::Line::from("Inspecting the directory first.")],
            /*is_first_line*/ true,
        )),
        Arc::new(succeeded),
    ]
}

fn rendered_text(view: &mut TranscriptView, cells: &[Arc<dyn HistoryCell>]) -> String {
    let mut texts = Vec::new();
    for index in 0..cells.len() {
        let layout = view
            .current_layout(cells, index)
            .expect("layout for fixture cell");
        texts.push(layout.text().to_string());
    }
    texts.join("\n")
}

#[test]
fn owned_transcript_condenses_committed_cells_under_focus() {
    let cells = fixture_cells();
    let text = rendered_text(&mut view_with(/*focus*/ true), &cells);

    assert!(
        text.contains("Summarize the build setup."),
        "user prompt renders in full:\n{text}"
    );
    assert!(
        text.contains("Inspecting the directory first."),
        "agent message renders in full:\n{text}"
    );
    assert!(
        text.contains("Ran cargo build --quiet"),
        "tool activity renders a one-line summary:\n{text}"
    );
    assert!(
        !text.contains("Compiling hushdex"),
        "successful command output must not leak into the focused transcript:\n{text}"
    );
    assert!(
        !text.contains("reasoning that must not appear"),
        "reasoning must stay hidden in the focused transcript:\n{text}"
    );
    assert!(
        !text.contains("Show details"),
        "disclosure affordances are suppressed for condensed cells:\n{text}"
    );
}

#[test]
fn owned_transcript_keeps_full_presentation_without_focus() {
    let cells = fixture_cells();
    let text = rendered_text(&mut view_with(/*focus*/ false), &cells);

    assert!(
        text.contains("Compiling hushdex"),
        "command output preview returns without focus:\n{text}"
    );
    assert!(
        text.contains("reasoning that must not appear"),
        "displayable reasoning returns without focus:\n{text}"
    );
}

#[test]
fn hidden_cell_produces_no_rows_or_separator_under_focus() {
    let reasoning: Arc<dyn HistoryCell> = Arc::new(ReasoningSummaryCell::new(
        "header".to_string(),
        "hidden body".to_string(),
        Path::new("/repo"),
        /*transcript_only*/ false,
    ));
    let cells = vec![reasoning];
    let mut view = view_with(/*focus*/ true);
    let layout = view
        .current_layout(&cells, /*index*/ 0)
        .expect("layout for reasoning cell");
    assert_eq!(layout.row_count(), 0, "hidden cells render no rows");
}
