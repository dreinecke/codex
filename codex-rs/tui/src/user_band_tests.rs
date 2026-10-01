use super::*;
use crate::history_cell::HistoryCell;
use crate::history_cell::UserHistoryCell;
use std::borrow::Cow;

fn user_cell(message: &str) -> UserHistoryCell {
    UserHistoryCell {
        message: message.to_string(),
        text_elements: Vec::new(),
        local_image_paths: Vec::new(),
        remote_image_urls: Vec::new(),
        spoken: false,
    }
}

fn sanitize(message: &str) -> String {
    crate::history_cell::sanitize_user_text(Cow::Borrowed(message)).into_owned()
}

#[test]
fn user_prompt_renders_as_a_banded_block() {
    let lines = user_cell(&sanitize("hello there")).display_lines(/*width*/ 40);
    assert!(!lines.is_empty());
    for (index, line) in lines.iter().enumerate() {
        let first = line
            .spans
            .first()
            .unwrap_or_else(|| panic!("line {index} carries the bar prefix span"));
        assert!(
            first.content.starts_with("▌"),
            "line {index} starts with the band bar"
        );
        assert!(first.style.fg.is_some(), "line {index} bar is accented");
        assert!(
            line.style.bg.is_some(),
            "line {index} line style carries the fill for full-row painting"
        );
    }
    let rendered: Vec<String> = lines
        .iter()
        .map(|line| {
            line.spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        })
        .collect();
    assert!(
        rendered.iter().any(|text| text.contains("hello there")),
        "message text survives banding: {rendered:?}"
    );
    assert_eq!(rendered.first().map(|text| text.trim_end()), Some("▌"));
    assert_eq!(rendered.last().map(|text| text.trim_end()), Some("▌"));
}

#[test]
fn blank_band_rows_carry_no_copy_text() {
    let cell = user_cell(&sanitize("measure me"));
    for line in cell.display_hyperlink_lines(/*width*/ 40) {
        if let Some(source) = &line.source {
            assert!(
                source.text.is_empty() || !source.text.contains('▌'),
                "the bar never leaks into logical copy text"
            );
        }
    }
}

#[test]
fn spoken_prompts_use_a_red_bar() {
    let mut cell = user_cell(&sanitize("note to self"));
    cell.spoken = true;
    let lines = cell.display_lines(/*width*/ 40);
    let bar = lines[0].spans[0].style.fg.expect("spoken bar fg");
    assert_eq!(bar, Color::LightRed);
}

#[test]
fn agent_messages_have_no_band() {
    let cell = crate::history_cell::AgentMessageCell::new(
        vec![ratatui::text::Line::from("plain agent words")],
        /*is_first_line*/ true,
    );
    let lines = cell.display_lines(/*width*/ 40);
    for line in &lines {
        assert!(
            line.spans
                .first()
                .is_none_or(|span| !span.content.starts_with("▌")),
            "agent text carries no user band"
        );
        assert!(line.style.bg.is_none(), "agent text has no background fill");
    }
}
