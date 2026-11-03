//! `slink doctor` – comprehensive link health diagnostics.
//!
//! Walks a directory tree, inspects every link, and emits a structured
//! health report with counts and a percentage score:
//!
//! * Broken links        – the stored target path does not exist.
//! * Permission errors   – the link or its target exists but cannot be read.
//! * Circular references – a symlink chain that loops back on itself.
//! * Duplicate targets   – two or more distinct links pointing at the same
//!   resolved absolute path.
//!
//! The health score is computed as:
//!   healthy / (healthy + broken + permission_errors + circular) × 100

use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use crate::link::{self, LinkInfo, LinkKind};

// ─── public entry point ──────────────────────────────────────────────────────

pub fn run(root: &Path, verbose: bool) -> Result<()> {
    let root_abs =
        std::fs::canonicalize(root).with_context(|| format!("cannot scan {}", root.display()))?;

    println!("{} scanning {}", "doctor".cyan().bold(), root_abs.display());
    println!();

    let mut all_links: Vec<LinkInfo> = Vec::new();
    walk(&root_abs, 0, &mut all_links);

    let total = all_links.len();
    if total == 0 {
        println!(
            "{} no links found under {}",
            "◇".yellow(),
            root_abs.display()
        );
        return Ok(());
    }

    // ── classify every link ──────────────────────────────────────────────────
    let mut broken: Vec<&LinkInfo> = Vec::new();
    let mut perm_errors: Vec<(&LinkInfo, String)> = Vec::new();
    let mut circular: Vec<&LinkInfo> = Vec::new();
    let mut healthy: Vec<&LinkInfo> = Vec::new();

    for info in &all_links {
        // 1. Circular reference check (only meaningful for symlinks).
        if info.kind == LinkKind::Symlink && is_circular(&info.path) {
            circular.push(info);
            continue;
        }

        // 2. Permission error: path accessible but we cannot stat the resolved target.
        let resolved = link::resolve_target(info);
        if !info.alive {
            // Might be a genuine missing target or a permission error.
            if let Some(msg) = permission_error_message(&resolved) {
                perm_errors.push((info, msg));
                continue;
            }
            broken.push(info);
            continue;
        }

        // 3. Check we can actually read the target (not just that it exists).
        if let Some(msg) = permission_error_message(&resolved) {
            perm_errors.push((info, msg));
            continue;
        }

        healthy.push(info);
    }

    // ── duplicate target detection ────────────────────────────────────────────
    // Map resolved-absolute-target -> list of links pointing there.
    let mut target_map: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
    for info in &all_links {
        let resolved = link::resolve_target(info);
        // Only follow things we can actually canonicalize (alive links).
        let key = resolved.canonicalize().unwrap_or(resolved);
        target_map.entry(key).or_default().push(info.path.clone());
    }
    let dup_groups: Vec<Vec<PathBuf>> = target_map.into_values().filter(|v| v.len() > 1).collect();
    let dup_link_count: usize = dup_groups.iter().map(|g| g.len()).sum();

    // ── print per-link details when verbose ───────────────────────────────────
    if verbose {
        if !broken.is_empty() {
            println!("{}", "Broken links:".red().bold());
            for info in &broken {
                println!(
                    "  {} {} -> {}",
                    info.kind.to_string().dimmed(),
                    rel(&info.path, &root_abs)
                        .display()
                        .to_string()
                        .red()
                        .bold(),
                    info.target.display().to_string().dimmed()
                );
            }
            println!();
        }

        if !perm_errors.is_empty() {
            println!("{}", "Permission errors:".yellow().bold());
            for (info, msg) in &perm_errors {
                println!(
                    "  {} {} -> {}",
                    info.kind.to_string().dimmed(),
                    rel(&info.path, &root_abs)
                        .display()
                        .to_string()
                        .yellow()
                        .bold(),
                    msg.dimmed()
                );
            }
            println!();
        }

        if !circular.is_empty() {
            println!("{}", "Circular references:".red().bold());
            for info in &circular {
                println!(
                    "  {} {}",
                    info.kind.to_string().dimmed(),
                    rel(&info.path, &root_abs)
                        .display()
                        .to_string()
                        .red()
                        .bold()
                );
            }
            println!();
        }

        if !dup_groups.is_empty() {
            println!("{}", "Duplicate targets:".cyan());
            for group in &dup_groups {
                println!(
                    "  {} link(s) -> {}",
                    group.len(),
                    rel(&group[0], &root_abs).display()
                );
                for p in group {
                    println!("      {}", rel(p, &root_abs).display().to_string().dimmed());
                }
            }
            println!();
        }
    }

    // ── summary lines ─────────────────────────────────────────────────────────
    let n_healthy = healthy.len();
    let n_broken = broken.len();
    let n_perm = perm_errors.len();
    let n_circ = circular.len();

    // "Problems" are things that count against health; duplicates are
    // informational only (INFO, not WARN).
    let problems = n_broken + n_perm + n_circ;
    let health_pct = if total == 0 {
        100u32
    } else {
        ((n_healthy as f64 / total as f64) * 100.0).round() as u32
    };

    println!("{}", "─".repeat(45));
    println!("{}  {} links healthy", "[OK]  ".green().bold(), n_healthy);
    if n_broken > 0 {
        println!(
            "{}  {} broken {}",
            "[WARN]".yellow().bold(),
            n_broken,
            if n_broken == 1 {
                "link"
            } else {
                "links (target missing)"
            }
        );
    }
    if n_perm > 0 {
        println!(
            "{}  {} permission {}",
            "[WARN]".yellow().bold(),
            n_perm,
            if n_perm == 1 { "error" } else { "errors" }
        );
    }
    if n_circ > 0 {
        println!(
            "{}  {} circular reference{}",
            "[WARN]".yellow().bold(),
            n_circ,
            if n_circ == 1 { "" } else { "s" }
        );
    }
    if dup_link_count > 0 {
        println!(
            "{}  {} link{} share {} duplicate target{}",
            "[INFO]".cyan(),
            dup_link_count,
            if dup_link_count == 1 { "" } else { "s" },
            dup_groups.len(),
            if dup_groups.len() == 1 { "" } else { "s" }
        );
    }

    println!();

    // Health bar
    let bar = health_bar(health_pct);
    let health_str = format!("Health: {}%  {}", health_pct, bar);
    let colored_health = if health_pct == 100 {
        health_str.green().bold().to_string()
    } else if health_pct >= 80 {
        health_str.yellow().bold().to_string()
    } else {
        health_str.red().bold().to_string()
    };
    println!("{colored_health}");

    if problems > 0 && !verbose {
        println!(
            "\n{} re-run with {} to see per-link details",
            "tip:".dimmed(),
            "--verbose".bold()
        );
        println!(
            "{} run {} to attempt automatic repairs",
            "tip:".dimmed(),
            "`slink fix .`".bold()
        );
    }

    // Exit non-zero if there are genuine problems (broken / perm / circular).
    if problems > 0 {
        std::process::exit(1);
    }
    Ok(())
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Recursive walk collecting all detected links; never descends through a link,
/// depth-capped at 32 (matching the convention in fix/init/git).
pub fn walk(dir: &Path, depth: usize, out: &mut Vec<LinkInfo>) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(_meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if let Ok(Some(info)) = link::detect(&path) {
            out.push(info);
            continue; // never walk *through* a link
        }
        if _meta.is_dir() {
            walk(&path, depth + 1, out);
        }
    }
}

/// Check whether following a symlink produces a cycle.  We follow up to 40
/// hops (matching the common OS `ELOOP` limit) recording visited canonical
/// paths; a revisit means a loop.
fn is_circular(path: &Path) -> bool {
    let mut current = normalize_path(path);
    let mut visited: Vec<PathBuf> = Vec::new();

    for _ in 0..40 {
        let target = match std::fs::read_link(&current) {
            Ok(t) => t,
            Err(_) => return false, // not a symlink or unreadable → not circular
        };
        let resolved = if target.is_absolute() {
            target
        } else {
            match current.parent() {
                Some(p) => p.join(target),
                None => target,
            }
        };

        // canonicalize follows symlinks and fails with ELOOP for exactly the
        // cycles we need to report. Normalize lexically so each hop can be
        // compared without following the link.
        let canonical = normalize_path(&resolved);

        if visited.contains(&canonical) {
            return true;
        }
        visited.push(canonical.clone());
        current = canonical;

        // If the next hop is not itself a symlink we reached a real file/dir.
        if std::fs::symlink_metadata(&current)
            .map(|m| !m.file_type().is_symlink())
            .unwrap_or(true)
        {
            return false;
        }
    }
    // Exceeded 40 hops without resolving — treat as circular.
    true
}

pub fn normalize_path(path: &Path) -> PathBuf {
    use std::path::Component;

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some() {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
        }
    }
    normalized
}

/// Returns `Some(message)` when the path exists but we cannot read it due to
/// permissions, `None` otherwise (absent path is a plain broken link).
fn permission_error_message(path: &Path) -> Option<String> {
    match std::fs::metadata(path) {
        Ok(_) => None, // accessible — no permission problem
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            Some(format!("{}: permission denied", path.display()))
        }
        #[cfg(windows)]
        Err(e) if e.raw_os_error() == Some(5) => {
            // ERROR_ACCESS_DENIED on Windows
            Some(format!("{}: access denied", path.display()))
        }
        Err(_) => None, // all other errors = absent target (broken link)
    }
}

/// 20-character ASCII progress bar: `█` filled, `░` empty.
fn health_bar(pct: u32) -> String {
    let filled = (pct.min(100) * 20 / 100) as usize;
    let empty = 20usize.saturating_sub(filled);
    format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
}

/// Relative path from `root` for display; falls back to absolute.
fn rel(path: &Path, root: &Path) -> PathBuf {
    pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_symlink_cycle_without_canonicalizing_it() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        #[cfg(unix)]
        std::os::unix::fs::symlink("second", &first).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file("second", &first).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("first", &second).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file("first", &second).unwrap();

        assert!(is_circular(&first));
    }

    #[test]
    fn does_not_report_a_missing_target_as_a_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("missing-link");
        #[cfg(unix)]
        std::os::unix::fs::symlink("missing-target", &link).unwrap();
        #[cfg(windows)]
        std::os::windows::fs::symlink_file("missing-target", &link).unwrap();

        assert!(!is_circular(&link));
    }
}
