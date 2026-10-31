# smartlink

Unified cross-platform CLI for filesystem links — symlinks, hardlinks, junctions and
bind mounts. Stops links from breaking when projects move between machines, drives
and teammates. See [plan.md](plan.md) for the full roadmap.

## Status

Phase 1 (MVP) implemented: `make` + `ls`. Phases 2–3 (`convert`, `fix`, `dedupe`,
manifests, git hooks) are stubbed and error out with a milestone pointer.

## Quick start

```sh
# auto-detect: same-volume file -> hardlink, Windows dir -> junction, else relative symlink
smartlink make ../shared/utils ./src/utils

# explicit types
smartlink make big.mp4 media/big.mp4 --hard
smartlink make C:\libs\mylib vendor\mylib --junction
smartlink make ../config/app.yaml config/app.yaml --relative

# inspect a directory (color-coded OK/DEAD, hardlinks grouped by inode/file-id)
smartlink ls ./src
smartlink ls ./data --hardlinks
```

On Windows, plain symlinks need Developer Mode (or elevation); junctions do not.

## Building

```sh
cargo build --release   # single static binary in target/release
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

MIT
