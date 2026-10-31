use std::fmt;
use std::path::{Path, PathBuf};

/// The kind of link smartlink knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Symlink,
    Hardlink,
    /// Windows NTFS junction point.
    Junction,
    /// Linux bind mount (detected via device-id change); Windows builds never
    /// construct it, hence the explicit allow.
    #[allow(dead_code)]
    BindMount,
}

impl fmt::Display for LinkKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            LinkKind::Symlink => "symlink",
            LinkKind::Hardlink => "hardlink",
            LinkKind::Junction => "junction",
            LinkKind::BindMount => "bind",
        };
        f.write_str(s)
    }
}

/// A detected link plus its resolved (possibly broken) target.
#[derive(Debug, Clone)]
pub struct LinkInfo {
    pub path: PathBuf,
    pub kind: LinkKind,
    pub target: PathBuf,
    /// False when the target does not exist.
    pub alive: bool,
    /// Hardlink count (Unix `st_nlink` / Windows `NumberOfLinks`). Read by the
    /// convert/fix commands in Phase 2.
    #[allow(dead_code)]
    pub nlinks: u64,
    /// Inode number / Windows file index, used to group hardlinks.
    #[allow(dead_code)]
    pub file_id: Option<u64>,
}

/// Inspect `path` and return link metadata if it is a link of some kind.
///
/// Hardlinks cannot be distinguished from plain files by a single stat, so
/// `None` is returned for them here; use [`hardlink_count`] / [`file_id`] on
/// regular files when grouping is requested (e.g. by `smartlink ls`).
pub fn detect(path: &Path) -> anyhow::Result<Option<LinkInfo>> {
    #[cfg(windows)]
    {
        crate::fs::windows::detect(path)
    }
    #[cfg(not(windows))]
    {
        crate::fs::unix::detect(path)
    }
}

/// `st_nlink` on Unix, `NumberOfLinks` on Windows.
pub fn hardlink_count(path: &Path) -> anyhow::Result<u64> {
    #[cfg(windows)]
    {
        crate::fs::windows::hardlink_count(path)
    }
    #[cfg(not(windows))]
    {
        crate::fs::unix::hardlink_count(path)
    }
}

/// Inode number (Unix) or 128-bit file index truncated for grouping (Windows).
pub fn file_id(path: &Path) -> anyhow::Result<Option<u64>> {
    #[cfg(windows)]
    {
        crate::fs::windows::file_id(path)
    }
    #[cfg(not(windows))]
    {
        crate::fs::unix::file_id(path)
    }
}

/// Where a hardlink can be created: both paths on the same volume, target is a file.
/// Used by `convert` (Phase 2); kept here as part of the public abstraction.
#[allow(dead_code)]
pub fn can_hardlink(target: &Path, link: &Path) -> bool {
    crate::fs::same_volume(target, link) && target.is_file()
}

/// Compute the relative target string for a link stored at `link_path`.
pub fn relative_target(target: &Path, link_path: &Path) -> PathBuf {
    // A link with no directory component (bare "name", i.e. created in the
    // current dir) is anchored at the CWD; otherwise `diff_paths` would be
    // handed a relative base against an absolute target and fall back to
    // storing the absolute path (undermining `--relative` and `git check`).
    let link_dir = match link_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
    };
    pathdiff::diff_paths(target, &link_dir).unwrap_or_else(|| target.to_path_buf())
}

/// Absolute path a link's stored target string resolves to (relative targets
/// are anchored at the link's parent directory).
pub fn resolve_target(info: &LinkInfo) -> PathBuf {
    if info.target.is_absolute() {
        info.target.clone()
    } else {
        crate::fs::link_base_dir(&info.path).join(&info.target)
    }
}

/// Strip the Windows verbatim-device prefix (`\\?\`) that `canonicalize()`
/// returns, so paths shown to users / stored in manifests stay plain. No-op on
/// paths without the prefix (and always a no-op on Unix).
pub fn strip_verbatim(path: PathBuf) -> PathBuf {
    let s = path.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest.to_string()),
        None => path,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_target_walks_up_to_common_ancestor() {
        let target = Path::new("/proj/libs/shared");
        let link = Path::new("/proj/src/utils");
        assert_eq!(relative_target(target, link), PathBuf::from("../libs/shared"));
    }

    #[test]
    fn relative_target_sibling_file() {
        let target = Path::new("/proj/data/a.txt");
        let link = Path::new("/proj/data/b.txt");
        assert_eq!(relative_target(target, link), PathBuf::from("a.txt"));
    }

    #[test]
    fn relative_target_falls_back_when_no_common_root() {
        // different drives on Windows: diff_paths yields None -> keep target
        // (on Unix these are just unrelated roots and the fallback also applies)
        let target = Path::new("C:/data/x");
        let link = Path::new("D:/proj/link");
        assert_eq!(relative_target(target, link), PathBuf::from("C:/data/x"));
    }

    #[cfg(windows)]
    #[test]
    fn strip_verbatim_removes_device_prefix_only() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\proj\libs")),
            PathBuf::from(r"C:\proj\libs")
        );
        // a plain path is returned untouched
        assert_eq!(
            strip_verbatim(PathBuf::from(r"C:\proj\libs")),
            PathBuf::from(r"C:\proj\libs")
        );
    }
}
