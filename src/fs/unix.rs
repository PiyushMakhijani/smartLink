//! Unix implementation: symlinks, hardlinks and (Linux only) bind mounts.

use std::path::Path;

use anyhow::{Context, Result};

use crate::link::{LinkInfo, LinkKind};

#[cfg(target_os = "linux")]
use nix::mount::MsFlags;

/// Detect a symlink at `path`; on Linux also flag directories whose device
/// differs from the parent (classic sign of a bind mount).
pub fn detect(path: &Path) -> Result<Option<LinkInfo>> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };

    if meta.file_type().is_symlink() {
        let target = std::fs::read_link(path)?;
        let resolved = if target.is_absolute() {
            target.clone()
        } else {
            crate::fs::link_base_dir(path).join(&target)
        };
        return Ok(Some(LinkInfo {
            path: path.to_path_buf(),
            kind: LinkKind::Symlink,
            target,
            alive: resolved.exists(),
            nlinks: hardlink_count(path).unwrap_or(1),
            file_id: file_id(path).unwrap_or(None),
        }));
    }

    #[cfg(target_os = "linux")]
    {
        if meta.is_dir() {
            if let (Some(dev), Some(parent_dev)) = (stat_dev(path)?, parent_dev(path)?) {
                if dev != parent_dev {
                    return Ok(Some(LinkInfo {
                        path: path.to_path_buf(),
                        kind: LinkKind::BindMount,
                        target: path.to_path_buf(),
                        alive: true,
                        nlinks: 1,
                        file_id: Some(dev),
                    }));
                }
            }
        }
    }

    Ok(None)
}

fn stat(path: &Path) -> Result<libc::stat> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    let cstr = std::ffi::CString::new(path.as_os_str().to_string_lossy().as_ref())
        .context("path contains interior nul")?;
    if unsafe { libc::lstat(cstr.as_ptr(), &mut st) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(st)
}

#[cfg(target_os = "linux")]
fn stat_dev(path: &Path) -> Result<Option<u64>> {
    Ok(Some(stat(path)?.st_dev))
}

#[cfg(target_os = "linux")]
fn parent_dev(path: &Path) -> Result<Option<u64>> {
    match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => Ok(Some(stat(p)?.st_dev)),
        _ => Ok(None),
    }
}

/// `st_nlink` for the file.
pub fn hardlink_count(path: &Path) -> Result<u64> {
    Ok(stat(path)?.st_nlink)
}

/// Inode number used to group hardlinks.
pub fn file_id(path: &Path) -> Result<Option<u64>> {
    Ok(Some(stat(path)?.st_ino))
}

/// Same `st_dev` means the same mounted filesystem.
pub fn same_volume(a: &Path, b: &Path) -> bool {
    match (stat(a), stat(b)) {
        (Ok(x), Ok(y)) => x.st_dev == y.st_dev,
        _ => false,
    }
}

/// `ln` (hard) equivalent.
pub fn create_hardlink(target: &Path, link: &Path) -> Result<()> {
    nix::unistd::link(target, link)
        .with_context(|| format!("hardlink {} -> {} failed (same filesystem, files only)", link.display(), target.display()))
}

/// `ln -s` equivalent.
pub fn create_symlink(target: &Path, link: &Path) -> Result<()> {
    nix::unistd::symlink(target, link).context("symlink creation failed")
}

/// `mount --bind` equivalent (Linux, requires privileges).
#[cfg(target_os = "linux")]
pub fn create_bind_mount(target: &Path, link: &Path) -> Result<()> {
    std::fs::create_dir_all(link)?;
    nix::mount::mount(
        Some(target),
        link,
        Option::<&str>::None,
        MsFlags::MS_BIND,
        Option::<&str>::None,
    )
    .with_context(|| format!("bind mount {} -> {} failed (needs root/CAP_SYS_ADMIN)", target.display(), link.display()))
}
