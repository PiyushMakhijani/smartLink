//! `smartlink sync` – recreate every link declared in `.smartlink.json`
//! (plan.md §2 Phase 3).
//!
//! The manifest directory is treated as the project root: `link` and `target`
//! paths are resolved against it, per-OS `os_specific` overrides are applied
//! for the current platform, and each entry is reconciled with whatever already
//! exists so a second run is a no-op. `--dry-run` reports without touching the
//! filesystem; `--force` replaces an existing link that disagrees with the
//! manifest.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::link::{self, LinkKind};
use crate::manifest::{self, LinkEntry, LinkType, Manifest};
use super::make;

pub fn run(manifest_path: &Path, force: bool, dry_run: bool) -> Result<()> {
    let text = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("could not read manifest {}", manifest_path.display()))?;
    let m = Manifest::parse(&text)
        .with_context(|| format!("invalid manifest {}", manifest_path.display()))?;

    let root = manifest_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let root_abs = link::strip_verbatim(std::fs::canonicalize(&root).unwrap_or(root));
    let os = manifest::current_os_key();

    let mut created = 0usize;
    let mut uptodate = 0usize;
    let mut conflicts = 0usize;
    let mut failures = 0usize;

    for entry in &m.links {
        match reconcile(entry, &root_abs, os, force, dry_run) {
            Ok(Action::Created) => created += 1,
            Ok(Action::UpToDate) => uptodate += 1,
            Ok(Action::Conflict) => conflicts += 1,
            Err(err) => {
                failures += 1;
                eprintln!("{} {}: {err:#}", "error".red().bold(), entry.link.display());
            }
        }
    }

    println!(
        "{} {} created, {} up to date, {} conflict(s), {} error(s)",
        if dry_run { "dry-run".yellow() } else { "sync".green().bold() },
        created.to_string().bold(),
        uptodate,
        conflicts.if_white_default(),
        failures.if_white_default(),
    );

    if !dry_run && (conflicts > 0 || failures > 0) {
        bail!(
            "sync incomplete: {conflicts} conflict(s) (re-run with --force) and \
             {failures} error(s)"
        );
    }
    Ok(())
}

enum Action {
    Created,
    UpToDate,
    Conflict,
}

fn reconcile(
    entry: &LinkEntry,
    root: &Path,
    os: &str,
    force: bool,
    dry_run: bool,
) -> Result<Action> {
    // Apply per-OS overrides, then resolve both endpoints against the root.
    let ov = entry.os_specific.get(os);
    let link_type: LinkType = ov.and_then(|o| o.link_type).unwrap_or(entry.link_type);
    let target_ref: PathBuf = ov
        .and_then(|o| o.target.clone())
        .unwrap_or_else(|| entry.target.clone());

    let link_abs = root.join(&entry.link);
    let target_abs = if target_ref.is_absolute() {
        target_ref.clone()
    } else {
        root.join(&target_ref)
    };
    let kind = link_type.to_kind();

    // Does something already live at the link path, and does it match?
    let existing = link::detect(&link_abs)?;
    let matches = match &existing {
        Some(info) => {
            info.kind == kind
                && info.alive
                && same_file(&link::resolve_target(info), &target_abs)
        }
        None => false,
    };

    if matches {
        println!("  {} {}", "ok".dimmed(), link_abs.display());
        return Ok(Action::UpToDate);
    }

    if existing.is_some() && !force {
        println!(
            "  {} {} exists and differs from manifest (use --force)",
            "conflict".yellow().bold(),
            link_abs.display()
        );
        return Ok(Action::Conflict);
    }

    if dry_run {
        println!(
            "  {} {kind} {} -> {}",
            "would create".yellow(),
            link_abs.display(),
            target_abs.display()
        );
        return Ok(Action::Created);
    }

    // Symlinks are recreated with a portable relative stored target; the other
    // kinds only ever store absolute paths.
    let relative = kind == LinkKind::Symlink;
    make::create_link(kind, &target_abs, &link_abs, force || existing.is_some(), relative)?;
    println!(
        "  {} {kind} {} -> {}",
        "created".green().bold(),
        link_abs.display(),
        target_abs.display()
    );
    Ok(Action::Created)
}

/// Two paths point at the same existing file/dir (compare canonical forms).
fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Small helper so zero counts stay dim while non-zero counts stand out.
trait IfWhiteDefault {
    fn if_white_default(self) -> String;
}
impl IfWhiteDefault for usize {
    fn if_white_default(self) -> String {
        if self == 0 {
            self.to_string()
        } else {
            self.to_string().red().to_string()
        }
    }
}
