//! `slink explain` – explain why a link type is (or was) selected.

use std::path::Path;

use anyhow::{bail, Result};

use crate::commands::make::choose_kind;
use crate::fs;
use crate::link::{self, LinkKind};

pub fn run(target: &Path, link_path: Option<&Path>) -> Result<()> {
    // An existing link: describe it.
    if link_path.is_none() {
        if let Ok(Some(info)) = link::detect(target) {
            println!("{} is a {}", target.display(), info.kind);
            println!("  stored target: {}", info.target.display());
            println!("  status: {}", if info.alive { "alive" } else { "broken" });
            return Ok(());
        }
    }
    if !target.exists() {
        bail!("{} does not exist", target.display());
    }
    let default_link = Path::new("link");
    let lp = link_path.unwrap_or(default_link);
    let lp_abs = if lp.is_absolute() {
        lp.to_path_buf()
    } else {
        std::env::current_dir()?.join(lp)
    };
    let kind = choose_kind(target, &lp_abs, false, false, false)?;

    let is_file = target.is_file();
    let same_vol = fs::same_volume(target, &lp_abs);
    println!("Target: {}", target.display());
    println!(
        "  type:        {}",
        if is_file { "file" } else { "directory" }
    );
    println!("  same volume: {}", if same_vol { "yes" } else { "no" });
    println!("  OS:          {}", std::env::consts::OS);
    println!("Selected link type: {kind}");
    let reason = match kind {
        LinkKind::Hardlink => "a file on the same volume can share its data via a hardlink: no dangling risk and no extra privileges",
        LinkKind::Junction => "directories on Windows use junctions: they need no elevated privilege",
        LinkKind::Symlink if is_file && !same_vol => "the file is on a different volume, so a hardlink is impossible; a symlink is used",
        LinkKind::Symlink => "directories on this OS use symlinks (relative when the target is relative, so the tree stays portable)",
        LinkKind::BindMount => "bind mounts are used only when explicitly requested",
    };
    println!("Why: {reason}");
    println!("Override with --hard, --symlink or --junction, or set `default_type` via `slink config set`.");
    if let Ok(Some((v, src))) = crate::state::effective_setting("default_type") {
        println!("Note: default_type={v} ({src}) applies to `slink make` when no flag is given.");
    }
    Ok(())
}
