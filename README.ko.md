# CodeSmith

CodeSmith는 Rust 기반의 CLI-only, 실행 전용 로컬 코딩 에이전트입니다. 로컬 OpenAI-compatible LLM endpoint에 연결하고, assistant 출력에서 command proposal을 추출하며, 사용자가 명시적으로 승인한 명령만 실행합니다.

기존 `egui` desktop shell은 `archive-gui-egui` 브랜치에 보존되어 있습니다. `main`은 이제 `codesmith-cli` 중심입니다.

## 기능

- Rich REPL: `codesmith-cli chat`, history, 방향키, backspace, slash command, workspace trust.
- Ollama, vLLM, LiteLLM, custom OpenAI-compatible endpoint용 local model profile.
- `command`, `cwd`, `reason` JSON command proposal 파싱.
- 모든 명령 실행 전 명시적 `y/n` 승인.
- destructive, privileged, credential, exfiltration, workspace 밖 명령 차단.
- stdout/stderr streaming, exit status, timeout, run summary, retry, last-run inspection.
- settings, SQLite metadata, JSONL transcript, Markdown wiki page 저장.
- wiki ingest/query/lint/log/source 명령과 `@workspace`, `@file:` context helper.

## 요구사항

- macOS, Linux, Windows
- `rust-toolchain.toml`로 고정된 stable Rust toolchain
- 로컬 OpenAI-compatible LLM 서버

Ollama 확인:

```bash
ollama list
```

설정 경로:

```text
~/.codesmith/settings.toml
```

## 빠른 시작

```bash
cd /Users/gim-yonghyeon/CodeSmith
cargo run -p codesmith-cli -- chat
```

릴리스 CLI 빌드:

```bash
cargo build --release -p codesmith-cli
./target/release/codesmith-cli doctor
```

## Rich REPL 명령

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

일회성 prompt:

```bash
cargo run -p codesmith-cli -- -p "Return exactly OK"
```

Command proposal 미리보기와 승인 실행:

```bash
cargo run -p codesmith-cli -- proposal --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
cargo run -p codesmith-cli -- proposal --yes --json '{"command":"printf hello","cwd":"/Users/gim-yonghyeon/CodeSmith","reason":"test command"}'
```

모델 프로파일:

```bash
cargo run -p codesmith-cli -- models list
cargo run -p codesmith-cli -- models add-local --id qwen35-opus --backend ollama --base-url http://localhost:11434/v1 --model gag0/qwen35-opus-distil:27b
cargo run -p codesmith-cli -- models use qwen35-opus
cargo run -p codesmith-cli -- models show
```

Wiki 명령:

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

## 안전 모델

- 명령은 명시적 승인 전에는 실행되지 않습니다.
- 승인은 `y` 또는 `n`을 직접 입력해야 합니다. Enter만으로는 넘어가지 않습니다.
- 차단된 명령은 승인할 수 없습니다.
- 설정된 workspace 밖 명령은 차단됩니다.
- destructive/privileged/exfiltration 의심 패턴을 차단합니다.
- `@file:` context는 trusted workspace 내부 파일로 제한됩니다.

## 로컬 데이터

```text
~/.codesmith/settings.toml          settings
~/.codesmith/trusted-workspaces.txt trusted CLI workspace 경로
~/.codesmith/codesmith.sqlite3      session 및 command metadata
~/.codesmith/sessions/              JSONL transcript
~/.codesmith/wiki/                  Markdown wiki page
~/.codesmith/raw/                   source snapshot
~/.codesmith/schema/                wiki schema note
~/.codesmith/index.md               wiki navigation
~/.codesmith/log.md                 operation log
```

## Workspace 구조

```text
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

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-cli
```
