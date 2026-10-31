//! Cross-platform filesystem abstraction layer.
//!
//! All OS-specific syscalls live behind this module: the command layer only
//! talks to [`create`] / [`detect`] / [`same_volume`].

pub mod windows;
#[cfg(unix)]
pub mod unix;

use std::path::{Path, PathBuf};

use crate::link::LinkKind;

/// Unified link creation. `target_str` is the exact string stored in the link
/// (already resolved to relative or absolute by the caller).
pub fn create(
    kind: LinkKind,
    target_str: &Path,
    target_abs: &Path,
    link: &Path,
) -> anyhow::Result<()> {
    match kind {
        LinkKind::Symlink => create_symlink(target_str, link),
        LinkKind::Hardlink => {
            #[cfg(windows)]
            {
                windows::create_hardlink(target_abs, link)
            }
            #[cfg(unix)]
            {
                unix::create_hardlink(target_abs, link)
            }
        }
        LinkKind::Junction => {
            #[cfg(windows)]
            {
                windows::create_junction(target_abs, link)
            }
            #[cfg(not(windows))]
            {
                // macOS has no junctions and Linux bind mounts are privileged;
                // the unix command layer maps --junction to BindMount itself.
                let _ = (target_str, target_abs, link);
                anyhow::bail!("junctions are a Windows-only concept; use --junction on Linux for a bind mount")
            }
        }
        LinkKind::BindMount => {
            #[cfg(target_os = "linux")]
            {
                unix::create_bind_mount(target_abs, link)
            }
            #[cfg(not(target_os = "linux"))]
            {
                anyhow::bail!(
                    "bind mounts are only supported on Linux (on macOS use a symlink instead)"
                )
            }
        }
    }
}

/// Native Copy-on-Write clone (plan.md §2 Phase 2 `dedupe`): ReFS
/// `FSCTL_DUPLICATE_EXTENTS_TO_FILE` on Windows, `FICLONE` on Linux,
/// `clonefile` on macOS. Returns false when the operation is unsupported or
/// not permitted by the filesystem — the caller then reports, never silently
/// falls back to a space-eating plain copy.
pub fn try_clone_file(target: &Path, link: &Path) -> anyhow::Result<bool> {
    #[cfg(windows)]
    {
        windows::try_duplicate_extents(target, link)
    }
    #[cfg(target_os = "linux")]
    {
        unix::try_ficclone(target, link)
    }
    #[cfg(target_os = "macos")]
    {
        Ok(unix::try_clonefile(target, link))
    }
    #[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
    {
        let _ = (target, link);
        Ok(false)
    }
}

/// Target-safe deletion of an existing link (or plain file/dir) so `--force`
/// replacement, `convert` and `fix` can clear the old entry first. On Windows
/// junctions need `RemoveDirectoryW` (std's `remove_dir` hits ACCESS_DENIED),
/// so the OS layers own this decision.
pub fn remove(path: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        windows::remove(path)
    }
    #[cfg(unix)]
    {
        unix::remove(path)
    }
}

fn create_symlink(target: &Path, link: &Path) -> anyhow::Result<()> {
    #[cfg(windows)]
    {
        if target.is_dir() {
            windows::create_symlink_dir(target, link)
        } else {
            windows::create_symlink_file(target, link)
        }
    }
    #[cfg(unix)]
    {
        unix::create_symlink(target, link)
    }
}

/// True when both paths live on the same volume / device, i.e. a hardlink
/// between them is possible at all. Non-existent paths are probed via their
/// nearest existing ancestor (typically the link's parent directory).
pub fn same_volume(a: &Path, b: &Path) -> bool {
    let a = existing_ancestor(a);
    let b = existing_ancestor(b);
    #[cfg(windows)]
    {
        windows::same_volume(&a, &b)
    }
    #[cfg(not(windows))]
    {
        unix::same_volume(&a, &b)
    }
}

fn existing_ancestor(path: &Path) -> std::path::PathBuf {
    let mut cur = path.to_path_buf();
    while !cur.exists() {
        match cur.parent() {
            Some(p) if !p.as_os_str().is_empty() => cur = p.to_path_buf(),
            _ => break,
        }
    }
    cur
}

/// Parent directory the link's target string is resolved against.
pub fn link_base_dir(link: &Path) -> PathBuf {
    link.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."))
}
