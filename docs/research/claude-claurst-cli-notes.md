# Claude Code and Claurst CLI Notes

Date: 2026-04-25

## Sources Checked

- Claude Code official overview: https://code.claude.com/docs/en/overview
- Claude Code slash commands and skills docs: https://code.claude.com/docs/en/slash-commands
- Claurst GitHub README: https://github.com/Kuberwastaken/claurst
- Local Claude Code install: `/opt/homebrew/bin/claude`, version `2.1.119 (Claude Code)`

## Claude Code CLI Surface

Claude Code is an agentic coding tool that can read a codebase, edit files, run commands, and integrate with development tools across terminal, IDE, desktop, and browser surfaces.

Observed local CLI features from `claude --help`:

- Interactive mode by default.
- Non-interactive print mode with `-p` / `--print`.
- Tool shaping with `--tools`, `--allowed-tools`, and `--disallowed-tools`.
- Permission shaping with `--permission-mode`.
- MCP configuration with `--mcp-config`, `mcp`, and plugin commands.
- Session continuation with `--continue`, `--resume`, and `--session-id`.
- Automation-friendly output formats: text, JSON, and stream JSON.
- `doctor` for health checks.
- Worktree support with `--worktree`.

## Claurst Reference Notes

Claurst describes itself as a Rust terminal coding agent inspired by Claude Code behavior. The README highlights:

- Rust implementation.
- Multi-provider support including Anthropic, OpenAI, Google, GitHub Copilot, Ollama, DeepSeek, Groq, Mistral, and others.
- TUI pair-programming surface.
- Plugin system.
- Memory consolidation.
- Chat forking.
- Managed agents preview with manager-executor relation.
- Headless one-shot mode with `claurst -p "..."`.

The repository is GPL-3.0 licensed. CodeSmith does not copy Claurst code. The CLI work here only uses high-level product/architecture ideas: headless `-p` mode, local command execution boundary, provider-neutral local endpoint configuration, and visible tool/risk status.

## CodeSmith CLI Scope

The CodeSmith CLI intentionally remains narrower than Claude Code and Claurst:

- Uses the existing local OpenAI-compatible LLM client.
- Provides interactive `chat` mode for local terminal conversation.
- Requires workspace trust before interactive LLM prompts or command approvals.
- Supports in-chat settings inspection and updates with `/settings` and `/set`.
- Supports lightweight prompt helpers with `/prompts`, `@workspace`, and workspace-scoped `@file:<path>`.
- Uses strict JSON command proposals.
- Requires explicit `--yes` before executing an allowed command.
- Reuses the existing policy denylist and workspace boundary.
- Reuses the existing local wiki storage/search.
- Does not edit files, commit, push, open PRs, run MCP tools, or start multi-agent loops.

Implemented commands:

```bash
codesmith-cli -p "prompt"
codesmith-cli --yes -p "prompt"
codesmith-cli chat
codesmith-cli proposal --json '{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"debug cli runner"}'
codesmith-cli proposal --json '{"command":"printf cli-ok","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"debug cli runner"}' --yes
codesmith-cli doctor
codesmith-cli wiki list
codesmith-cli wiki search tool-ok
```

## Debug Results

- `claude --version` returned `2.1.119 (Claude Code)`.
- `codesmith-cli proposal ...` without `--yes` printed approval-required text and did not run.
- `codesmith-cli proposal ... --yes` ran `printf cli-ok` and captured stdout `cli-ok`.
- `codesmith-cli proposal` with `rm -rf target --yes` was blocked by policy and did not run.
- `codesmith-cli -p "Return exactly CLI-OK"` connected to local Ollama and printed `CLI-OK`.
- `codesmith-cli doctor` reported `Connection OK` for `http://localhost:11434/v1` and model `gemma4:e4b-mlx-bf16`.
- `codesmith-cli wiki list` showed `Command: printf 'tool-ok\n'`.
- `codesmith-cli wiki search tool-ok` found the same saved page.
- `codesmith-cli chat` accepted `/help`, `/settings`, `/set timeout`, `/wiki list`, `/wiki search`, and `/exit`.
- `codesmith-cli chat` showed `CodeSmith is generating a response...` while waiting for Ollama.
- `codesmith-cli chat` accepted a fenced JSON command proposal from the model, prompted for `y`, and only then ran `printf chat-run-ok`.
- `codesmith-cli chat` asked for workspace trust before interactive use and persisted trusted workspace paths under `~/.codesmith/trusted-workspaces.txt`.
- `/prompts` displayed recommended prompt starters.
- `@file:<path>` was constrained to files inside the trusted workspace.
