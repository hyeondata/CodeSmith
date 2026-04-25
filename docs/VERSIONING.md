# CodeSmith Versioning

This document defines how CodeSmith versions, release notes, and release checks are managed.

## Version Scheme

CodeSmith uses semantic versioning:

```text
MAJOR.MINOR.PATCH
```

- `MAJOR`: incompatible architecture, storage, CLI, or policy changes.
- `MINOR`: new compatible features, UI flows, CLI commands, storage additions, or wiki capabilities.
- `PATCH`: bug fixes, documentation updates, small UX fixes, and test-only changes.

Current workspace version:

```text
0.1.0
```

The source of truth is:

```text
Cargo.toml -> [workspace.package].version
```

## Pre-1.0 Policy

Before `1.0.0`, CodeSmith may still change public CLI behavior, storage format, and UI flows. Even so, changes should be documented clearly.

Use this guide for pre-1.0 bumps:

- `0.1.x`: fixes and documentation for the initial execution-only agent.
- `0.2.0`: meaningful feature additions, such as richer interactive CLI, stronger wiki indexing, or UI workflow changes.
- `0.3.0+`: larger v1 expansions that remain execution-only.
- `1.0.0`: first stable execution-only release with documented compatibility guarantees.

## Release Checklist

Before tagging a release:

```bash
cargo fmt --all --check
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p codesmith-app
cargo build --release -p codesmith-cli
```

Also manually verify:

- GUI starts and shows the three-panel app layout.
- Local LLM connection test works against the configured endpoint.
- Command proposals require approval before execution.
- Blocked commands cannot be approved.
- CLI `doctor` reports the expected local LLM status.
- CLI `chat` starts only after workspace trust.
- `/prompts`, `/settings`, `/wiki list`, and `/exit` work in CLI chat.
- `@file:<path>` cannot escape the trusted workspace.

## Release Notes

Each release should have a Markdown note under:

```text
docs/releases/
```

File name format:

```text
vMAJOR.MINOR.PATCH.md
```

Suggested structure:

```markdown
# CodeSmith v0.1.0

## Highlights

## Added

## Changed

## Fixed

## Security And Safety

## Verification
```

For the current initial release notes, see:

```text
docs/v1-release-notes.md
```

## Git Tags

Use annotated tags:

```bash
git tag -a v0.1.0 -m "CodeSmith v0.1.0"
git push origin v0.1.0
```

Do not tag until the release checklist passes.

## Compatibility Notes

Track compatibility-sensitive changes in release notes:

- CLI command names, flags, prompts, and approval behavior.
- Settings file format under `~/.codesmith/settings.toml`.
- SQLite schema under `~/.codesmith/codesmith.sqlite3`.
- Transcript JSONL format under `~/.codesmith/sessions`.
- Wiki Markdown/frontmatter format under `~/.codesmith/wiki`.
- Command policy behavior.

## Version Bump Process

1. Decide the next version using the version scheme above.
2. Update `[workspace.package].version` in `Cargo.toml`.
3. Update or add release notes.
4. Run the release checklist.
5. Commit with a release-focused message.
6. Create an annotated tag after the commit is finalized.

Suggested commit message:

```text
Release v0.1.0
```

