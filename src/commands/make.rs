//! `smartlink make` – create a link with auto-detection.

use std::path::Path;

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
            "target {} does not exist (use `smartlink fix` later to heal dead links)",
            target.display().to_string().bold()
        );
    }
    if symlink_metadata(link_path).is_ok() {
        if !force {
            bail!(
                "{} already exists; pass --force to replace it",
                link_path.display().to_string().bold()
            );
        }
        remove_existing(link_path)?;
    }

    let kind = choose_kind(target, link_path, hard, symlink, junction)?;
    // canonicalize() yields a verbatim (\\?\) path; strip it so the string we
    // store stays a plain Win32/POSIX path.
    let strip = |p: std::path::PathBuf| -> std::path::PathBuf {
        let s = p.to_string_lossy().to_string();
        match s.strip_prefix(r"\\?\") {
            Some(rest) => std::path::PathBuf::from(rest.to_string()),
            None => p,
        }
    };
    let target_abs = strip(
        target
            .canonicalize()
            .with_context(|| format!("could not resolve {}", target.display()))?,
    );

    let target_str: std::path::PathBuf = match kind {
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

    if let Some(parent) = link_path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("could not create {}", parent.display()))?;
        }
    }

    fs::create(kind, &target_str, &target_abs, link_path)
        .with_context(|| format!("failed to create {kind} at {}", link_path.display()))?;

    println!(
        "{} {kind} {} -> {}",
        "created".green().bold(),
        link_path.display(),
        target_str.display()
    );
    Ok(())
}

fn choose_kind(target: &Path, link_path: &Path, hard: bool, symlink: bool, junction: bool) -> Result<LinkKind> {
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
    let meta = symlink_metadata(path)?;
    if meta.is_symlink() || meta.is_file() {
        std::fs::remove_file(path)?;
    } else if meta.is_dir() {
        // A junction removed via remove_dir does not touch its target.
        std::fs::remove_dir(path)?;
    }
    Ok(())
}
