#![allow(dead_code)] // schema shared by init/sync (Phase 3) and git check (Phase 4)
//! `.smartlink.json` manifest schema (draft v1, plan.md §2 Phase 3).
//!
//! Types, parsing and validation used by the `init` and `sync` commands.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Schema version this binary writes.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LinkType {
    Symlink,
    Hardlink,
    Junction,
    BindMount,
}

impl LinkType {
    /// Map a detected runtime link kind to its manifest spelling.
    pub fn from_kind(kind: crate::link::LinkKind) -> Self {
        use crate::link::LinkKind as K;
        match kind {
            K::Symlink => LinkType::Symlink,
            K::Hardlink => LinkType::Hardlink,
            K::Junction => LinkType::Junction,
            K::BindMount => LinkType::BindMount,
        }
    }

    /// Map a manifest type back to the kind `make::create_link` understands.
    pub fn to_kind(self) -> crate::link::LinkKind {
        match self {
            LinkType::Symlink => crate::link::LinkKind::Symlink,
            LinkType::Hardlink => crate::link::LinkKind::Hardlink,
            LinkType::Junction => crate::link::LinkKind::Junction,
            LinkType::BindMount => crate::link::LinkKind::BindMount,
        }
    }
}

/// The `os_specific` key that matches the platform this binary runs on.
pub fn current_os_key() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// One declared link. `target` may be relative to the manifest directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkEntry {
    /// Where the link itself is created (relative to repo root).
    pub link: PathBuf,
    /// What it points to (relative to repo root preferred; absolute tolerated
    /// but flagged by `smartlink git check`).
    pub target: PathBuf,
    #[serde(rename = "type")]
    pub link_type: LinkType,
    /// Per-OS overrides, e.g. `{"windows": {"type": "junction"}}`.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub os_specific: HashMap<String, OsOverride>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OsOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<PathBuf>,
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub link_type: Option<LinkType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub links: Vec<LinkEntry>,
}

impl Manifest {
    pub fn parse(json: &str) -> anyhow::Result<Self> {
        let m: Manifest = serde_json::from_str(json)?;
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.version != SCHEMA_VERSION {
            anyhow::bail!(
                "unsupported manifest version {} (this build understands v{SCHEMA_VERSION})",
                self.version
            );
        }
        for entry in &self.links {
            if entry.link == entry.target {
                anyhow::bail!("circular link: {} points to itself", entry.link.display());
            }
            for key in entry.os_specific.keys() {
                if !matches!(key.as_str(), "windows" | "linux" | "macos") {
                    anyhow::bail!("unknown os_specific key '{key}' for {}", entry.link.display());
                }
            }
            if entry.link.is_absolute() {
                anyhow::bail!("link path must be relative to the repo: {}", entry.link.display());
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_manifest() {
        let json = r#"{ "version": 1, "links": [
            { "link": "src/shared", "target": "../libs/shared", "type": "symlink" }
        ] }"#;
        let m = Manifest::parse(json).unwrap();
        assert_eq!(m.links.len(), 1);
        assert_eq!(m.links[0].link_type, LinkType::Symlink);
    }

    #[test]
    fn rejects_unknown_version() {
        let json = r#"{ "version": 99, "links": [] }"#;
        assert!(Manifest::parse(json).is_err());
    }

    #[test]
    fn rejects_self_pointing_link() {
        let json = r#"{ "version": 1, "links": [
            { "link": "a", "target": "a", "type": "symlink" }
        ] }"#;
        assert!(Manifest::parse(json).is_err());
    }

    #[test]
    fn os_overrides_roundtrip() {
        let m = Manifest {
            version: 1,
            description: None,
            links: vec![LinkEntry {
                link: PathBuf::from("data"),
                target: PathBuf::from("../big-data"),
                link_type: LinkType::Symlink,
                os_specific: HashMap::from([(
                    "windows".into(),
                    OsOverride {
                        target: None,
                        link_type: Some(LinkType::Junction),
                    },
                )]),
            }],
        };
        let json = serde_json::to_string(&m).unwrap();
        let back = Manifest::parse(&json).unwrap();
        assert_eq!(back.links[0].os_specific["windows"].link_type, Some(LinkType::Junction));
    }
}
