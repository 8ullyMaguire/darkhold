//! `M63-17` — completions and the man page are generated from the parser, never maintained.
//!
//! A subcommand nobody can discover may as well not exist, and a hand-written completion list is
//! wrong the first time a flag is added — silently, because nobody re-reads a completion file in
//! review. Both artefacts here come out of the same `clap::Command` the binary parses with, so they
//! cannot drift: adding `--foo` to an argument adds it to the completions in the same commit.
//!
//! ```text
//! $ dh completions bash > ~/.local/share/bash-completion/completions/dh
//! $ dh completions zsh  > ~/.zfunc/_dh
//! $ dh completions fish > ~/.config/fish/completions/dh.fish
//! $ dh man | man -l -
//! ```
//!
//! The artefacts go to **stdout**, so they compose with the shell the user already has; the
//! installation path is the shell's business, not this program's.

use clap::CommandFactory;
use clap_complete::Shell;

use crate::Cli;

/// Print a completion script for `shell`.
pub fn completions(shell: Shell) {
    // `generate` needs the binary's name as the user types it, which is not the crate's name.
    let mut command = Cli::command();
    // Render into a buffer, then write once. `generate` unwraps its own write (`failed to write
    // completion file: BrokenPipe`), so piping into `head` — the way anyone looks at a completion
    // script before installing it — panicked the process. Buffering moves the write here, where a
    // closed pipe is not an error: the caller asked for the first lines and got them.
    let mut page: Vec<u8> = Vec::new();
    clap_complete::generate(shell, &mut command, "dh", &mut page);
    write_ignoring_a_closed_pipe(&page);
}

/// Write to stdout, treating a closed pipe as success.
///
/// `dh rec | head` and `dh completions zsh | head` are both normal uses, and a SIGPIPE-shaped exit
/// in the middle of a shell pipeline is noise, not a failure.
fn write_ignoring_a_closed_pipe(bytes: &[u8]) {
    use std::io::Write;
    let mut stdout = std::io::stdout();
    let _ = stdout.write_all(bytes);
    let _ = stdout.flush();
}

/// Print the man page.
pub fn man() {
    let command = Cli::command();
    let mut stdout = std::io::stdout();
    // `render` writes to the writer and returns `()`; a failure here is a broken pipe (the page went
    // to `head`), which is not an error worth a message and certainly not worth a non-zero exit.
    let _ = clap_mangen::Man::new(command).render(&mut stdout);
}
