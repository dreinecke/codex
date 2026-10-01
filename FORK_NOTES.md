# Hushdex fork notes

Hushdex = stock Codex CLI + a **focus mode** that condenses the live transcript to the user's
prompts, one-line summaries of tool activity (with diffstats for edits), and all agent messages.
This file records the fork's base, the exact upstream touch points, and the checks that make
automated rebases safe. Read it top to bottom before rebasing.

## Base release and default TUI

- **Base tag: `rust-v0.159.2`** (latest stable upstream release at fork time).
- **Default TUI crate: `codex-rs/tui` (crate `codex-tui`).** The `codex` binary lives in
  `codex-rs/cli` and launches the TUI through `run_interactive_tui` (see
  `codex-rs/cli/src/main.rs`). Upstream's older parallel TUIs (`tui2`, `tui_app_server`) no
  longer exist in this release; `codex-tui` is the only TUI crate. The drift-guard test
  `default_tui_guard` in `tui/src/focus_tests.rs` fails loudly if this ever changes.

## Architecture findings (Phase 0 recon)

The plan's assumptions held, with these precise locations:

1. **`HistoryCell` trait** lives in `codex-rs/tui/src/history_cell/mod.rs` (a module directory,
   not a single `history_cell.rs`). It exposes `display_lines(&self, width: u16) ->
   Vec<Line<'static>>` as expected.
2. **Scrollback insertion**: `codex-rs/tui/src/insert_history.rs` writes pre-rendered lines into
   terminal scrollback above the live viewport. Cells become lines one step earlier: every
   scrollback path funnels through `HistoryCell::display_hyperlink_lines_for_mode(width, mode)`.
   There are **seven call sites** (all in `impl App` methods), not one:
   - `tui/src/app/history_ui.rs` — `render_inserted_history_cell` (inserted-cell bookkeeping),
     `refresh_thread_usage_history_tail` (tail replace comparison).
   - `tui/src/app/resize_reflow.rs` — `display_lines_for_history_insert` (the incremental
     insertion point), `render_transcript_lines_for_reflow` ×2 (resize/reflow rebuild), and
     `reflow_transcript_now` ×2 (post-reflow tail bookkeeping).
   Hushdex routes all seven through one helper, `App::scrollback_cell_hyperlink_lines` in
   `tui/src/focus.rs`, so insert-time rendering, resize reflow, and tail-repair always agree.
3. **Transcript pager**: `tui/src/pager_overlay.rs` + `pager_overlay/transcript.rs` render full
   history from stored cells via the `transcript_lines`/`transcript_hyperlink_lines` trait
   methods (opened with the `open_transcript` keybinding, default `Ctrl+T`). Hushdex never
   touches those methods, so the pager stays the unfiltered "show everything" view.
4. **Notable upstream behavior**: completed reasoning is already `transcript_only` upstream
   (`new_reasoning_summary_block`), i.e. hidden from scrollback in stock Codex; the transcript
   pager still shows it. Hushdex's `hidden` classification for `ReasoningSummaryCell` makes this
   an explicit, guarded invariant instead of an implicit one.
5. Upstream test file `tui/tests/suite/focus_palette.rs` is about *terminal focus events* —
   unrelated to Hushdex focus mode, despite the name.

## Every upstream file touched

| File | Reason |
| --- | --- |
| `codex-rs/tui/src/lib.rs` | Register `mod focus;` (+1 line). |
| `codex-rs/tui/src/history_cell/mod.rs` | Add `focus_lines` default + `focus_activity_counts` default to `HistoryCell`. |
| `codex-rs/tui/src/history_cell/messages.rs` | `ReasoningSummaryCell::focus_lines` → hidden. |
| `codex-rs/tui/src/history_cell/mcp.rs` | `McpToolCallCell::focus_lines` → summary. |
| `codex-rs/tui/src/history_cell/dynamic.rs` | `DynamicToolCallCell::focus_lines` → summary. |
| `codex-rs/tui/src/history_cell/search.rs` | `WebSearchCell::focus_lines` → summary. |
| `codex-rs/tui/src/history_cell/patches.rs` | `PatchHistoryCell::focus_lines` → diffstat summary. |
| `codex-rs/tui/src/history_cell/computer_activity.rs` | `ComputerActivityCell::focus_lines` → summary. |
| `codex-rs/tui/src/exec_cell/render.rs` | `ExecCell::focus_lines` → command summary. |
| `codex-rs/tui/src/app/history_ui.rs` | 2 call sites route through the focus-aware helper; insertion absorbs into aggregate runs. |
| `codex-rs/tui/src/app.rs` | Register `mod focus_aggregate;` (+1 line). |
| `codex-rs/tui/src/app/native_history.rs` | `NativeHistory::is_empty` guard for aggregation (+3 lines). |
| `codex-rs/tui/src/app/resize_reflow.rs` | 5 call sites route through the focus-aware helper. |
| `codex-rs/tui/src/app/owned_transcript.rs` | Per-frame sync of focus state into the owned transcript view (+1 line). |
| `codex-rs/tui/src/transcript_view.rs` | `focus` field + `set_focus_mode` (cache-invalidating) + fork test module. |
| `codex-rs/tui/src/transcript_view/layout.rs` | `CellPresentation.focus` cache key; committed cells render `focus_lines` when focused. |
| `codex-rs/tui/src/transcript_view/layout_tests.rs` | `focus: false` in `CellPresentation` test literals (+3 lines). |
| `codex-rs/tui/src/chatwidget.rs` | `focus_mode: bool` field on `ChatWidget` (+1 line). |
| `codex-rs/tui/src/chatwidget/constructor.rs` | Initialize `focus_mode` from `focus::default_enabled()` (+1 line). |
| `codex-rs/tui/src/slash_command.rs` | `Focus` variant + description + availability lists. |
| `codex-rs/tui/src/chatwidget/slash_dispatch.rs` | `SlashCommand::Focus` arm (toggle + notice). |
| `codex-rs/tui/src/bottom_pane/chat_composer.rs` | Allow `/focus` while viewing a parent-owned child thread (+1 line). |
| `codex-rs/tui/src/bottom_pane/slash_commands.rs` | Add `Focus` to the side-conversation command-list test (+1 line). |
| `codex-rs/tui/src/multi_agents.rs` | Wrap normal sub-agent lifecycle events in the focus-hidden decorator (+9 lines). |
| `codex-rs/tui/src/chatwidget/tool_lifecycle.rs` | `on_collab_event` takes a boxed cell (+2 lines). |
| `codex-rs/tui/src/thread_transcript/other_items.rs` | Adapt to the boxed lifecycle cell (+1 line). |
| `codex-rs/tui/src/history_cell/messages.rs` | User prompts render as the accented user band: bar prefix spans replace the `›` chevron, guaranteed fill, band framing via `user_band` (+12 lines). |
| `codex-rs/tui/src/style.rs` | `deterministic_accent_on` pins the color level so the band renders identically under every test runner (+20 lines). |
| `codex-rs/tui/src/transcript_view/prompt_header.rs` | The pinned prompt header mirrors the banded prompt instead of its own chevron (+4 lines). |
| `codex-rs/tui/src/bottom_pane/chat_composer.rs` | Composer frame renders as the user band: fill fallback plus an accent bar down `composer_rect`'s left edge; the plain-state prompt glyph is the bar itself (+10 lines, doc line updated per bottom-pane AGENTS). |
| `codex-rs/tui/BUILD.bazel` | Declare `focus_allowlist.toml` as compile data for `include_str!`. |

New files (fork-owned): `codex-rs/tui/src/focus.rs`, `codex-rs/tui/src/focus_summaries.rs`,
`codex-rs/tui/src/focus_tests.rs`, `codex-rs/tui/src/app/focus_aggregate.rs`,
`codex-rs/tui/src/user_band.rs` (accented user band for prompts and the composer),

Upstream test adjustments (assertion wording only, same intent): `history_cell/messages_tests.rs`
and `history_cell/tests.rs` (bar prefix replaces the chevron/gutter; band frame rows count as
blank once the bar is stripped), `insert_history.rs` tests (same), `chatwidget/realtime_tests/transcripts.rs`
(spoken marker is the red bar), `app/tests/new_session_tests.rs` (composer window widened by one
banded context row), plus inline-snapshot updates in `agents_overview_tests.rs`,
`background_task_defaults_tests.rs`, `key_chords.rs`, `startup_defaults_tests.rs`,
`turn_submission.rs`, `disconnect_tests.rs`, `request_user_input/mod.rs`,
`recording_controls_tests.rs`, and `replay_render_tests.rs`.

Testing notes:
- Band snapshots were converged under the canonical runner (`just test`, i.e. nextest, one
  process per test). `cargo test --lib` (threaded) can flake on band-bearing snapshots because
  `with_test_default_colors` leaves a sticky thread-local on pooled test threads, changing
  fills for tests that do not declare colors; nextest is unaffected and is what CI runs.
- `deterministic_accent_on` in `style.rs` pins the band bar's color level for the same reason.
- Full parallel runs on a loaded machine flake a large app-server/PTY family at baseline
  (816 failures at the pre-band HEAD in identical conditions); run affected modules scoped.


`codex-rs/tui/src/app/focus_aggregate_tests.rs`,
`codex-rs/tui/src/transcript_view/focus_layout_tests.rs`,
`codex-rs/tui/focus_allowlist.toml`,
`codex-rs/tui/src/snapshots/codex_tui__focus__tests__*.snap` (generated),
`scripts/install-hushdex.sh`, `FORK_NOTES.md` (this file).

## Classification table

`focus_allowlist.toml` (in `codex-rs/tui/`) is the source of truth; it maps every production
`HistoryCell` type to one class. A `cargo test` guard (`focus_allowlist_is_complete`) fails if a
type is missing, stale, lacks an override, or if the user/agent message types are not `full`.

### `summary` (one-line-per-item; failures always fall back to full output)

| Type | Focus rendering |
| --- | --- |
| `ExecCell` | Standalone: `• Ran <command>` per call (truncated to width); running calls show `• Running <command>`. Hard failures (exit ≥ 2, interrupted) render the full transcript form; benign exit 1 from read-only commands stays condensed (see the failure rule below). User `!` shell commands render in full — the user asked for that output directly. Consecutive absorbable cells merge into `FocusAggregateCell` runs (see below). |
| `McpToolCallCell` | `• Called <server>.<tool>`; errored/is-error results render in full. |
| `DynamicToolCallCell` | `• Called <namespace>.<tool>`; failed/interrupted results render in full. |
| `WebSearchCell` | First line of normal display (already a one-line "Searched the web for …"). |
| `ViewImageHistoryCell` | Standalone: one `• Viewed image <name>` line; absorbable into runs as `viewed N images`. |
| `PatchHistoryCell` | `• Edited <path> (+A −D)` per file, `• Added <path>` / `• Deleted <path>` for new/removed files; caps at 5 files then `• …and N more files`. |
| `ComputerActivityCell` | `• Used computer · N actions` (+ failure count); any failed action renders in full. |

### `hidden`

| Type | Focus rendering |
| --- | --- |
| `ReasoningSummaryCell` | Nothing in scrollback (matches upstream's completed-reasoning behavior; the transcript pager still shows it). |
| `FocusHiddenHistoryCell` | The lifecycle-telemetry wrapper (fork-owned): sub-agent `Started`/`Interacted with`/`Completed` events render nothing in focus mode — normal-case lifecycle state carries no signal. `Interrupted` stays visible; the pager and focus-off keep everything. |
| `UserHistoryCell` | Full, rendered as the user band (fork-owned `user_band` module): an accent `▌` bar down the left edge of every row (spoken prompts use a red bar instead of the old red chevron) and a full-width tint fill. Agent messages render with no background, so user content is visually distinct. |
| `FocusAggregateCell` | The run-aggregation cell (fork-owned): one dim Claude-Code-style line, e.g. `Searched for 2 patterns, read 5 files, edited 3 files, called 1 tool, ran 9 shell commands`. |

### `full` (render unchanged; no `focus_lines` override needed)

`UserHistoryCell`, `AgentMessageCell`, `AgentMarkdownCell`, `StreamingAgentTailCell`,
`StreamingPlanTailCell`, `ProposedPlanCell`, `ProposedPlanStreamCell`, `PlanUpdateCell`,
`PlainHistoryCell`, `WebHyperlinkHistoryCell`, `PrefixedWrappedHistoryCell` (all approval /
denial / review cells), `CompositeHistoryCell`, `WarningHistoryCell`, `StartupWarningsCell`,
`FinalMessageSeparator`, `RequestUserInputResultCell`,
`UpdateAvailableHistoryCell`, `SafetyAccessBlockCell`, `DeprecationNoticeCell`,
`ThreadRecapLoadingCell`, `ThreadRecapHistoryCell`, `TooltipHistoryCell`, `SessionNoticeCell`,
`SessionInfoCell`, `SessionHeaderHistoryCell`, `McpInventoryLoadingCell`, `HookCell`,
`AgentStatusHistoryCell`, `Arc<StatusHistoryCell>`, `SplitFlapTranscriptCell`,
`UnifiedExecInteractionCell`, `UnifiedExecProcessesCell`, `PatchFailureCell` (failures are never
condensed).

## Behavior notes

- **Build identity**: `HUSHDEX_BUILD` in `tui/src/focus.rs` (format `YYYY-MM-DD.N`) identifies the
  fork build; it prints in the `/focus` notice because `--version` reports only the upstream
  number. Bump it in every fork commit that changes behavior, and record the mapping in the
  commit message.
- Focus mode is **on by default**; `HUSHDEX_FOCUS=0` starts a session with focus off; `/focus`
  toggles it for the session and prints an info line naming the transcript-overlay key.
- The toggle affects only cells inserted afterward; already-printed scrollback is not re-rendered
  (a resize reflow rebuild re-renders everything, and it applies the current focus state so the
  rebuilt scrollback stays consistent).
- Cells whose `focus_lines` is empty are skipped entirely, including the blank separator line —
  the existing empty-line machinery in `display_lines_for_history_insert` already does this once
  the lines are empty.
- **Failure rendering is tiered by exit code.** Exit ≥ 2 (and interrupted work) always renders
  in the full transcript form with the exit code. Exit 1 is treated as benign — "no matches" —
  and stays condensed when the command is read-only: parsed reads/searches, or a script led by a
  read-only command from `BENIGN_EXIT_1_LEADS` in `focus_summaries.rs` (cat/rg/grep/tail/git/…,
  skipping `cd` segments). Exit 1 from anything else (pytest, cargo, …) renders in full.
- **Fullscreen (owned-screen) mode — the TUI default — condenses too.** The owned transcript
  renders committed cells through `TranscriptView::current_layout` in
  `tui/src/transcript_view/layout.rs`; when focus is on, those cells render `focus_lines`
  instead of the compact/retained presentation, and disclosure affordances
  (`+ Show details` / `+ N lines`) are suppressed. The live tail (in-flight cell), detailed
  browsing (the details mode), raw output mode, and the `Ctrl+T` overlay keep full rendering.
  The `focus_layout_tests` module pins this behavior and fails loudly if upstream moves the
  layout path.
- **Run aggregation (Claude Code style).** Consecutive absorbable tool cells merge into one
  `FocusAggregateCell` (`tui/src/app/focus_aggregate.rs`) that renders a single dim line with
  per-category counts. Absorption happens in `App::insert_history_cell`; merging rewrites the
  last scrollback block in place via upstream's `replace_visible_history_tail`, so a growing
  run stays one line. Cells report their counts through the `focus_activity_counts` trait
  method. Failures, user `!` commands, agent messages, prompts, and anything unclassified close
  the run and render normally; hidden reasoning cells do not interrupt it. The `Ctrl+T` pager
  receives every original cell, and turning focus off expands aggregates back to the full
  presentations. Aggregates do not survive session resume (rebuilt transcripts render per-cell
  summaries). Stream-continuation fragments and focus-hidden cells do not close a run, so a
  message streamed in fragments or interleaved lifecycle telemetry keeps one aggregate line.
- Raw output mode (`/raw`) wins over focus mode: raw mode is for verbatim terminal selection, so
  focus condensation is suspended while it is active.
- The live viewport (in-progress cells) keeps its normal rendering; the one-line summaries apply
  when the finished cell is committed to scrollback. In-progress work stays visible either way.
- No `config.toml` keys were added. `HUSHDEX_FOCUS` is read once per session at ChatWidget
  construction.
- Test builds (`cfg!(test)`) default focus **off** so upstream unit tests keep asserting full
  scrollback output; Hushdex's own tests set the state explicitly. Integration tests that spawn
  the real binary run with focus on (the product default) — they assert agent text and UI
  labels, which are always rendered in full.

## Running the checks

From `codex-rs/` (requires `just`, per repo AGENTS.md):

```sh
just fmt                 # after any code change
just test -p codex-tui   # unit + integration tests, incl. all drift guards below
just fix -p codex-tui    # before finalizing (do not re-run tests after fix/fmt)
```

**Known pre-existing failures at release tags:** checking out a `rust-vX.Y.Z` tag builds with the
workspace version from that tag (e.g. `0.159.2`), but the committed insta snapshots record
`v0.0.0` (upstream main always carries `0.0.0`). About 38 snapshot tests therefore fail on the
pristine tag itself — before and after the Hushdex patch, with the identical diff
(`>_ OpenAI Codex (v0.0.0)` → `v0.159.2`). Do not accept those snapshots into the fork (it would
bloat the patch and conflict on every rebase); instead verify the failing set matches the
pristine-tag baseline and that no *new* failures appear. Unit tests also run with focus off by
default (see above), so the Hushdex hook is a no-op in them.

Snapshot workflow (if a snapshot intentionally changes): `just test -p codex-tui`, review
`*.snap.new`, then `cargo insta accept -p codex-tui`.

Drift guards included in `just test -p codex-tui`:

1. `focus_allowlist_is_complete` — scans non-test crate sources for every
   `impl HistoryCell for <Type>`; fails with exact instructions if the allowlist is missing a
   type, names a stale type, a `summary`/`hidden` type lacks a `focus_lines` override, or the
   user/agent message types are not `full`.
2. `default_tui_guard` — fails with "Default TUI changed: manual port required" if the `codex`
   binary no longer depends on `codex-tui` or no longer launches it via
   `run_interactive_tui`.
3. Snapshot + assertion tests over a fixture transcript (user message, reasoning, agent message
   between tool calls, successful command, failed command, multi-file patch, successful MCP
   call, failed MCP call, final agent message) at width 80 with focus on and off; assertions
   check agent text appears verbatim, reasoning text is absent, and failed commands show their
   exit code.

## On rebase

1. **Update the base tag**: note the new `rust-vX.Y.Z` at the top of this file (and in the
   release notes of the rebase commit).
2. **Resolve conflicts** in the files listed above. Upstream edits are tiny by design; prefer
   re-applying the one-line hooks over merging hunk-by-hunk. All real logic is in
   `focus.rs` / `focus_summaries.rs` / `focus_tests.rs`, which should never conflict.
3. **Run the tests**: `just fmt && just test -p codex-tui`, then the full suite if core crates
   changed (`just test`, ask first per AGENTS.md). Compare failures against the pristine-tag
   baseline described under "Running the checks": the version-string snapshot failures are
   expected; any *new* failure is a real regression from the rebase.
4. **Classify any new cell types**: the `focus_allowlist_is_complete` guard prints exactly what
   to do, e.g. "New HistoryCell type `FooCell`: classify it in `focus_allowlist.toml`". Add a
   `focus_lines` override (one-line delegation into `focus_summaries.rs`) for `summary`/`hidden`
   types; update the fixture/snapshot tests to cover the new cell if it is user-visible.
5. **Stop for human review** if `default_tui_guard` fails ("Default TUI changed: manual port
   required") — that means upstream switched default TUIs or renamed the launch path, and the
   focus hooks must be re-ported by hand to the new insertion points before the rebase can
   proceed.
