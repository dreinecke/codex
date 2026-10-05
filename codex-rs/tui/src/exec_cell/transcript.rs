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

/// Hushdex: focus-mode failures keep this many trailing output lines; failure summaries
/// live at the end of a run, everything above is bookkeeping.
const FOCUS_FAILURE_OUTPUT_TAIL: usize = 6;

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
    let omitted_source_lines = if clamp_command_echo {
        script.lines().count().saturating_sub(1)
    } else {
        0
    };
    let script_echo = if clamp_command_echo {
        script.lines().next().unwrap_or_default().to_string()
    } else {
        script
    };
    let highlighted_script = highlight_bash_to_lines(&script_echo);
    let cmd_display = adaptive_wrap_hyperlink_lines(
        &plain_hyperlink_lines(highlighted_script),
        RtOptions::new(width as usize)
            .initial_indent("$ ".magenta().into())
            .subsequent_indent("    ".into()),
    );
    lines.extend(cmd_display);
    if omitted_source_lines > 0 {
        lines.push(HyperlinkLine::new(Line::from(vec![
            "    ⋯ +".dim(),
            format!("{omitted_source_lines} source lines").dim(),
        ])));
    }

    let Some(output) = call.output.as_ref() else {
        return;
    };
    if !call.is_unified_exec_interaction() {
        let wrap_width = width.max(/*other*/ 1) as usize;
        let wrap_opts = RtOptions::new(wrap_width);
        let mut unwrapped_lines = output
            .transcript_lines()
            .map(|line| ansi_escape_line(line.as_ref()))
            .collect::<Vec<_>>();
        if let Some(tail) = clamp_output_tail
            && unwrapped_lines.len() > tail
        {
            let omitted = unwrapped_lines.len() - tail;
            let kept = unwrapped_lines.split_off(unwrapped_lines.len() - tail);
            unwrapped_lines = kept;
            lines.push(HyperlinkLine::new(Line::from(vec![
                "⋯ +".dim(),
                format!("{omitted} output lines").dim(),
            ])));
        }
        for unwrapped in unwrapped_lines {
            lines.extend(adaptive_wrap_hyperlink_lines(
                &[unwrapped.into()],
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
