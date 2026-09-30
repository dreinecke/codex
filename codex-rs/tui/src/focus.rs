//! Hushdex focus mode: condense terminal scrollback to prompts, tool summaries, and agent text.
//!
//! Focus mode changes only the lines written to terminal scrollback above the live viewport. It
//! picks `HistoryCell::focus_lines` instead of the display presentation when a committed cell is
//! inserted or re-rendered for scrollback. The transcript pager (`Ctrl+T`), the live viewport,
//! raw output mode, copy, and export are unaffected; they keep using the full presentation.
//!
//! The toggle is session-local: `/focus` flips it, `HUSHDEX_FOCUS=0` starts a session with focus
//! off, and no `config.toml` key exists. Toggling never rewrites scrollback that is already
//! printed; it only affects cells inserted afterward. Resize reflow rebuilds scrollback from
//! source cells and applies the current focus state so rebuilt rows stay consistent with
//! insert-time rows.

use crate::app::App;
use crate::chatwidget::ChatWidget;
use crate::history_cell::HistoryCell;
use crate::history_cell::HistoryRenderMode;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use ratatui::text::Line;

/// Whether new sessions start with focus mode on.
///
/// `HUSHDEX_FOCUS=0` opts out. Unit tests default to off so upstream tests keep asserting full
/// scrollback output; Hushdex tests set the state explicitly instead of relying on this default.
pub(crate) fn default_enabled() -> bool {
    if cfg!(test) {
        return false;
    }
    !matches!(std::env::var("HUSHDEX_FOCUS").as_deref(), Ok("0" | "false"))
}

/// The `/focus` notice: the new state plus the key that opens the full transcript.
pub(crate) fn focus_notice(enabled: bool) -> (String, Option<String>) {
    let message = if enabled {
        "Focus mode on: new tool activity is condensed to one-line summaries."
    } else {
        "Focus mode off: new tool activity is shown in full."
    }
    .to_string();
    let hint = crate::keymap::RuntimeKeymap::defaults()
        .primary_hint(crate::keymap::KeymapContext::Global, "open_transcript")
        .map(|binding| format!("Press {} for the full transcript.", binding.display_label()));
    (message, hint)
}

impl ChatWidget {
    pub(crate) fn focus_mode(&self) -> bool {
        self.focus_mode
    }

    pub(crate) fn set_focus_mode(&mut self, enabled: bool) {
        self.focus_mode = enabled;
    }

    /// Toggle focus mode and print the session notice naming the transcript-overlay key.
    pub(crate) fn toggle_focus_mode_and_notify(&mut self) -> bool {
        let enabled = !self.focus_mode;
        self.set_focus_mode(enabled);
        let (message, hint) = focus_notice(enabled);
        self.add_info_message(message, hint);
        enabled
    }
}

impl App {
    /// Lines written to terminal scrollback for a committed cell, honoring focus mode.
    ///
    /// This is the single Hushdex hook for every scrollback path (insertion, resize reflow
    /// rebuild, and rendered-tail bookkeeping), so those paths can never disagree. Raw output
    /// mode wins over focus mode because raw mode exists for verbatim terminal selection.
    pub(crate) fn scrollback_cell_hyperlink_lines(
        &self,
        cell: &dyn HistoryCell,
        width: u16,
    ) -> Vec<HyperlinkLine> {
        let mode = self.chat_widget.history_render_mode();
        if self.chat_widget.focus_mode() && mode == HistoryRenderMode::Rich {
            plain_hyperlink_lines(cell.focus_lines(width))
        } else {
            cell.display_hyperlink_lines_for_mode(width, mode)
        }
    }
}

/// Empty scrollback rendering for `hidden` cells.
pub(crate) fn hidden_focus_lines() -> Vec<Line<'static>> {
    Vec::new()
}

#[cfg(test)]
#[path = "focus_tests.rs"]
mod tests;
