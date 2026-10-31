//! `slink convert` – morph one link type into another (plan.md §2 Phase 2).
//!
//! Logic: read target -> delete old link -> create new link (via the shared
//! `make::create_link` with force). Safety: a dead link cannot be converted
//! unless `--preserve-broken` is used; hardlink conversions with several twins
//! prompt unless `--yes` is given.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::link::{self, LinkKind};
use super::make;

pub fn run(link_path: &Path, to: &str, preserve_broken: bool, yes: bool) -> Result<()> {
    let want = parse_want(to)?;
    let info = link::detect(link_path)?
        .with_context(|| format!("{} is not a link slink can convert", link_path.display()))?;

    let target: PathBuf = if info.kind == LinkKind::Hardlink {
        hardlink_target(link_path, &info, yes)?
    } else {
        link::resolve_target(&info)
    };

    if !target.exists() && !preserve_broken {
        bail!(
            "{} is dead (target {} missing); re-link it with `slink fix` or convert \
             with --preserve-broken",
            link_path.display().to_string().bold(),
            target.display()
        );
    }

    make::create_link(want, &target, link_path, true, false).with_context(|| {
        format!(
            "could not convert {} ({}) to {}",
            link_path.display(),
            info.kind,
            want
        )
    })?;
    println!(
        "{} {} ({} -> {})",
        "converted".green().bold(),
        link_path.display(),
        info.kind.to_string().yellow(),
        want.to_string().green(),
    );
    println!("   target: {}", target.display());
    Ok(())
}

fn parse_want(to: &str) -> Result<LinkKind> {
    match to.to_ascii_lowercase().as_str() {
        "hard" | "hardlink" => Ok(LinkKind::Hardlink),
        "symlink" => Ok(LinkKind::Symlink),
        "junction" => Ok(LinkKind::Junction),
        "bind" | "bindmount" | "bind-mount" => Ok(LinkKind::BindMount),
        other => bail!("unknown link type '{other}' (use hard, symlink, junction or bind)"),
    }
}

/// A hardlink stores no target string; find its content twin to use as the
/// conversion target (plan.md §2: prompt when multiple targets exist).
fn hardlink_target(path: &Path, info: &crate::link::LinkInfo, yes: bool) -> Result<PathBuf> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let mut twins = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p == *path {
                continue;
            }
            if let (Ok(Some(id)), Some(my)) = (link::file_id(&p), info.file_id) {
                if id == my && p.is_file() {
                    twins.push(p);
                }
            }
        }
    }
    if twins.is_empty() {
        // Only this name holds the content: converting would destroy it.
        bail!(
            "cannot convert hardlink {}: no second name shares the content (inode/file-id {}); \
             pass an explicit `make --hard` instead",
            path.display(),
            info.file_id.unwrap_or(0)
        );
    }
    if twins.len() > 1 && !yes {
        let list = twins
            .iter()
            .map(|t| t.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let chosen = dialoguer::Confirm::new()
            .with_prompt(format!(
                "{} shares content with [{list}] - use first as the link target?",
                path.display()
            ))
            .default(true)
            .interact()?;
        if !chosen {
            bail!("conversion cancelled");
        }
    }
    Ok(twins.remove(0))
}
