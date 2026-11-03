//! `slink tree` – visualise (nested) link structures.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use crate::link::{self, LinkKind};

pub fn run(root: &Path, max_depth: usize, follow: bool, links_only: bool) -> Result<()> {
    let root_abs = link::strip_verbatim(
        std::fs::canonicalize(root).with_context(|| format!("cannot read {}", root.display()))?,
    );
    println!("{}", root.display().to_string().bold());
    let mut stack = vec![root_abs.clone()];
    let lines = build(&root_abs, "", 0, max_depth, follow, links_only, &mut stack);
    for l in &lines {
        println!("{l}");
    }
    let n = lines.iter().filter(|l| l.contains(" -> ")).count();
    println!("\n{n} link(s)");
    Ok(())
}

fn kind_label(k: LinkKind) -> colored::ColoredString {
    match k {
        LinkKind::Symlink => "symlink".cyan(),
        LinkKind::Junction => "junction".magenta(),
        LinkKind::BindMount => "bind".yellow(),
        LinkKind::Hardlink => "hardlink".green(),
    }
}

/// Targets reached by following link-to-link hops from `first`.
fn chain(first: PathBuf) -> Vec<PathBuf> {
    let mut hops = Vec::new();
    let mut cur = first;
    for _ in 0..8 {
        match link::detect(&cur) {
            Ok(Some(info)) => {
                cur = link::resolve_target(&info);
                hops.push(cur.clone());
            }
            _ => break,
        }
    }
    hops
}

fn build(
    dir: &Path,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    follow: bool,
    links_only: bool,
    stack: &mut Vec<PathBuf>,
) -> Vec<String> {
    let mut out = Vec::new();
    if depth >= max_depth {
        return out;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return out;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();

    let mut rows: Vec<(String, Vec<String>)> = Vec::new();
    for path in entries {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if let Ok(Some(info)) = link::detect(&path) {
            let resolved = link::resolve_target(&info);
            let mut label = format!(
                "{} -> {} [{}]",
                name,
                info.target.display(),
                kind_label(info.kind)
            );
            for h in chain(resolved.clone()) {
                label.push_str(&format!(" => {}", h.display()));
            }
            if !info.alive {
                label.push_str(&format!(" {}", "(broken)".red().bold()));
            }
            let mut children = Vec::new();
            if follow && info.alive && resolved.is_dir() {
                if let Ok(canon) = std::fs::canonicalize(&resolved) {
                    let canon = link::strip_verbatim(canon);
                    if stack.contains(&canon) {
                        label.push_str(&format!(" {}", "(cycle)".red()));
                    } else {
                        stack.push(canon.clone());
                        children =
                            build(&canon, "", depth + 1, max_depth, follow, links_only, stack);
                        stack.pop();
                    }
                }
            }
            rows.push((label, children));
        } else if meta.is_dir() {
            let children = build(&path, "", depth + 1, max_depth, follow, links_only, stack);
            if !links_only || !children.is_empty() {
                rows.push((name.blue().to_string(), children));
            }
        } else if !links_only {
            rows.push((name, Vec::new()));
        }
    }

    let count = rows.len();
    for (i, (label, children)) in rows.into_iter().enumerate() {
        let last = i + 1 == count;
        out.push(format!(
            "{prefix}{}{label}",
            if last { "└── " } else { "├── " }
        ));
        let child_prefix = if last { "    " } else { "│   " };
        for c in children {
            out.push(format!("{prefix}{child_prefix}{c}"));
        }
    }
    out
}
