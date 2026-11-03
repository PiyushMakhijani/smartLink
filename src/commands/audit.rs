//! `slink audit` – security and permission auditing of links.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;

use crate::commands::doctor;
use crate::link::{self, LinkKind};

pub struct Finding {
    pub severity: &'static str,
    pub link: PathBuf,
    pub message: String,
}

const SENSITIVE: &[&str] = &[
    "/etc/",
    "/root",
    "/proc",
    "/sys",
    ".ssh",
    ".gnupg",
    ".aws",
    "/etc/shadow",
    "\\windows\\system32",
];

pub fn audit_links(root: &Path) -> Vec<Finding> {
    let mut links = Vec::new();
    doctor::walk(root, 0, &mut links);
    let mut out = Vec::new();
    for l in links {
        let resolved = link::resolve_target(&l);
        let rel = l.path.strip_prefix(root).unwrap_or(&l.path).to_path_buf();
        let mut add = |severity, message: String| {
            out.push(Finding {
                severity,
                link: rel.clone(),
                message,
            })
        };
        if !l.alive {
            add("medium", "broken link (target missing)".into());
        }
        if l.kind == LinkKind::Symlink && l.target.is_absolute() {
            add("low", "absolute symlink target is not portable".into());
        }
        let norm = crate::commands::doctor::normalize_path(&resolved);
        if !norm.starts_with(root) {
            add(
                "medium",
                format!("target escapes the audited root: {}", norm.display()),
            );
        }
        let lower = norm.to_string_lossy().to_lowercase().replace('\\', "/");
        let lower_win = norm.to_string_lossy().to_lowercase();
        if SENSITIVE
            .iter()
            .any(|s| lower.contains(&s.replace('\\', "/")) || lower_win.contains(s))
            && !norm.starts_with(root)
        {
            add("high", "target points into a sensitive location".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(m) = std::fs::metadata(&resolved) {
                if m.permissions().mode() & 0o002 != 0 {
                    add("high", "target is world-writable".into());
                }
            }
            if let Some(parent) = l.path.parent() {
                if let Ok(m) = std::fs::metadata(parent) {
                    if m.permissions().mode() & 0o002 != 0 && m.permissions().mode() & 0o1000 == 0 {
                        add(
                            "high",
                            "link lives in a world-writable directory without sticky bit".into(),
                        );
                    }
                }
            }
        }
    }
    out
}

pub fn run(root: &Path, strict: bool) -> Result<()> {
    let root = link::strip_verbatim(
        std::fs::canonicalize(root).with_context(|| format!("cannot read {}", root.display()))?,
    );
    let findings = audit_links(&root);
    if findings.is_empty() {
        println!("{} no issues found under {}", "✓".green(), root.display());
        return Ok(());
    }
    for f in &findings {
        let sev = match f.severity {
            "high" => "HIGH  ".red().bold(),
            "medium" => "MEDIUM".yellow().bold(),
            _ => "LOW   ".normal(),
        };
        println!("{sev} {}: {}", f.link.display(), f.message);
    }
    println!("\n{} finding(s)", findings.len());
    if strict {
        bail!("audit found {} issue(s)", findings.len());
    }
    Ok(())
}
