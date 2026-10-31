//! Phase 4 (M4) – git integration (plan.md §2 Phase 3, second half).
//!
//! * [`check`] fails the build when the tree contains a committed symlink with
//!   an **absolute** target — those break the moment the repo is cloned to
//!   another machine or path.
//! * [`install_hooks`] drops `post-checkout` / `post-merge` hooks that replay
//!   `.smartlink.json` automatically, and [`uninstall_hooks`] removes just the
//!   block this tool manages, leaving any user hook code intact.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::commands::init::DEFAULT_MANIFEST_NAME;
use crate::link::{self, LinkKind};

/// Wraps the block this tool owns inside a hook file so install/uninstall are
/// idempotent and never clobber a teammate's own hook logic.
const MARKER: &str = "# >>> smartlink >>>";
const END_MARKER: &str = "# <<< smartlink <<<";
const MANAGED_HOOKS: [&str; 2] = ["post-checkout", "post-merge"];

/// Exit non-zero if any symlink under `path` stores an absolute target.
pub fn check(path: &Path) -> Result<()> {
    let root = std::fs::canonicalize(path)
        .with_context(|| format!("cannot scan {}", path.display()))?;
    let mut offenders: Vec<(PathBuf, PathBuf)> = Vec::new();
    walk(&root, 0, &mut offenders);

    if offenders.is_empty() {
        println!(
            "{} no absolute symlinks under {}",
            "✓".green().bold(),
            root.display()
        );
        return Ok(());
    }
    for (link, target) in &offenders {
        println!(
            "  {} {} -> {}",
            "absolute symlink".red().bold(),
            rel(link, &root).display(),
            target.display()
        );
    }
    bail!(
        "git check failed: {} absolute symlink(s) found; re-create them relative \
         (`smartlink make --symlink --relative --force`) so they survive cloning",
        offenders.len()
    )
}

/// Install `post-checkout` / `post-merge` hooks that run `smartlink sync`.
pub fn install_hooks(path: &Path, force: bool) -> Result<()> {
    let git_dir = find_git_dir(path)
        .with_context(|| format!("no git repository found at or above {}", path.display()))?;
    let hooks = git_dir.join("hooks");
    std::fs::create_dir_all(&hooks)
        .with_context(|| format!("could not create {}", hooks.display()))?;

    let block = hook_block();
    for name in MANAGED_HOOKS {
        let dest = hooks.join(name);
        match std::fs::read_to_string(&dest) {
            // Our block already present -> refresh it in place.
            Ok(existing) if existing.contains(MARKER) => {
                write_hook(&dest, &replace_block(&existing, &block))?;
                println!("{} updated {name}", "hook".green().bold());
            }
            // Someone else's hook -> only touch it when told to.
            Ok(existing) => {
                if !force {
                    bail!(
                        "{} hook already exists without a smartlink block; \
                         re-run with --force to append ours",
                        dest.display()
                    );
                }
                let mut s = existing;
                if !s.ends_with('\n') {
                    s.push('\n');
                }
                s.push_str(&block);
                s.push('\n');
                write_hook(&dest, &s)?;
                println!("{} appended to {name}", "hook".green().bold());
            }
            // Nothing there yet -> create a fresh hook.
            Err(_) => {
                let script = format!("#!/bin/sh\nset -e\n{block}\n");
                write_hook(&dest, &script)?;
                println!("{} created {name}", "hook".green().bold());
            }
        }
    }
    println!(
        "{} git will now run `smartlink sync` after checkout and merge",
        "done".green().bold()
    );
    Ok(())
}

/// Remove the managed block; delete the hook file if nothing meaningful is left.
pub fn uninstall_hooks(path: &Path) -> Result<()> {
    let git_dir = find_git_dir(path)
        .with_context(|| format!("no git repository found at or above {}", path.display()))?;
    let hooks = git_dir.join("hooks");
    for name in MANAGED_HOOKS {
        let dest = hooks.join(name);
        match std::fs::read_to_string(&dest) {
            Ok(existing) if existing.contains(MARKER) => {
                let trimmed = remove_block(&existing);
                if is_trivial_hook(&trimmed) {
                    std::fs::remove_file(&dest).ok();
                    println!("{} removed {name}", "hook".green().bold());
                } else {
                    write_hook(&dest, &trimmed)?;
                    println!("{} stripped smartlink block from {name}", "hook".green().bold());
                }
            }
            _ => println!("{} {name}: not installed", "-".dimmed()),
        }
    }
    Ok(())
}

/// The shell fragment written between the markers: only acts when a manifest is
/// present and `smartlink` is on PATH, and never fails the git operation.
fn hook_block() -> String {
    format!(
        "{MARKER}\n\
         # Managed by `smartlink git install-hooks` - recreates links after git ops.\n\
         if [ -f {manifest} ] && command -v smartlink >/dev/null 2>&1; then\n\
         \x20 smartlink sync || echo \"smartlink sync failed\" >&2\n\
         fi\n\
         {END_MARKER}",
        manifest = DEFAULT_MANIFEST_NAME
    )
}

fn write_hook(dest: &Path, content: &str) -> Result<()> {
    std::fs::write(dest, content)
        .with_context(|| format!("could not write hook {}", dest.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dest, std::fs::Permissions::from_mode(0o755)).ok();
    }
    Ok(())
}

/// Replace everything between the markers with a fresh `block`, keeping the
/// user's surrounding hook code untouched.
fn replace_block(existing: &str, block: &str) -> String {
    match existing.find(MARKER) {
        Some(i) => {
            let mut out = String::from(&existing[..i]);
            out.push_str(block);
            if let Some(j) = existing[i..].find(END_MARKER) {
                out.push_str(&existing[i + j + END_MARKER.len()..]);
            }
            out
        }
        None => {
            let mut out = existing.to_string();
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push_str(block);
            out
        }
    }
}

/// Cut the whole managed block (markers included) plus its trailing newline.
fn remove_block(existing: &str) -> String {
    if let Some(i) = existing.find(MARKER) {
        if let Some(j) = existing[i..].find(END_MARKER) {
            let after = i + j + END_MARKER.len();
            let mut tail = &existing[after..];
            if let Some(stripped) = tail.strip_prefix('\n') {
                tail = stripped;
            }
            return format!("{}{}", &existing[..i], tail);
        }
    }
    existing.to_string()
}

/// True when a hook holds nothing but a shebang / `set -e` / blank lines, so it
/// is safe to delete after the managed block is stripped out.
fn is_trivial_hook(content: &str) -> bool {
    content
        .lines()
        .all(|l| l.trim().is_empty() || l.starts_with("#!") || l.trim() == "set -e")
}

/// Locate the `.git` directory at or above `path`, resolving worktree/submodule
/// indirection where `.git` is a file containing `gitdir: <path>`.
fn find_git_dir(path: &Path) -> Option<PathBuf> {
    let mut dir = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };
    loop {
        let cand = dir.join(".git");
        if cand.is_dir() {
            return Some(cand);
        }
        if cand.is_file() {
            if let Ok(txt) = std::fs::read_to_string(&cand) {
                if let Some(rest) = txt.trim().strip_prefix("gitdir:") {
                    let p = PathBuf::from(rest.trim());
                    return Some(if p.is_absolute() { p } else { dir.join(p) });
                }
            }
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Path relative to `root`, falling back to the absolute path.
fn rel(path: &Path, root: &Path) -> PathBuf {
    pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf())
}

/// Depth-capped walk that flags absolute-target symlinks. Never descends through
/// a link, and skips `.git` so hook/branch internals are not scanned.
fn walk(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, PathBuf)>) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .map(|n| n == ".git" || n == DEFAULT_MANIFEST_NAME)
            .unwrap_or(false)
        {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(&path) else { continue };
        if let Ok(Some(info)) = link::detect(&path) {
            // Only symlinks are committed by git; junctions / bind mounts are
            // machine-local and hardlinks show up as independent files.
            if info.kind == LinkKind::Symlink && info.target.is_absolute() {
                out.push((info.path.clone(), info.target.clone()));
            }
            continue; // never walk *through* a link
        }
        if meta.is_dir() {
            walk(&path, depth + 1, out);
        }
    }
}
