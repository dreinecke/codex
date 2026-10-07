//! One-line focus summaries for tool-activity history cells.
//!
//! Every builder here either returns a one-line-per-item condensed form or falls back to the
//! cell's full presentation when the activity failed: failures are never condensed, so errored,
//! denied, and interrupted work stays visible with its exit code and diagnostics. The transcript
//! pager never uses these; it always renders the full presentation.
//!
//! `FocusActivityCounts` and `aggregate_line` power run aggregation: consecutive absorbable tool
//! cells merge into one dim line ("Searched for 2 patterns, ran 9 shell commands").

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use crate::diff_model::FileChange;
use crate::diff_render::calculate_add_remove_from_diff;
use crate::diff_render::display_path_for;
use crate::exec_cell::ExecCall;
use crate::exec_cell::ExecCell;
use crate::exec_command::strip_bash_lc_and_escape;
use crate::history_cell::HistoryCell;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::render::highlight::highlight_bash_to_lines;
use codex_app_server_protocol::CommandExecutionSource;
use codex_protocol::parse_command::ParsedCommand;
use ratatui::style::Modifier;
use ratatui::style::Stylize;
use ratatui::text::Line;
use ratatui::text::Span;

/// Maximum per-file lines before a patch summary collapses the remainder.
const PATCH_FILE_SUMMARY_LIMIT: usize = 5;

/// Focus summaries render at the same muted level as the working-status indicator, so tool
/// activity reads as ambient rather than competing with agent messages. The dim modifier is
/// applied per span because line-level styles do not survive the transcript's wrapping
/// pipeline; failures and user-shell output bypass the summary builders and stay full
/// brightness.
fn mute(line: Line<'static>) -> Line<'static> {
    let mut line = line;
    for span in &mut line.spans {
        span.style = span.style.add_modifier(Modifier::DIM);
    }
    line
}

fn clipped(line: Line<'static>, width: u16) -> Line<'static> {
    truncate_line_with_ellipsis_if_overflow(mute(line), usize::from(width.max(1)))
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

/// Successful tool activity, counted by category for the aggregate line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FocusActivityCounts {
    pub(crate) patterns: usize,
    pub(crate) reads: usize,
    pub(crate) edits: usize,
    pub(crate) failures: usize,
    pub(crate) images: usize,
    pub(crate) tools: usize,
    pub(crate) shells: usize,
}

impl std::ops::Add for FocusActivityCounts {
    type Output = Self;

    fn add(self, rhs: Self) -> Self {
        Self {
            patterns: self.patterns + rhs.patterns,
            reads: self.reads + rhs.reads,
            edits: self.edits + rhs.edits,
            failures: self.failures + rhs.failures,
            images: self.images + rhs.images,
            tools: self.tools + rhs.tools,
            shells: self.shells + rhs.shells,
        }
    }
}

/// Leading tokens of commands whose exit code 1 almost always means "no matches" rather than
/// failure: searches, reads, and comparisons. Anything else exiting 1 (or any exit ≥ 2) keeps
/// the full failure rendering.
const BENIGN_EXIT_1_LEADS: &[&str] = &[
    "awk", "cat", "diff", "echo", "file", "find", "git", "grep", "head", "hg", "jq", "less", "ls",
    "printf", "pwd", "rg", "sed", "sort", "stat", "tail", "true", "uniq", "wc", "which",
];

/// Whether an exec call may condense: exit 0 always; exit 1 only for parsed reads/searches or
/// scripts led by a read-only command (skipping `cd` segments). Exit ≥ 2 and interrupted work
/// are never condensed.
fn absorbable_exit(call: &ExecCall) -> bool {
    let Some(output) = call.output.as_ref() else {
        return false;
    };
    if output.exit_code == 0 {
        return true;
    }
    if output.exit_code != 1 || call.duration.is_none() {
        return false;
    }
    if call.parsed.iter().all(|parsed| {
        matches!(
            parsed,
            ParsedCommand::Read { .. } | ParsedCommand::Search { .. }
        )
    }) {
        return true;
    }
    let script = strip_bash_lc_and_escape(&call.command);
    script
        .split([';', '&'])
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .find_map(|segment| {
            let lead = segment.split_whitespace().next()?;
            (!matches!(lead, "cd" | "pushd")).then_some(lead)
        })
        .map(|lead| {
            let lead = lead.rsplit('/').next().unwrap_or(lead);
            BENIGN_EXIT_1_LEADS.contains(&lead.to_ascii_lowercase().as_str())
        })
        .unwrap_or(false)
}

/// Count an ExecCell's calls: parsed reads become file reads, parsed searches become patterns,
/// everything else is a shell command. Hard failures and user-driven cells are never
/// absorbable.
pub(crate) fn exec_activity_counts(cell: &ExecCell) -> Option<FocusActivityCounts> {
    let mut counts = FocusActivityCounts::default();
    for call in cell.iter_calls() {
        if matches!(call.source, CommandExecutionSource::UserShell) {
            return None;
        }
        // Hushdex: failed calls count into the run instead of breaking it; each still
        // renders its own one-liner so the exit code stays visible.
        if !absorbable_exit(call) {
            counts.failures += 1;
            counts.shells += 1;
            continue;
        }
        let mut shell_call = false;
        for parsed in &call.parsed {
            match parsed {
                ParsedCommand::Read { .. } => counts.reads += 1,
                ParsedCommand::Search { .. } => counts.patterns += 1,
                ParsedCommand::ListFiles { .. } | ParsedCommand::Unknown { .. } => {
                    shell_call = true
                }
            }
        }
        if shell_call || call.parsed.is_empty() {
            counts.shells += 1;
        }
    }
    Some(counts)
}

fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        noun.to_string()
    } else {
        format!("{noun}s")
    }
}

/// The aggregate summary line: comma-separated verb phrases in a fixed order, first word
/// capitalized, no marker and no trailing period — matching Claude Code's focus phrasing.
pub(crate) fn aggregate_line(counts: &FocusActivityCounts) -> Line<'static> {
    let mut phrases = Vec::new();
    if counts.patterns > 0 {
        phrases.push(format!(
            "searched for {} {}",
            counts.patterns,
            plural(counts.patterns, "pattern")
        ));
    }
    if counts.reads > 0 {
        phrases.push(format!(
            "read {} {}",
            counts.reads,
            plural(counts.reads, "file")
        ));
    }
    if counts.edits > 0 {
        phrases.push(format!(
            "edited {} {}",
            counts.edits,
            plural(counts.edits, "file")
        ));
    }
    if counts.images > 0 {
        phrases.push(format!(
            "viewed {} {}",
            counts.images,
            plural(counts.images, "image")
        ));
    }
    if counts.tools > 0 {
        phrases.push(format!(
            "called {} {}",
            counts.tools,
            plural(counts.tools, "tool")
        ));
    }
    if counts.shells > 0 {
        phrases.push(format!(
            "ran {} shell {}",
            counts.shells,
            plural(counts.shells, "command")
        ));
    }
    if counts.failures > 0 {
        phrases.push(format!("{} failed", counts.failures));
    }
    let mut text = phrases.join(", ");
    let mut characters = text.chars();
    if let Some(first) = characters.next()
        && first.is_alphabetic()
    {
        text = first.to_uppercase().collect::<String>() + characters.as_str();
    }
    mute(Line::from(text))
}

/// `• Ran <command>` per call (or `• Running <command>` while a call is in flight).
///
/// Any failed call — exit code ≠ 0, which includes `mark_failed` interruption — renders the full
/// transcript form (`$ command`, complete output, `✗ (exit)` result) so the exit code stays
/// visible, and any user `!` shell command renders in full because its output is what the user
/// explicitly asked to run.
pub(crate) fn exec_focus_lines(cell: &ExecCell, width: u16) -> Vec<Line<'static>> {
    if cell
        .iter_calls()
        .any(|call| matches!(call.source, CommandExecutionSource::UserShell))
    {
        return cell.display_lines(width);
    }
    // Hushdex: every call condenses to one muted row — failures carry their exit code
    // instead of rendering output; the full block stays in the transcript pager.
    cell.iter_calls()
        .map(|call| {
            if absorbable_exit(call) {
                success_call_line(call, width)
            } else {
                failure_call_line(call, width)
            }
        })
        .collect()
}

/// `✗ <command> (exit N)` — the failed-call one-liner, muted to the working-status level.
fn failure_call_line(call: &ExecCall, width: u16) -> Line<'static> {
    let exit_code = call
        .output
        .as_ref()
        .map(|output| output.exit_code)
        .unwrap_or_default();
    let script = strip_bash_lc_and_escape(&call.command);
    let first = script.lines().next().unwrap_or_default().to_string();
    let mut line = Line::from(vec!["✗".red().bold(), " ".into()]);
    let highlighted = highlight_bash_to_lines(&first);
    if let Some(command) = highlighted.into_iter().next() {
        line.extend(command.spans);
    }
    line.push_span(format!(" (exit {exit_code})").dim());
    clipped(line, width)
}

/// `• Ran <command>` (or `• Running <command>` while in flight), muted to the working-status
/// level by the shared `clipped` wrapper.
fn success_call_line(call: &ExecCall, width: u16) -> Line<'static> {
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
    lines.into_iter().take(/*n*/ 1).map(mute).collect()
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

#[cfg(test)]
#[path = "focus_summaries_tests.rs"]
mod tests;
