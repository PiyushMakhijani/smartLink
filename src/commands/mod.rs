pub mod convert;
pub mod dedupe;
pub mod fix;
pub mod ls;
pub mod make;

use std::path::PathBuf;

/// Placeholder until Phase 3 commands land (see plan.md §2).
#[allow(dead_code)] // used by init/sync/git in Phase 3
pub fn pending(path: PathBuf, name: &str, what: String) -> anyhow::Result<()> {
    anyhow::bail!(
        "`smartlink {name}` ({what}) is scheduled for a later milestone \
         (see plan.md); not implemented yet. Input: {}",
        path.display()
    )
}
