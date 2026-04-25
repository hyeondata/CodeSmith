# CodeSmith v1 Dev Build Notes

CodeSmith is a local-only Rust/egui execution agent. The v1 build connects to an OpenAI-compatible local LLM server, proposes shell commands as JSON, requires explicit approval for every command, streams stdout/stderr into the GUI/CLI, and persists settings, transcripts, command runs, and wiki notes under `~/.codesmith`.

## Run

```bash
cargo run --release -p codesmith-app
```

Default local LLM settings:

- Base URL: `http://localhost:11434/v1`
- Model: configured in `~/.codesmith/settings.toml` or the app settings panel
- API key: optional placeholder
- Settings file: `~/.codesmith/settings.toml`

For Ollama, pull or create the configured model first, then use the app's settings panel to test the connection.

## CLI

Interactive terminal mode:

```bash
cargo run -p codesmith-cli -- chat
```

Useful chat commands:

```text
/help
/prompts
/settings
/set model <name>
/set base-url <url>
/doctor
/wiki list
/wiki search <query>
/exit
```

The CLI requires workspace trust before interactive LLM prompts or command approvals. It also supports `@workspace` and workspace-scoped `@file:<path>` prompt context.

## Command Proposal Format

The LLM must return strict JSON when it wants to propose a command:

```json
{"command":"echo hello","cwd":"/path/to/workspace","reason":"inspect output"}
```

Malformed JSON and non-proposal JSON are treated as normal assistant text. Commands never run before approval.

The CLI layer also accepts a single fenced ` ```json ` command proposal because local models often wrap otherwise valid JSON in Markdown fences during interactive use.

## Safety Defaults

All commands require approval. The policy blocks commands outside the configured workspace and patterns such as `rm -rf`, `sudo`, recursive permission/ownership changes, disk formatting, credential reads, and common network exfiltration commands.

## Verification

Before release, run:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```

The CLI tool runner was also smoke-tested with an ignored Python 구구단 workflow under `target/tool-smoke`: generate code, run it, capture a deliberate `SyntaxError`, rewrite the broken file, rerun successfully, and confirm destructive cleanup remained blocked by policy.
