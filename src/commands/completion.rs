//! `slink completion` – shell autocomplete scripts.

use clap::CommandFactory;
use clap_complete::Shell;

pub fn run<C: CommandFactory>(shell: Shell) {
    let mut cmd = C::command();
    clap_complete::generate(shell, &mut cmd, "slink", &mut std::io::stdout());
}
