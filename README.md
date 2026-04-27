# CodeSmith

CodeSmith is an execution-only local agent built with Rust, a CLI-first runtime, and a frozen `egui` desktop shell.
It connects to a local OpenAI-compatible LLM server, proposes shell commands, and runs only commands that the user explicitly approves.

The current v1.1 direction is intentionally narrow: local CLI chat, command proposals, command approval, command execution logs, local wiki ingest/query/lint, settings, and persistence. It does not edit files automatically, commit or push Git changes, open PRs, run MCP tools, or execute remotely.

## Features

- Native `eframe/egui` desktop app with a Codex-style three-panel layout.
- Local OpenAI-compatible LLM client for Ollama, LM Studio, llama.cpp, llama-cpp-python, vLLM, and similar servers.
- Default local endpoint: `http://localhost:11434/v1`.
- Strict command proposal format with `command`, `cwd`, and `reason`.
- Explicit approval before every command execution.
- Policy blocking for destructive, privileged, credential, exfiltration, and out-of-workspace commands.
- Streaming stdout/stderr, exit status, timeout handling, and run logs.
- Local settings, SQLite metadata, JSONL transcripts, and Markdown wiki pages.
- Interactive CLI mode with workspace trust, slash commands, recommended prompts, `@` context helpers, and CLI-first wiki ingest/query/lint/log commands.
- Korean/CJK font fallback in the GUI.

## Requirements

- macOS, Linux, or Windows.
- Stable Rust toolchain pinned by `rust-toolchain.toml`.
- A local OpenAI-compatible LLM server.

For Ollama, start the app/server and make sure the configured model exists:

```bash
ollama list
```

The app currently reads settings from:

```text
~/.codesmith/settings.toml
```

## Quick Start

From the repository root:

```bash
cd /Users/gim-yonghyeon/CodeSmith
cargo run -p codesmith-app
```

Build release binaries:

```bash
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```

Run the desktop app:

```bash
./target/release/codesmith
```

Run the CLI:

```bash
./target/release/codesmith-cli doctor
```

## CLI Usage

Start Claude Code-like interactive chat:

```bash
cargo run -p codesmith-cli -- chat
```

On first use for a workspace, CodeSmith asks whether to trust that folder. Interactive LLM prompts and command approvals are only available after the workspace is trusted.

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

Context helpers:

```text
@workspace
@file:<workspace-relative-path>
```

Examples:

```text
Explain @file:Cargo.toml
Inspect @workspace and propose a read-only diagnostic command.
```

One-shot local LLM prompt:

```bash
cargo run -p codesmith-cli -- -p "Return exactly OK"
```

Check local endpoint and model:

```bash
cargo run -p codesmith-cli -- doctor
```

Preview a command proposal without running it:

```bash
cargo run -p codesmith-cli -- proposal --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
```

Approve and run an allowed command:

```bash
cargo run -p codesmith-cli -- proposal --yes --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
```

Inspect wiki pages:

```bash
cargo run -p codesmith-cli -- wiki list
cargo run -p codesmith-cli -- wiki search hello
```

Ingest trusted workspace sources into the local wiki:

```bash
cargo run -p codesmith-cli -- ingest file Cargo.toml
cargo run -p codesmith-cli -- ingest folder crates
cargo run -p codesmith-cli -- query "cargo workspace"
cargo run -p codesmith-cli -- lint wiki
cargo run -p codesmith-cli -- log recent
cargo run -p codesmith-cli -- sources
```

## Safety Model

CodeSmith v1 is execution-only.

- Commands never run before explicit approval.
- Blocked commands cannot be approved.
- Commands outside the configured workspace are blocked.
- Common destructive and privileged patterns are blocked, including recursive deletion, `sudo`, recursive `chmod`/`chown`, disk formatting, credential reads, and suspicious exfiltration tools.
- `@file:` context is limited to files inside the trusted workspace.

## Local Data

CodeSmith stores local data under `~/.codesmith`:

```text
settings.toml                 settings
trusted-workspaces.txt        trusted CLI workspace paths
codesmith.sqlite3             session and command metadata
sessions/                     JSONL transcripts
wiki/                         Markdown wiki pages
raw/                          immutable source snapshots
schema/                       wiki schema and policy notes
index.md                      wiki navigation entrypoint
log.md                        parseable operation log
index/                        reserved compatibility directory
logs/                         reserved compatibility directory
```

## Workspace Layout

```text
crates/app       eframe app entry point
crates/ui        egui UI, layout, approval panels, run logs
crates/cli       interactive and headless CLI
crates/core      shared data types and events
crates/llm       OpenAI-compatible local LLM client
crates/agent     assistant output parser
crates/policy    command safety policy
crates/runner    approved shell command runner
crates/storage   settings, SQLite, JSONL persistence
crates/wiki      local Markdown wiki and search
```

## Verification

Run these before claiming a change is working:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Build release targets:

```bash
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```

## Documentation

- Architecture and debugging notes: `docs/architecture.md`
- Versioning and release process: `docs/VERSIONING.md`
- Claude Code and Claurst CLI notes: `docs/research/claude-claurst-cli-notes.md`
- CLI implementation plan: `docs/superpowers/plans/2026-04-25-codesmith-cli.md`
