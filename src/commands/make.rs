//! `slink make` – create a link with auto-detection.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::fs;
use crate::link::{self, LinkKind};

#[allow(clippy::too_many_arguments)]
pub fn run(
    target: &Path,
    link_path: &Path,
    hard: bool,
    symlink: bool,
    junction: bool,
    relative: bool,
    force: bool,
) -> Result<()> {
    if !target.exists() {
        bail!(
            "target {} does not exist (use `slink fix` later to heal dead links)",
            target.display().to_string().bold()
        );
    }
    let kind = choose_kind(target, link_path, hard, symlink, junction)?;
    create_link(kind, target, link_path, force, relative)?;
    println!(
        "{} {kind} {} -> {}",
        "created".green().bold(),
        link_path.display(),
        display_target(kind, target).display()
    );
    Ok(())
}

/// Shared creation path for `make` and `convert` (M2): validates the target
/// per kind, decides the stored target string (relative vs absolute), removes
/// an existing link when `force`, and creates through the OS abstraction.
/// Returns the string stored inside the created link.
pub fn create_link(
    kind: LinkKind,
    target: &Path,
    link_path: &Path,
    force: bool,
    relative: bool,
) -> Result<PathBuf> {
    // canonicalize() yields a verbatim (\\?\) path; strip it so the string we
    // store stays a plain Win32/POSIX path.
    let strip = |p: PathBuf| -> PathBuf {
        let s = p.to_string_lossy().to_string();
        match s.strip_prefix(r"\\?\") {
            Some(rest) => PathBuf::from(rest.to_string()),
            None => p,
        }
    };
    let target_abs = match target.canonicalize() {
        Ok(t) => strip(t),
        Err(_) => {
            if kind == LinkKind::Symlink {
                target.to_path_buf()
            } else {
                bail!("target {} must exist", target.display());
            }
        }
    };
    match kind {
        LinkKind::Hardlink => {
            if !target_abs.is_file() {
                bail!("hardlinks need an existing regular-file target");
            }
            if !crate::fs::same_volume(&target_abs, link_path) {
                bail!("hardlink requires target and link on the same volume");
            }
        }
        LinkKind::Junction | LinkKind::BindMount => {
            // Junctions store absolute paths only, so --relative is ignored.
            if !target_abs.exists() {
                bail!("{kind} target {} must exist", target.display());
            }
        }
        LinkKind::Symlink => {}
    }

    let target_str: PathBuf = match kind {
        // Junctions / bind mounts only store absolute paths; hardlinks store none.
        LinkKind::Junction | LinkKind::BindMount | LinkKind::Hardlink => target_abs.clone(),
        LinkKind::Symlink => {
            // --relative always; bare symlinks stay relative too when the user
            // passed a relative target (portable default, plan.md §1).
            if relative || !target.is_absolute() {
                link::relative_target(&target_abs, link_path)
            } else {
                target_abs.clone()
            }
        }
    };

    if symlink_metadata(link_path).is_ok() {
        if !force {
            bail!(
                "{} already exists; pass --force to replace it",
                link_path.display().to_string().bold()
            );
        }
        remove_existing(link_path)?;
    }
    if let Some(parent) = link_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
    }

    fs::create(kind, &target_str, &target_abs, link_path)
        .with_context(|| format!("failed to create {kind} at {}", link_path.display()))?;
    Ok(target_str)
}

/// Best-effort string shown to the user for "what was linked to".
fn display_target(kind: LinkKind, target: &Path) -> PathBuf {
    match kind {
        LinkKind::Symlink => target.to_path_buf(),
        _ => target
            .canonicalize()
            .unwrap_or_else(|_| target.to_path_buf()),
    }
}

fn choose_kind(
    target: &Path,
    link_path: &Path,
    hard: bool,
    symlink: bool,
    junction: bool,
) -> Result<LinkKind> {
    let requested = [hard, symlink, junction].iter().filter(|b| **b).count();
    if requested > 1 {
        bail!("--hard, --symlink and --junction are mutually exclusive");
    }
    if hard {
        if target.is_dir() && !junction {
            bail!("hardlinks to directories are not allowed by any supported filesystem");
        }
        if !fs::same_volume(target, link_path) {
            bail!("--hard requires target and link on the same volume");
        }
        return Ok(LinkKind::Hardlink);
    }
    if symlink {
        return Ok(LinkKind::Symlink);
    }
    if junction {
        if !target.is_dir() {
            #[cfg(windows)]
            bail!("--junction requires a directory target on Windows (files should use --hard or --symlink)");
            #[cfg(target_os = "linux")]
            {
                // bind mounts work on files too, but the target must exist as a file
                return Ok(LinkKind::BindMount);
            }
            #[cfg(not(any(windows, target_os = "linux")))]
            return Ok(LinkKind::Symlink); // macOS: junction degrades to symlink
        }
        #[cfg(windows)]
        return Ok(LinkKind::Junction);
        #[cfg(target_os = "linux")]
        return Ok(LinkKind::BindMount);
        #[cfg(not(any(windows, target_os = "linux")))]
        return Ok(LinkKind::Symlink);
    }

    // Auto-detection (plan.md §2 Phase 1): same-volume file -> hardlink,
    // Windows directory -> junction, everything else -> relative symlink.
    if target.is_file() && fs::same_volume(target, link_path) {
        return Ok(LinkKind::Hardlink);
    }
    if target.is_dir() {
        #[cfg(windows)]
        return Ok(LinkKind::Junction);
        #[cfg(not(windows))]
        return Ok(LinkKind::Symlink);
    }
    Ok(LinkKind::Symlink)
}

fn symlink_metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    std::fs::symlink_metadata(path)
}

fn remove_existing(path: &Path) -> Result<()> {
    // Delegates to the OS layer: on Windows a junction must go through
    // RemoveDirectoryW (std::fs::remove_dir returns ACCESS_DENIED there),
    // while symlinks/hardlinks/plain entries use the std calls.
    fs::remove(path)
}
