//! Transaction log for operations that modify filesystem links.
//!
//! A transaction is written to disk *before* anything is touched and updated
//! after every operation, so an interrupted run can still be undone with
//! `slink transaction rollback <id>`. Each operation records the previous
//! link type and target; no file or directory content is ever deleted, only
//! the link entry itself is replaced.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use colored::Colorize;
use serde::{Deserialize, Serialize};

use crate::commands::make;
use crate::link::{self, LinkKind};
use crate::manifest::LinkType;
use crate::state;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pending,
    Applied,
    #[serde(rename = "rolled-back")]
    RolledBack,
    Failed,
}

impl Status {
    fn label(self) -> colored::ColoredString {
        match self {
            Status::Pending => "pending".yellow(),
            Status::Applied => "applied".green(),
            Status::RolledBack => "rolled-back".cyan(),
            Status::Failed => "failed".red(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Op {
    pub path: PathBuf,
    pub from_type: LinkType,
    /// Absolute target the link pointed to (the twin file for a hardlink).
    pub from_target: PathBuf,
    /// Whether the old symlink stored a relative target.
    pub from_relative: bool,
    pub to_type: LinkType,
    pub to_target: PathBuf,
    #[serde(default)]
    pub applied: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    pub id: String,
    pub created: u64,
    pub command: String,
    pub status: Status,
    pub ops: Vec<Op>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

enum State {
    Missing,
    Link(LinkKind),
    Other,
}

fn classify(path: &Path) -> State {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return State::Missing;
    };
    if let Ok(Some(info)) = link::detect(path) {
        return State::Link(info.kind);
    }
    if meta.is_file() && link::hardlink_count(path).is_ok_and(|n| n > 1) {
        return State::Link(LinkKind::Hardlink);
    }
    State::Other
}

impl Transaction {
    pub fn new(command: &str, ops: Vec<Op>) -> Self {
        let created = state::now_secs();
        let dir = state::transactions_dir().ok();
        let mut n = state::now_millis();
        let mut id = format!("{n:x}");
        while dir
            .as_ref()
            .is_some_and(|d| d.join(format!("{id}.json")).exists())
        {
            n += 1;
            id = format!("{n:x}");
        }
        Transaction {
            id,
            created,
            command: command.to_string(),
            status: Status::Pending,
            ops,
            error: None,
        }
    }

    fn file(id: &str) -> Result<PathBuf> {
        if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric()) {
            bail!("invalid transaction id '{id}'");
        }
        Ok(state::transactions_dir()?.join(format!("{id}.json")))
    }

    pub fn save(&self) -> Result<()> {
        state::save_json(&Self::file(&self.id)?, self)
    }

    pub fn load(id: &str) -> Result<Self> {
        let path = Self::file(id)?;
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("transaction '{id}' not found"))?;
        serde_json::from_str(&text).with_context(|| format!("invalid transaction {id}"))
    }

    pub fn list() -> Result<Vec<Transaction>> {
        let dir = state::transactions_dir()?;
        let mut out: Vec<Transaction> = match std::fs::read_dir(&dir) {
            Ok(rd) => rd
                .flatten()
                .filter_map(|e| std::fs::read_to_string(e.path()).ok())
                .filter_map(|t| serde_json::from_str(&t).ok())
                .collect(),
            Err(_) => Vec::new(),
        };
        out.sort_by(|a, b| (a.created, &a.id).cmp(&(b.created, &b.id)));
        Ok(out)
    }

    /// Run every operation. If one fails, the failing link and all earlier
    /// operations are restored before the error is returned.
    pub fn execute(&mut self) -> Result<()> {
        self.save()?;
        for i in 0..self.ops.len() {
            let op = self.ops[i].clone();
            if let Err(err) = apply_op(&op) {
                let mut msg = format!("{}: {err:#}", op.path.display());
                let mut clean = restore_op(&op).is_ok();
                for j in (0..i).rev() {
                    if restore_op(&self.ops[j]).is_err() {
                        clean = false;
                    } else {
                        self.ops[j].applied = false;
                    }
                }
                if !clean {
                    msg.push_str(" (automatic rollback was incomplete)");
                }
                self.status = if clean {
                    Status::RolledBack
                } else {
                    Status::Failed
                };
                self.error = Some(msg.clone());
                self.save()?;
                bail!(
                    "transaction {} failed and was {}: {msg}",
                    self.id,
                    if clean {
                        "rolled back"
                    } else {
                        "only partially rolled back; run `slink transaction rollback`"
                    }
                );
            }
            self.ops[i].applied = true;
            self.save()?;
        }
        self.status = Status::Applied;
        self.save()
    }

    /// Restore the pre-transaction state, newest operation first. Entries that
    /// were changed by someone else since are left untouched.
    pub fn rollback(&mut self) -> Result<()> {
        if self.status == Status::RolledBack {
            bail!("transaction {} is already rolled back", self.id);
        }
        let mut problems = Vec::new();
        for op in self.ops.iter_mut().rev() {
            match restore_op(op) {
                Ok(()) => op.applied = false,
                Err(e) => problems.push(format!("{}: {e:#}", op.path.display())),
            }
        }
        if problems.is_empty() {
            self.status = Status::RolledBack;
            self.error = None;
            self.save()
        } else {
            self.status = Status::Failed;
            self.error = Some(problems.join("; "));
            self.save()?;
            bail!("rollback incomplete:\n  {}", problems.join("\n  "));
        }
    }
}

fn apply_op(op: &Op) -> Result<()> {
    make::create_link(op.to_type.to_kind(), &op.to_target, &op.path, true, false)?;
    Ok(())
}

/// Put the link at `op.path` back as it was, if it is still in a state this
/// transaction could have produced.
fn restore_op(op: &Op) -> Result<()> {
    let from = op.from_type.to_kind();
    match classify(&op.path) {
        State::Link(k) if k == from => Ok(()),
        State::Link(k) if k == op.to_type.to_kind() => restore(op),
        State::Missing => restore(op),
        State::Link(k) => bail!("now a {k}, not what this transaction created; left untouched"),
        State::Other => bail!("is now a regular file or directory; left untouched"),
    }
}

fn restore(op: &Op) -> Result<()> {
    make::create_link(
        op.from_type.to_kind(),
        &op.from_target,
        &op.path,
        true,
        op.from_relative,
    )?;
    Ok(())
}

pub fn history() -> Result<()> {
    let all = Transaction::list()?;
    if all.is_empty() {
        println!("no transactions recorded");
        return Ok(());
    }
    for t in all {
        println!(
            "{}  {}  {:<11} {} op(s)  {}",
            t.id.bold(),
            state::format_utc(t.created).dimmed(),
            t.status.label(),
            t.ops.len(),
            t.command
        );
    }
    Ok(())
}

pub fn show(id: &str) -> Result<()> {
    let t = Transaction::load(id)?;
    println!(
        "{} {} {}",
        t.id.bold(),
        state::format_utc(t.created),
        t.status.label()
    );
    println!("  command: {}", t.command);
    if let Some(e) = &t.error {
        println!("  error: {e}");
    }
    for op in &t.ops {
        println!(
            "  {} {} -> {} ({})",
            op.path.display(),
            op.from_type.to_kind(),
            op.to_type.to_kind(),
            op.to_target.display()
        );
    }
    Ok(())
}

pub fn rollback(id: &str) -> Result<()> {
    let mut t = Transaction::load(id)?;
    t.rollback()?;
    println!(
        "{} transaction {} ({} link(s) restored)",
        "rolled back".green().bold(),
        t.id.bold(),
        t.ops.len()
    );
    Ok(())
}
