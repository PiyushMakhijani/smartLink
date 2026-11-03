//! `slink config` – central configuration management.

use anyhow::{bail, Result};

use crate::state::{self, Config};

pub fn list() -> Result<()> {
    let cfg = Config::load()?;
    println!("home: {}", state::home()?.display());
    println!(
        "active profile: {}",
        cfg.active_profile.as_deref().unwrap_or("(none)")
    );
    for (key, help, allowed) in state::SETTINGS {
        match state::effective_setting(key)? {
            Some((v, src)) => println!("{key} = {v}  [{src}]"),
            None => println!("{key} = (unset)"),
        }
        println!("    {help} ({})", allowed.join("|"));
    }
    Ok(())
}

pub fn get(key: &str) -> Result<()> {
    if !state::SETTINGS.iter().any(|(k, _, _)| *k == key) {
        bail!("unknown setting '{key}'");
    }
    match state::effective_setting(key)? {
        Some((v, _)) => println!("{v}"),
        None => bail!("{key} is not set"),
    }
    Ok(())
}

pub fn set(key: &str, value: &str) -> Result<()> {
    state::validate_setting(key, value)?;
    let mut cfg = Config::load()?;
    cfg.settings.insert(key.to_string(), value.to_string());
    cfg.save()?;
    println!("{key} = {value}");
    Ok(())
}

pub fn unset(key: &str) -> Result<()> {
    let mut cfg = Config::load()?;
    if cfg.settings.remove(key).is_none() {
        bail!("{key} is not set in the central config");
    }
    cfg.save()?;
    println!("unset {key}");
    Ok(())
}

pub fn path() -> Result<()> {
    println!("{}", state::home()?.display());
    Ok(())
}
