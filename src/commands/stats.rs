//! `slink stats` – filesystem link statistics.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::commands::doctor;
use crate::link::{self, LinkKind};

#[derive(Default)]
struct Counts {
    files: u64,
    dirs: u64,
    hardlinked_files: u64,
}

fn count(dir: &Path, depth: usize, c: &mut Counts) {
    if depth > 32 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        let Ok(m) = std::fs::symlink_metadata(&p) else {
            continue;
        };
        if matches!(link::detect(&p), Ok(Some(_))) {
            continue;
        }
        if m.is_dir() {
            c.dirs += 1;
            count(&p, depth + 1, c);
        } else if m.is_file() {
            c.files += 1;
            if link::hardlink_count(&p).unwrap_or(1) > 1 {
                c.hardlinked_files += 1;
            }
        }
    }
}

pub fn run(root: &Path, json: bool) -> Result<()> {
    let root =
        std::fs::canonicalize(root).with_context(|| format!("cannot read {}", root.display()))?;
    let root = link::strip_verbatim(root);

    let mut c = Counts::default();
    count(&root, 0, &mut c);
    let mut links = Vec::new();
    doctor::walk(&root, 0, &mut links);

    let mut per_kind: BTreeMap<String, u64> = BTreeMap::new();
    let (mut broken, mut abs, mut rel) = (0u64, 0u64, 0u64);
    for l in &links {
        *per_kind.entry(l.kind.to_string()).or_default() += 1;
        if !l.alive {
            broken += 1;
        }
        if l.kind == LinkKind::Symlink {
            if l.target.is_absolute() {
                abs += 1;
            } else {
                rel += 1;
            }
        }
    }

    if json {
        let v = serde_json::json!({
            "root": root.display().to_string(),
            "files": c.files,
            "directories": c.dirs,
            "links": links.len(),
            "by_kind": per_kind,
            "broken": broken,
            "absolute_symlinks": abs,
            "relative_symlinks": rel,
            "hardlinked_files": c.hardlinked_files,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    println!("Link statistics for {}", root.display());
    println!("  files:              {}", c.files);
    println!("  directories:        {}", c.dirs);
    println!("  links:              {}", links.len());
    for (k, n) in &per_kind {
        println!("    {k:<10}        {n}");
    }
    println!("  broken:             {broken}");
    println!("  relative symlinks:  {rel}");
    println!("  absolute symlinks:  {abs}");
    println!("  hardlinked files:   {}", c.hardlinked_files);
    Ok(())
}
