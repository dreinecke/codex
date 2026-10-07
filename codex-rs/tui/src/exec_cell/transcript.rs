//! Expanded command history with source-backed selection and chronological activity details.
//! Raw history omits transcript-only reasoning while retaining full command output.

use super::model::ExecCall;
use super::model::ExecCell;
use crate::exec_command::strip_bash_lc_and_escape;
use crate::history_cell::HistoryRenderMode;
use crate::render::highlight::highlight_bash_to_lines;
use crate::terminal_hyperlinks::HyperlinkLine;
use crate::terminal_hyperlinks::adaptive_wrap_hyperlink_lines;
use crate::terminal_hyperlinks::plain_hyperlink_lines;
use crate::wrapping::RtOptions;
use codex_ansi_escape::ansi_escape_line;
use codex_utils_elapsed::format_duration;
use ratatui::prelude::*;

/// Hushdex: focus-mode failures keep at most this many trailing output lines, anchored to
/// error-looking lines — in `A; B` chains the error lands last, so an unanchored tail would
/// showcase the successful sibling's output instead of the failure.
const FOCUS_FAILURE_OUTPUT_TAIL: usize = 6;

/// True for diagnostic-style output: tool-prefixed messages (`sed: …`, `cargo: …`) or lines
/// carrying a common failure keyword. Non-matching lines above the anchored tail are dropped.
fn looks_like_error_line(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() {
        return false;
    }
    let lowered = line.to_ascii_lowercase();
    lowered.contains("error")
        || lowered.contains("failed")
        || lowered.contains("failure")
        || lowered.contains("traceback")
        || lowered.contains("exception")
        || lowered.contains("no such file")
        || lowered.contains("not found")
        || lowered.contains("refused")
        || lowered.contains("denied")
        || lowered.contains("cannot")
        || lowered.contains("can't")
        || lowered.contains("fatal")
        || lowered.contains("panic")
        || lowered.contains("warning")
        || line.split_once(':').is_some_and(|(tool, rest)| {
            !tool.is_empty()
                && tool.len() <= 24
                && tool
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.')
                && rest.starts_with(' ')
        })
}

/// Selects the failure-visible slice of a command's output: trailing blank lines are
/// dropped, then lines are kept from the end while they look diagnostic, always keeping at
/// least the final line, capped at [`FOCUS_FAILURE_OUTPUT_TAIL`]. Returns the start index
/// of the kept slice within `texts`.
fn failure_output_tail_start(texts: &[String]) -> usize {
    let mut end = texts.len();
    while end > 0 && texts[end - 1].trim().is_empty() {
        end -= 1;
    }
    if end == 0 {
        return texts.len();
    }
    let mut kept = 1;
    while kept < end
        && kept < FOCUS_FAILURE_OUTPUT_TAIL
        && looks_like_error_line(&texts[end - kept - 1])
    {
        kept += 1;
    }
    end - kept
}

fn line_text(line: &Line<'static>) -> String {
    line.spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect()
}

impl ExecCell {
    pub(super) fn detailed_hyperlink_lines(
        &self,
        width: u16,
        mode: HistoryRenderMode,
    ) -> Vec<HyperlinkLine> {
        self.detailed_hyperlink_lines_with_options(
            width, mode, /*clamp_command_echo*/ false, /*clamp_output_tail*/ None,
        )
    }

    /// Hushdex: focus-mode failure rendering for a single call. The command echo clamps to
    /// its first line so a multi-line heredoc script cannot flood the transcript, and the
    /// output clamps to its tail — failure summaries live at the end — keeping the exit
    /// footer visible. Successful sibling calls in a mixed cell condense instead.
    pub(crate) fn focus_failure_call_hyperlink_lines(
        &self,
        call: &ExecCall,
        width: u16,
    ) -> Vec<HyperlinkLine> {
        let mut lines = Vec::new();
        push_call_hyperlink_lines(
            &mut lines,
            call,
            width,
            /*clamp_command_echo*/ true,
            /*clamp_output_tail*/ Some(FOCUS_FAILURE_OUTPUT_TAIL),
        );
        lines
    }

    fn detailed_hyperlink_lines_with_options(
        &self,
        width: u16,
        mode: HistoryRenderMode,
        clamp_command_echo: bool,
        clamp_output_tail: Option<usize>,
    ) -> Vec<HyperlinkLine> {
        let mut lines: Vec<HyperlinkLine> = vec![];
        for (i, call) in self.iter_calls().enumerate() {
            if i > 0 {
                lines.push("".into());
            }
            push_call_hyperlink_lines(
                &mut lines,
                call,
                width,
                clamp_command_echo,
                clamp_output_tail,
            );
            lines.extend(self.group.details.lines_after(i + 1, width, mode));
        }
        lines
    }
}

/// One call's expanded rendering: highlighted command echo (optionally clamped to its
/// first line), output (optionally clamped to its trailing lines), and the result footer.
fn push_call_hyperlink_lines(
    lines: &mut Vec<HyperlinkLine>,
    call: &ExecCall,
    width: u16,
    clamp_command_echo: bool,
    clamp_output_tail: Option<usize>,
) {
    let script = strip_bash_lc_and_escape(&call.command);
    // Hushdex: the focus failure form clamps the command echo to ONE visual row — a long
    // semicolon chain is a single logical line that would otherwise wrap across many rows.
    if clamp_command_echo {
        let omitted_source_lines = script.lines().count().saturating_sub(1);
        let first = script.lines().next().unwrap_or_default().to_string();
        let mut highlighted = highlight_bash_to_lines(&first);
        let Some(mut head) = (if highlighted.is_empty() {
            None
        } else {
            Some(highlighted.remove(0))
        }) else {
            return;
        };
        head.spans.insert(0, "$ ".magenta());
        let marker_width = if omitted_source_lines > 0 {
            " ⋯ +N source lines".len()
        } else {
            " ⋯".len()
        };
        let budget = (width as usize).saturating_sub(marker_width).max(1);
        let mut head =
            crate::line_truncation::truncate_line_with_ellipsis_if_overflow(head, budget);
        if omitted_source_lines > 0 {
            head.push_span(format!(" ⋯ +{omitted_source_lines} source lines").dim());
        }
        lines.push(HyperlinkLine::new(head));
    } else {
        let highlighted_script = highlight_bash_to_lines(&script);
        let cmd_display = adaptive_wrap_hyperlink_lines(
            &plain_hyperlink_lines(highlighted_script),
            RtOptions::new(width as usize)
                .initial_indent("$ ".magenta().into())
                .subsequent_indent("    ".into()),
        );
        lines.extend(cmd_display);
    }

    let Some(output) = call.output.as_ref() else {
        return;
    };
    if !call.is_unified_exec_interaction() {
        let wrap_width = width.max(/*other*/ 1) as usize;
        let wrap_opts = RtOptions::new(wrap_width);
        let all_lines = output
            .transcript_lines()
            .map(|line| ansi_escape_line(line.as_ref()))
            .collect::<Vec<_>>();
        // Hushdex: failures anchor the tail to error-looking lines so `A; B` chains show
        // the failure, not the successful sibling's output.
        let visible_from = match clamp_output_tail {
            Some(_) => {
                let texts: Vec<String> = all_lines.iter().map(line_text).collect();
                failure_output_tail_start(&texts)
            }
            None => 0,
        };
        if visible_from > 0 {
            let omitted = visible_from;
            lines.push(HyperlinkLine::new(Line::from(vec![
                "⋯ +".dim(),
                format!("{omitted} output lines").dim(),
            ])));
        }
        let unwrapped_lines = &all_lines[visible_from..];
        for unwrapped in unwrapped_lines {
            lines.extend(adaptive_wrap_hyperlink_lines(
                &[unwrapped.clone().into()],
                wrap_opts.clone(),
            ));
        }
    }
    if call.duration.is_some() || output.exit_code != 0 {
        let mut result: Line = if output.exit_code == 0 {
            Line::from("✓".green().bold())
        } else {
            Line::from(vec![
                "✗".red().bold(),
                format!(" ({})", output.exit_code).into(),
            ])
        };
        if let Some(duration) = call.duration {
            let duration = format_duration(duration);
            result.push_span(format!(" • {duration}").dim());
        }
        lines.push(result.into());
    }
}

#[cfg(test)]
#[path = "transcript_tests.rs"]
mod tests;
