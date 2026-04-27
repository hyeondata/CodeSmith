# CodeSmith Architecture

CodeSmith v1 is an execution-only local agent. The GUI remains in the workspace, but the active architecture is CLI-first: the command line owns new chat, tool execution, ingest, query, lint, and local wiki workflows while the approval boundary stays unchanged.

## Current Workspace

- `crates/app`: native `eframe` entry point and default window sizing.
- `crates/cli`: primary runtime for local LLM chat, settings, command proposals, ingest/query/lint/log, doctor checks, and wiki inspection.
- `crates/ui`: egui shell, Codex-style three-panel layout, settings UI, command approval flow, run log actions, CJK font fallback.
- `crates/core`: shared settings, chat, command proposal, command run, policy, wiki, and event types.
- `crates/llm`: OpenAI-compatible local chat completion client.
- `crates/agent`: strict JSON command proposal parser. Malformed JSON is treated as assistant text.
- `crates/policy`: workspace boundary and destructive command checks.
- `crates/runner`: approved shell command execution with streamed stdout/stderr and timeout handling.
- `crates/storage`: settings, session metadata, transcripts, command run persistence, and CLI-first wiki metadata.
- `crates/wiki`: `raw/`, `wiki/`, `schema/`, `index.md`, `log.md`, Markdown frontmatter, ingest, lint, and lightweight term-frequency search.

## CLI Runtime Flow

1. The user starts `codesmith-cli chat` or runs a headless CLI subcommand.
2. `chat` verifies workspace trust before interactive LLM prompts or command approvals.
3. The CLI expands `@workspace` and workspace-scoped `@file:` prompt context.
4. `wiki` assembles `index.md` plus matching wiki pages as local context.
5. `llm` streams from the configured OpenAI-compatible local endpoint.
6. `agent` parses strict JSON command proposals; normal text stays assistant text.
7. `policy` blocks commands outside the workspace or matching destructive patterns.
8. The CLI asks for explicit approval before `runner` spawns any command.
9. `runner` streams stdout/stderr, applies timeout handling, and returns status.
10. `storage` persists transcripts, command runs, source metadata, ingest jobs, and wiki page metadata.

## Legacy GUI Runtime Flow

1. The user enters a prompt in the central chat composer.
2. `ui` loads relevant wiki context and calls `llm`.
3. `agent` parses the assistant output:
   - strict JSON with `command`, `cwd`, and `reason` becomes a `CommandProposal`;
   - anything else becomes a normal assistant message.
4. `policy` evaluates the proposal against the configured workspace and deny patterns.
5. The right `Activity` panel shows the raw command and cwd before any action.
6. The command runs only after the user clicks `Approve`.
7. Approved or rejected proposals are removed from the pending list so the same proposal is not accidentally reused.
8. `runner` streams stdout/stderr back to `ui`.
9. Finished runs are persisted and can be copied, saved to wiki, or used as a follow-up prompt.

## PRD Fit Check

Implemented:

- Cargo workspace split into `app`, `ui`, `core`, `llm`, `agent`, `runner`, `policy`, `wiki`, and `storage`.
- `codesmith-cli` binary for headless local LLM, Claude Code-like interactive chat, settings, command proposal, doctor, and wiki flows.
- Stable Rust toolchain pin and stable crate versions in the workspace manifest.
- Native `eframe/egui` app with a Codex-style three-panel layout.
- Local OpenAI-compatible LLM client using `/v1/chat/completions` streaming.
- Ollama-compatible default endpoint support through configurable settings.
- Strict JSON command proposal parsing.
- Approval-before-execution boundary for all commands.
- Workspace-scoped policy checks and destructive-command blocking.
- `tokio::process::Command` runner with stdout/stderr streaming and timeout handling.
- Settings persisted under `~/.codesmith/settings.toml`.
- Session metadata and command runs persisted in SQLite.
- Chat transcript persisted as JSONL.
- Wiki pages persisted as Markdown with YAML frontmatter under `~/.codesmith/wiki`.
- App restart restores the prior transcript and command runs.
- CLI commands implemented: `chat`, `-p/--print`, `proposal --json`, `doctor`, `wiki list`, and `wiki search`.
- CLI-first wiki commands implemented: `ingest file`, `ingest folder`, `query`, `lint wiki`, `log recent`, and `sources`.

Partially implemented:

- Local wiki retrieval is connected to the agent prompt path, but the search implementation is a lightweight term-frequency scorer with title bonus, not `tantivy` BM25.
- Raw ingest, `index.md`, `log.md`, SQLite source metadata, wiki lint, and query context exist for CLI-first workflows. Full normalize/extract/integrate conflict handling is not implemented yet.
- Run actions exist for copy, save-to-wiki, and ask-follow-up. Broader per-message actions are still pending.
- Command cancellation support exists in the runner design through timeout handling, but there is no user-facing cancel button yet.

Not implemented in v1 yet:

- Full LLM wiki pipeline: normalization, extraction, integration, conflict state workflow, and LLM-reviewed wiki write proposals.
- Tantivy-backed index.
- MCP client/server support.
- Embedded inference.
- Multi-agent execution.
- File editing, Git commit/push/PR automation, and remote execution.

## Approval Boundary

All commands require explicit user approval in v1. The GUI may display proposals, but it must not spawn a process before the approval action. Policy-blocked commands keep the approve button disabled.

The current deny policy covers destructive or privileged patterns including recursive deletion, `sudo`, recursive ownership/permission changes, disk formatting, credential reads, and common exfiltration tools. Commands outside the configured workspace are blocked.

## Local Data

- Settings: `~/.codesmith/settings.toml`
- SQLite metadata: `~/.codesmith/codesmith.sqlite3`
- JSONL transcripts: under `~/.codesmith/sessions`
- Wiki pages: under `~/.codesmith/wiki`
- Raw source snapshots: under `~/.codesmith/raw`
- Wiki schema notes: under `~/.codesmith/schema`
- Wiki navigation: `~/.codesmith/index.md`
- Operation log: `~/.codesmith/log.md`

The app default profile is Ollama-compatible:

- Base URL: `http://localhost:11434/v1`
- API key: optional
- Model: configured in the settings panel

## UI Architecture

The UI follows a Codex-like desktop layout:

- Left sidebar: product identity, session entry, local LLM settings, workspace, timeout, save/test actions.
- Center panel: chat transcript and bottom prompt composer.
- Right panel: command proposals, run logs, wiki context.
- Bottom status bar: current state and selected model.

`crates/ui` installs a dark egui theme and registers macOS/Linux/Windows CJK fallback fonts so Korean and mixed symbols render correctly.

The chat transcript scroll area reserves fixed vertical space for the composer. This keeps the input visible and focusable even when restored transcripts contain long assistant messages.

When a prompt is submitted and the app is waiting for the local LLM, the transcript shows an inline spinner row with `CodeSmith is generating a response...` in addition to the bottom status bar state. The indicator is status-driven and disappears when the assistant response, command proposal, or error event arrives.

The `Command proposals` section is pending-only. Approved or rejected proposals are recorded under `Runs`, so an empty proposal section after approval is expected. The `Wiki` section distinguishes current-prompt retrieved context from saved wiki pages; saved pages remain visible even when the current prompt has no matching context.

## Manual Tool Verification

Manual verification was run through the macOS app with Computer Use on 2026-04-25.

Verified:

- Ollama connection test returned `Connection OK`.
- A local LLM response produced this command proposal:

```json
{"command":"printf 'tool-ok\\n'","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"verify approved command runner"}
```

- The proposal appeared in the `Activity` panel with raw command and cwd visible.
- Clicking `Approve` executed the command.
- The run finished as `Succeeded`, exit code `Some(0)`, stdout `tool-ok`, empty stderr.
- `Save to wiki` showed `Saved run to wiki`.
- `Ask follow-up` populated the composer with a follow-up prompt based on `tool-ok`.
- `Copy` showed `Copied run output`.
- Restarting the app restored the prior transcript and the saved successful command run.

Browser Use was also used on 2026-04-25 to open this local architecture document from `file:///Users/gim-yonghyeon/CodeSmith/docs/architecture.md` and verify that the documentation is readable from the in-app browser.

Current accessibility note: the egui composer is visible and works for normal use, but Computer Use `set_value` did not reliably inject text into the composer during this review. Earlier typed prompts and clicks through the same app did verify the LLM, proposal, approval, run, and wiki-save path.

Follow-up Computer Use debugging on 2026-04-25 found that long restored transcripts could visually push the composer out of the usable area. The UI now caps the transcript scroll height before rendering the composer. After the fix, direct keyboard input through Computer Use succeeded:

- Typed `Return exactly OK-INPUT-DEBUG` into the composer.
- Clicked `Send`.
- The user message appeared in the transcript.
- The local LLM responded with `OK-INPUT-DEBUG`.
- Typed a strict JSON proposal for `printf 'debug-run-ok\n'`.
- The proposal appeared in `Activity`.
- Clicking `Approve` executed the command.
- The run finished with exit code `Some(0)`, stdout `debug-run-ok`, and empty stderr.

Follow-up UI debugging on 2026-04-25 added the response-generation indicator. Computer Use verified the flow by typing `Return exactly SLOW-WAIT-DEBUG`, clicking `Send`, observing `CodeSmith is generating a response...` while the local LLM was pending, and then observing the final assistant response `SLOW-WAIT-DEBUG`.

Follow-up Activity panel debugging on 2026-04-25 found two visibility issues:

- Saved wiki pages existed on disk under `~/.codesmith/wiki`, but the UI only showed current-prompt context, so it displayed `No context loaded` even when a saved page existed. The UI now shows `Saved pages` separately.
- Wiki retrieval was called after clearing the composer, so searches used an empty query. The app now searches wiki with the submitted prompt before clearing it. Computer Use verified this by submitting `tool-ok` and observing `Context loaded for current prompt` with `Command: printf 'tool-ok\n'`.

CLI debugging on 2026-04-25 compared CodeSmith with the installed Claude Code CLI (`/opt/homebrew/bin/claude`, version `2.1.119`) and added a narrower execution-only CLI. Reference notes are in `docs/research/claude-claurst-cli-notes.md`.

Verified CLI commands:

```bash
cargo run -p codesmith-cli -- proposal --json '{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"debug cli runner"}'
cargo run -p codesmith-cli -- proposal --json '{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"debug cli runner"}' --yes
cargo run -p codesmith-cli -- proposal --json '{"command":"rm -rf target","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"debug block"}' --yes
cargo run -p codesmith-cli -- -p "Return exactly CLI-OK"
cargo run -p codesmith-cli -- doctor
cargo run -p codesmith-cli -- wiki list
cargo run -p codesmith-cli -- wiki search tool-ok
```

Results:

- Safe proposals without `--yes` print the approval-required message and do not execute.
- Safe proposals with `--yes` execute through the existing runner and captured stdout `cli-ok`.
- Blocked proposals stay blocked even with `--yes`.
- `-p` mode connected to the local Ollama-compatible endpoint and returned `CLI-OK`.
- `doctor` reported `Connection OK`.
- `wiki list` and `wiki search tool-ok` showed the saved `Command: printf 'tool-ok\n'` page.

Interactive CLI debugging on 2026-04-25 added `codesmith-cli chat`, a terminal REPL closer to Claude Code while staying execution-only.

Interactive commands:

```text
/help
/prompts
/settings
/set base-url <url>
/set model <name>
/set api-key <key|none>
/set workspace <path>
/set timeout <seconds>
/ingest file <path>
/ingest folder <path>
/query <question>
/lint wiki
/log recent
/sources
/doctor
/wiki list
/wiki search <query>
/exit
```

Verified interactive CLI flows:

```bash
cargo run -p codesmith-cli -- chat
printf '/help\n/settings\n/wiki list\n/exit\n' | cargo run -p codesmith-cli -- chat
printf 'Return exactly CHAT-OK\n/exit\n' | cargo run -p codesmith-cli -- chat
printf 'Return exactly this JSON and nothing else: {"command":"printf chat-run-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"interactive cli approval debug"}\ny\n/exit\n' | cargo run -p codesmith-cli -- chat
```

Results:

- `/help`, `/settings`, `/wiki list`, and `/exit` worked in the REPL.
- `/set timeout 121` updated `~/.codesmith/settings.toml`; `/set timeout 120` restored the original timeout.
- General chat showed `CodeSmith is generating a response...` and returned `CHAT-OK` from the local Ollama-compatible endpoint.
- A model response containing a fenced ` ```json ` command proposal was accepted by the CLI layer and converted into a command proposal. The strict `agent` parser remains unchanged; the tolerance is limited to CLI interaction.
- The interactive approval prompt required `y` before execution. After approval, `printf chat-run-ok` finished with status `Succeeded`, exit code `Some(0)`, stdout `chat-run-ok`, and empty stderr.

Follow-up interactive CLI work on 2026-04-25 added a Claude Code-like workspace trust gate and lightweight prompt helpers.

Workspace trust:

- `codesmith-cli chat` checks `~/.codesmith/trusted-workspaces.txt` before allowing interactive LLM prompts or command approvals.
- If the configured workspace is not trusted, the CLI asks: `Trust this workspace?`.
- The user must type `yes` or `y`; otherwise chat exits before LLM or command execution paths are available.
- Trust is persisted by canonical workspace path.

Prompt helpers:

- `/prompts` prints recommended prompts for project inspection, read-only diagnostics, file explanation, and wiki-aware answers.
- `@workspace` expands to the trusted workspace path in the prompt context.
- `@file:<relative-path>` attaches a UTF-8 text file from inside the trusted workspace.
- `@file` paths are canonicalized and blocked if they escape the workspace.
- File attachments are truncated at 12,000 bytes.

CLI-first wiki smoke debugging on 2026-04-26 verified the new ingest/query/lint/log/source commands against the local `~/.codesmith` store.

Verified flow:

- `cargo run -p codesmith-cli -- ingest file Cargo.toml` wrote a raw source snapshot, SQLite source metadata, `index.md`, and `log.md`.
- `cargo run -p codesmith-cli -- ingest folder crates/core` recursively ingested supported Rust/TOML files and skipped unsupported files.
- `cargo run -p codesmith-cli -- query "cargo workspace"` assembled `index.md` plus matching wiki pages inside the context budget.
- `cargo run -p codesmith-cli -- sources` listed the ingested source hashes and paths from SQLite metadata.
- `cargo run -p codesmith-cli -- log recent` printed parseable operation log entries.
- `cargo run -p codesmith-cli -- lint wiki` detected historical duplicate `Source: Cargo.toml` titles from earlier smoke data. New source summary pages now include a short hash suffix such as `Source: README.md (9302e2a9)` to avoid new title collisions.
- Interactive `codesmith-cli chat` accepted `/help`, `/sources`, `/query`, `/log recent`, and `/exit` in one piped smoke session.

CLI tool smoke debugging on 2026-04-25 verified that the CLI can act as an approval-gated local tool runner, not just a text interface.

Verified flow:

- Prepared ignored workspace `target/tool-smoke` through `codesmith-cli proposal --yes`.
- Generated `target/tool-smoke/gugudan.py` through an approved command proposal.
- Ran `python3 target/tool-smoke/gugudan.py` through the CLI runner and captured stdout from `2 x 1 = 2` through `2 x 9 = 18`.
- Generated an intentionally broken Python file, ran it, and captured `status: Failed`, `exit: Some(1)`, and `SyntaxError` in stderr.
- Rewrote the broken file with the missing `:` fixed, reran it, and captured successful 구구단 output.
- Verified `rm -rf target/tool-smoke` stayed blocked even with `--yes`; the smoke files remained present after the blocked proposal.

## Verification Commands

Use these before claiming the app is working:

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```
