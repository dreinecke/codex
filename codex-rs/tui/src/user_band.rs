//! User-authored blocks (submitted prompts and the composer) render as an accented band:
//! a solid accent bar down the left edge and a full-width tint fill, so user content is
//! visually distinct from agent messages, which render without a background.
//!
//! The fill reuses the terminal-aware prompt tint but does not give up when the terminal
//! does not report a background color (OSC 11); such terminals are assumed dark so the band
//! stays visible. The fill lives on line styles — painted across whole rows by the
//! transcript view and cleared to end-of-line by scrollback printing — and copies are
//! built from logical source text, so the band never leaks into copied text.

#[cfg(not(test))]
use crate::style::history_prompt_style;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::LogicalLineSource;
use ratatui::style::Color;
use ratatui::style::Style;
use ratatui::text::Span;

const BAR_GLYPH: &str = "▌";

/// The prompt tint with a dark-terminal fallback so the band never disappears.
pub(crate) fn user_band_fill() -> Style {
    #[cfg(test)]
    // Thread-local probe state leaks between threaded tests; a constant fill keeps the
    // band identical under every runner while production keeps the terminal-aware tint.
    {
        Style::default().bg(fallback_fill_bg())
    }
    #[cfg(not(test))]
    {
        ensure_fill(history_prompt_style())
    }
}

/// The composer frame fill, matching the submitted-prompt band.
pub(crate) fn composer_fill() -> Style {
    ensure_fill(crate::style::user_message_style())
}

/// Terminals that do not report a background are nearly always dark; fall back to a
/// fixed dark tint rather than dropping the band entirely. The constant also survives
/// terminals with an unknown color level, where palette-matched colors degrade to Reset.
pub(crate) fn ensure_fill(style: Style) -> Style {
    match style.bg {
        Some(_) => style,
        None => Style::default().bg(fallback_fill_bg()),
    }
}

fn fallback_fill_bg() -> Color {
    // A constant indexed gray keeps tests deterministic (the effective color level is a
    // sticky thread-local) and renders as a stable dark tint on every palette.
    crate::terminal_palette::indexed_color(236)
}

/// Voice-submitted prompts keep their distinct marker by recoloring the bar. The accent
/// pins its color level so the band renders identically under every test runner.
pub(crate) fn bar_color(fill: Style, spoken: bool) -> Color {
    if spoken {
        Color::LightRed
    } else {
        crate::style::deterministic_accent_on(fill.bg)
    }
}

pub(crate) fn bar_span(fill: Style, spoken: bool) -> Span<'static> {
    Span::styled(BAR_GLYPH, Style::default().fg(bar_color(fill, spoken)))
}

/// The bar and its one-column gutter as a single prefix span, matching the width of the
/// live composer prefix. Passed as both the initial and continuation indent so every
/// wrapped row of the block keeps the bar through source-level reconstruction and rewrap.
pub(crate) fn bar_prefix_span(fill: Style, spoken: bool) -> Span<'static> {
    Span::styled("▌ ", Style::default().fg(bar_color(fill, spoken)))
}

/// Frames the block: every row gets the fill as its line style — the transcript view
/// paints whole rows with it and terminal scrollback clears to end-of-line with it —
/// and source-less blank rows get the bar prefix plus an empty source so copies stay
/// clean. Span styles stay fg-only so callers can override the fill coherently.
pub(crate) fn apply_user_band(
    lines: Vec<HyperlinkLine>,
    fill: Style,
    spoken: bool,
) -> Vec<HyperlinkLine> {
    lines
        .into_iter()
        .map(|mut hyperlink_line| {
            let mut line = hyperlink_line.line;
            line.style = line.style.patch(fill);
            if !line
                .spans
                .first()
                .is_some_and(|span| span.content.starts_with(BAR_GLYPH))
            {
                line.spans.insert(0, bar_prefix_span(fill, spoken));
                if hyperlink_line.source.is_none() {
                    // Blank band rows carry no logical text: an empty source keeps the bar
                    // prefix out of copies and the transcript's logical reconstruction.
                    let mut source = LogicalLineSource::new(String::new());
                    source.prefix_bytes = bar_prefix_span(fill, spoken).content.len();
                    hyperlink_line.source = Some(source);
                }
            }
            hyperlink_line.line = line;
            hyperlink_line
        })
        .collect()
}

#[cfg(test)]
#[path = "user_band_tests.rs"]
mod tests;
