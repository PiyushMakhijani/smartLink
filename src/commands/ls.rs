//! `slink ls` – visualise links in a directory (plan.md §2 Phase 1).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use crate::link::{self, LinkInfo, LinkKind};

/// Links (and optionally hardlink groups) found directly inside `dir`.
pub struct Scan {
    pub links: Vec<LinkInfo>,
    /// file-id -> names, only files with link count > 1 (when requested).
    pub hard_groups: BTreeMap<u64, Vec<PathBuf>>,
}

/// Non-recursive scan shared by `ls` and the interactive prompts.
pub fn scan(dir: &Path, show_hardlinks: bool) -> Result<Scan> {
    let entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("could not read directory {}", dir.display()))?
        .filter_map(|e| e.ok())
        .collect();

    let mut links: Vec<LinkInfo> = Vec::new();
    let mut hard_groups: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();

    for entry in &entries {
        let path = entry.path();
        if let Some(info) = link::detect(&path)? {
            links.push(info);
        } else if show_hardlinks {
            let is_file = entry.metadata().map(|m| m.is_file()).unwrap_or(false);
            if is_file {
                if let (Ok(n), Ok(Some(id))) = (link::hardlink_count(&path), link::file_id(&path)) {
                    if n > 1 {
                        hard_groups.entry(id).or_default().push(path);
                    }
                }
            }
        }
    }

    links.sort_by_key(|r| r.path.clone());
    Ok(Scan { links, hard_groups })
}

pub fn run(dir: &Path, show_hardlinks: bool) -> Result<()> {
    let scan = scan(dir, show_hardlinks)?;
    let rows = &scan.links;
    let hard_groups = &scan.hard_groups;

    if rows.is_empty() && hard_groups.is_empty() {
        println!(
            "{} no links found in {}",
            "◇".yellow(),
            dir.display().to_string().bold()
        );
        return Ok(());
    }

    println!(
        "{:<9} {:<6} {:<32} {}",
        "TYPE".bold(),
        "STATUS".bold(),
        "NAME".bold(),
        "TARGET".bold()
    );
    println!("{}", "-".repeat(72));

    for info in rows {
        let status = if info.alive {
            "OK".green().to_string()
        } else {
            "DEAD".red().bold().to_string()
        };
        let name = info
            .path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        println!(
            "{:<9} {:<6} {:<32} {}",
            info.kind.to_string(),
            status,
            name,
            info.target.display()
        );
    }

    for (id, files) in hard_groups {
        println!(
            "{:<9} {:<6} {:<32} inode/file-id {id} ({} links)",
            LinkKind::Hardlink.to_string(),
            "OK".green(),
            format!("group of {}", files.len()),
            files
                .iter()
                .map(|f| f
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    Ok(())
}
