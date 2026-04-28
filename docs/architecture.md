# CodeSmith Architecture

CodeSmith is now CLI-only on `main`. The archived `egui` desktop shell is preserved on the pushed `archive-gui-egui` branch, while active development happens in `codesmith-cli`.

## Workspace

- `crates/cli`: Rich REPL, headless commands, settings, model profiles, command proposal approval, wiki commands, and doctor checks.
- `crates/core`: shared settings, chat, command proposal, command run, policy, wiki, and event types.
- `crates/llm`: OpenAI-compatible local chat completion client.
- `crates/agent`: assistant output parser. Strict JSON and embedded JSON command proposals are supported; malformed JSON remains assistant text.
- `crates/policy`: workspace boundary and destructive-command checks.
- `crates/runner`: approved shell command execution with stdout/stderr capture and timeout handling.
- `crates/storage`: settings, session metadata, transcripts, command run persistence, source records, ingest jobs, and wiki metadata.
- `crates/wiki`: `raw/`, `wiki/`, `schema/`, `index.md`, `log.md`, Markdown frontmatter, ingest, lint, and lightweight search.

## CLI Runtime Flow

1. The user starts `codesmith-cli chat` or a headless CLI subcommand.
2. `chat` verifies workspace trust before interactive LLM prompts or command approvals.
3. The REPL uses `rustyline` for history, arrow keys, backspace, and Ctrl-C behavior.
4. The CLI expands `@workspace` and workspace-scoped `@file:` prompt context.
5. Wiki context is assembled from `index.md` and matching pages.
6. The active local model profile supplies backend kind, base URL, model, optional API key, temperature, context hint, and system prompt.
7. `llm` streams from the configured OpenAI-compatible local endpoint.
8. `agent` extracts command proposal JSON even when surrounded by assistant prose.
9. Relative proposal cwd values are resolved inside the configured workspace.
10. `policy` blocks commands outside the workspace or matching destructive patterns.
11. Allowed commands require explicit `y` or `n`; Enter alone does not proceed.
12. `runner` executes approved commands, captures stdout/stderr/status, and returns the result to the REPL.
13. Command results are added to chat history so follow-up debugging can use prior stderr/stdout.
14. Storage persists transcripts, command runs, source metadata, ingest jobs, and wiki metadata.

## Rich REPL Surface

Interactive commands:

```text
/help
/tools
/runs
/last
/retry
/clear
/prompts
/settings
/set base-url <url>
/set model <name>
/set api-key <key|none>
/set workspace <path>
/set timeout <seconds>
/models
/model use <id>
/model show
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

`/tools` explains available tool surfaces and approval policy. `/runs` and `/last` expose current REPL command results. `/retry` replays the last proposal or run proposal through the approval boundary. `/clear` clears only in-memory REPL history and does not delete persisted data.

## Approval Boundary

All commands require explicit user approval in v1. Policy-blocked commands never prompt for approval and never run.

The deny policy covers recursive deletion, `sudo`, recursive ownership/permission changes, disk formatting, credential reads, common exfiltration tools, and commands outside the configured workspace.

## Local Data

- Settings: `~/.codesmith/settings.toml`
- Trusted workspaces: `~/.codesmith/trusted-workspaces.txt`
- SQLite metadata: `~/.codesmith/codesmith.sqlite3`
- JSONL transcripts: `~/.codesmith/sessions`
- Wiki pages: `~/.codesmith/wiki`
- Raw source snapshots: `~/.codesmith/raw`
- Wiki schema notes: `~/.codesmith/schema`
- Wiki navigation: `~/.codesmith/index.md`
- Operation log: `~/.codesmith/log.md`

## PRD Fit

Implemented:

- CLI-first local LLM chat and headless command surface.
- Local model profiles for Ollama, vLLM, LiteLLM, and custom OpenAI-compatible endpoints.
- Explicit approval-gated shell execution.
- Command output capture, timeout handling, run summaries, retry, and last-run inspection.
- Workspace trust, `@workspace`, `@file:`, wiki ingest/query/lint/log/source workflows.
- SQLite metadata, JSONL transcripts, and Markdown wiki pages.

Not implemented:

- Full-screen TUI.
- MCP.
- Embedded inference.
- Multi-agent execution.
- Automatic file editing mode.
- Git commit/push/PR automation inside the product.
