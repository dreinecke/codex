//! One-line focus summaries for tool-activity history cells.
//!
//! Every builder here either returns a one-line-per-item condensed form or falls back to the
//! cell's full presentation when the activity failed: failures are never condensed, so errored,
//! denied, and interrupted work stays visible with its exit code and diagnostics. The transcript
//! pager never uses these; it always renders the full presentation.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use crate::diff_model::FileChange;
use crate::diff_render::calculate_add_remove_from_diff;
use crate::diff_render::display_path_for;
use crate::exec_cell::ExecCell;
use crate::exec_command::strip_bash_lc_and_escape;
use crate::history_cell::HistoryCell;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::render::highlight::highlight_bash_to_lines;
use codex_app_server_protocol::CommandExecutionSource;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

/// Maximum per-file lines before a patch summary collapses the remainder.
const PATCH_FILE_SUMMARY_LIMIT: usize = 5;

fn clipped(line: Line<'static>, width: u16) -> Line<'static> {
    truncate_line_with_ellipsis_if_overflow(line, usize::from(width.max(1)))
}

fn label_line(
    marker: Span<'static>,
    verb: &'static str,
    label: String,
    width: u16,
) -> Vec<Line<'static>> {
    vec![clipped(
        Line::from(vec![
            marker,
            " ".into(),
            verb.bold(),
            " ".into(),
            label.into(),
        ]),
        width,
    )]
}

/// `• Ran <command>` per call (or `• Running <command>` while a call is in flight).
///
/// Any failed call — exit code ≠ 0, which includes `mark_failed` interruption — renders the full
/// transcript form (`$ command`, complete output, `✗ (exit)` result) so the exit code stays
/// visible, and any user `!` shell command renders in full because its output is what the user
/// explicitly asked to run.
pub(crate) fn exec_focus_lines(cell: &ExecCell, width: u16) -> Vec<Line<'static>> {
    let failed = cell.iter_calls().any(|call| {
        call.output
            .as_ref()
            .is_some_and(|output| output.exit_code != 0)
    });
    let user_shell = cell
        .iter_calls()
        .any(|call| matches!(call.source, CommandExecutionSource::UserShell));
    if failed {
        return cell.transcript_lines(width);
    }
    if user_shell {
        return cell.display_lines(width);
    }
    cell.iter_calls()
        .map(|call| {
            let running = call.duration.is_none();
            let marker = if running {
                "•".dim()
            } else {
                "•".green().bold()
            };
            let verb = if running { "Running" } else { "Ran" };
            let script = strip_bash_lc_and_escape(&call.command);
            let mut command_lines = highlight_bash_to_lines(&script).into_iter();
            let mut command = command_lines.next().unwrap_or_default();
            if command_lines.next().is_some() {
                command.push_span(" …".dim());
            }
            let mut line = Line::from(vec![marker, " ".into(), verb.bold(), " ".into()]);
            line.extend(command.spans);
            clipped(line, width)
        })
        .collect()
}

/// `• Called <server>.<tool>` for a successful MCP call; running calls show `Calling`.
/// Failed calls (transport errors and `is_error` results) render in full.
pub(crate) fn mcp_call_focus_lines(
    success: Option<bool>,
    server: &str,
    tool: &str,
    width: u16,
    full: impl FnOnce() -> Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    let label = format!("{server}.{tool}");
    match success {
        Some(false) => full(),
        None => label_line("•".dim(), "Calling", label, width),
        Some(true) => label_line("•".green().bold(), "Called", label, width),
    }
}

/// `• Called <namespace>.<tool>` for dynamic tools; failed or interrupted calls render in full.
pub(crate) fn dynamic_call_focus_lines(
    running: bool,
    failed: bool,
    name: &str,
    width: u16,
    full: impl FnOnce() -> Vec<Line<'static>>,
) -> Vec<Line<'static>> {
    if failed {
        return full();
    }
    let label = name.to_string();
    if running {
        label_line("•".dim(), "Calling", label, width)
    } else {
        label_line("•".green().bold(), "Called", label, width)
    }
}

/// `• Used computer · N actions`; any failed action renders the full cell.
pub(crate) fn computer_activity_focus_lines(
    action_count: usize,
    active: bool,
    width: u16,
) -> Vec<Line<'static>> {
    let label = format!("{action_count} actions");
    if active {
        label_line("•".dim(), "Using computer", label, width)
    } else {
        label_line("•".green().bold(), "Used computer", label, width)
    }
}

/// Keep only the first line of the full presentation for already one-line tool cells.
pub(crate) fn first_display_line(lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    lines.into_iter().take(/*n*/ 1).collect()
}

/// `• Edited <path> (+A -D)` per file, with `Added`/`Deleted` for new or removed files.
/// Caps at five files, then `• …and N more files`.
pub(crate) fn patch_focus_lines(
    changes: &HashMap<PathBuf, FileChange>,
    cwd: &Path,
    width: u16,
) -> Vec<Line<'static>> {
    let mut rows: Vec<(&Path, &FileChange)> = changes
        .iter()
        .map(|(path, change)| (path.as_path(), change))
        .collect();
    rows.sort_by_key(|(path, _)| *path);

    let mut lines = Vec::new();
    for (index, (path, change)) in rows.iter().enumerate() {
        if index == PATCH_FILE_SUMMARY_LIMIT {
            let remaining = rows.len() - PATCH_FILE_SUMMARY_LIMIT;
            let noun = if remaining == 1 { "file" } else { "files" };
            lines.push(clipped(
                Line::from(vec![
                    "• ".dim(),
                    format!("…and {remaining} more {noun}").dim(),
                ]),
                width,
            ));
            return lines;
        }
        lines.push(patch_file_line(path, change, cwd, width));
    }
    lines
}

fn patch_file_line(path: &Path, change: &FileChange, cwd: &Path, width: u16) -> Line<'static> {
    let display_path = |path: &Path| -> Span<'static> { display_path_for(path, cwd).into() };
    match change {
        FileChange::Add { .. } => clipped(
            Line::from(vec![
                "• ".dim(),
                "Added".bold(),
                " ".into(),
                display_path(path),
            ]),
            width,
        ),
        FileChange::Delete { .. } => clipped(
            Line::from(vec![
                "• ".dim(),
                "Deleted".bold(),
                " ".into(),
                display_path(path),
            ]),
            width,
        ),
        FileChange::Update {
            unified_diff,
            move_path,
        } => {
            let (added, removed) = calculate_add_remove_from_diff(unified_diff);
            let mut spans = vec!["• ".dim(), "Edited".bold(), " ".into(), display_path(path)];
            if let Some(move_path) = move_path {
                spans.push(" → ".into());
                spans.push(display_path(move_path));
            }
            spans.push(" ".into());
            spans.push("(".into());
            spans.push(format!("+{added}").green());
            spans.push(" ".into());
            spans.push(format!("-{removed}").red());
            spans.push(")".into());
            clipped(Line::from(spans), width)
        }
    }
}
