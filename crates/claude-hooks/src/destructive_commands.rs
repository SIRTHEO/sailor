//! Il gancio PreToolUse che ferma i gesti distruttivi.
//!
//! Il giudizio sta in `guards::destructive_commands`; qui c'è solo ciò che tocca
//! il mondo: le radici vere, la domanda al disco sui collegamenti e la domanda a
//! git su quale ramo sia davvero attivo. È la stessa divisione di
//! `worktree_deletes`, che è il gancio gemello dal lato che **concede**.
//!
//! IL RAMO SI CHIEDE A GIT, NON AL TESTO, e si chiede **solo** quando serve: la
//! lettura costa un processo, e su un gancio che gira a ogni comando Bash sarebbe
//! il costo a dettare il muro dell'intera catena. La chiusura viene invocata dal
//! giudizio soltanto dentro il ramo del riazzeramento forzato, che è raro: 21
//! comandi su 29.874 in sette giorni.

use guards::destructive_commands::{judge, Facts};
use hook_io::Decision;
use std::fs;
use std::process::Command;

fn home() -> String {
    std::env::var("HOME").unwrap_or_default()
}

fn workspaces() -> String {
    format!("{}/orca/workspaces", home())
}

/// Il ramo su cui quel repository si trova ora. `None` se non è un repository,
/// se git non risponde, o se la testa è staccata — e in tutti e tre i casi il
/// giudizio si astiene invece di indovinare.
fn current_branch(repo: &str) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if name.is_empty() || name == "HEAD" {
        return None;
    }
    Some(name)
}

pub fn run() -> i32 {
    let Some(input) = hook_io::read_input() else {
        return 0; // invocato fuori contesto
    };
    if !input.is_tool("Bash") {
        return 0;
    }
    let home = home();
    let workspaces = workspaces();
    let cwd = input
        .cwd
        .clone()
        .or_else(|| std::env::var("CLAUDE_PROJECT_DIR").ok())
        .unwrap_or_default();
    // `symlink_metadata` e non `metadata`: il secondo segue il collegamento, che
    // è esattamente ciò da cui protegge la concessione riusata da qui.
    let is_link = |p: &str| {
        fs::symlink_metadata(p)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
    };
    let decision = judge(&Facts {
        command: input.bash_command(),
        cwd: &cwd,
        workspaces: &workspaces,
        home: &home,
        is_link: &is_link,
        current_branch: &|repo: &str| current_branch(repo),
    });
    if matches!(decision, Decision::Pass) {
        return 0;
    }
    hook_io::emit("block-destructive", &decision)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La testa staccata non è un nome di ramo, e va distinta da un ramo vero.
    ///
    /// MUTANTE DICHIARATO: togliere `|| name == "HEAD"` dalla condizione.
    /// Eseguito. **NON UCCIDE**, e lo scrivo invece di sostituirlo in silenzio:
    /// col mutante il ramo torna «HEAD», che non compare fra i condivisi, quindi
    /// il verdetto finale non cambia. La riga resta perché «HEAD» non è un ramo
    /// e prima o poi qualcuno aggiungerà un nome all'elenco — non perché oggi
    /// regga un blocco.
    #[test]
    fn a_detached_head_is_not_a_branch_name() {
        let repo = format!("{}/.claude", home());
        if let Some(b) = current_branch(&repo) {
            assert_ne!(b, "HEAD");
            assert!(!b.is_empty());
        }
    }

    /// MUTANTE: sostituire `if !out.status.success() { return None; }` con
    /// `let _ = out.status;`. Eseguito: questa prova diventa rossa, perché su una
    /// cartella che non esiste git esce con errore e stdout vuoto — e senza il
    /// controllo si tornerebbe `Some("")`. Ripristinato.
    #[test]
    fn a_directory_that_is_not_a_repository_has_no_branch() {
        assert!(current_branch("/questo/percorso/non/esiste").is_none());
    }
}
