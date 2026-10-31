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
