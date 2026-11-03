use clap::{Parser, Subcommand};
use std::path::PathBuf;

mod commands;
mod fs;
mod link;
mod manifest;
mod state;
mod transaction;

#[derive(Parser)]
#[command(
    name = "slink",
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
        /// Preview the transaction plan without changing anything
        #[arg(long, conflicts_with = "apply")]
        dry_run: bool,
        /// Apply the plan (required to convert whole directories)
        #[arg(long)]
        apply: bool,
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
    /// Scan links and report a structured health summary
    Doctor {
        /// Directory to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Print every broken / problematic link individually
        #[arg(long, short = 'v')]
        verbose: bool,
    },
    /// Continuously monitor link health and report changes
    Watch {
        /// Directory to watch (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Polling interval in seconds (minimum 1)
        #[arg(long, default_value = "2")]
        interval: u64,
        /// Automatically repair newly-broken links when an unambiguous
        /// replacement target can be found (mirrors `slink fix --yes`)
        #[arg(long)]
        repair: bool,
        /// Print a heartbeat line on every tick even when nothing changed
        #[arg(long, short = 'v')]
        verbose: bool,
    },
    /// Named configurations for different environments
    Profile {
        #[command(subcommand)]
        command: ProfileCommand,
    },
    /// Inspect and roll back recorded link transactions
    Transaction {
        #[command(subcommand)]
        command: TransactionCommand,
    },
    /// Export the link structure as a manifest (optionally portable)
    Export {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Normalise paths and link types so the manifest works on any OS
        #[arg(long)]
        portable: bool,
        /// Output file (default: smartlink-manifest.json)
        #[arg(long)]
        out: Option<PathBuf>,
        #[arg(long)]
        description: Option<String>,
        /// Also record same-volume hardlink groups
        #[arg(long)]
        hardlinks: bool,
    },
    /// Recreate links from an exported manifest on this machine
    Import {
        file: PathBuf,
        /// Directory to recreate the links in (default: current directory)
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        dry_run: bool,
    },
    /// Validate a manifest, optionally for cross-platform portability
    Validate {
        #[arg(default_value = "smartlink-manifest.json")]
        file: PathBuf,
        #[arg(long)]
        portable: bool,
    },
    /// Visualize nested link structures
    Tree {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Maximum depth to display
        #[arg(long, default_value = "8")]
        depth: usize,
        /// Descend into directory link targets
        #[arg(long)]
        follow: bool,
        /// Only show branches that contain links
        #[arg(long)]
        links_only: bool,
    },
    /// Filesystem link statistics
    Stats {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        json: bool,
    },
    /// Compare link creation and resolution speed
    Benchmark {
        #[arg(long, default_value = "100")]
        iterations: usize,
    },
    /// Generate shell completion scripts
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    /// Central configuration management
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Audit links for security and permission problems
    Audit {
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Exit non-zero when any finding is reported
        #[arg(long)]
        strict: bool,
    },
    /// Explain why a link type is selected for a target
    Explain {
        /// Target to link to (or an existing link to describe)
        target: PathBuf,
        /// Hypothetical link location (affects same-volume checks)
        link: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
pub enum ProfileCommand {
    /// Create a profile
    Create {
        name: String,
        /// Setting as KEY=VALUE (repeatable)
        #[arg(long = "set")]
        sets: Vec<String>,
        #[arg(long)]
        description: Option<String>,
    },
    /// Activate a profile
    Use { name: String },
    /// List profiles
    List,
    /// Show a profile (default: the active one)
    Show { name: Option<String> },
    /// Change settings on a profile
    Set {
        name: String,
        #[arg(long = "set", required = true)]
        sets: Vec<String>,
    },
    /// Delete a profile
    Delete { name: String },
}

#[derive(Subcommand)]
pub enum TransactionCommand {
    /// List recorded transactions
    History,
    /// Show the operations of a transaction
    Show { id: String },
    /// Undo a transaction
    Rollback { id: String },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// List all settings
    List,
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
    },
    Unset {
        key: String,
    },
    /// Print the configuration directory
    Path,
}

#[derive(Subcommand)]
pub enum GitCommand {
    /// Fail (exit 1) if any symlink in the tree uses an absolute target
    Check {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Install post-checkout / post-merge hooks that run `slink sync`
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
        Commands::Convert {
            link,
            to,
            preserve_broken,
            yes,
            dry_run,
            apply,
        } => commands::convert::run(&link, &to, preserve_broken, yes, dry_run, apply),
        Commands::Fix { path, dry_run, yes } => commands::fix::run(&path, dry_run, yes),
        Commands::Dedupe {
            source,
            dest,
            force,
        } => commands::dedupe::run(&source, &dest, force),
        Commands::Init {
            path,
            out,
            description,
            hardlinks,
            dry_run,
        } => commands::init::run(&path, out, description, hardlinks, dry_run),
        Commands::Sync {
            manifest,
            force,
            dry_run,
        } => commands::sync::run(&manifest, force, dry_run),
        Commands::Git { command } => match command {
            GitCommand::Check { path } => commands::git::check(&path),
            GitCommand::InstallHooks { path, force } => commands::git::install_hooks(&path, force),
            GitCommand::UninstallHooks { path } => commands::git::uninstall_hooks(&path),
        },
        Commands::Doctor { path, verbose } => commands::doctor::run(&path, verbose),
        Commands::Watch {
            path,
            interval,
            repair,
            verbose,
        } => commands::watch::run(&path, interval, repair, verbose),
        Commands::Profile { command } => match command {
            ProfileCommand::Create {
                name,
                sets,
                description,
            } => commands::profile::create(&name, &sets, description),
            ProfileCommand::Use { name } => commands::profile::use_profile(&name),
            ProfileCommand::List => commands::profile::list(),
            ProfileCommand::Show { name } => commands::profile::show(name.as_deref()),
            ProfileCommand::Set { name, sets } => commands::profile::set(&name, &sets),
            ProfileCommand::Delete { name } => commands::profile::delete(&name),
        },
        Commands::Transaction { command } => match command {
            TransactionCommand::History => transaction::history(),
            TransactionCommand::Show { id } => transaction::show(&id),
            TransactionCommand::Rollback { id } => transaction::rollback(&id),
        },
        Commands::Export {
            path,
            portable,
            out,
            description,
            hardlinks,
        } => commands::portable::export(&path, out, description, hardlinks, portable),
        Commands::Import {
            file,
            root,
            force,
            dry_run,
        } => commands::portable::import(&file, root, force, dry_run),
        Commands::Validate { file, portable } => commands::portable::validate(&file, portable),
        Commands::Tree {
            path,
            depth,
            follow,
            links_only,
        } => commands::tree::run(&path, depth, follow, links_only),
        Commands::Stats { path, json } => commands::stats::run(&path, json),
        Commands::Benchmark { iterations } => commands::benchmark::run(iterations),
        Commands::Completion { shell } => {
            commands::completion::run::<Cli>(shell);
            Ok(())
        }
        Commands::Config { command } => match command {
            ConfigCommand::List => commands::config::list(),
            ConfigCommand::Get { key } => commands::config::get(&key),
            ConfigCommand::Set { key, value } => commands::config::set(&key, &value),
            ConfigCommand::Unset { key } => commands::config::unset(&key),
            ConfigCommand::Path => commands::config::path(),
        },
        Commands::Audit { path, strict } => commands::audit::run(&path, strict),
        Commands::Explain { target, link } => commands::explain::run(&target, link.as_deref()),
    }
}
