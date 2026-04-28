# CodeSmith v1 Dev Build Notes

CodeSmith는 `main`에서 CLI-only Rich REPL runtime을 가진 local-only Rust execution agent입니다. Archived egui shell은 `archive-gui-egui` branch에 보존되어 있습니다. 현재 build는 OpenAI-compatible local LLM server에 연결하고, shell command를 JSON으로 제안하며, 모든 command에 명시적 approval을 요구합니다. stdout/stderr는 CLI에 streaming되고 settings, transcript, command run, source record, wiki note는 `~/.codesmith` 아래에 저장됩니다.

## 실행

```bash
cargo run -p codesmith-cli -- chat
```

기본 local LLM settings:

- Base URL: `http://localhost:11434/v1`
- Model: `~/.codesmith/settings.toml`의 active model profile로 선택
- API key: optional placeholder
- Settings file: `~/.codesmith/settings.toml`

Ollama를 사용할 경우 먼저 configured model을 pull 또는 생성한 뒤 다음 명령으로 연결을 확인합니다.

```bash
cargo run -p codesmith-cli -- doctor
```

## CLI

대화형 terminal mode:

```bash
cargo run -p codesmith-cli -- chat
```

주요 chat command:

```text
/help
/tools
/runs
/last
/retry
/clear
/prompts
/settings
/set model <name>
/set base-url <url>
/models
/model use <id>
/model show
/doctor
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
/wiki list
/wiki search <query>
/exit
```

CLI는 interactive LLM prompt 또는 command approval 전에 workspace trust를 요구합니다. `@workspace`와 workspace-scoped `@file:<path>` prompt context를 지원합니다.

CLI에는 Superpowers-style workflow command가 포함됩니다. `/plan`은 실행 전 intent와 verification을 정리하고, `/debug`는 root-cause investigation을 안내하며, `/verify`는 completion claim 전에 run evidence를 요약하고, `/review`는 failed/blocked evidence를 점검합니다.

Model profile command:

```bash
cargo run -p codesmith-cli -- models list
cargo run -p codesmith-cli -- models add-local --id qwen35-opus --backend ollama --base-url http://localhost:11434/v1 --model gag0/qwen35-opus-distil:27b
cargo run -p codesmith-cli -- models use qwen35-opus
cargo run -p codesmith-cli -- models show
```

기존 single-model settings는 `default` profile로 migrate됩니다. Profile은 Ollama, vLLM, LiteLLM, custom OpenAI-compatible local endpoint를 표현하며 모델별 전체 system prompt를 함께 저장합니다.

Headless wiki command:

```bash
cargo run -p codesmith-cli -- ingest file Cargo.toml
cargo run -p codesmith-cli -- ingest folder crates
cargo run -p codesmith-cli -- query "cargo workspace"
cargo run -p codesmith-cli -- lint wiki
cargo run -p codesmith-cli -- log recent
cargo run -p codesmith-cli -- sources
```

Wiki layout은 CLI-first local wiki 구조를 따릅니다. `raw/` snapshot, `wiki/` Markdown page, `schema/` schema note, `index.md` navigation, parseable `log.md`를 사용합니다.

## Command Proposal Format

LLM이 command를 제안하려면 strict JSON을 반환해야 합니다.

```json
{"command":"echo hello","cwd":"/path/to/workspace","reason":"inspect output"}
```

Malformed JSON과 non-proposal JSON은 일반 assistant text로 처리합니다. Command는 approval 전에 절대 실행되지 않습니다.

CLI layer는 local model이 valid JSON을 설명문이나 Markdown fence로 감싸는 경우가 많아 embedded/fenced command proposal도 허용합니다.

## Safety Defaults

모든 command는 approval이 필요합니다. Policy는 configured workspace 밖 command와 `rm -rf`, `sudo`, recursive permission/ownership change, disk formatting, credential read, common network exfiltration command를 차단합니다.

## Verification

Release 전에 다음을 실행합니다.

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-cli
```

CLI tool runner는 ignored `target/tool-smoke` 아래 Python 구구단 workflow로도 smoke-tested 되었습니다. Code 생성, 실행, 의도적 `SyntaxError` capture, broken file rewrite, successful rerun, destructive cleanup block을 확인했습니다.
