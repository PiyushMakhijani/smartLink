//! `slink benchmark` – compare link creation and resolution speed.

use std::time::Instant;

use anyhow::Result;

use crate::commands::make::create_link;
use crate::link::{self, LinkKind};

pub fn run(iterations: usize) -> Result<()> {
    let iterations = iterations.max(1);
    let tmp = tempdir()?;
    let file = tmp.join("target.txt");
    std::fs::write(&file, b"benchmark")?;
    let dir = tmp.join("target_dir");
    std::fs::create_dir(&dir)?;

    println!(
        "Benchmarking {iterations} iteration(s) in {}",
        tmp.display()
    );
    println!(
        "{:<10} {:<6} {:>14} {:>14}",
        "kind", "on", "create (µs)", "resolve (µs)"
    );

    let cases = [
        (LinkKind::Hardlink, &file, "file"),
        (LinkKind::Symlink, &file, "file"),
        (LinkKind::Symlink, &dir, "dir"),
        (LinkKind::Junction, &dir, "dir"),
    ];
    for (kind, target, on) in cases {
        let sub = tmp.join(format!("{kind}-{on}"));
        std::fs::create_dir_all(&sub)?;
        let mut created = Vec::new();
        let start = Instant::now();
        let mut failure = None;
        for i in 0..iterations {
            let p = sub.join(format!("l{i}"));
            match create_link(kind, target, &p, false, false) {
                Ok(_) => created.push(p),
                Err(e) => {
                    failure = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failure {
            println!("{:<10} {:<6} unavailable: {e:#}", kind.to_string(), on);
            continue;
        }
        let create_us = start.elapsed().as_micros() as f64 / iterations as f64;
        let start = Instant::now();
        for p in &created {
            let _ = link::detect(p);
            let _ = std::fs::metadata(p);
        }
        let resolve_us = start.elapsed().as_micros() as f64 / iterations as f64;
        println!(
            "{:<10} {:<6} {:>14.1} {:>14.1}",
            kind.to_string(),
            on,
            create_us,
            resolve_us
        );
    }
    let _ = std::fs::remove_dir_all(&tmp);
    Ok(())
}

fn tempdir() -> Result<std::path::PathBuf> {
    let base = std::env::temp_dir().join(format!(
        "slink-bench-{}-{}",
        std::process::id(),
        crate::state::now_millis()
    ));
    std::fs::create_dir_all(&base)?;
    Ok(base)
}
