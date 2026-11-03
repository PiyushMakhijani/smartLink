//! `slink export --portable`, `slink import` and `slink validate --portable` –
//! carry a link structure between machines (e.g. Windows -> Fedora).
//!
//! A portable manifest uses `/` separators and relative targets only. Link
//! types that exist on a single OS are stored as `symlink` with an
//! `os_specific` override that keeps the native type where it came from:
//!
//! | exported type | base type | override                      |
//! |---------------|-----------|-------------------------------|
//! | junction      | symlink   | windows: junction             |
//! | bindmount     | symlink   | linux: bindmount              |
//! | symlink / hardlink | unchanged | none                     |
//!
//! `import` applies the same rules for the machine it runs on, so even a plain
//! manifest never asks Linux for a junction or Windows for a bind mount.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use super::{init, sync};
use crate::manifest::{self, LinkEntry, LinkType, Manifest, OsOverride};

pub const PORTABLE_MANIFEST_NAME: &str = "smartlink-manifest.json";

fn slashes(p: &Path) -> PathBuf {
    PathBuf::from(p.to_string_lossy().replace('\\', "/"))
}

fn looks_absolute(s: &str) -> bool {
    let b = s.as_bytes();
    s.starts_with('/') || (b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic())
}

/// The type `ty` should have on `os`, or itself when the OS supports it.
pub fn type_for_os(ty: LinkType, os: &str) -> LinkType {
    match ty {
        LinkType::Junction if os != "windows" => LinkType::Symlink,
        LinkType::BindMount if os != "linux" => LinkType::Symlink,
        other => other,
    }
}

/// Convert one entry to its portable form; returns notes about what changed.
fn portabilize(entry: &LinkEntry, notes: &mut Vec<String>) -> LinkEntry {
    let mut out = entry.clone();
    out.link = slashes(&entry.link);
    out.target = slashes(&entry.target);
    if looks_absolute(&out.target.to_string_lossy()) {
        notes.push(format!(
            "{}: absolute target {} is machine-specific",
            out.link.display(),
            out.target.display()
        ));
    }
    let mut overrides: HashMap<String, OsOverride> = HashMap::new();
    match entry.link_type {
        LinkType::Junction => {
            out.link_type = LinkType::Symlink;
            overrides.insert(
                "windows".into(),
                OsOverride {
                    target: None,
                    link_type: Some(LinkType::Junction),
                },
            );
            notes.push(format!(
                "{}: junction -> symlink on linux/macos",
                out.link.display()
            ));
        }
        LinkType::BindMount => {
            out.link_type = LinkType::Symlink;
            overrides.insert(
                "linux".into(),
                OsOverride {
                    target: None,
                    link_type: Some(LinkType::BindMount),
                },
            );
            notes.push(format!(
                "{}: bind mount -> symlink on windows/macos",
                out.link.display()
            ));
        }
        _ => {}
    }
    for (os, ov) in &entry.os_specific {
        overrides.insert(
            os.clone(),
            OsOverride {
                target: ov.target.as_deref().map(slashes),
                link_type: ov.link_type,
            },
        );
    }
    out.os_specific = overrides;
    out
}

pub fn export(
    path: &Path,
    out: Option<PathBuf>,
    description: Option<String>,
    hardlinks: bool,
    portable: bool,
) -> Result<()> {
    let (root, mut m) = init::build_manifest(path, description, hardlinks)?;
    let mut notes = Vec::new();
    if portable {
        m.links = m.links.iter().map(|e| portabilize(e, &mut notes)).collect();
        m.portable = true;
        m.source_os = Some(manifest::current_os_key().to_string());
    }
    let json = serde_json::to_string_pretty(&m)? + "\n";
    let default_name = if portable {
        PORTABLE_MANIFEST_NAME
    } else {
        init::DEFAULT_MANIFEST_NAME
    };
    let dest = out.unwrap_or_else(|| root.join(default_name));
    std::fs::write(&dest, json)
        .with_context(|| format!("could not write manifest {}", dest.display()))?;
    println!(
        "{} {} link(s) -> {}",
        "exported".green().bold(),
        m.links.len(),
        dest.display()
    );
    for n in notes {
        println!("  {} {n}", "note".yellow());
    }
    Ok(())
}

/// Problems found in a manifest; errors make a manifest unusable elsewhere.
pub struct Report {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn check_portable(m: &Manifest) -> Report {
    let mut r = Report {
        errors: Vec::new(),
        warnings: Vec::new(),
    };
    if !m.portable {
        r.warnings
            .push("manifest was not created with `export --portable`".into());
    }
    let mut seen: HashMap<String, String> = HashMap::new();
    for e in &m.links {
        let link = e.link.to_string_lossy().into_owned();
        let target = e.target.to_string_lossy().into_owned();
        if link.contains('\\') || target.contains('\\') {
            r.errors
                .push(format!("{link}: path uses '\\' separators; use '/'"));
        }
        if looks_absolute(&link) {
            r.errors.push(format!("{link}: link path must be relative"));
        }
        if link.split(['/', '\\']).any(|c| c == "..") {
            r.errors
                .push(format!("{link}: link path escapes the project root"));
        }
        if link
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*'))
        {
            r.errors.push(format!(
                "{link}: name contains characters invalid on Windows"
            ));
        }
        if looks_absolute(&target) {
            r.errors.push(format!(
                "{link}: absolute target '{target}' will not exist on another machine"
            ));
        }
        if let Some(prev) = seen.insert(link.to_lowercase(), link.clone()) {
            r.errors.push(format!(
                "{link}: collides with '{prev}' on case-insensitive filesystems"
            ));
        }
        for (os, ov) in &e.os_specific {
            if let Some(t) = &ov.target {
                if looks_absolute(&t.to_string_lossy()) {
                    r.errors.push(format!(
                        "{link}: absolute {os} override target '{}'",
                        t.display()
                    ));
                }
            }
        }
        for os in ["windows", "linux", "macos"] {
            let ty = e
                .os_specific
                .get(os)
                .and_then(|o| o.link_type)
                .unwrap_or(e.link_type);
            let eff = type_for_os(ty, os);
            if eff != ty {
                r.warnings.push(format!(
                    "{link}: {ty:?} is unavailable on {os}; will be created as {eff:?}"
                ));
            }
        }
    }
    r
}

pub fn validate(file: &Path, portable: bool) -> Result<()> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("could not read manifest {}", file.display()))?;
    let m =
        Manifest::parse(&text).with_context(|| format!("invalid manifest {}", file.display()))?;
    if !portable {
        println!("{} {} link(s)", "valid".green().bold(), m.links.len());
        return Ok(());
    }
    let report = check_portable(&m);
    for w in &report.warnings {
        println!("  {} {w}", "warn".yellow());
    }
    for e in &report.errors {
        println!("  {} {e}", "error".red().bold());
    }
    if !report.errors.is_empty() {
        bail!(
            "{} portability error(s) in {}",
            report.errors.len(),
            file.display()
        );
    }
    println!(
        "{} {} link(s) are portable to windows, linux and macos",
        "valid".green().bold(),
        m.links.len()
    );
    Ok(())
}

pub fn import(file: &Path, root: Option<PathBuf>, force: bool, dry_run: bool) -> Result<()> {
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("could not read manifest {}", file.display()))?;
    let mut m =
        Manifest::parse(&text).with_context(|| format!("invalid manifest {}", file.display()))?;
    let os = manifest::current_os_key();

    let report = check_portable(&m);
    let fatal: Vec<&String> = report
        .errors
        .iter()
        .filter(|e| e.contains("escapes"))
        .collect();
    if !fatal.is_empty() {
        bail!(
            "manifest cannot be restored safely:\n  {}",
            fatal
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }

    for e in &mut m.links {
        e.link = slashes(&e.link);
        e.target = slashes(&e.target);
        let ov = e.os_specific.remove(os);
        let ty = ov.as_ref().and_then(|o| o.link_type).unwrap_or(e.link_type);
        if let Some(t) = ov.and_then(|o| o.target) {
            e.target = slashes(&t);
        }
        let eff = type_for_os(ty, os);
        if eff != e.link_type {
            println!(
                "  {} {}: {:?} -> {:?} for {os}",
                "convert".cyan(),
                e.link.display(),
                e.link_type,
                eff
            );
        }
        e.link_type = eff;
        e.os_specific.clear();
    }

    let root = root.unwrap_or_else(|| {
        file.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    });
    let root_abs = crate::link::strip_verbatim(
        std::fs::canonicalize(&root).with_context(|| format!("no such root {}", root.display()))?,
    );
    sync::apply(&m, &root_abs, force, dry_run)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(link: &str, target: &str, ty: LinkType) -> LinkEntry {
        LinkEntry {
            link: link.into(),
            target: target.into(),
            link_type: ty,
            os_specific: Default::default(),
        }
    }

    #[test]
    fn junction_becomes_symlink_with_windows_override() {
        let mut notes = Vec::new();
        let p = portabilize(&entry("a\\b", "..\\c", LinkType::Junction), &mut notes);
        assert_eq!(p.link, PathBuf::from("a/b"));
        assert_eq!(p.target, PathBuf::from("../c"));
        assert_eq!(p.link_type, LinkType::Symlink);
        assert_eq!(p.os_specific["windows"].link_type, Some(LinkType::Junction));
    }

    #[test]
    fn type_rules_per_os() {
        assert_eq!(type_for_os(LinkType::Junction, "linux"), LinkType::Symlink);
        assert_eq!(
            type_for_os(LinkType::Junction, "windows"),
            LinkType::Junction
        );
        assert_eq!(
            type_for_os(LinkType::BindMount, "windows"),
            LinkType::Symlink
        );
        assert_eq!(type_for_os(LinkType::Hardlink, "linux"), LinkType::Hardlink);
    }

    #[test]
    fn validation_flags_problems() {
        let m = Manifest {
            version: 1,
            description: None,
            source_os: None,
            portable: true,
            links: vec![
                entry("a", "C:/x", LinkType::Symlink),
                entry("A", "b", LinkType::Symlink),
                entry("c\\d", "b", LinkType::Symlink),
            ],
        };
        let r = check_portable(&m);
        assert_eq!(r.errors.len(), 3, "{:?}", r.errors);
    }
}
