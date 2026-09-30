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

impl HistoryCell for FocusAggregateCell {
    fn focus_lines(&self, width: u16) -> Vec<Line<'static>> {
        vec![truncate_line_with_ellipsis_if_overflow(
            aggregate_line(&self.counts()),
            usize::from(width.max(/*other*/ 1)),
        )]
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
    if !app.chat_widget.focus_mode() || app.chat_widget.raw_output_mode() {
        return false;
    }
    if app.overlay.is_some() || app.initial_history_replay_buffer.is_some() {
        return false;
    }
    if !app.native_history.is_empty() {
        // Deferred cells are still queued; merging here would reorder their output.
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

/// Index of the aggregate cell ending the current run, if any.
///
/// Hidden reasoning cells do not interrupt a run; the first visible non-aggregate cell does.
fn trailing_aggregate_index(app: &App) -> Option<usize> {
    for (index, cell) in app.transcript_cells.iter().enumerate().rev().take(/*n*/ 4) {
        if cell.as_any().downcast_ref::<FocusAggregateCell>().is_some() {
            return Some(index);
        }
        cell.as_any().downcast_ref::<ReasoningSummaryCell>()?;
    }
    None
}

#[cfg(test)]
#[path = "focus_aggregate_tests.rs"]
mod tests;
