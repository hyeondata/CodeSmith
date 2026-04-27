# CodeSmith 아키텍처

CodeSmith v1은 실행 전용 로컬 에이전트입니다. GUI 코드는 workspace에 남아 있지만, 현재 활성 개발 방향은 CLI-first입니다. 새 채팅, 도구 실행, ingest, query, lint, local wiki workflow는 CLI가 담당하며, 명령 승인 경계는 그대로 유지합니다.

## 현재 Workspace

- `crates/app`: native `eframe` entry point와 기본 window sizing.
- `crates/cli`: local LLM chat, settings, command proposal, ingest/query/lint/log, doctor check, wiki inspection의 주 runtime.
- `crates/ui`: egui shell, Codex 스타일 3패널 layout, settings UI, command approval flow, run log action, CJK font fallback.
- `crates/core`: settings, chat, command proposal, command run, policy, wiki, event shared type.
- `crates/llm`: OpenAI-compatible local chat completion client.
- `crates/agent`: 엄격한 JSON command proposal parser. 잘못된 JSON은 assistant text로 처리합니다.
- `crates/policy`: workspace boundary와 destructive command check.
- `crates/runner`: 승인된 shell command 실행, stdout/stderr streaming, timeout handling.
- `crates/storage`: settings, session metadata, transcript, command run persistence, CLI-first wiki metadata.
- `crates/wiki`: `raw/`, `wiki/`, `schema/`, `index.md`, `log.md`, Markdown frontmatter, ingest, lint, lightweight term-frequency search.

## CLI Runtime Flow

1. 사용자가 `codesmith-cli chat`을 시작하거나 headless CLI subcommand를 실행합니다.
2. `chat`은 interactive LLM prompt 또는 command approval 전에 workspace trust를 확인합니다.
3. CLI는 `@workspace`와 workspace-scoped `@file:` prompt context를 확장합니다.
4. `wiki`는 `index.md`와 matching wiki page를 local context로 조립합니다.
5. `storage`는 `~/.codesmith/settings.toml`에서 active local model profile을 찾습니다.
6. Active profile은 backend kind, base URL, model name, optional API key, temperature, context hint, 모델별 전체 system prompt를 제공합니다.
7. `llm`은 설정된 OpenAI-compatible local endpoint에서 streaming 응답을 받습니다.
8. `agent`는 엄격한 JSON command proposal을 파싱하고, 일반 text는 assistant text로 둡니다.
9. `policy`는 workspace 밖 명령과 destructive pattern을 차단합니다.
10. CLI는 `runner`가 process를 spawn하기 전에 명시적 승인을 요청합니다.
11. `runner`는 stdout/stderr를 streaming하고 timeout을 적용한 뒤 status를 반환합니다.
12. `storage`는 transcript, command run, source metadata, ingest job, wiki page metadata를 저장합니다.

## Legacy GUI Runtime Flow

1. 사용자가 중앙 chat composer에 prompt를 입력합니다.
2. `ui`가 관련 wiki context를 로드하고 `llm`을 호출합니다.
3. `agent`가 assistant output을 파싱합니다.
   - `command`, `cwd`, `reason`을 가진 strict JSON은 `CommandProposal`이 됩니다.
   - 그 외 output은 일반 assistant message가 됩니다.
4. `policy`가 proposal을 configured workspace와 deny pattern 기준으로 평가합니다.
5. 오른쪽 `Activity` panel은 실행 action보다 먼저 raw command와 cwd를 보여줍니다.
6. command는 사용자가 `Approve`를 클릭한 뒤에만 실행됩니다.
7. 승인 또는 거절된 proposal은 pending list에서 제거되어 같은 proposal이 재사용되지 않습니다.
8. `runner`가 stdout/stderr를 `ui`로 streaming합니다.
9. 완료된 run은 저장되며 copy, save to wiki, follow-up prompt에 사용할 수 있습니다.

## PRD 적합성

구현됨:

- Cargo workspace가 `app`, `ui`, `core`, `llm`, `agent`, `runner`, `policy`, `wiki`, `storage`로 분리됨.
- `codesmith-cli` binary가 headless local LLM, Claude Code-like interactive chat, settings, command proposal, doctor, wiki flow를 제공함.
- Stable Rust toolchain pin과 workspace manifest의 stable crate version.
- Codex 스타일 3패널 `eframe/egui` native app.
- `/v1/chat/completions` streaming 기반 OpenAI-compatible LLM client.
- Ollama, vLLM, LiteLLM, custom OpenAI-compatible endpoint를 위한 local model profile.
- 모델별 전체 system prompt. `gag0/qwen35-opus-distil:27b`는 strict unfenced JSON proposal을 선호하도록 prompt를 둠.
- Strict JSON command proposal parsing.
- 모든 command에 approval-before-execution boundary.
- Workspace-scoped policy check와 destructive-command blocking.
- `tokio::process::Command` 기반 runner와 stdout/stderr streaming, timeout handling.
- `~/.codesmith/settings.toml` settings persistence.
- SQLite session metadata와 command run persistence.
- JSONL chat transcript persistence.
- `~/.codesmith/wiki` Markdown + YAML frontmatter wiki page persistence.
- CLI command: `chat`, `-p/--print`, `proposal --json`, `doctor`, `wiki list`, `wiki search`.
- CLI model profile command: `models list`, `models show`, `models use <id>`, `models add-local`.
- CLI-first wiki command: `ingest file`, `ingest folder`, `query`, `lint wiki`, `log recent`, `sources`.

부분 구현:

- Local wiki retrieval은 agent prompt path에 연결되어 있지만 search는 `tantivy` BM25가 아니라 title bonus가 있는 lightweight term-frequency scorer입니다.
- Raw ingest, `index.md`, `log.md`, SQLite source metadata, wiki lint, query context는 CLI-first workflow에 존재합니다. 전체 normalize/extract/integrate conflict handling은 아직 없습니다.
- Run action은 copy, save-to-wiki, ask-follow-up을 제공합니다. 더 넓은 per-message action은 아직 남아 있습니다.
- Timeout 기반 cancel handling은 runner 설계에 있지만 user-facing cancel button은 아직 없습니다.

v1 미구현:

- Full LLM wiki pipeline: normalization, extraction, integration, conflict state workflow, LLM-reviewed wiki write proposal.
- Tantivy-backed index.
- MCP client/server.
- Embedded inference.
- Multi-agent execution.
- File editing, Git commit/push/PR automation, remote execution.

## 승인 경계

v1에서는 모든 command에 명시적 사용자 승인이 필요합니다. GUI나 CLI는 proposal을 표시할 수 있지만, 승인 전에는 process를 spawn하면 안 됩니다. Policy-blocked command는 approve할 수 없습니다.

현재 deny policy는 recursive deletion, `sudo`, recursive ownership/permission change, disk formatting, credential read, common exfiltration tool을 포함한 destructive/privileged pattern을 다룹니다. Configured workspace 밖 command는 차단됩니다.

## 로컬 데이터

- Settings: `~/.codesmith/settings.toml`
- SQLite metadata: `~/.codesmith/codesmith.sqlite3`
- JSONL transcripts: `~/.codesmith/sessions` 아래
- Wiki pages: `~/.codesmith/wiki` 아래
- Raw source snapshots: `~/.codesmith/raw` 아래
- Wiki schema notes: `~/.codesmith/schema` 아래
- Wiki navigation: `~/.codesmith/index.md`
- Operation log: `~/.codesmith/log.md`

기본 profile은 Ollama-compatible입니다.

- Base URL: `http://localhost:11434/v1`
- API key: optional
- Model: settings panel 또는 settings file에서 설정

## UI 아키텍처

UI는 Codex-like desktop layout을 따릅니다.

- Left sidebar: product identity, session entry, local LLM settings, workspace, timeout, save/test action.
- Center panel: chat transcript와 bottom prompt composer.
- Right panel: command proposal, run log, wiki context.
- Bottom status bar: current state와 selected model.

`crates/ui`는 dark egui theme과 macOS/Linux/Windows CJK fallback font를 등록해 Korean 및 mixed symbol을 렌더링합니다.

## 검증 기록

Computer Use와 CLI smoke를 통해 다음을 검증했습니다.

- Ollama connection test가 `Connection OK`를 반환함.
- LLM이 strict JSON command proposal을 생성하고 Activity panel에 raw command/cwd가 표시됨.
- `Approve` 클릭 후 command가 실행되고 stdout/stderr가 기록됨.
- `Save to wiki`, `Ask follow-up`, `Copy` action이 동작함.
- App restart 후 이전 transcript와 successful command run이 복원됨.
- CLI `proposal --json`, `proposal --yes`, blocked proposal, `-p`, `doctor`, `wiki list`, `wiki search`가 동작함.
- `codesmith-cli chat`에서 `/help`, `/settings`, `/wiki list`, `/exit`, general chat, fenced JSON command proposal, interactive approval이 동작함.
- Workspace trust gate와 `@workspace`, `@file:` helper가 동작하고 workspace escape를 차단함.
- CLI-first wiki smoke에서 `ingest file`, `ingest folder`, `query`, `sources`, `log recent`, `lint wiki`를 검증함.
- Python 구구단 tool smoke에서 code 생성, 정상 실행, 의도적 `SyntaxError`, 수정 후 재실행, destructive cleanup block을 확인함.

## 검증 명령

작동한다고 말하기 전에 다음을 실행합니다.

```bash
cargo fmt --all
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```
