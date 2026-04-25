# CodeSmith CLI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a stable Rust CLI for CodeSmith that reuses the existing local LLM, command proposal, policy, runner, storage, and wiki crates.

**Architecture:** The CLI is a new `crates/cli` package with a thin `clap` command surface. It does not add new agent privileges; command execution still requires an explicit `--yes` approval flag and policy-blocked commands never run. Shared behavior stays in existing crates.

**Tech Stack:** Rust 1.91 stable, `clap` 4, existing `codesmith-*` workspace crates, Tokio, SQLite/JSONL storage, local OpenAI-compatible LLM endpoint.

---

### Task 1: Workspace and CLI Skeleton

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/cli/Cargo.toml`
- Create: `crates/cli/src/main.rs`
- Create: `crates/cli/src/lib.rs`

- [ ] **Step 1: Add a failing CLI text helper test**

```rust
#[test]
fn approval_hint_mentions_yes_flag() {
    assert_eq!(
        approval_hint(),
        "approval required: rerun with --yes to execute this allowed command"
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p codesmith-cli approval_hint_mentions_yes_flag`

Expected: FAIL because `codesmith-cli` package or `approval_hint` does not exist.

- [ ] **Step 3: Add workspace member and minimal CLI crate**

Add `crates/cli` to workspace members and add `clap = { version = "4", features = ["derive"] }` to workspace dependencies. Create a `codesmith-cli` package with a small `lib.rs` exposing `approval_hint()` and a `main.rs` using `clap`.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p codesmith-cli approval_hint_mentions_yes_flag`

Expected: PASS.

### Task 2: Proposal Policy and Runner Command

**Files:**
- Modify: `crates/cli/src/lib.rs`
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: Add tests for proposal execution decisions**

Test that a safe command without `--yes` returns an approval-required outcome, a blocked command returns blocked, and `--yes` on `printf cli-ok` returns succeeded stdout.

- [ ] **Step 2: Implement `proposal --json <JSON> [--yes]`**

Use `codesmith-agent::parse_agent_output`, `codesmith-policy::evaluate`, and `codesmith-runner::run_approved_command`. Print the proposal, policy decision, and either an approval hint, blocked reason, or final run output.

- [ ] **Step 3: Verify proposal flow**

Run:

```bash
cargo run -p codesmith-cli -- proposal --json '{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"debug cli runner"}'
cargo run -p codesmith-cli -- proposal --json '{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"debug cli runner"}' --yes
```

Expected: first command does not spawn; second prints `cli-ok`.

### Task 3: Local LLM Print Mode

**Files:**
- Modify: `crates/cli/src/lib.rs`
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: Add tests for prompt message construction**

Test that wiki context is inserted as a system message before the user message when matching pages exist, and omitted when no pages match.

- [ ] **Step 2: Implement `-p/--print <PROMPT> [--yes]`**

Load settings from `~/.codesmith/settings.toml`, search wiki with the submitted prompt, call the local OpenAI-compatible endpoint, parse strict command proposal JSON, and reuse proposal handling.

- [ ] **Step 3: Verify with Ollama**

Run:

```bash
cargo run -p codesmith-cli -- -p "Return exactly CLI-OK"
```

Expected: prints `CLI-OK` with no command execution.

### Task 4: Doctor and Wiki Commands

**Files:**
- Modify: `crates/cli/src/lib.rs`
- Modify: `crates/cli/src/main.rs`

- [ ] **Step 1: Add tests for wiki list formatting**

Test that saved wiki page titles render under `Saved wiki pages`.

- [ ] **Step 2: Implement `doctor`, `wiki list`, and `wiki search <QUERY>`**

`doctor` prints settings path, base URL, model, workspace, and connection test result. `wiki list` shows saved pages. `wiki search` shows current-prompt retrievable context titles.

- [ ] **Step 3: Verify commands**

Run:

```bash
cargo run -p codesmith-cli -- doctor
cargo run -p codesmith-cli -- wiki list
cargo run -p codesmith-cli -- wiki search tool-ok
```

Expected: doctor reaches Ollama, saved wiki pages list includes `Command: printf 'tool-ok\n'`, and search finds the same page.

### Task 5: Documentation and End-to-End Debugging

**Files:**
- Modify: `docs/architecture.md`
- Create: `docs/research/claude-claurst-cli-notes.md`

- [ ] **Step 1: Document external references**

Summarize Claude Code CLI surfaces and Claurst architecture using links to official Claude Code docs and the Claurst GitHub README. State that GPL-3.0 code was not copied.

- [ ] **Step 2: Document CLI commands and debug results**

Add CodeSmith CLI usage and actual verification results to `docs/architecture.md`.

- [ ] **Step 3: Final verification**

Run:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-cli
```

Expected: all pass.
