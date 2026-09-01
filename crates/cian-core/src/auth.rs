//! Answering a password prompt on a terminal screen.
//!
//! `:ssh` in both builds is *run `ssh` in the shell*, and a host configured
//! with a password in `init.lua` should not then ask for it again. Neither
//! build can hand a secret to `ssh` up front — OpenSSH reads the password
//! from the terminal on purpose, and every way around that (`sshpass`, an
//! askpass helper) is a dependency or a file on disk holding the secret. So
//! both builds watch the screen instead, and type it when it is asked for.
//!
//! This lives here because it is the *rule*, and the rule has to be the same
//! in the terminal build and the window: a screen that gets a password out of
//! the terminal build must not get one out of the window, and the other way
//! round. It was in `cian-tui` alone, so the window had no way to be right.

use std::time::Duration;

/// How long to watch for a password prompt before giving up.
///
/// Long enough for a slow DNS lookup and a handshake, short enough that a
/// secret is not sitting armed while you go for coffee.
pub const AUTH_WINDOW: Duration = Duration::from_secs(20);

/// Does this screen end in something asking for a password?
///
/// Deliberately narrow: only a prompt on the last non-empty line counts, so
/// the word "password" scrolling past in a log cannot trigger a send. A
/// host-key question also ends in a colon and must never be answered with a
/// password — that one is the person's to answer.
pub fn looks_like_password_prompt(screen: &str) -> bool {
    let Some(last) = screen.lines().map(|l| l.trim_end()).rfind(|l| !l.is_empty()) else {
        return false;
    };
    let l = last.to_lowercase();
    if l.contains("yes/no") || l.contains("fingerprint") {
        return false;
    }
    (l.contains("password") || l.contains("passphrase")) && l.trim_end().ends_with(':')
}

/// The command that opens an interactive session on a host.
///
/// One place, so the two builds cannot drift on the port flag — the terminal
/// build had this inline and the window had nothing.
pub fn ssh_command(user: &str, host: &str, port: Option<u16>) -> String {
    match port {
        Some(p) if p != 22 => format!("ssh {user}@{host} -p {p}"),
        _ => format!("ssh {user}@{host}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // These came over from cian-tui with the rule. They were the terminal
    // build's alone while the window had no way to be right about any of it.
    #[test]
    fn a_password_prompt_is_recognised_only_at_the_end_of_the_screen() {
        assert!(looks_like_password_prompt("root@10.0.2.31's password:"));
        assert!(looks_like_password_prompt("Password:"));
        assert!(looks_like_password_prompt("Enter passphrase for key '/x/id_ed25519':"));
        // Trailing blank lines are ignored.
        assert!(looks_like_password_prompt("Password:\n\n  \n"));
    }

    #[test]
    fn things_that_must_not_be_mistaken_for_a_password_prompt() {
        // The word scrolling past in output is not a prompt.
        assert!(!looks_like_password_prompt("password rotation done\n$ "));
        assert!(!looks_like_password_prompt("Failed password for root\n$ "));
        assert!(!looks_like_password_prompt("password: hunter2\n$ "));
        // A host-key question ends in a colon but must be answered by a human.
        assert!(!looks_like_password_prompt(
            "The authenticity of host 'x' can't be established.\n\
             ED25519 key fingerprint is SHA256:abc.\n\
             Are you sure you want to continue connecting (yes/no)?:"
        ));
        assert!(!looks_like_password_prompt(""));
        assert!(!looks_like_password_prompt("$ "));
    }

    #[test]
    fn the_port_is_only_written_when_it_is_not_the_default() {
        assert_eq!(ssh_command("t", "h", None), "ssh t@h");
        assert_eq!(ssh_command("t", "h", Some(22)), "ssh t@h");
        assert_eq!(ssh_command("t", "h", Some(2222)), "ssh t@h -p 2222");
    }
}
