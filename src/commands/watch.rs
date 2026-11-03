//! `slink watch` – continuously monitor link health and report changes.
//!
//! Polls the directory tree at a configurable interval (default 2 s).  On
//! each tick a full scan is compared against the previous snapshot:
//!
//! * A link that was **alive** and is now **dead** → target deleted or moved.
//! * A link that was **dead** and is now **alive** → target restored.
//! * A link that **disappeared** entirely from the tree → link itself removed.
//! * A link that is **new** → link was just created.
//! * A link whose **target path changed** → link was re-pointed.
//!
//! With `--repair`, every newly-broken link triggers the same filename-search
//! heuristic as `slink fix` and is repaired automatically when an unambiguous
//! candidate exists.
//!
//! Ctrl-C exits cleanly with a final summary line.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use colored::Colorize;

use super::make;
use crate::link::{self, LinkKind};

// ─── public entry point ──────────────────────────────────────────────────────

pub fn run(root: &Path, interval_secs: u64, repair: bool, verbose: bool) -> Result<()> {
    let root_abs =
        std::fs::canonicalize(root).with_context(|| format!("cannot scan {}", root.display()))?;
    anyhow::ensure!(interval_secs >= 1, "interval must be at least 1 second");

    // Graceful Ctrl-C handling.
    RUNNING.store(true, Ordering::SeqCst);
    ctrlc_setup()?;

    let interval = Duration::from_secs(interval_secs);

    println!(
        "{} watching {} (interval: {}s{})",
        "watch".cyan().bold(),
        root_abs.display(),
        interval_secs,
        if repair { ", auto-repair: on" } else { "" }
    );
    println!("{}", "Press Ctrl-C to stop.".dimmed());
    println!();

    // Build initial snapshot — no events emitted on the first scan.
    let mut snapshot = build_snapshot(&root_abs);
    let start = Instant::now();
    let mut events_total: usize = 0;

    while RUNNING.load(Ordering::SeqCst) {
        let deadline = Instant::now() + interval;
        while RUNNING.load(Ordering::SeqCst) && Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(Instant::now());
            std::thread::sleep(remaining.min(Duration::from_millis(100)));
        }

        if !RUNNING.load(Ordering::SeqCst) {
            break;
        }

        let next = build_snapshot(&root_abs);
        let events = diff(&snapshot, &next, &root_abs);

        for event in &events {
            print_event(event, &root_abs);
            events_total += 1;

            if repair {
                if let Some(healed) = try_repair(event, &root_abs) {
                    println!(
                        "  {} re-linked to {}",
                        "repaired".green().bold(),
                        healed.display()
                    );
                }
            }
        }

        if verbose && events.is_empty() {
            let elapsed = start.elapsed().as_secs();
            println!("{} tick ({elapsed}s elapsed, all OK)", "·".dimmed());
        }

        snapshot = next;
    }

    println!();
    println!(
        "{} stopped after {:.0}s — {} event(s) observed",
        "watch".cyan().bold(),
        start.elapsed().as_secs_f64(),
        events_total
    );
    Ok(())
}

// ─── snapshot ────────────────────────────────────────────────────────────────

/// Lightweight description of a link at a point in time.
#[derive(Clone, Debug)]
struct LinkSnap {
    kind: LinkKind,
    target: PathBuf, // stored (possibly relative) target
    alive: bool,
}

type Snapshot = HashMap<PathBuf, LinkSnap>;

fn build_snapshot(root: &Path) -> Snapshot {
    let mut map = Snapshot::new();
    walk_snapshot(root, 0, &mut map);
    map
}

fn walk_snapshot(dir: &Path, depth: usize, out: &mut Snapshot) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(_meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if let Ok(Some(info)) = link::detect(&path) {
            out.insert(
                path,
                LinkSnap {
                    kind: info.kind,
                    target: info.target,
                    alive: info.alive,
                },
            );
            continue; // never descend through a link
        }
        if _meta.is_dir() {
            walk_snapshot(&path, depth + 1, out);
        }
    }
}

// ─── diffing ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Event {
    /// A previously healthy link's target is now missing.
    BecameBroken { path: PathBuf, target: PathBuf },
    /// A previously dead link's target is now present again.
    Restored { path: PathBuf, target: PathBuf },
    /// The target path stored in the link changed between ticks.
    TargetChanged {
        path: PathBuf,
        old: PathBuf,
        new: PathBuf,
    },
    /// A link that existed in the previous snapshot is no longer present.
    LinkRemoved { path: PathBuf, kind: LinkKind },
    /// A link that wasn't in the previous snapshot now exists.
    LinkAdded {
        path: PathBuf,
        kind: LinkKind,
        target: PathBuf,
        alive: bool,
    },
}

fn diff(prev: &Snapshot, next: &Snapshot, _root: &Path) -> Vec<Event> {
    let mut events = Vec::new();

    // Compare entries present in prev against next.
    for (path, old) in prev {
        match next.get(path) {
            None => {
                events.push(Event::LinkRemoved {
                    path: path.clone(),
                    kind: old.kind,
                });
            }
            Some(new) => {
                // Target path changed (re-pointed).
                if new.target != old.target {
                    events.push(Event::TargetChanged {
                        path: path.clone(),
                        old: old.target.clone(),
                        new: new.target.clone(),
                    });
                } else if old.alive && !new.alive {
                    // Was healthy, now broken.
                    events.push(Event::BecameBroken {
                        path: path.clone(),
                        target: new.target.clone(),
                    });
                } else if !old.alive && new.alive {
                    // Was broken, now restored.
                    events.push(Event::Restored {
                        path: path.clone(),
                        target: new.target.clone(),
                    });
                }
            }
        }
    }

    // New links not present in prev.
    for (path, new) in next {
        if !prev.contains_key(path) {
            events.push(Event::LinkAdded {
                path: path.clone(),
                kind: new.kind,
                target: new.target.clone(),
                alive: new.alive,
            });
        }
    }

    events
}

// ─── display ─────────────────────────────────────────────────────────────────

fn print_event(event: &Event, root: &Path) {
    let ts = timestamp();
    match event {
        Event::BecameBroken { path, target } => {
            println!(
                "{ts} {} {} -> {}",
                "[BROKEN]  ".red().bold(),
                rel(path, root).display().to_string().bold(),
                target.display().to_string().red()
            );
        }
        Event::Restored { path, target } => {
            println!(
                "{ts} {} {} -> {}",
                "[RESTORED]".green().bold(),
                rel(path, root).display().to_string().bold(),
                target.display().to_string().green()
            );
        }
        Event::TargetChanged { path, old, new } => {
            println!(
                "{ts} {} {}",
                "[CHANGED] ".yellow().bold(),
                rel(path, root).display().to_string().bold(),
            );
            println!("           old: {}", old.display().to_string().dimmed());
            println!("           new: {}", new.display().to_string().yellow());
        }
        Event::LinkRemoved { path, kind } => {
            println!(
                "{ts} {} {} (was {})",
                "[REMOVED] ".yellow().bold(),
                rel(path, root).display().to_string().bold(),
                kind.to_string().dimmed()
            );
        }
        Event::LinkAdded {
            path,
            kind,
            target,
            alive,
        } => {
            let status = if *alive {
                "alive".green()
            } else {
                "DEAD".red()
            };
            println!(
                "{ts} {} {} ({}, {}) -> {}",
                "[NEW]     ".cyan(),
                rel(path, root).display().to_string().bold(),
                kind,
                status,
                target.display()
            );
        }
    }
}

// ─── auto-repair ─────────────────────────────────────────────────────────────

/// Attempt to heal a newly-broken link by searching the tree for a file/dir
/// with the same name.  Returns the new target path on success, `None` on
/// failure or when the fix is ambiguous (multiple candidates found).
fn try_repair(event: &Event, root: &Path) -> Option<PathBuf> {
    let (link_path, dead_target) = match event {
        Event::BecameBroken { path, target } => (path, target),
        _ => return None,
    };

    // Re-detect to get the full LinkInfo.
    let info = link::detect(link_path).ok()??;

    let target_name = dead_target.file_name()?;
    let wanted_dir = dead_target.to_string_lossy().ends_with(['/', '\\']);

    let mut candidates: Vec<PathBuf> = Vec::new();
    search_candidates(root, target_name, wanted_dir, link_path, &mut candidates);

    // Only repair automatically when there is exactly one unambiguous match.
    if candidates.len() != 1 {
        if candidates.len() > 1 {
            println!(
                "  {} {} candidates for {}; skipping auto-repair (run `slink fix .`)",
                "ambiguous".yellow(),
                candidates.len(),
                rel(link_path, root).display()
            );
        }
        return None;
    }

    let cand = &candidates[0];
    let kind = info.kind;
    let relative = kind == LinkKind::Symlink;
    match make::create_link(kind, cand, link_path, true, relative) {
        Ok(_) => Some(cand.clone()),
        Err(e) => {
            eprintln!(
                "  {} auto-repair failed for {}: {e}",
                "error".red().bold(),
                rel(link_path, root).display()
            );
            None
        }
    }
}

/// Depth-capped filename search, mirrors `fix::search_matches`.
fn search_candidates(
    dir: &Path,
    name: &std::ffi::OsStr,
    wanted_dir: bool,
    skip: &Path,
    out: &mut Vec<PathBuf>,
) {
    search_candidates_inner(dir, name, wanted_dir, skip, out, 0);
}

fn search_candidates_inner(
    dir: &Path,
    name: &std::ffi::OsStr,
    wanted_dir: bool,
    skip: &Path,
    out: &mut Vec<PathBuf>,
    depth: usize,
) {
    if depth > 32 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };

        if path == skip {
            continue; // a link cannot be its own cure
        }
        if meta.file_type().is_symlink() {
            continue; // candidates must be real files / dirs
        }
        if path.file_name() == Some(name)
            && ((wanted_dir && meta.is_dir()) || (!wanted_dir && meta.is_file()))
        {
            out.push(path.clone());
        }
        if meta.is_dir() {
            search_candidates_inner(&path, name, wanted_dir, skip, out, depth + 1);
        }
    }
}

// ─── utilities ───────────────────────────────────────────────────────────────

/// Relative display path, falls back to absolute.
fn rel(path: &Path, root: &Path) -> PathBuf {
    pathdiff::diff_paths(path, root).unwrap_or_else(|| path.to_path_buf())
}

/// Short HH:MM:SS timestamp prefix.
fn timestamp() -> String {
    // Use a simple seconds-since-epoch display to avoid chrono/time dependency.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let s = secs % 60;
    let m = (secs / 60) % 60;
    let h = (secs / 3600) % 24;
    format!("[{h:02}:{m:02}:{s:02}]").dimmed().to_string()
}

/// Install a Ctrl-C handler that sets the `running` flag to false.
fn ctrlc_setup() -> Result<()> {
    #[cfg(unix)]
    {
        unsafe {
            nix::sys::signal::signal(
                nix::sys::signal::Signal::SIGINT,
                nix::sys::signal::SigHandler::Handler(sigint_handler),
            )
            .context("could not install SIGINT handler")?;
        }
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
        let registered = unsafe { SetConsoleCtrlHandler(Some(ctrl_handler), 1) };
        if registered == 0 {
            return Err(std::io::Error::last_os_error())
                .context("could not install Ctrl-C handler");
        }
    }
    Ok(())
}

// ─── platform-specific signal glue ───────────────────────────────────────────

static RUNNING: AtomicBool = AtomicBool::new(true);

#[cfg(unix)]
extern "C" fn sigint_handler(_: libc::c_int) {
    RUNNING.store(false, Ordering::SeqCst);
}

#[cfg(windows)]
unsafe extern "system" fn ctrl_handler(ctrl_type: u32) -> windows_sys::Win32::Foundation::BOOL {
    use windows_sys::Win32::System::Console::CTRL_C_EVENT;
    if ctrl_type == CTRL_C_EVENT {
        RUNNING.store(false, Ordering::SeqCst);
        1 // TRUE: we handled it
    } else {
        0 // FALSE: let default handler run
    }
}
