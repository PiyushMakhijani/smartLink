//! `slink dedupe` – native Copy-on-Write cloning (plan.md §2 Phase 2).
//!
//! Uses the filesystem's own block-sharing primitive (`FSCTL_DUPLICATE_EXTENTS_TO_FILE`
//! on Windows/ReFS, `FICLONE` on Linux, `clonefile(2)` on macOS) so the clone
//! costs no extra space and no visible link is created. Unsupported filesystems
//! are reported explicitly; we never fake it with a plain copy.

use std::path::Path;

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::fs;

pub fn run(src: &Path, dest: &Path, force: bool) -> Result<()> {
    let meta =
        std::fs::metadata(src).with_context(|| format!("source {} not found", src.display()))?;
    if !meta.is_file() {
        bail!(
            "dedupe clones regular files only ({} is not a file)",
            src.display()
        );
    }
    if dest.exists() {
        if !force {
            bail!(
                "{} already exists; pass --force to replace it",
                dest.display().to_string().bold()
            );
        }
        std::fs::remove_file(dest)?;
    }
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let done = fs::try_clone_file(src, dest).with_context(|| {
        format!(
            "CoW clone {} -> {} failed on this filesystem",
            src.display(),
            dest.display()
        )
    })?;
    if !done {
        let _ = std::fs::remove_file(dest);
        bail!(
            "this filesystem/volume does not permit CoW cloning here (needs ReFS or \
             Windows 11 24H2+ NTFS on Windows, btrfs/XFS on Linux, APFS on macOS); \
             nothing was written"
        );
    }
    println!(
        "{} {} -> {}",
        "cloned".green().bold(),
        src.display(),
        dest.display()
    );
    Ok(())
}
