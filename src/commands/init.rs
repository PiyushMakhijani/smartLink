//! `slink init` – generate a `.smartlink.json` manifest from the links that
//! already exist in a directory tree (plan.md §2 Phase 3).
//!
//! The scan records every reparse-point link (symlink / junction / bind mount)
//! it finds, plus — with `--hardlinks` — groups of files that share an inode /
//! file-id. Link and target paths are stored relative to the manifest root so
//! the result is portable; absolute targets (e.g. a junction) are kept as-is.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use colored::Colorize;

use crate::link;
use crate::manifest::{self, LinkEntry, LinkType, Manifest, SCHEMA_VERSION};

pub const DEFAULT_MANIFEST_NAME: &str = ".smartlink.json";

/// Scan `root` and emit a manifest. `out` overrides the destination file,
/// `dry_run` prints JSON to stdout instead of writing, and `include_hardlinks`
/// additionally records same-volume hardlink groups.
pub fn run(
    root: &Path,
    out: Option<PathBuf>,
    description: Option<String>,
    include_hardlinks: bool,
    dry_run: bool,
) -> Result<()> {
    let root_abs = link::strip_verbatim(
        std::fs::canonicalize(root)
            .with_context(|| format!("cannot scan {}", root.display()))?,
    );

    let mut links: Vec<LinkEntry> = Vec::new();
    let mut hard_groups: BTreeMap<u64, Vec<PathBuf>> = BTreeMap::new();
    walk(&root_abs, &root_abs, 0, include_hardlinks, &mut links, &mut hard_groups);

    if include_hardlinks {
        for (_id, names) in hard_groups {
            if names.len() < 2 {
                continue;
            }
            // The first name in each group becomes the canonical target; the
            // rest become hardlink entries pointing back at it.
            let target = &names[0];
            for name in &names[1..] {
                links.push(LinkEntry {
                    link: rel(name, &root_abs),
                    target: rel(target, &root_abs),
                    link_type: LinkType::Hardlink,
                    os_specific: Default::default(),
                });
            }
        }
    }

    links.sort_by(|a, b| a.link.cmp(&b.link));
    let manifest = Manifest {
        version: SCHEMA_VERSION,
        description,
        links,
    };
    let json = serde_json::to_string_pretty(&manifest)? + "\n";

    if dry_run {
        print!("{json}");
        return Ok(());
    }
    let dest = out.unwrap_or_else(|| root_abs.join(DEFAULT_MANIFEST_NAME));
    std::fs::write(&dest, &json)
        .with_context(|| format!("could not write manifest {}", dest.display()))?;
    println!(
        "{} {} ({} link(s)) -> {}",
        "wrote".green().bold(),
        manifest::current_os_key().dimmed(),
        manifest.links.len(),
        dest.display()
    );
    Ok(())
}

/// Path relative to `root`, falling back to the absolute path (verbatim prefix
/// stripped) when there is no common ancestor, e.g. a junction to another drive.
fn rel(path: &Path, root: &Path) -> PathBuf {
    pathdiff::diff_paths(path, root)
        .unwrap_or_else(|| link::strip_verbatim(path.to_path_buf()))
}

/// Recursive walk. Detects reparse links directly and, when asked, buckets
/// regular files by file-id to discover hardlink groups. Never descends through
/// a link (that could loop), and depth-caps like `fix` (plan.md §5).
fn walk(
    dir: &Path,
    root: &Path,
    depth: usize,
    include_hardlinks: bool,
    links: &mut Vec<LinkEntry>,
    hard_groups: &mut BTreeMap<u64, Vec<PathBuf>>,
) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.file_name().map(|n| n == DEFAULT_MANIFEST_NAME).unwrap_or(false) {
            continue; // never capture our own output
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };

        if let Ok(Some(info)) = link::detect(&path) {
            // A reparse link: store its resolved (absolute) target so `sync`
            // can rebuild it faithfully, preferring a root-relative form.
            let resolved = link::resolve_target(&info);
            links.push(LinkEntry {
                link: rel(&info.path, root),
                target: rel(&resolved, root),
                link_type: LinkType::from_kind(info.kind),
                os_specific: Default::default(),
            });
            continue; // never walk *through* a link
        }

        if meta.is_dir() {
            walk(&path, root, depth + 1, include_hardlinks, links, hard_groups);
        } else if include_hardlinks && meta.is_file() {
            if let (Ok(n), Ok(Some(id))) = (link::hardlink_count(&path), link::file_id(&path)) {
                if n > 1 {
                    hard_groups.entry(id).or_default().push(path);
                }
            }
        }
    }
}
