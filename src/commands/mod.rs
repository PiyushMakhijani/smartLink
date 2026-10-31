pub mod make;
pub mod ls;

use std::path::PathBuf;

/// Placeholder until Phase 2/3 commands land (see plan.md §2).
pub fn pending(path: PathBuf, name: &str, what: String) -> anyhow::Result<()> {
    anyhow::bail!(
        "`smartlink {name}` ({what}) is scheduled for a later milestone \
         (see plan.md); not implemented yet. Input: {}",
        path.display()
    )
}
