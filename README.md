# smartlink

![smartlink logo](logo.png)

[![CI](https://github.com/piyush/smartlink/actions/workflows/ci.yml/badge.svg)](https://github.com/piyush/smartlink/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org)

Unified cross-platform CLI for filesystem links — symlinks, hardlinks, junctions and
bind mounts. Stops links from breaking when projects move between machines, drives
and teammates. The project is **smartlink**; the CLI is invoked as the shorter
**`slink`**. See [plan.md](plan.md) for the full roadmap.

## Status

The full roadmap is now implemented (M1–M5). Everything the plan called for
through packaging and CI is live:

- **Foundation** — `make` (auto-detection) and `ls` (color-coded, hardlinks
  grouped by inode/file-id).
- **Manager** — `convert` (morph a link type), `fix` (auto-heal dead links by
  filename search), `dedupe` (native CoW cloning via
  `FSCTL_DUPLICATE_EXTENTS_TO_FILE` / `FICLONE` / `clonefile`).
- **Manifests** — `init` (generate `.smartlink.json` from live links) and `sync`
  (recreate them, applying per-OS `os_specific` overrides).
- **Git integration** — `git check` (CI guard that fails on committed absolute
  symlinks) and `git install-hooks` / `git uninstall-hooks` (auto-`sync` on
  `post-checkout` / `post-merge`).
- **Polish** — CI matrix (Linux/macOS/Windows: fmt, clippy, build, test), a
  tag-triggered release workflow that ships cross-platform binaries, and
  Homebrew/Scoop/Winget packaging templates under [`packaging/`](packaging).

See [plan.md](plan.md) for the full roadmap.

## Installation

Pre-built binaries are attached to each [GitHub
Release](https://github.com/piyush/smartlink/releases). Package managers install
the `slink` command:

```sh
# Cargo (any platform)
cargo install smartlink

# Homebrew (macOS / Linux)
brew tap piyush/slink && brew install slink

# Scoop (Windows)
scoop bucket add slink https://github.com/piyush/scoop-bucket && scoop install slink

# Winget (Windows)
winget install smartlink.slink
```

Recipes and the placeholders to fill before submitting live in
[`packaging/README.md`](packaging/README.md). To build from source, see
[Building](#building) below.

## Quick start

```sh
# auto-detect: same-volume file -> hardlink, Windows dir -> junction, else relative symlink
slink make ../shared/utils ./src/utils

# explicit types
slink make big.mp4 media/big.mp4 --hard
slink make C:\libs\mylib vendor\mylib --junction
slink make ../config/app.yaml config/app.yaml --relative

# inspect a directory (color-coded OK/DEAD, hardlinks grouped by inode/file-id)
slink ls ./src
slink ls ./data --hardlinks

# convert one link type into another (safe: dead links need --preserve-broken)
slink convert vendor\mylib --to symlink

# auto-heal dead links by searching the tree for a matching filename
slink fix . --dry-run
slink fix . --yes

# native copy-on-write clone (ReFS/NTFS-24H2, btrfs/XFS, APFS); never a plain copy
slink dedupe base.bin snapshot.bin

# capture the links in a project as a portable .smartlink.json manifest
slink init . --hardlinks

# replay a manifest on a fresh clone (idempotent; --force replaces mismatches)
slink sync
slink sync --dry-run

# CI guard: fail if any committed symlink stores an absolute target
slink git check .

# install post-checkout / post-merge hooks that auto-run `slink sync`
slink git install-hooks
slink git uninstall-hooks
```

On Windows, creating symlinks *and* junctions as a regular user requires Developer
Mode (the `SeCreateSymbolicLinkPrivilege`); administrators can do both directly.

## Building

```sh
cargo build --release   # single static binary (slink) in target/release
cargo test              # unit + integration tests
```

## Supported link types

| Type      | Windows            | Linux              | macOS              |
|-----------|--------------------|--------------------|--------------------|
| Hardlink  | NTFS `CreateHardLinkW` | `link(2)`        | `link(2)`          |
| Symlink   | Developer Mode required | `symlink(2)`   | `symlink(2)`       |
| Junction  | `mklink /J` equivalent (reparse point) | –      | –                  |
| Bind mount| –                  | `mount --bind` (root) | –               |

## License

MIT — see [LICENSE](LICENSE).
