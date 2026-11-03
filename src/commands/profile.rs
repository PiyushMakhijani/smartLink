//! `slink profile` – named configurations for different environments.

use anyhow::{bail, Result};
use colored::Colorize;

use crate::state::{self, Config, Profile};

pub fn parse_pair(s: &str) -> Result<(String, String)> {
    let (k, v) = s
        .split_once('=')
        .ok_or_else(|| anyhow::anyhow!("expected KEY=VALUE, got '{s}'"))?;
    state::validate_setting(k.trim(), v.trim())?;
    Ok((k.trim().to_string(), v.trim().to_string()))
}

pub fn create(name: &str, sets: &[String], description: Option<String>) -> Result<()> {
    state::validate_profile_name(name)?;
    if Profile::exists(name)? {
        bail!("profile '{name}' already exists");
    }
    let mut profile = Profile {
        name: name.to_string(),
        description,
        ..Default::default()
    };
    for s in sets {
        let (k, v) = parse_pair(s)?;
        profile.settings.insert(k, v);
    }
    profile.save()?;
    println!("{} profile {}", "created".green().bold(), name.bold());
    Ok(())
}

pub fn use_profile(name: &str) -> Result<()> {
    if !Profile::exists(name)? {
        bail!("profile '{name}' does not exist (create it with `slink profile create {name}`)");
    }
    let mut cfg = Config::load()?;
    cfg.active_profile = Some(name.to_string());
    cfg.save()?;
    println!("{} profile {}", "using".green().bold(), name.bold());
    Ok(())
}

pub fn list() -> Result<()> {
    let names = Profile::list()?;
    if names.is_empty() {
        println!("no profiles (create one with `slink profile create <name>`)");
        return Ok(());
    }
    let active = Config::load()?.active_profile;
    for n in names {
        let marker = if active.as_deref() == Some(n.as_str()) {
            "*".green().bold().to_string()
        } else {
            " ".to_string()
        };
        println!("{marker} {n}");
    }
    Ok(())
}

pub fn show(name: Option<&str>) -> Result<()> {
    let name = match name {
        Some(n) => n.to_string(),
        None => Config::load()?
            .active_profile
            .ok_or_else(|| anyhow::anyhow!("no active profile; pass a name"))?,
    };
    let p = Profile::load(&name)?;
    println!("{}", p.name.bold());
    if let Some(d) = &p.description {
        println!("  {d}");
    }
    if p.settings.is_empty() {
        println!("  (no settings)");
    }
    for (k, v) in &p.settings {
        println!("  {k} = {v}");
    }
    Ok(())
}

pub fn set(name: &str, sets: &[String]) -> Result<()> {
    let mut p = Profile::load(name)?;
    for s in sets {
        let (k, v) = parse_pair(s)?;
        p.settings.insert(k, v);
    }
    p.save()?;
    println!("{} profile {}", "updated".green().bold(), name.bold());
    Ok(())
}

pub fn delete(name: &str) -> Result<()> {
    if !Profile::exists(name)? {
        bail!("profile '{name}' does not exist");
    }
    let mut cfg = Config::load()?;
    if cfg.active_profile.as_deref() == Some(name) {
        cfg.active_profile = None;
        cfg.save()?;
    }
    std::fs::remove_file(state::home()?.join("profiles").join(format!("{name}.json")))?;
    println!("{} profile {}", "deleted".green().bold(), name.bold());
    Ok(())
}
