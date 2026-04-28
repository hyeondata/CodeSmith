# CodeSmith 아키텍처

`main`의 CodeSmith는 이제 CLI-only입니다. 기존 `egui` desktop shell은 원격에 push된 `archive-gui-egui` 브랜치에 보존되어 있고, 활성 개발 표면은 `codesmith-cli`입니다.

## Workspace

- `crates/cli`: Rich REPL, headless command, settings, model profile, command approval, wiki command, doctor check.
- `crates/core`: settings, chat, command proposal, command run, policy, wiki, event type.
- `crates/llm`: OpenAI-compatible local chat completion client.
- `crates/agent`: assistant output parser. strict JSON과 설명문 안 embedded JSON command proposal을 지원합니다.
- `crates/policy`: workspace boundary와 destructive command check.
- `crates/runner`: 승인된 shell command 실행, stdout/stderr capture, timeout 처리.
- `crates/storage`: settings, session metadata, transcript, command run, source record, ingest job, wiki metadata.
- `crates/wiki`: `raw/`, `wiki/`, `schema/`, `index.md`, `log.md`, Markdown frontmatter, ingest, lint, lightweight search.

## CLI Runtime Flow

1. 사용자가 `codesmith-cli chat` 또는 headless CLI command를 실행합니다.
2. `chat`은 interactive LLM prompt와 command approval 전에 workspace trust를 확인합니다.
3. REPL은 `rustyline`으로 history, 방향키, backspace, Ctrl-C를 처리합니다.
4. `@workspace`, workspace 내부 `@file:` context를 prompt에 붙입니다.
5. Wiki context는 `index.md`와 matching page로 구성됩니다.
6. active local model profile이 backend kind, base URL, model, optional API key, temperature, context hint, system prompt를 제공합니다.
7. `llm`은 configured OpenAI-compatible local endpoint에서 streaming합니다.
8. `agent`는 assistant 설명문 안에 섞인 command proposal JSON도 추출합니다.
9. relative proposal cwd는 configured workspace 내부로 해석됩니다.
10. `policy`는 workspace 밖 명령과 destructive pattern을 차단합니다.
11. allowed command는 명시적 `y` 또는 `n` 입력을 요구합니다. Enter만으로는 진행하지 않습니다.
12. `runner`는 승인된 명령을 실행하고 stdout/stderr/status를 REPL에 반환합니다.
13. command result는 chat history에 추가되어 후속 디버깅이 stderr/stdout을 참고할 수 있습니다.
14. storage는 transcript, command run, source metadata, ingest job, wiki metadata를 저장합니다.

## Rich REPL Surface

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

`/tools`는 tool surface와 approval policy를 보여줍니다. `/runs`, `/last`는 현재 REPL session의 command result를 보여줍니다. `/retry`는 마지막 proposal 또는 run proposal을 approval boundary로 다시 보냅니다. `/clear`는 in-memory REPL history만 초기화하며 persisted data는 삭제하지 않습니다.

## Approval Boundary

모든 명령은 명시적 사용자 승인이 필요합니다. Policy-blocked command는 approval prompt를 띄우지 않고 절대 실행하지 않습니다.

deny policy는 recursive deletion, `sudo`, recursive ownership/permission change, disk formatting, credential read, common exfiltration tool, configured workspace 밖 command를 차단합니다.

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

구현됨:

- CLI-first local LLM chat과 headless command surface.
- Ollama, vLLM, LiteLLM, custom OpenAI-compatible endpoint용 local model profile.
- explicit approval-gated shell execution.
- command output capture, timeout handling, run summary, retry, last-run inspection.
- workspace trust, `@workspace`, `@file:`, wiki ingest/query/lint/log/source workflow.
- SQLite metadata, JSONL transcript, Markdown wiki page.

미구현:

- Full-screen TUI.
- MCP.
- Embedded inference.
- Multi-agent execution.
- Automatic file editing mode.
- 제품 내부 Git commit/push/PR automation.
