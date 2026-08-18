//! Ferma la creazione a mano di un albero di lavoro dentro `~/orca/workspaces/`.
//!
//! PERCHÉ ESISTE. Misura del 18/08/2026 su 2.934 sessioni in trenta giorni:
//! 126 creano un albero con `orca worktree create`. Passato questo giudizio su
//! tutti i 4.150 comandi distinti che nominano `worktree`, **20** creano un
//! albero permanente dentro la cartella che Orca gestisce, e nessun altro viene
//! toccato: zero falsi positivi.
//!
//! LA PRIMA STIMA DICEVA TRE, ED ERA SBAGLIATA. Cercava `git worktree add`
//! alla lettera, mentre la forma prevalente è `git -C <repo> worktree add` —
//! la stessa che le regole di casa prescrivono per non usare `cd`. Il grep
//! letterale ne vedeva un sesto. È il motivo per cui il giudizio va passato sul
//! traffico vero invece che su una ricerca a occhio.
//!
//! Il danno non è teorico. `orca/workspaces/sailor/interfaccia` esiste ancora e
//! non è fra le 32 copie registrate: dentro ha sei commit mai finiti sul
//! remoto. Un albero che il gestore non conosce è indistinguibile da quello di
//! un'altra sessione, e per smontarlo bisogna dimostrare che il lavoro è già
//! sul tronco.
//!
//! COSA LASCIA PASSARE, di proposito: tutto ciò che nasce fuori da
//! `orca/workspaces` — `mktemp`, lo scratchpad di sessione, i worktree di
//! confronto sotto `/private/tmp`. Sono i tre quarti degli usi misurati, e sono
//! la tecnica giusta per mettere due rami a confronto senza sporcare niente.
//!
//! PERCHÉ BLOCCA E NON AVVISA: la sostituzione è meccanica e il nome della
//! copia si legge dal percorso stesso. Dove non c'è giudizio da dare, un avviso
//! è solo un blocco che si può ignorare — la stessa scelta di `cd_guard` sul
//! suo ramo `git`.
//!
//! COSA C'ERA GIÀ, e perché non bastava. `guard-scope.sh` instrada a
//! `bash-guard.mjs`, che questa creazione la vieta già — con un messaggio quasi
//! identico. Ma è un instradatore con un perimetro: esce subito se il `cwd` non
//! sta nei repo Other-repo e se il comando non li nomina. Provato il 18/08/2026
//! sull'esemplare vero, quello dei sei commit: **passa**, perché è su `sailor`.
//! Dentro il perimetro Other-repo i due si sovrappongono e negano entrambi. La
//! sovrapposizione si tiene: il primo vive in un repo che ha le sue regole e
//! può cambiarle senza avvisare la configurazione globale, e un buco vale più
//! caro di un messaggio in doppio.
//!
//! `inside_worktree` di `worktree_deletes` non serve qui e non è riusabile:
//! cerca materiale **dentro** una copia (profondità 3) e interroga il disco sui
//! collegamenti, mentre qui la copia è il bersaglio stesso (profondità 2).

use crate::cd_guard::strip_noise;
use hook_io::Decision;
use regex::Regex;
use std::sync::OnceLock;

/// La cartella che Orca gestisce. Fuori di qui un albero a mano è legittimo:
/// non c'è nessun gestore a cui diventare invisibile.
const MANAGED_ROOT: &str = "/orca/workspaces/";

/// Le opzioni di `git worktree add` che si portano dietro un argomento.
/// Saltarle serve a non scambiare il nome di un ramo per il percorso.
const OPTIONS_WITH_VALUE: &[&str] = &["-b", "-B", "--reason"];

fn add_pattern() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bworktree\s+add\b").unwrap())
}

/// Il percorso di destinazione e il ramo chiesto con `-b`, se c'è.
///
/// `git worktree add [<opzioni>] <percorso> [<commit>]`: il percorso è il primo
/// argomento che non sia un'opzione né il valore di un'opzione.
fn destination(after_add: &str) -> (Option<String>, Option<String>) {
    let mut branch = None;
    let mut words = after_add.split_whitespace();
    while let Some(word) = words.next() {
        if OPTIONS_WITH_VALUE.contains(&word) {
            let value = words.next();
            if branch.is_none() && word != "--reason" {
                branch = value.map(str::to_string);
            }
            continue;
        }
        if word.starts_with('-') {
            continue; // flag senza argomento
        }
        return (Some(word.to_string()), branch);
    }
    (None, branch)
}

/// Il repo e il nome che Orca darebbe a questa copia, letti dal percorso:
/// `…/orca/workspaces/<repo>/<nome>`.
fn repo_and_name(path: &str) -> Option<(String, String)> {
    let tail = path.split(MANAGED_ROOT).nth(1)?;
    let mut parts = tail.trim_matches('/').split('/');
    let repo = parts.next()?;
    let name = parts.next()?;
    if repo.is_empty() || name.is_empty() {
        return None;
    }
    Some((repo.to_string(), name.to_string()))
}

pub fn judge(command: &str) -> Decision {
    let bare = strip_noise(command);
    let Some(m) = add_pattern().find(&bare) else {
        return Decision::Pass;
    };
    let (Some(path), branch) = destination(&bare[m.end()..]) else {
        return Decision::Pass; // nessun percorso: non è una creazione
    };
    if !path.contains(MANAGED_ROOT) {
        return Decision::Pass; // fuori dal territorio di Orca: legittimo
    }
    let Some((repo, name)) = repo_and_name(&path) else {
        return Decision::Pass;
    };
    let base = branch
        .as_deref()
        .map(|b| format!("  · il ramo `{b}` lo crea Orca da sé, dalla base che gli dai\n"))
        .unwrap_or_default();
    Decision::Block(format!(
        "niente `git worktree add` dentro `orca/workspaces/`: la copia nasce \
         invisibile al gestore, e allora non si capisce di chi sia — per \
         smontarla bisogna dimostrare che il lavoro è già sul tronco. Il \
         18/08/2026 una di queste teneva sei commit mai finiti sul remoto.\n  \
         · scrivi:  orca worktree create --repo id:<repoId> --name {name} \
         --base-branch origin/develop --no-parent --setup run\n  · `orca repo \
         list` dà il `repoId` di `{repo}`\n{base}  · con `--setup run` le \
         dipendenze si installano da sole, invece che a mano in ogni albero"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn blocks(command: &str) -> bool {
        matches!(judge(command), Decision::Block(_))
    }

    #[test]
    fn a_persistent_tree_inside_the_managed_root_is_blocked() {
        assert!(blocks(
            "git worktree add /home/someone/orca/workspaces/suite/nuova-copia"
        ));
    }

    #[test]
    fn a_throwaway_tree_under_tmp_is_left_alone() {
        assert!(!blocks(
            "git worktree add /private/tmp/claude-501/scratchpad/confronto origin/main"
        ));
    }

    #[test]
    fn a_mktemp_tree_is_left_alone() {
        assert!(!blocks(
            "cd /home/someone/personal/sailor && W=$(mktemp -d)/mut && git worktree add -q $W HEAD"
        ));
    }

    #[test]
    fn the_branch_flag_is_not_mistaken_for_the_path() {
        // `-b <ramo>` sta prima del percorso: leggere il primo argomento
        // avrebbe preso il nome del ramo e lasciato passare la violazione.
        let decision = judge(
            "git worktree add -b work/nuova /home/someone/orca/workspaces/packages/nuova develop",
        );
        let Decision::Block(message) = decision else {
            panic!("this must block");
        };
        assert!(message.contains("--name nuova"), "{message}");
        assert!(message.contains("`work/nuova`"), "{message}");
        assert!(message.contains("di `packages`"), "{message}");
    }

    #[test]
    fn flags_without_a_value_do_not_shift_the_path() {
        assert!(blocks(
            "git worktree add -q --detach /home/someone/orca/workspaces/a-client/x"
        ));
    }

    #[test]
    fn the_reason_flag_does_not_become_the_branch() {
        let Decision::Block(message) =
            judge("git worktree add --reason parking /home/someone/orca/workspaces/suite/x")
        else {
            panic!("this must block");
        };
        assert!(!message.contains("il ramo"), "{message}");
    }

    #[test]
    fn a_worktree_command_that_is_not_add_passes() {
        assert!(!blocks("git worktree list"));
        assert!(!blocks(
            "git worktree remove /home/someone/orca/workspaces/suite/vecchia"
        ));
    }

    #[test]
    fn the_managed_root_needs_both_repo_and_name() {
        // `orca/workspaces/suite` è la cartella di un repo, non una copia:
        // nessun nome da proporre, e bloccare direbbe una cosa falsa.
        assert!(!blocks("git worktree add /home/someone/orca/workspaces/suite"));
    }

    #[test]
    fn a_quoted_command_inside_a_string_is_not_a_call() {
        // Citare il comando in un messaggio non è eseguirlo: `strip_noise`
        // toglie le virgolette, ed è la stessa scelta di `cd_guard`.
        assert!(!blocks(
            "echo 'git worktree add /home/someone/orca/workspaces/suite/x'"
        ));
    }

    #[test]
    fn a_heredoc_body_belongs_to_the_receiving_script() {
        assert!(!blocks(
            "cat > /tmp/x.sh <<'EOF'\ngit worktree add /home/someone/orca/workspaces/suite/x\nEOF"
        ));
    }
}
