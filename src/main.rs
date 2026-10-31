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
    /// Convert an existing link into another type
    Convert {
        /// Existing link (symlink / junction / hardlink) to morph
        link: PathBuf,
        /// Target type: hard, symlink, junction, bind
        #[arg(long)]
        to: String,
        /// Allow converting a link whose target is missing
        #[arg(long)]
        preserve_broken: bool,
        /// Accept all interactive prompts
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Auto-heal broken links by searching for matching files
    Fix {
        /// Project root to scan for dead links (and their replacements)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Only report the replacement candidates, do not touch anything
        #[arg(long)]
        dry_run: bool,
        /// Accept the first matching candidate for every dead link
        #[arg(long, short = 'y')]
        yes: bool,
    },
    /// Copy-on-write clone a file to save space (native FS APIs)
    Dedupe {
        source: PathBuf,
        dest: PathBuf,
        #[arg(long)]
        force: bool,
    },
    /// Generate a .smartlink.json manifest from the current directory
    Init {
        /// Directory to scan (its links are recorded; defaults to current)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Write the manifest somewhere other than <path>/.smartlink.json
        #[arg(long)]
        out: Option<PathBuf>,
        /// Free-text description stored in the manifest
        #[arg(long)]
        description: Option<String>,
        /// Also record same-volume hardlink groups
        #[arg(long)]
        hardlinks: bool,
        /// Print the manifest to stdout instead of writing a file
        #[arg(long)]
        dry_run: bool,
    },
    /// Recreate all links defined in .smartlink.json
    Sync {
        #[arg(long, default_value = ".smartlink.json")]
        manifest: PathBuf,
        /// Replace existing links that disagree with the manifest
        #[arg(long)]
        force: bool,
        /// Report what would change without touching the filesystem
        #[arg(long)]
        dry_run: bool,
    },
    /// Git integration: CI checks and auto-sync hooks
    Git {
        #[command(subcommand)]
        command: GitCommand,
    },
}

#[derive(Subcommand)]
pub enum GitCommand {
    /// Fail (exit 1) if any symlink in the tree uses an absolute target
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Install post-checkout / post-merge hooks that run `smartlink sync`
    InstallHooks {
        /// Repo (or any path inside it) to install hooks into
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Append to an existing hook that has no smartlink block
        #[arg(long)]
        force: bool,
    },
    /// Remove the smartlink block from installed hooks
    UninstallHooks {
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
        Commands::Convert { link, to, preserve_broken, yes } =>
            commands::convert::run(&link, &to, preserve_broken, yes),
        Commands::Fix { path, dry_run, yes } => commands::fix::run(&path, dry_run, yes),
        Commands::Dedupe { source, dest, force } => commands::dedupe::run(&source, &dest, force),
        Commands::Init { path, out, description, hardlinks, dry_run } =>
            commands::init::run(&path, out, description, hardlinks, dry_run),
        Commands::Sync { manifest, force, dry_run } =>
            commands::sync::run(&manifest, force, dry_run),
        Commands::Git { command } => match command {
            GitCommand::Check { path } => commands::git::check(&path),
            GitCommand::InstallHooks { path, force } => commands::git::install_hooks(&path, force),
            GitCommand::UninstallHooks { path } => commands::git::uninstall_hooks(&path),
        },
    }
}
