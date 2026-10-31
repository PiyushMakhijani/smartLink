use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod fs;
mod link;
mod manifest;

#[derive(Parser)]
#[command(
    name = "smartlink",
    version,
    about = "Unified cross-platform links: symlinks, hardlinks, junctions, bind mounts"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a link with auto-detection (same-volume file -> hardlink, else relative symlink)
    Make {
        /// Existing file or directory the link should point to
        target: PathBuf,
        /// Path of the link to create
        link: PathBuf,
        /// Force a hardlink (files only, same volume)
        #[arg(long)]
        hard: bool,
        /// Force a symlink / junction for directories (default on Windows)
        #[arg(long)]
        symlink: bool,
        /// Force a junction (Windows) or bind mount (Linux) / symlink (macOS)
        #[arg(long)]
        junction: bool,
        /// Store the link target as a relative path
        #[arg(long)]
        relative: bool,
        /// Overwrite an existing link at the destination
        #[arg(long)]
        force: bool,
    },
    /// List links in a directory with type, status and target
    Ls {
        /// Directory to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Also show files that are hardlinked (grouped by inode/file id)
        #[arg(long)]
        hardlinks: bool,
    },
    /// Convert an existing link into another type (Phase 2)
    Convert {
        link: PathBuf,
        /// Target type: hard, symlink, junction
        #[arg(long)]
        to: Option<String>,
    },
    /// Auto-heal broken links by searching for matching files (Phase 2)
    Fix {
        /// Directory to scan for dead links
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Copy-on-write clone a file to save space (Phase 2)
    Dedupe {
        source: PathBuf,
        dest: PathBuf,
    },
    /// Generate a .smartlink.json manifest from the current directory (Phase 3)
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Recreate all links defined in .smartlink.json (Phase 3)
    Sync {
        #[arg(long, default_value = ".smartlink.json")]
        manifest: PathBuf,
    },
    /// Git integrations: manifest checks for CI (Phase 3)
    Git {
        #[command(subcommand)]
        command: GitCommand,
    },
}

#[derive(Subcommand)]
pub enum GitCommand {
    /// Fail (exit 1) if any committed symlink in the repo uses an absolute target
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Make {
            target,
            link,
            hard,
            symlink,
            junction,
            relative,
            force,
        } => commands::make::run(&target, &link, hard, symlink, junction, relative, force),
        Commands::Ls { path, hardlinks } => commands::ls::run(&path, hardlinks),
        Commands::Convert { link, to } => {
            let to = to.map(|t| format!("-> {t}")).unwrap_or_else(|| "type conversion".into());
            commands::pending(link, "convert", to)
        }
        Commands::Fix { path } => commands::pending(path, "fix", "auto-heal".into()),
        Commands::Dedupe { source, dest } => commands::pending(source, "dedupe", format!("clone -> {}", dest.display())),
        Commands::Init { path } => commands::pending(path, "init", "manifest generation".into()),
        Commands::Sync { manifest } => commands::pending(manifest, "sync", "manifest replay".into()),
        Commands::Git { command } => match command {
            GitCommand::Check { path } => commands::pending(path, "git check", "absolute-symlink CI check".into()),
        },
    }
}
