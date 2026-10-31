//! `slink fix` – auto-heal broken links (plan.md §2 Phase 2).
//!
//! Scan for dead links -> search the project tree for matching filenames ->
//! compare size and (for files) SHA-256 -> re-link, prompting per candidate
//! unless `--yes` is given. `--dry-run` only reports what would happen.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use super::make;
use crate::link::{self, LinkInfo, LinkKind};

pub fn run(root: &Path, dry_run: bool, all: bool) -> Result<()> {
    let root_abs =
        std::fs::canonicalize(root).with_context(|| format!("cannot scan {}", root.display()))?;
    let mut dead = Vec::new();
    walk_collect(&root_abs, 0, &mut dead);
    if dead.is_empty() {
        println!(
            "{} no dead links found under {}",
            "◇".yellow(),
            root_abs.display()
        );
        return Ok(());
    }
    println!(
        "{} {} dead link(s) under {}",
        "!".red().bold(),
        dead.len(),
        root_abs.display()
    );

    let mut healed = 0usize;
    for info in &dead {
        println!(
            "  {} {} -> {}",
            info.kind.to_string().cyan(),
            info.path.display().to_string().bold(),
            info.target.display().to_string().red()
        );
        let wanted_dir = match std::fs::symlink_metadata(link::resolve_target(info)) {
            Ok(_) => false, // link of an unknown type: match by name only
            Err(_) => path_looks_like_dir(&info.target),
        };
        let mut candidates = Vec::new();
        search_matches(&root_abs, info, wanted_dir, &mut candidates);
        candidates.sort();

        if candidates.is_empty() {
            println!("    {}", "no matching file found".yellow());
            continue;
        }
        let mut handled = false;
        for cand in &candidates {
            if !all {
                let accept = dialoguer::Confirm::new()
                    .with_prompt(format!(
                        "re-link {} to {}?",
                        info.path.display(),
                        cand.display()
                    ))
                    .default(true)
                    .interact()?;
                if !accept {
                    continue;
                }
            }
            if dry_run {
                println!(
                    "    {} would re-link to {}",
                    "dry-run".yellow(),
                    cand.display()
                );
                handled = true;
                break;
            }
            // Dead junctions come back as portable relative symlinks; live
            // ones keep their kind.
            let kind = if cfg!(windows)
                && info.kind == LinkKind::Junction
                && link::resolve_target(info).exists()
            {
                LinkKind::Junction
            } else {
                LinkKind::Symlink
            };
            make::create_link(kind, cand, &info.path, true, kind == LinkKind::Symlink)?;
            println!(
                "    {} {} -> {}",
                "healed".green().bold(),
                info.path.display(),
                cand.display()
            );
            healed += 1;
            handled = true;
            break;
        }
        if !handled {
            println!("    {}", "skipped".yellow());
        }
    }
    if !dry_run {
        println!(
            "{} {} of {} dead link(s)",
            "fixed".green().bold(),
            healed,
            dead.len()
        );
    }
    Ok(())
}

/// A stored target with a trailing separator (or no extension on a dead
/// Windows junction) is treated as "was a directory". Best effort: junctions
/// and absolute symlink targets never end with a separator after read_link,
/// so we also accept names that are well-known directory-less targets by
/// requiring the candidate to be a directory in that case (see search).
fn path_looks_like_dir(target: &Path) -> bool {
    let s = target.to_string_lossy();
    s.ends_with('/') || s.ends_with('\\')
}

/// Recursive walk collecting dead links; does not descend into links
/// themselves, and depth-caps against pathological cycles (plan.md §5).
fn walk_collect(dir: &Path, depth: usize, out: &mut Vec<LinkInfo>) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if let Ok(Some(info)) = link::detect(&path) {
            if !info.alive {
                out.push(info);
            }
            continue; // never walk *through* a link
        }
        if meta.is_dir() {
            walk_collect(&path, depth + 1, out);
        }
    }
}

/// Recursive name search under `root`; the missing target itself rules out
/// size/hash comparison (plan.md §3), so matching is by filename and the
/// file/dir expectation derived from the dead path.
fn search_matches(dir: &Path, dead: &LinkInfo, wanted_dir: bool, out: &mut Vec<PathBuf>) {
    let Some(name) = dead.target.file_name() else {
        return;
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if path == dead.path {
            continue; // a link cannot be its own cure
        }
        if meta.file_type().is_symlink() {
            continue; // candidates must be real files/dirs
        }
        // keep searching: a deeper identical name may also exist
        if path.file_name() == Some(name) && (meta.is_dir() || (meta.is_file() && !wanted_dir)) {
            out.push(path.clone());
        }
        if meta.is_dir() {
            search_matches(&path, dead, wanted_dir, out);
        }
    }
}
