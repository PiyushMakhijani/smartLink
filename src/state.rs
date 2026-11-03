//! Persistent user state: central config, profiles and transaction records.
//!
//! Everything lives below one directory: `$SLINK_HOME` when set, otherwise
//! `%APPDATA%\smartlink` on Windows or `$XDG_CONFIG_HOME/smartlink`
//! (`~/.config/smartlink`) elsewhere.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

/// Settings understood by `slink config` and profiles, with their allowed values.
pub const SETTINGS: &[(&str, &str, &[&str])] = &[
    (
        "default_type",
        "link type `make` uses when no flag is given",
        &["auto", "hard", "symlink", "junction"],
    ),
    (
        "relative",
        "store symlink targets as relative paths",
        &["true", "false"],
    ),
];

pub fn home() -> Result<PathBuf> {
    if let Some(p) = std::env::var_os("SLINK_HOME").filter(|p| !p.is_empty()) {
        return Ok(PathBuf::from(p));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    };
    match base {
        Some(b) => Ok(b.join("smartlink")),
        None => bail!("cannot locate a config directory; set SLINK_HOME"),
    }
}

pub fn transactions_dir() -> Result<PathBuf> {
    Ok(home()?.join("transactions"))
}

fn profiles_dir() -> Result<PathBuf> {
    Ok(home()?.join("profiles"))
}

fn write_json<T: Serialize>(path: &std::path::Path, value: &T) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("could not create {}", parent.display()))?;
    }
    // Write-then-rename so a crash never leaves a truncated state file.
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(value)? + "\n")
        .with_context(|| format!("could not write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("could not write {}", path.display()))
}

pub fn save_json<T: Serialize>(path: &std::path::Path, value: &T) -> Result<()> {
    write_json(path, value)
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_profile: Option<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

impl Config {
    fn path() -> Result<PathBuf> {
        Ok(home()?.join("config.json"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text)
                .with_context(|| format!("invalid config {}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("could not read {}", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        write_json(&Self::path()?, self)
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
}

pub fn validate_profile_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.len() <= 64
        && !name.starts_with('.')
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    if !ok {
        bail!("invalid profile name '{name}' (use letters, digits, '-', '_' and '.')");
    }
    Ok(())
}

impl Profile {
    fn path(name: &str) -> Result<PathBuf> {
        validate_profile_name(name)?;
        Ok(profiles_dir()?.join(format!("{name}.json")))
    }

    pub fn exists(name: &str) -> Result<bool> {
        Ok(Self::path(name)?.is_file())
    }

    pub fn load(name: &str) -> Result<Self> {
        let path = Self::path(name)?;
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("profile '{name}' does not exist"))?;
        serde_json::from_str(&text).with_context(|| format!("invalid profile {}", path.display()))
    }

    pub fn save(&self) -> Result<()> {
        write_json(&Self::path(&self.name)?, self)
    }

    pub fn list() -> Result<Vec<String>> {
        let dir = profiles_dir()?;
        let mut names: Vec<String> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| {
                    let p = e.path();
                    if p.extension().is_some_and(|x| x == "json") {
                        p.file_stem().map(|s| s.to_string_lossy().into_owned())
                    } else {
                        None
                    }
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        names.sort();
        Ok(names)
    }
}

/// Check that `key` is a known setting and `value` one of its allowed values.
pub fn validate_setting(key: &str, value: &str) -> Result<()> {
    let Some((_, _, allowed)) = SETTINGS.iter().find(|(k, _, _)| *k == key) else {
        let keys: Vec<&str> = SETTINGS.iter().map(|(k, _, _)| *k).collect();
        bail!("unknown setting '{key}' (known: {})", keys.join(", "));
    };
    if !allowed.contains(&value) {
        bail!(
            "invalid value '{value}' for {key} (allowed: {})",
            allowed.join(", ")
        );
    }
    Ok(())
}

/// Value of `key` together with where it came from: the active profile wins
/// over the central config.
pub fn effective_setting(key: &str) -> Result<Option<(String, String)>> {
    let cfg = Config::load()?;
    if let Some(name) = &cfg.active_profile {
        if let Ok(p) = Profile::load(name) {
            if let Some(v) = p.settings.get(key) {
                return Ok(Some((v.clone(), format!("profile '{name}'"))));
            }
        }
    }
    Ok(cfg
        .settings
        .get(key)
        .map(|v| (v.clone(), "config".to_string())))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// `YYYY-MM-DD HH:MM:SS` in UTC (civil-from-days algorithm).
pub fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_epoch_dates() {
        assert_eq!(format_utc(0), "1970-01-01 00:00:00");
        assert_eq!(format_utc(1_709_210_096), "2024-02-29 12:34:56");
    }

    #[test]
    fn validates_names_and_settings() {
        assert!(validate_profile_name("dev-1").is_ok());
        assert!(validate_profile_name("../x").is_err());
        assert!(validate_profile_name("").is_err());
        assert!(validate_setting("default_type", "hard").is_ok());
        assert!(validate_setting("default_type", "bogus").is_err());
        assert!(validate_setting("nope", "x").is_err());
    }
}
