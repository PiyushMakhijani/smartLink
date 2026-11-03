//! `slink convert` – morph links into another type through a transaction
//! (plan.md §2 Phase 2).
//!
//! The command first builds a plan (one operation per link), then either
//! prints it (`--dry-run`) or executes it as a recorded transaction
//! (`--apply`) that can be undone with `slink transaction rollback`. Passing a
//! directory only ever previews unless `--apply` is given; a single link keeps
//! the original behaviour of converting immediately.
//!
//! Safety: dead links are skipped unless `--preserve-broken` is used, real
//! files and directories are never removed, and a hardlink only gives up its
//! name when another name still holds the content.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use super::doctor;
use crate::link::{self, LinkInfo, LinkKind};
use crate::manifest::LinkType;
use crate::transaction::{Op, Transaction};

pub fn run(
    path: &Path,
    to: &str,
    preserve_broken: bool,
    yes: bool,
    dry_run: bool,
    apply: bool,
) -> Result<()> {
    let want = parse_want(to)?;
    if let Some(reason) = unsupported(want) {
        bail!("{reason}");
    }

    let single = link::detect(path)?.is_some() || is_hardlinked_file(path);
    let mut ops: Vec<Op> = Vec::new();
    let mut skipped: Vec<(PathBuf, String)> = Vec::new();

    if single {
        let info = describe(path)?;
        let op = plan_one(&info, want, preserve_broken, yes, !dry_run)
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display().to_string().bold()))?;
        ops.push(op);
    } else if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        let mut found = Vec::new();
        doctor::walk(path, 0, &mut found);
        found.sort_by(|a, b| a.path.cmp(&b.path));
        for info in found {
            match plan_one(&info, want, preserve_broken, true, false) {
                Ok(op) => ops.push(op),
                Err(e) => skipped.push((info.path.clone(), e.to_string())),
            }
        }
    } else {
        bail!("{} is not a link slink can convert", path.display());
    }

    print_plan(&ops, &skipped, want);

    if ops.is_empty() {
        println!("nothing to convert");
        return Ok(());
    }
    if dry_run {
        println!("{} no changes made", "dry-run:".yellow());
        return Ok(());
    }
    if !single && !apply {
        println!(
            "{} re-run with --apply to execute this plan",
            "preview only:".yellow()
        );
        return Ok(());
    }

    let mut tx = Transaction::new(&format!("convert {} --to {want}", path.display()), ops);
    tx.execute()?;
    println!(
        "{} {} link(s) in transaction {}",
        "converted".green().bold(),
        tx.ops.len(),
        tx.id.bold()
    );
    println!("   undo with: slink transaction rollback {}", tx.id);
    Ok(())
}

fn print_plan(ops: &[Op], skipped: &[(PathBuf, String)], want: LinkKind) {
    for op in ops {
        println!(
            "  {} {} ({} -> {})",
            "convert".green(),
            op.path.display(),
            op.from_type.to_kind().to_string().yellow(),
            want.to_string().green()
        );
        println!("          target: {}", op.to_target.display());
    }
    for (p, why) in skipped {
        println!("  {} {}: {why}", "skip".dimmed(), p.display());
    }
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

fn unsupported(want: LinkKind) -> Option<&'static str> {
    match want {
        LinkKind::Junction if !cfg!(windows) => Some("junctions are only available on Windows"),
        LinkKind::BindMount if !cfg!(target_os = "linux") => {
            Some("bind mounts are only available on Linux")
        }
        _ => None,
    }
}

fn is_hardlinked_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.is_file())
        && link::hardlink_count(path).is_ok_and(|n| n > 1)
}

fn describe(path: &Path) -> Result<LinkInfo> {
    if let Some(info) = link::detect(path)? {
        return Ok(info);
    }
    if is_hardlinked_file(path) {
        return Ok(LinkInfo {
            path: path.to_path_buf(),
            kind: LinkKind::Hardlink,
            target: PathBuf::new(),
            alive: true,
            nlinks: link::hardlink_count(path)?,
            file_id: link::file_id(path)?,
        });
    }
    bail!("{} is not a link slink can convert", path.display())
}

/// Build the operation that turns `info` into `want`, or explain why it can't.
fn plan_one(
    info: &LinkInfo,
    want: LinkKind,
    preserve_broken: bool,
    yes: bool,
    may_prompt: bool,
) -> Result<Op> {
    if info.kind == want {
        bail!("already a {want}");
    }
    let from_relative;
    let target: PathBuf = if info.kind == LinkKind::Hardlink {
        from_relative = false;
        hardlink_target(&info.path, info, yes, may_prompt)?
    } else {
        from_relative = !info.target.is_absolute();
        link::resolve_target(info)
    };

    if !target.exists() && !preserve_broken {
        bail!(
            "dead link (target {} missing); re-link it with `slink fix` or convert with \
             --preserve-broken",
            target.display()
        );
    }
    match want {
        LinkKind::Hardlink => {
            if !target.is_file() {
                bail!("hardlinks need a regular-file target");
            }
            if !crate::fs::same_volume(&target, &info.path) {
                bail!("hardlink requires target and link on the same volume");
            }
        }
        LinkKind::Junction | LinkKind::BindMount if !target.is_dir() && cfg!(windows) => {
            bail!("a junction needs a directory target");
        }
        _ => {}
    }

    Ok(Op {
        path: info.path.clone(),
        from_type: LinkType::from_kind(info.kind),
        from_target: target.clone(),
        from_relative,
        to_type: LinkType::from_kind(want),
        to_target: target,
        applied: false,
    })
}

/// A hardlink stores no target string; find its content twin to use as the
/// conversion target (plan.md §2: prompt when multiple targets exist).
fn hardlink_target(path: &Path, info: &LinkInfo, yes: bool, may_prompt: bool) -> Result<PathBuf> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
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
        bail!(
            "cannot convert hardlink: no second name in the same directory shares the content \
             (inode/file-id {})",
            info.file_id.unwrap_or(0)
        );
    }
    if twins.len() > 1 && !yes && may_prompt {
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
            .interact()
            .context("could not prompt (pass --yes)")?;
        if !chosen {
            bail!("conversion cancelled");
        }
    }
    Ok(twins.remove(0))
}
