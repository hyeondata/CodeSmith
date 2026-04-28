# CodeSmith

CodeSmith is a CLI-only, execution-only local coding agent built in Rust. It connects to a local OpenAI-compatible LLM endpoint, turns assistant output into command proposals, and runs only commands that the user explicitly approves.

The archived `egui` desktop shell is preserved on the pushed `archive-gui-egui` branch. `main` is now focused on `codesmith-cli`.

## Features

- Rich REPL: `codesmith-cli chat` with history, arrow keys, backspace, slash commands, and local workspace trust.
- Local model profiles for Ollama, vLLM, LiteLLM, and custom OpenAI-compatible endpoints.
- Strict or embedded JSON command proposal parsing with `command`, `cwd`, and `reason`.
- Explicit `y/n` approval before every allowed command execution.
- Policy blocking for destructive, privileged, credential, exfiltration, and out-of-workspace commands.
- Streaming stdout/stderr, exit status, timeout handling, run summaries, retry, and last-run inspection.
- Local settings, SQLite metadata, JSONL transcripts, and Markdown wiki pages.
- Wiki ingest/query/lint/log/source commands and `@workspace` / `@file:` context helpers.
- Superpowers-style workflow commands for planning, systematic debugging, verification, and review.

## Requirements

- macOS, Linux, or Windows.
- Stable Rust toolchain pinned by `rust-toolchain.toml`.
- A local OpenAI-compatible LLM server.

For Ollama:

```bash
ollama list
```

Settings are read from:

```text
~/.codesmith/settings.toml
```

## Quick Start

```bash
cd /Users/gim-yonghyeon/CodeSmith
cargo run -p codesmith-cli -- chat
```

Build the CLI:

```bash
cargo build --release -p codesmith-cli
./target/release/codesmith-cli doctor
```

## Rich REPL Commands

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
/plan <goal>
/debug <symptom>
/verify
/review
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

One-shot prompt:

```bash
cargo run -p codesmith-cli -- -p "Return exactly OK"
```

Command proposal preview and approved execution:

```bash
cargo run -p codesmith-cli -- proposal --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
cargo run -p codesmith-cli -- proposal --yes --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
```

Model profiles:

```bash
cargo run -p codesmith-cli -- models list
cargo run -p codesmith-cli -- models add-local --id qwen35-opus --backend ollama --base-url http://localhost:11434/v1 --model gag0/qwen35-opus-distil:27b
cargo run -p codesmith-cli -- models use qwen35-opus
cargo run -p codesmith-cli -- models show
```

Wiki commands:

```bash
cargo run -p codesmith-cli -- ingest file Cargo.toml
cargo run -p codesmith-cli -- ingest folder crates
cargo run -p codesmith-cli -- query "cargo workspace"
cargo run -p codesmith-cli -- lint wiki
cargo run -p codesmith-cli -- log recent
cargo run -p codesmith-cli -- sources
cargo run -p codesmith-cli -- wiki list
cargo run -p codesmith-cli -- wiki search hello
```

Workflow commands:

```text
/plan add a safer diagnostic flow
/debug python SyntaxError
/verify
/review
```

The default prompt favors intent before action, systematic debugging over guessing, read-only diagnostics before mutation, and evidence before completion claims.

## Safety Model

CodeSmith is execution-only.

- Commands never run before explicit approval.
- Approval requires an explicit `y` or `n`; Enter alone does not approve or reject.
- Blocked commands cannot be approved.
- Commands outside the configured workspace are blocked.
- Common destructive and privileged patterns are blocked.
- `@file:` context is limited to files inside the trusted workspace.

## Local Data

```text
~/.codesmith/settings.toml          settings
~/.codesmith/trusted-workspaces.txt trusted CLI workspace paths
~/.codesmith/codesmith.sqlite3      session and command metadata
~/.codesmith/sessions/              JSONL transcripts
~/.codesmith/wiki/                  Markdown wiki pages
~/.codesmith/raw/                   source snapshots
~/.codesmith/schema/                wiki schema notes
~/.codesmith/index.md               wiki navigation
~/.codesmith/log.md                 operation log
```

## Workspace Layout

```text
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

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-cli
```

## Documentation

- Architecture: `docs/architecture.md`
- Architecture (Korean): `docs/architecture.ko.md`
- Versioning: `docs/VERSIONING.md`
- Versioning (Korean): `docs/VERSIONING.ko.md`
- Research notes: `docs/research/claude-claurst-cli-notes.md`
- Korean README: `README.ko.md`
