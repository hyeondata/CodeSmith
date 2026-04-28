# CodeSmith 버전 관리

이 문서는 CodeSmith의 version, release note, release check 관리 방식을 정의합니다.

## 버전 체계

CodeSmith는 semantic versioning을 사용합니다.

```text
MAJOR.MINOR.PATCH
```

- `MAJOR`: architecture, storage, CLI, policy의 호환되지 않는 변경.
- `MINOR`: 호환 가능한 CLI command, storage addition, wiki capability.
- `PATCH`: bug fix, documentation update, 작은 UX fix, test-only change.

현재 workspace version:

```text
0.1.0
```

Source of truth:

```text
Cargo.toml -> [workspace.package].version
```

## Pre-1.0 정책

`1.0.0` 전에는 public CLI behavior와 storage format이 바뀔 수 있습니다. 그래도 변경 사항은 명확히 문서화해야 합니다.

Pre-1.0 bump 기준:

- `0.1.x`: 초기 execution-only agent의 fix와 documentation.
- `0.2.0`: ingest/query/lint/log workflow, stronger wiki indexing, storage metadata change 같은 의미 있는 CLI-first feature.
- `0.3.0+`: execution-only 원칙을 유지하는 더 큰 v1 expansion.
- `1.0.0`: compatibility guarantee가 문서화된 첫 stable execution-only release.

## Release Checklist

Tag를 만들기 전 다음을 실행합니다.

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-cli
```

CLI-first flow도 수동 검증합니다.

- Local LLM connection test가 configured endpoint에서 동작함.
- Command proposal은 실행 전에 approval을 요구함.
- Blocked command는 approve할 수 없음.
- CLI `doctor`가 expected local LLM status를 보고함.
- CLI `chat`이 workspace trust 후에만 시작됨.
- `/tools`, `/runs`, `/last`, `/retry`, `/clear`, `/plan`, `/debug`, `/verify`, `/review`, `/prompts`, `/settings`, `/ingest file`, `/query`, `/lint wiki`, `/log recent`, `/sources`, `/wiki list`, `/exit`가 CLI chat에서 동작함.
- `@file:<path>`가 trusted workspace 밖으로 escape하지 못함.
- `codesmith-cli ingest file <path>`가 raw snapshot, source metadata, `index.md`, `log.md`를 생성함.
- `codesmith-cli ingest folder <path>`가 hidden/build/cache directory를 skip함.
- `codesmith-cli lint wiki`가 broken wikilink와 malformed frontmatter를 보고하고 wiki page를 변경하지 않음.

Archived GUI는 `archive-gui-egui` branch에 보존되어 있으며 `main` release checklist에는 포함하지 않습니다.

## Release Notes

각 release는 다음 위치에 Markdown note를 둡니다.

```text
docs/releases/
```

파일 이름 형식:

```text
vMAJOR.MINOR.PATCH.md
```

권장 구조:

```markdown
# CodeSmith v0.1.0

## Highlights

## Added

## Changed

## Fixed

## Security And Safety

## Verification
```

현재 초기 release note:

```text
docs/v1-release-notes.md
docs/v1-release-notes.ko.md
```

## Git Tags

Annotated tag를 사용합니다.

```bash
git tag -a v0.1.0 -m "CodeSmith v0.1.0"
git push origin v0.1.0
```

Release checklist가 통과하기 전에는 tag를 만들지 않습니다.

## 호환성 Notes

Release note에서 호환성에 민감한 변경을 추적합니다.

- CLI command name, flag, prompt, approval behavior.
- `~/.codesmith/settings.toml` settings file format. `active_profile`과 `model_profiles` 포함.
- Model profile field: backend kind, base URL, model name, API key placeholder, temperature, context hint, system prompt.
- `~/.codesmith/codesmith.sqlite3` SQLite schema.
- SQLite `source_records`, `ingest_jobs`, `wiki_page_metadata`에 저장되는 source/ingest metadata.
- `~/.codesmith/sessions` transcript JSONL format.
- `~/.codesmith/wiki` wiki Markdown/frontmatter format.
- `~/.codesmith/raw` raw source snapshot, `~/.codesmith/schema` wiki schema note, parseable `index.md`/`log.md`.
- Command policy behavior.

## Version Bump 절차

1. Version scheme에 따라 다음 version을 결정합니다.
2. `Cargo.toml`의 `[workspace.package].version`을 업데이트합니다.
3. Release note를 업데이트하거나 추가합니다.
4. Release checklist를 실행합니다.
5. Release-focused message로 commit합니다.
6. Commit이 확정된 뒤 annotated tag를 만듭니다.

권장 commit message:

```text
Release v0.1.0
```
