//! Drift guards, fixture snapshots, and assertions for Hushdex focus mode.
//!
//! These tests keep automated upstream rebases safe:
//! - `focus_allowlist_is_complete` fails when a `HistoryCell` type is unclassified, stale, or
//!   missing its `focus_lines` override.
//! - `default_tui_guard` fails when the `codex` binary no longer launches `codex-tui`.
//! - The fixture snapshots pin focus-mode scrollback at width 80 with focus on and off.

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use codex_app_server_protocol::CommandExecutionSource;
use codex_app_server_protocol::WebSearchAction;
use codex_protocol::mcp::CallToolResult;
use codex_protocol::parse_command::ParsedCommand;
use pretty_assertions::assert_eq;
use ratatui::text::Line;

use crate::diff_model::FileChange;
use crate::exec_cell::CommandOutput;
use crate::exec_cell::ExecCell;
use crate::exec_cell::new_active_exec_command;
use crate::history_cell::AgentMarkdownCell;
use crate::history_cell::AgentMessageCell;
use crate::history_cell::HistoryCell;
use crate::history_cell::McpInvocation;
use crate::history_cell::McpToolCallCell;
use crate::history_cell::PatchHistoryCell;
use crate::history_cell::ReasoningSummaryCell;
use crate::history_cell::UserHistoryCell;
use crate::history_cell::WebSearchCell;
use crate::history_cell::new_active_mcp_tool_call;
use crate::history_cell::new_patch_event;
use crate::history_cell::new_user_prompt;
use crate::history_cell::new_web_search_call;

const WIDTH: u16 = 80;

/// One of each cell class, in transcript order.
struct FixtureCells {
    user: UserHistoryCell,
    reasoning: ReasoningSummaryCell,
    agent_between_tools: AgentMessageCell,
    command_succeeded: ExecCell,
    command_failed: ExecCell,
    patch: PatchHistoryCell,
    mcp_succeeded: McpToolCallCell,
    mcp_failed: McpToolCallCell,
    web_search: WebSearchCell,
    final_message: AgentMarkdownCell,
}

fn fixture_cells() -> FixtureCells {
    let cwd = Path::new("/repo");

    let user = new_user_prompt(
        "List the files in the current directory and summarize the build setup.".to_string(),
        Vec::new(),
        Vec::new(),
        Vec::new(),
    );

    let reasoning = ReasoningSummaryCell::new(
        "**Considering the request**".to_string(),
        "I should cross-check the manifest before listing files so the summary is accurate."
            .to_string(),
        cwd,
        /*transcript_only*/ false,
    );

    let agent_between_tools = AgentMessageCell::new(
        vec![Line::from(
            "I will inspect the directory first, then report back.",
        )],
        /*is_first_line*/ true,
    );

    let mut command_succeeded = exec_cell("exec-ok", "cargo build --quiet");
    let mut command_failed = exec_cell("exec-fail", "cargo test");
    command_succeeded
        .complete_call(
            "exec-ok",
            CommandOutput::new(
                /*exit_code*/ 0,
                "   Compiling hushdex\n    Finished `dev`".to_string(),
            ),
            std::time::Duration::from_millis(1_200),
        )
        .then_some(())
        .expect("complete exec-ok");
    command_failed
        .complete_call(
            "exec-fail",
            CommandOutput::new(
                /*exit_code*/ 2,
                "error: could not compile `hushdex` due to 3 previous errors".to_string(),
            ),
            std::time::Duration::from_millis(3_400),
        )
        .then_some(())
        .expect("complete exec-fail");

    let mut changes = HashMap::new();
    changes.insert(
        PathBuf::from("src/app.rs"),
        FileChange::Update {
            unified_diff: "--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1,2 +1,4 @@\n context\n-delta\n+alpha\n+beta\n+gamma\n"
                .to_string(),
            move_path: None,
        },
    );
    changes.insert(
        PathBuf::from("src/new_module.rs"),
        FileChange::Add {
            content: "one\ntwo\nthree\nfour\nfive\nsix\nseven".to_string(),
        },
    );
    changes.insert(
        PathBuf::from("src/gone_module.rs"),
        FileChange::Delete {
            content: "one\ntwo".to_string(),
        },
    );
    for name in ["four", "five", "six", "seven"] {
        changes.insert(
            PathBuf::from(format!("src/{name}.rs")),
            FileChange::Update {
                unified_diff: format!(
                    "--- a/src/{name}.rs\n+++ b/src/{name}.rs\n@@ -1,1 +1,2 @@\n context\n+x\n"
                ),
                move_path: None,
            },
        );
    }
    let patch = new_patch_event(changes, cwd);

    let mut mcp_succeeded = new_active_mcp_tool_call(
        "mcp-ok".to_string(),
        McpInvocation {
            server: "github".to_string(),
            tool: "search_code".to_string(),
            arguments: None,
        },
        /*animations_enabled*/ false,
    );
    mcp_succeeded.complete(
        std::time::Duration::from_millis(900),
        Ok(CallToolResult {
            content: vec![serde_json::json!({
                "type": "text",
                "text": "found 3 matching repositories",
            })],
            structured_content: None,
            is_error: None,
            meta: None,
        }),
    );

    let mut mcp_failed = new_active_mcp_tool_call(
        "mcp-fail".to_string(),
        McpInvocation {
            server: "github".to_string(),
            tool: "get_file".to_string(),
            arguments: None,
        },
        /*animations_enabled*/ false,
    );
    mcp_failed.complete(
        std::time::Duration::from_millis(60_000),
        Err("tool server timed out".to_string()),
    );

    let mut web_search = new_web_search_call(
        "web-1".to_string(),
        "rust terminal ui framework".to_string(),
        WebSearchAction::Search {
            query: Some("rust terminal ui framework".to_string()),
            queries: None,
        },
    );
    web_search.complete();

    let final_message = AgentMarkdownCell::new(
        "All done — **3 files** updated and the build is green.".to_string(),
        cwd,
    );

    FixtureCells {
        user,
        reasoning,
        agent_between_tools,
        command_succeeded,
        command_failed,
        patch,
        mcp_succeeded,
        mcp_failed,
        web_search,
        final_message,
    }
}

fn exec_cell(call_id: &str, command: &str) -> ExecCell {
    new_active_exec_command(
        call_id.to_string(),
        vec!["bash".into(), "-lc".into(), command.into()],
        vec![ParsedCommand::Unknown {
            cmd: command.to_string(),
        }],
        CommandExecutionSource::Agent,
        /*interaction_input*/ None,
        /*animations_enabled*/ false,
    )
}

impl FixtureCells {
    fn all(&self) -> Vec<&dyn HistoryCell> {
        vec![
            &self.user,
            &self.reasoning,
            &self.agent_between_tools,
            &self.command_succeeded,
            &self.command_failed,
            &self.patch,
            &self.mcp_succeeded,
            &self.mcp_failed,
            &self.web_search,
            &self.final_message,
        ]
    }
}

fn render(cells: &[&dyn HistoryCell], focus: bool) -> String {
    let mut lines = Vec::new();
    for cell in cells {
        let cell_lines = if focus {
            cell.focus_lines(WIDTH)
        } else {
            cell.display_lines(WIDTH)
        };
        if cell_lines.is_empty() {
            continue;
        }
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.extend(cell_lines.into_iter().map(|line| line.to_string()));
    }
    lines.join("\n")
}

fn lines_to_string(lines: &[Line<'static>]) -> String {
    lines
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn snapshot_focus_transcript_focus_on() {
    let cells = fixture_cells();
    insta::assert_snapshot!(render(&cells.all(), /*focus*/ true));
}

#[test]
fn snapshot_focus_transcript_focus_off() {
    let cells = fixture_cells();
    insta::assert_snapshot!(render(&cells.all(), /*focus*/ false));
}

#[test]
fn focus_output_keeps_agent_text_verbatim_and_hides_reasoning() {
    let cells = fixture_cells();
    let focus_on = render(&cells.all(), /*focus*/ true);
    assert!(
        focus_on.contains("I will inspect the directory first, then report back."),
        "agent text between tool calls must appear verbatim:\n{focus_on}"
    );
    assert!(
        focus_on.contains("All done — 3 files updated and the build is green."),
        "final agent message must appear verbatim:\n{focus_on}"
    );
    assert!(
        focus_on.contains("List the files in the current directory"),
        "user prompt must appear:\n{focus_on}"
    );
    assert!(
        !focus_on.contains("cross-check the manifest"),
        "reasoning text must not appear in focus output:\n{focus_on}"
    );
}

#[test]
fn failed_command_renders_in_full_with_exit_code() {
    let cells = fixture_cells();
    let focus_lines = cells.command_failed.focus_lines(WIDTH);
    // Hushdex: failures condense to one muted row carrying the command and exit code;
    // the full block (command, output, footer) stays in the transcript pager.
    assert_eq!(
        focus_lines.len(),
        1,
        "failed commands condense to one row:\n{}",
        lines_to_string(&focus_lines)
    );
    let text = lines_to_string(&focus_lines);
    assert!(
        text.contains("cargo test"),
        "failed command must show the command:\n{text}"
    );
    assert!(
        text.contains("✗"),
        "failed command must show the failure marker:\n{text}"
    );
    assert!(
        text.contains("(exit 2)"),
        "failed command must show its exit code:\n{text}"
    );
    assert!(
        !text.contains("could not compile"),
        "failed command output stays in the pager:\n{text}"
    );
}

#[test]
fn successful_command_condenses_to_one_line() {
    let cells = fixture_cells();
    let lines = cells.command_succeeded.focus_lines(WIDTH);
    assert_eq!(lines.len(), 1, "one line per command");
    let text = lines_to_string(&lines);
    assert!(
        text.contains("Ran cargo build --quiet"),
        "command summary names the command:\n{text}"
    );
    assert!(
        !text.contains("Compiling"),
        "command output must not appear in the summary:\n{text}"
    );
}

#[test]
fn failed_mcp_call_renders_in_full() {
    let cells = fixture_cells();
    assert_eq!(
        cells.mcp_failed.focus_lines(WIDTH),
        cells.mcp_failed.display_lines(WIDTH),
        "failed MCP calls render in full"
    );
    let text = lines_to_string(&cells.mcp_failed.display_lines(WIDTH));
    assert!(
        text.contains("timed out"),
        "failed MCP call must show the error:\n{text}"
    );
}

#[test]
fn successful_mcp_call_condenses_to_one_line() {
    let cells = fixture_cells();
    let lines = cells.mcp_succeeded.focus_lines(WIDTH);
    assert_eq!(lines.len(), 1, "one line per MCP call");
    let text = lines_to_string(&lines);
    assert!(
        text.contains("Called github.search_code"),
        "MCP summary names server and tool:\n{text}"
    );
    assert!(
        !text.contains("matching repositories"),
        "MCP result content must not appear in the summary:\n{text}"
    );
}

#[test]
fn patch_summary_caps_files_and_shows_diffstats() {
    let cells = fixture_cells();
    let lines = cells.patch.focus_lines(WIDTH);
    let text = lines_to_string(&lines);
    assert_eq!(
        lines.len(),
        6,
        "seven files must cap at five lines plus an overflow line:\n{text}"
    );
    assert!(
        text.contains("Added src/new_module.rs"),
        "new files use Added:\n{text}"
    );
    assert!(
        text.contains("Deleted src/gone_module.rs"),
        "removed files use Deleted:\n{text}"
    );
    assert!(
        text.contains("Edited src/app.rs (+3 -1)"),
        "updated files use Edited with a diffstat:\n{text}"
    );
    assert!(
        text.contains("…and 2 more files"),
        "the overflow line counts the remainder:\n{text}"
    );
}

#[test]
fn reasoning_cell_focus_lines_are_empty_even_when_displayable() {
    let reasoning = ReasoningSummaryCell::new(
        "header".to_string(),
        "reasoning that would render in the live viewport".to_string(),
        Path::new("/repo"),
        /*transcript_only*/ false,
    );
    assert!(!reasoning.display_lines(WIDTH).is_empty());
    assert!(reasoning.focus_lines(WIDTH).is_empty());
    // The transcript pager keeps the full presentation.
    assert!(!reasoning.transcript_lines(WIDTH).is_empty());
}

#[test]
fn focus_notice_names_state_and_transcript_key() {
    let (on_message, on_hint) = crate::focus::focus_notice(true);
    assert!(on_message.starts_with("Focus mode on"), "{on_message}");
    assert!(
        on_message.contains(crate::focus::HUSHDEX_BUILD),
        "notice identifies the hushdex build:\n{on_message}"
    );
    let hint = on_hint.expect("notice hints at the transcript key");
    assert!(hint.contains("full transcript"), "{hint}");

    let (off_message, _) = crate::focus::focus_notice(false);
    assert!(off_message.starts_with("Focus mode off"), "{off_message}");
    assert!(
        off_message.contains(crate::focus::HUSHDEX_BUILD),
        "notice identifies the hushdex build:\n{off_message}"
    );
}

#[tokio::test]
async fn app_scrollback_hook_selects_focus_lines_when_enabled() {
    let mut app = crate::app::test_support::make_test_app().await;
    let cells = fixture_cells();

    app.chat_widget.set_focus_mode(true);
    for cell in cells.all() {
        let focused = app.scrollback_cell_hyperlink_lines(cell, WIDTH);
        let expected = crate::terminal_hyperlinks::plain_hyperlink_lines(cell.focus_lines(WIDTH));
        assert_eq!(focused, expected);
    }

    app.chat_widget.set_focus_mode(false);
    for cell in cells.all() {
        let displayed = app.scrollback_cell_hyperlink_lines(cell, WIDTH);
        let expected = cell
            .display_hyperlink_lines_for_mode(WIDTH, crate::history_cell::HistoryRenderMode::Rich);
        assert_eq!(displayed, expected);
    }
}

// ---------------------------------------------------------------------------
// Drift guard: focus_allowlist.toml must classify every production HistoryCell.
// ---------------------------------------------------------------------------

const ALLOWLIST: &str = include_str!("../focus_allowlist.toml");

#[derive(serde::Deserialize)]
struct FocusAllowlist {
    cells: std::collections::BTreeMap<String, String>,
}

/// One `impl HistoryCell for <Type>` block found in production sources.
struct ImplBlock {
    type_name: String,
    has_focus_override: bool,
}

fn manifest_src_dir() -> PathBuf {
    codex_utils_cargo_bin::find_resource!("src").expect("crate src directory")
}

/// Recursively collect non-test `.rs` files under `dir`.
fn production_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = std::fs::read_dir(dir).expect("read crate source directory");
    for entry in entries {
        let path = entry.expect("read source entry").path();
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if path.is_dir() {
            if name == "tests" || name == "snapshots" {
                continue;
            }
            production_sources(&path, out);
            continue;
        }
        if name.ends_with(".rs")
            && name != "tests.rs"
            && !name.ends_with("_tests.rs")
            && !name.contains("test_support")
        {
            out.push(path);
        }
    }
}

/// Remove `#[cfg(test)] mod … { … }` bodies so inline test modules cannot introduce impls.
fn strip_cfg_test_blocks(source: &str) -> String {
    let mut kept = String::with_capacity(source.len());
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    while let Some(found) = source[cursor..].find("#[cfg(test)]") {
        let attribute_at = cursor + found;
        kept.push_str(&source[cursor..attribute_at]);
        // Skip the attribute itself plus any following attributes.
        let mut scan = attribute_at + "#[cfg(test)]".len();
        scan = skip_ws(source, scan);
        while scan < bytes.len() && bytes[scan] == b'#' {
            scan = skip_attribute(source, scan);
            scan = skip_ws(source, scan);
        }
        if source[scan..].starts_with("mod") {
            scan += "mod".len();
            scan = skip_ws(source, scan);
            // Skip the module name.
            while scan < bytes.len()
                && !bytes[scan].is_ascii_whitespace()
                && bytes[scan] != b';'
                && bytes[scan] != b'{'
            {
                scan += 1;
            }
            scan = skip_ws(source, scan);
        }
        if scan < bytes.len() && bytes[scan] == b'{' {
            // Drop the whole test module body; continue scanning after it.
            cursor = matching_brace(source, scan);
        } else {
            // Declaration form (`mod x;`) or unexpected shape: keep the marker and move on.
            kept.push_str("#[cfg(test)]");
            cursor = attribute_at + "#[cfg(test)]".len();
        }
    }
    kept.push_str(&source[cursor..]);
    kept
}

fn skip_ws(source: &str, mut index: usize) -> usize {
    let bytes = source.as_bytes();
    while index < bytes.len() && bytes[index].is_ascii_whitespace() {
        index += 1;
    }
    index
}

/// Skip one `#[…]` attribute and return the index just after its closing bracket.
fn skip_attribute(source: &str, mut index: usize) -> usize {
    let bytes = source.as_bytes();
    if index >= bytes.len() || bytes[index] != b'#' {
        return index;
    }
    index += 1;
    if index < bytes.len() && bytes[index] == b'!' {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b'[' {
        return matching_bracket(source, index);
    }
    index
}

/// Index just after the `}` matching the `{` at `open`.
fn matching_brace(source: &str, open: usize) -> usize {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    let mut in_string = false;
    let mut in_char = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_line_comment {
            if byte == b'\n' {
                in_line_comment = false;
            }
        } else if in_block_comment {
            if byte == b'*' && source[index..].starts_with("*/") {
                in_block_comment = false;
                index += 1;
            }
        } else if in_string {
            if byte == b'\\' {
                index += 1;
            } else if byte == b'"' {
                in_string = false;
            }
        } else if in_char {
            if byte == b'\\' {
                index += 1;
            } else if byte == b'\'' {
                in_char = false;
            }
        } else {
            match byte {
                b'/' if source[index..].starts_with("//") => {
                    in_line_comment = true;
                    index += 1;
                }
                b'/' if source[index..].starts_with("/*") => {
                    in_block_comment = true;
                    index += 1;
                }
                b'"' => in_string = true,
                b'\'' => {
                    // Distinguish char literals ('x', '\n') from lifetimes ('a, 'static).
                    let is_char_literal = match bytes.get(index + 1) {
                        Some(b'\\') => bytes.get(index + 3) == Some(&b'\''),
                        Some(_) => bytes.get(index + 2) == Some(&b'\''),
                        None => false,
                    };
                    if is_char_literal {
                        in_char = true;
                    }
                }
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return index + 1;
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    source.len()
}

/// Index just after the `]` matching the `[` at `open`.
fn matching_bracket(source: &str, open: usize) -> usize {
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    for (offset, byte) in bytes[open..].iter().enumerate() {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth -= 1;
                if depth == 0 {
                    return open + offset + 1;
                }
            }
            _ => {}
        }
    }
    source.len()
}

fn scan_impl_blocks(source: &str) -> Vec<ImplBlock> {
    let mut blocks = Vec::new();
    let needle = "HistoryCell for ";
    let mut search_from = 0usize;
    while let Some(found) = source[search_from..].find(needle) {
        let after = search_from + found + needle.len();
        let Some(open) = source[after..].find('{') else {
            break;
        };
        let brace = after + open;
        let type_name = source[after..brace].trim().to_string();
        let body_end = matching_brace(source, brace);
        let body = &source[brace..body_end];
        if !type_name.is_empty() {
            blocks.push(ImplBlock {
                type_name,
                has_focus_override: body.contains("fn focus_lines"),
            });
        }
        search_from = body_end;
    }
    blocks
}

#[test]
fn focus_allowlist_is_complete() {
    let mut sources = Vec::new();
    production_sources(&manifest_src_dir(), &mut sources);
    assert!(
        !sources.is_empty(),
        "expected to scan codex-tui sources for HistoryCell impls"
    );

    let mut impls: std::collections::BTreeMap<String, bool> = std::collections::BTreeMap::new();
    for path in &sources {
        let source = std::fs::read_to_string(path).expect("read source file");
        let source = strip_cfg_test_blocks(&source);
        for block in scan_impl_blocks(&source) {
            impls.insert(block.type_name, block.has_focus_override);
        }
    }
    assert!(
        !impls.is_empty(),
        "expected to find impl HistoryCell blocks in codex-tui sources"
    );

    let allowlist: FocusAllowlist = toml::from_str(ALLOWLIST).expect("parse focus_allowlist.toml");

    let mut failures = Vec::new();
    for (type_name, has_focus_override) in &impls {
        let Some(class) = allowlist.cells.get(type_name) else {
            failures.push(format!(
                "New HistoryCell type `{type_name}`: classify it in focus_allowlist.toml"
            ));
            continue;
        };
        match class.as_str() {
            "summary" | "hidden" => {
                if !has_focus_override {
                    failures.push(format!(
                        "`{type_name}` is classified `{class}` but has no `focus_lines` \
                         override; add one delegating into focus_summaries.rs"
                    ));
                }
            }
            "full" => {}
            other => failures.push(format!(
                "`{type_name}` has unknown class `{other}`; use `full`, `summary`, or `hidden`"
            )),
        }
    }
    for type_name in allowlist.cells.keys() {
        if !impls.contains_key(type_name) {
            failures.push(format!(
                "focus_allowlist.toml lists `{type_name}` but no `impl HistoryCell for \
                 {type_name}` exists in production sources; remove or rename the entry"
            ));
        }
    }
    for (type_name, required) in [
        ("UserHistoryCell", "full"),
        ("AgentMessageCell", "full"),
        ("AgentMarkdownCell", "full"),
    ] {
        if allowlist.cells.get(type_name) != Some(&required.to_string()) {
            failures.push(format!(
                "`{type_name}` must stay classified `{required}`: prompts and agent messages \
                 render in full in focus mode"
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "focus_allowlist.toml is out of sync with codex-tui sources:\n{}",
        failures.join("\n")
    );
}

// ---------------------------------------------------------------------------
// Drift guard: the codex binary must still launch this TUI crate.
// ---------------------------------------------------------------------------

fn cli_crate_dir() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut candidates = vec![manifest_dir.parent().expect("codex-rs parent").join("cli")];
    let mut current = std::env::current_dir().expect("current directory");
    while current.parent().is_some() {
        candidates.push(current.join("cli"));
        current = current.parent().expect("parent directory").to_path_buf();
    }
    candidates
        .into_iter()
        .find(|candidate| candidate.join("Cargo.toml").is_file())
        .unwrap_or_else(|| {
            panic!(
                "Default TUI changed: manual port required — could not locate the `cli` crate \
                 next to codex-tui; see FORK_NOTES.md"
            )
        })
}

#[test]
fn default_tui_guard() {
    let cli = cli_crate_dir();
    let cargo_toml = std::fs::read_to_string(cli.join("Cargo.toml")).expect("read cli/Cargo.toml");
    assert!(
        cargo_toml.contains("codex-tui"),
        "Default TUI changed: manual port required — cli/Cargo.toml no longer depends on \
         codex-tui; see FORK_NOTES.md"
    );
    let main_rs = std::fs::read_to_string(cli.join("src/main.rs")).expect("read cli/src/main.rs");
    assert!(
        main_rs.contains("run_interactive_tui"),
        "Default TUI changed: manual port required — cli/src/main.rs no longer launches \
         codex-tui through run_interactive_tui; see FORK_NOTES.md"
    );
}
