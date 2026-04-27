# CodeSmith

CodeSmith는 Rust 기반의 실행 전용 로컬 에이전트입니다. 현재 개발 방향은 CLI-first이며, 기존 `egui` 데스크톱 셸은 제거하지 않고 동결된 상태로 유지합니다.

로컬 OpenAI-compatible LLM 서버에 연결하고, LLM이 제안한 셸 명령을 사용자가 명시적으로 승인한 경우에만 실행합니다.

현재 v1.1 방향은 의도적으로 좁게 잡았습니다. 로컬 CLI 채팅, 명령 제안, 명령 승인, 실행 로그, local wiki ingest/query/lint, 설정, 영속화를 포함합니다. 자동 파일 수정, Git commit/push/PR 자동화, MCP 도구 실행, 원격 실행은 포함하지 않습니다.

## 기능

- `eframe/egui` 네이티브 데스크톱 앱은 기존 Codex 스타일 3패널 레이아웃으로 유지합니다.
- Ollama, LM Studio, llama.cpp, llama-cpp-python, vLLM 같은 OpenAI-compatible 로컬 LLM 서버를 지원합니다.
- 기본 로컬 엔드포인트는 `http://localhost:11434/v1`입니다.
- 명령 제안은 `command`, `cwd`, `reason`을 가진 엄격한 JSON 형식을 사용합니다.
- 모든 명령은 실행 전에 명시적 승인이 필요합니다.
- 파괴적 명령, 권한 상승, credential 읽기, exfiltration 의심 명령, workspace 밖 실행을 policy에서 차단합니다.
- stdout/stderr streaming, exit status, timeout, run log를 제공합니다.
- 설정, SQLite metadata, JSONL transcript, Markdown wiki page를 로컬에 저장합니다.
- CLI 대화 모드는 workspace trust, slash command, 추천 prompt, `@` context helper, wiki ingest/query/lint/log 명령을 제공합니다.
- GUI에는 Korean/CJK font fallback이 적용되어 있습니다.

## 요구사항

- macOS, Linux, Windows 중 하나
- `rust-toolchain.toml`로 고정된 stable Rust toolchain
- 로컬 OpenAI-compatible LLM 서버

Ollama를 사용할 경우 서버를 실행하고 설정된 모델이 존재하는지 확인합니다.

```bash
ollama list
```

설정 파일은 다음 경로에서 읽습니다.

```text
~/.codesmith/settings.toml
```

## 빠른 시작

저장소 루트에서 실행합니다.

```bash
cd /Users/gim-yonghyeon/CodeSmith
cargo run -p codesmith-cli -- chat
```

릴리스 바이너리 빌드:

```bash
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```

CLI 실행:

```bash
./target/release/codesmith-cli doctor
```

GUI 수동 smoke 확인:

```bash
./target/release/codesmith
```

## CLI 사용법

대화형 CLI를 시작합니다.

```bash
cargo run -p codesmith-cli -- chat
```

처음 사용하는 workspace에서는 신뢰 여부를 묻습니다. 대화형 LLM prompt와 명령 승인은 trusted workspace에서만 사용할 수 있습니다.

대화형 명령:

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

Context helper:

```text
@workspace
@file:<workspace-relative-path>
```

예시:

```text
Explain @file:Cargo.toml
Inspect @workspace and propose a read-only diagnostic command.
```

일회성 로컬 LLM prompt:

```bash
cargo run -p codesmith-cli -- -p "Return exactly OK"
```

로컬 엔드포인트와 모델 확인:

```bash
cargo run -p codesmith-cli -- doctor
```

명령 제안을 실행 없이 미리보기:

```bash
cargo run -p codesmith-cli -- proposal --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
```

승인 후 명령 실행:

```bash
cargo run -p codesmith-cli -- proposal --yes --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
```

Wiki 확인:

```bash
cargo run -p codesmith-cli -- wiki list
cargo run -p codesmith-cli -- wiki search hello
```

Trusted workspace source를 local wiki에 ingest:

```bash
cargo run -p codesmith-cli -- ingest file Cargo.toml
cargo run -p codesmith-cli -- ingest folder crates
cargo run -p codesmith-cli -- query "cargo workspace"
cargo run -p codesmith-cli -- lint wiki
cargo run -p codesmith-cli -- log recent
cargo run -p codesmith-cli -- sources
```

## 안전 모델

CodeSmith v1은 실행 전용입니다.

- 명령은 명시적 승인 전에는 실행되지 않습니다.
- 차단된 명령은 승인할 수 없습니다.
- 설정된 workspace 밖의 명령은 차단됩니다.
- `rm -rf`, `sudo`, recursive `chmod`/`chown`, disk formatting, credential read, exfiltration 의심 도구를 차단합니다.
- `@file:` context는 trusted workspace 내부 파일로 제한됩니다.

## 로컬 데이터

CodeSmith는 로컬 데이터를 `~/.codesmith` 아래에 저장합니다.

```text
settings.toml                 설정
trusted-workspaces.txt        trusted CLI workspace 경로
codesmith.sqlite3             session 및 command metadata
sessions/                     JSONL transcript
wiki/                         Markdown wiki page
raw/                          immutable source snapshot
schema/                       wiki schema 및 policy note
index.md                      wiki navigation entrypoint
log.md                        parseable operation log
index/                        reserved compatibility directory
logs/                         reserved compatibility directory
```

## Workspace 구조

```text
crates/app       eframe app entry point
crates/ui        egui UI, layout, approval panel, run log
crates/cli       interactive/headless CLI
crates/core      shared data type 및 event
crates/llm       OpenAI-compatible local LLM client
crates/agent     assistant output parser
crates/policy    command safety policy
crates/runner    approved shell command runner
crates/storage   settings, SQLite, JSONL persistence
crates/wiki      local Markdown wiki 및 search
```

## 검증

변경이 정상이라고 말하기 전에 다음을 실행합니다.

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

릴리스 빌드:

```bash
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```

## 문서

- Architecture and debugging notes: `docs/architecture.md`
- Architecture 한국어판: `docs/architecture.ko.md`
- Versioning and release process: `docs/VERSIONING.md`
- Versioning 한국어판: `docs/VERSIONING.ko.md`
- Release notes: `docs/v1-release-notes.md`
- Release notes 한국어판: `docs/v1-release-notes.ko.md`
