//! Claude-Code-style run aggregation for Hushdex focus mode.
//!
//! Consecutive absorbable tool cells merge into one `FocusAggregateCell` that renders a single
//! dim line ("Searched for 2 patterns, read 5 files, ran 9 shell commands"). Merging rewrites
//! the last scrollback block in place via `replace_visible_history_tail`, so a growing run stays
//! one line instead of stacking one-liners. Anything that must render in full — failures, user
//! `!` shell commands, agent messages, prompts — closes the run. The transcript pager keeps
//! every absorbed child cell, and turning focus off expands the aggregate back to the full
//! presentations of everything it absorbed.

use super::history_ui::RenderedHistoryTail;
use super::*;
use crate::focus_summaries::FocusActivityCounts;
use crate::focus_summaries::aggregate_line;
use crate::history_cell::HistoryCell;
use crate::history_cell::ReasoningSummaryCell;
use crate::line_truncation::truncate_line_with_ellipsis_if_overflow;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use ratatui::text::Line;
use std::sync::Arc;

/// One run of absorbed tool cells rendered as a single dim line.
#[derive(Debug)]
pub(crate) struct FocusAggregateCell {
    parts: Vec<Arc<dyn HistoryCell>>,
}

impl FocusAggregateCell {
    pub(crate) fn new(parts: Vec<Arc<dyn HistoryCell>>) -> Self {
        Self { parts }
    }

    fn counts(&self) -> FocusActivityCounts {
        self.parts
            .iter()
            .filter_map(|part| part.focus_activity_counts())
            .fold(FocusActivityCounts::default(), |sum, counts| sum + counts)
    }

    fn joined_lines(
        &self,
        width: u16,
        lines: impl Fn(&Arc<dyn HistoryCell>, u16) -> Vec<HyperlinkLine>,
    ) -> Vec<HyperlinkLine> {
        let mut out = Vec::new();
        for part in &self.parts {
            let part_lines = lines(part, width);
            if part_lines.is_empty() {
                continue;
            }
            if !out.is_empty() {
                out.push(HyperlinkLine::from(""));
            }
            out.extend(part_lines);
        }
        out
    }
}

/// Hushdex: cap how many failed-call one-liners render beneath the run summary.
const FOCUS_AGGREGATE_FAILURE_LIMIT: usize = 5;

impl HistoryCell for FocusAggregateCell {
    fn focus_lines(&self, width: u16) -> Vec<Line<'static>> {
        let mut lines = vec![truncate_line_with_ellipsis_if_overflow(
            aggregate_line(&self.counts()),
            usize::from(width.max(/*other*/ 1)),
        )];
        if self.counts().failures == 0 {
            return lines;
        }
        // The run's failed calls keep their one-liners visible beneath the summary so the
        // exit codes stay scannable; successful calls are already counted above them.
        let mut shown = 0;
        let mut hidden_failures = 0;
        for part in &self.parts {
            for line in part.focus_lines(width) {
                let is_failure_line = line
                    .spans
                    .first()
                    .is_some_and(|span| span.content.starts_with("✗"));
                if !is_failure_line {
                    continue;
                }
                if shown < FOCUS_AGGREGATE_FAILURE_LIMIT {
                    lines.push(line);
                    shown += 1;
                } else {
                    hidden_failures += 1;
                }
            }
        }
        if hidden_failures > 0 {
            lines.push(Line::from(
                format!("  ⋯ +{hidden_failures} more failed").dim(),
            ));
        }
        lines
    }

    fn display_lines(&self, width: u16) -> Vec<Line<'static>> {
        crate::terminal_hyperlinks::visible_lines(self.display_hyperlink_lines(width))
    }

    fn display_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.joined_lines(width, |part, width| part.display_hyperlink_lines(width))
    }

    fn transcript_hyperlink_lines(&self, width: u16) -> Vec<HyperlinkLine> {
        self.joined_lines(width, |part, width| part.transcript_hyperlink_lines(width))
    }

    fn raw_lines(&self) -> Vec<Line<'static>> {
        self.parts
            .iter()
            .flat_map(|part| part.raw_lines())
            .collect()
    }

    fn is_stream_continuation(&self) -> bool {
        false
    }
}

fn can_aggregate(app: &App, tui: &tui::Tui) -> bool {
    if !app.chat_widget.focus_mode() || app.chat_widget.raw_output_mode() {
        return false;
    }
    let inline_replay_buffering = !tui.is_owned_screen()
        && app
            .initial_history_replay_buffer
            .as_ref()
            .is_some_and(|buffer| !buffer.render_from_transcript_tail);
    if app.overlay.is_some() || inline_replay_buffering {
        return false;
    }
    // Deferred cells are still queued; merging here would reorder their output.
    app.native_history.is_empty()
}

/// Absorb `cell` into the trailing aggregate run if focus mode allows it.
///
/// Returns `true` when the cell was absorbed (the caller must not insert it separately). The
/// overlay pager already received the full cell before this runs, so the pager keeps complete
/// history while scrollback shows only the aggregate line.
pub(super) fn absorb_into_focus_aggregate(
    app: &mut App,
    tui: &mut tui::Tui,
    cell: &Arc<dyn HistoryCell>,
) -> bool {
    if !can_aggregate(app, tui) {
        return false;
    }
    if cell.focus_activity_counts().is_none() {
        return false;
    }

    let Some(run_index) = trailing_aggregate_index(app) else {
        let aggregate: Arc<dyn HistoryCell> = Arc::new(FocusAggregateCell::new(vec![cell.clone()]));
        app.transcript_cells.push(aggregate.clone());
        app.render_inserted_history_cell(tui, &aggregate, /*deferred*/ tui.is_owned_screen());
        return true;
    };

    let previous = app.transcript_cells[run_index].clone();
    let Some(mut parts) = previous
        .as_any()
        .downcast_ref::<FocusAggregateCell>()
        .map(|aggregate| aggregate.parts.clone())
    else {
        return false;
    };
    parts.push(cell.clone());
    let merged: Arc<dyn HistoryCell> = Arc::new(FocusAggregateCell::new(parts));
    app.transcript_cells[run_index] = merged.clone();

    if tui.is_owned_screen() {
        tui.frame_requester().schedule_frame();
        return true;
    }

    let width = app
        .chat_widget
        .history_wrap_width(tui.terminal.last_known_screen_size.width);
    let updated = plain_hyperlink_lines(merged.focus_lines(width));
    let previous_lines = app
        .last_rendered_history_tail
        .as_ref()
        .filter(|tail| {
            tail.cell
                .upgrade()
                .is_some_and(|tail_cell| Arc::ptr_eq(&tail_cell, &previous))
        })
        .map(|tail| tail.lines.clone());
    let replaced = previous_lines.is_some()
        && tui
            .replace_visible_history_tail(
                &previous_lines.unwrap_or_default(),
                &updated,
                app.history_line_wrap_policy(),
            )
            .unwrap_or(false);
    if !replaced {
        app.insert_history_cell_lines(tui, merged.as_ref(), width);
    }
    app.last_rendered_history_tail = if app.overlay.is_none() {
        Some(RenderedHistoryTail {
            cell: Arc::downgrade(&merged),
            lines: updated,
        })
    } else {
        None
    };
    true
}

/// Fold consecutive absorbable tool cells inside a projected batch (such as an older history
/// page) into `FocusAggregateCell` runs so paged-in history matches live and resumed turns.
pub(super) fn fold_focus_aggregates(
    app: &App,
    tui: &tui::Tui,
    cells: Vec<Arc<dyn HistoryCell>>,
) -> Vec<Arc<dyn HistoryCell>> {
    if !can_aggregate(app, tui) {
        return cells;
    }
    let mut out: Vec<Arc<dyn HistoryCell>> = Vec::with_capacity(cells.len());
    for cell in cells {
        if cell.focus_activity_counts().is_none() {
            out.push(cell);
            continue;
        }
        if let Some(run_index) = trailing_aggregate_index_in(&out)
            && let Some(mut parts) = out[run_index]
                .as_any()
                .downcast_ref::<FocusAggregateCell>()
                .map(|aggregate| aggregate.parts.clone())
        {
            parts.push(cell);
            out[run_index] = Arc::new(FocusAggregateCell::new(parts));
        } else {
            out.push(Arc::new(FocusAggregateCell::new(vec![cell])));
        }
    }
    out
}

/// Merge two `FocusAggregateCell` runs that were split across an older-history page boundary.
pub(super) fn join_older_focus_aggregate(app: &mut App, tui: &tui::Tui, boundary: usize) {
    if !can_aggregate(app, tui) || boundary == 0 || boundary >= app.transcript_cells.len() {
        return;
    }
    let Some(older_index) = trailing_aggregate_index_in(&app.transcript_cells[..boundary]) else {
        return;
    };
    let mut newer_index = None;
    for (offset, cell) in app.transcript_cells[boundary..]
        .iter()
        .enumerate()
        .take(/*n*/ 64)
    {
        if cell.as_any().downcast_ref::<FocusAggregateCell>().is_some() {
            newer_index = Some(boundary + offset);
            break;
        }
        if interrupts_run(cell) {
            return;
        }
    }
    let Some(newer_index) = newer_index else {
        return;
    };
    let Some(mut parts) = app.transcript_cells[older_index]
        .as_any()
        .downcast_ref::<FocusAggregateCell>()
        .map(|aggregate| aggregate.parts.clone())
    else {
        return;
    };
    let Some(newer_parts) = app.transcript_cells[newer_index]
        .as_any()
        .downcast_ref::<FocusAggregateCell>()
        .map(|aggregate| aggregate.parts.clone())
    else {
        return;
    };
    parts.extend(
        app.transcript_cells[older_index + 1..newer_index]
            .iter()
            .cloned(),
    );
    parts.extend(newer_parts);
    let merged: Arc<dyn HistoryCell> = Arc::new(FocusAggregateCell::new(parts));
    let range = older_index..newer_index + 1;
    app.transcript_view
        .replace_group(&app.transcript_cells, range.clone(), &merged);
    app.transcript_cells.splice(range, [merged]);
}

fn interrupts_run(cell: &Arc<dyn HistoryCell>) -> bool {
    !cell.is_stream_continuation()
        && cell
            .as_any()
            .downcast_ref::<crate::focus::FocusHiddenHistoryCell>()
            .is_none()
        && cell
            .as_any()
            .downcast_ref::<ReasoningSummaryCell>()
            .is_none()
}

/// Index of the aggregate cell ending the current run, if any.
///
/// Hidden cells, stream-continuation fragments, and hidden lifecycle telemetry do not interrupt
/// a run; the first visible non-aggregate cell does.
fn trailing_aggregate_index(app: &App) -> Option<usize> {
    trailing_aggregate_index_in(&app.transcript_cells)
}

fn trailing_aggregate_index_in(cells: &[Arc<dyn HistoryCell>]) -> Option<usize> {
    // Hushdex: reasoning-heavy turns push many hidden cells between the aggregate and the
    // next absorbable cell; a small window made consecutive runs emit adjacent aggregates
    // instead of merging. The scan stays cheap — pointer downcasts over hidden cells only.
    for (index, cell) in cells.iter().enumerate().rev().take(/*n*/ 64) {
        if cell.as_any().downcast_ref::<FocusAggregateCell>().is_some() {
            return Some(index);
        }
        if interrupts_run(cell) {
            return None;
        }
    }
    None
}

#[cfg(test)]
#[path = "focus_aggregate_tests.rs"]
mod tests;
