# Changelog

All notable changes to `smartlink` (the `slink` command) are documented here.
The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and
the project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `slink profile` (`create`, `use`, `list`, `show`, `set`, `delete`): named configurations.
- Transactions: `convert --dry-run` / `--apply`, `slink transaction history|show|rollback`,
  with rollback metadata and automatic recovery when an operation fails.
- Portable manifests: `slink export --portable`, `slink import`, `slink validate --portable`,
  with link-type conversion rules between Windows, Linux and macOS.
- `slink tree`, `stats`, `benchmark`, `completion`, `config`, `audit` and `explain`.

### Fixed

- Windows: hardlink count and file id were read from the wrong struct layout, so
  ordinary files could be reported as hardlinked.
- `doctor` circular-symlink detection and `watch` Ctrl-C / `--interval 0` handling.

## [0.1.0] - 2026-10-31

First public release covering the full roadmap (M1–M5).

### Added

- **`slink make`** — create a link with type auto-detection (same-volume file →
  hardlink, Windows directory → junction, otherwise a relative symlink);
  `--hard` / `--symlink` / `--junction` / `--relative` / `--force` overrides.
- **`slink ls`** — color-coded directory scan (OK/DEAD) with hardlinks grouped
  by inode/file-id.
- **`slink convert`** — morph an existing link between types (`--to`,
  `--preserve-broken`).
- **`slink fix`** — auto-heal dead links by searching the tree for a matching
  filename (`--dry-run`, `--yes`).
- **`slink dedupe`** — native copy-on-write cloning
  (`FSCTL_DUPLICATE_EXTENTS_TO_FILE` on NTFS/ReFS, `FICLONE` on Linux,
  `clonefile` on macOS); refuses rather than silently copying.
- **`slink init` / `slink sync`** — capture live links into a portable
  `.smartlink.json` manifest and replay it, applying per-OS `os_specific`
  overrides; `sync` is idempotent and conflicts unless `--force`.
- **`slink git check`** — CI guard that fails when a committed symlink stores an
  absolute target.
- **`slink git install-hooks` / `uninstall-hooks`** — manage `post-checkout` /
  `post-merge` hooks that auto-run `slink sync`, inside a marker-delimited
  managed block.
- Cross-platform link abstraction: symlinks, hardlinks, junctions and bind
  mounts behind a single `fs` module.
- CLI renamed from `smartlink` to the shorter `slink` (the project/package name
  stays `smartlink`).
- GitHub Actions CI (fmt, clippy `-D warnings`, build, test on Linux/macOS/Windows)
  and a tag-triggered release workflow shipping per-platform binaries.
- Packaging templates for Homebrew, Scoop and Winget under `packaging/`.
- MIT `LICENSE`, this `CHANGELOG`, and project logo in the README.

[Unreleased]: https://github.com/piyush/smartlink/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/piyush/smartlink/releases/tag/v0.1.0
