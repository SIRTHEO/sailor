//! Un albero di lavoro senza i controlli installati non può produrre commit.
//!
//! Porta di `skills/hooks/hooks-off.py`.
//!
//! PERCHÉ ESISTE, con la misura del 14/08/2026. Dei dodici controlli automatici
//! falliti negli ultimi 120 giri dei tre repo, **cinque venivano da un solo
//! albero** — `matching-engine/close-gaps`, dove `.husky/_/pre-commit` non
//! esiste. Quella cartella la genera l'installazione delle dipendenze; lì
//! `node_modules` era un collegamento a quello del repo canonico, quindi
//! l'installazione non è mai girata. Ma `core.hooksPath` resta `.husky/_`, che è
//! configurazione condivisa fra tutti gli alberi, e git — quando la cartella non
//! c'è — **non esegue niente e non dice niente**.
//!
//! Un albero cieco: nessun controllo gira, e chi ci lavora crede che girino.
//!
//! DOVE STA LA GUARDIA, E PERCHÉ QUI. Dentro i controlli del repo non può stare:
//! se i controlli non sono installati non gira nemmeno la guardia. Serve un
//! punto che vede ogni commit comunque, e quel punto è l'harness.
//!
//! COSA NON FA. Non guarda i comandi che non scrivono; non tocca i repo che non
//! hanno mai avuto controlli — se manca anche la cartella `.husky` versionata,
//! quel repo ha fatto una scelta e non è questo il posto per ribaltarla. E non
//! controlla che i ganci *funzionino*: solo che ci siano.

use crate::shell::split_words;
use hook_io::Decision;
use regex::Regex;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// I due soli gesti che i ganci difendono. `git -C <path>` sposta il bersaglio,
/// quindi va letto prima di decidere dove guardare.
fn git_write() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"\bgit\b(?:\s+-[^\s]+(?:\s+[^\s]+)?)*\s+(commit|push)\b").unwrap()
    })
}

fn hook_name(action: &str) -> &'static str {
    match action {
        "commit" => "pre-commit",
        _ => "pre-push",
    }
}

/// Le coppie (repo, gesto) che il comando sta per eseguire.
///
/// Un comando composto può contenerne più d'uno (`git -C a commit && git -C b
/// push`), quindi si spezza sui separatori invece di guardare solo il primo.
pub fn targets(command: &str) -> Vec<(Option<String>, String)> {
    static SPLIT: OnceLock<Regex> = OnceLock::new();
    let split = SPLIT.get_or_init(|| Regex::new(r"&&|\|\||;|\|").unwrap());

    let mut found = Vec::new();
    for segment in split.split(command) {
        let Some(m) = git_write().captures(segment) else {
            continue;
        };
        // Un segmento che non si sa spezzare viene saltato: meglio muto che a caso.
        let Some(parts) = split_words(segment) else {
            continue;
        };
        let path = parts
            .iter()
            .position(|p| p == "-C")
            .and_then(|i| parts.get(i + 1))
            .cloned();
        found.push((path, m[1].to_string()));
    }
    found
}

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// La cartella dove git cerca i ganci di questo repo, assoluta.
///
/// `core.hooksPath` vince su `.git/hooks` e può essere **relativo alla radice
/// del lavoro**, non alla cartella corrente: va risolto contro
/// `--show-toplevel`, altrimenti dentro una sottocartella si guarda un percorso
/// che non esiste e ogni commit verrebbe bloccato.
pub fn hook_dir(repo: &Path) -> Option<PathBuf> {
    let top = git(repo, &["rev-parse", "--show-toplevel"])?;
    match git(repo, &["config", "core.hooksPath"]) {
        Some(configured) if !configured.is_empty() => {
            let p = PathBuf::from(&configured);
            Some(if p.is_absolute() {
                p
            } else {
                Path::new(&top).join(p)
            })
        }
        // Senza configurazione git usa `.git/hooks`, che nei worktree vive nella
        // cartella comune: `--git-path` la risolve senza doverla indovinare.
        _ => {
            let resolved = git(repo, &["rev-parse", "--git-path", "hooks"])?;
            Some(Path::new(&top).join(resolved))
        }
    }
}

/// Il gancio che difende questo gesto manca? `None` significa «non lo so», e
/// allora si tace.
pub fn is_blind(repo: &Path, action: &str) -> Option<bool> {
    let directory = hook_dir(repo)?;
    if directory.join(hook_name(action)).exists() {
        return Some(false);
    }
    // Un repo che non ha mai avuto controlli non è un albero cieco: è un repo
    // senza controlli, e non è questo il gancio che glieli impone. Il segnale
    // che distingue i due casi è la cartella `.husky` versionata, che sta nel
    // repo e quindi c'è in ogni suo albero — anche dove l'installazione non è
    // mai girata.
    if !directory.parent()?.join("pre-commit").exists() {
        return None;
    }
    Some(true)
}

fn message(repo: &Path, action: &str, directory: &Path) -> String {
    format!(
        "I controlli locali di questo albero non esistono: `git {action}` girerebbe senza che nessuno guardi il codice.\n\
         \n\
         \x20 albero:  {}\n\
         \x20 manca:   {}/{}\n\
         \n\
         Succede quando `node_modules` è un collegamento invece di un'installazione vera: la cartella dei ganci la genera l'installazione, e senza quella git non trova niente da eseguire — in silenzio.\n\
         \n\
         Ripara con l'installazione vera in questo albero (`npm install` o `pnpm install`, secondo il repo), oppure ricrea l'albero con `orca worktree create … --setup run`, che la esegue da sé.\n\
         \n\
         Misurato il 14/08/2026: cinque dei dodici controlli automatici falliti venivano da un solo albero cieco, e gli errori erano tutti di quelli che il controllo locale prende al primo colpo.\n\
         Se devi davvero committare da qui, metti {VALVE} davanti al comando e dillo.",
        repo.display(),
        directory.display(),
        hook_name(action)
    )
}

/// Il nome della valvola, nella forma che il rifiuto insegna a scrivere.
pub const VALVE: &str = "GANCI_SPENTI=off";

pub fn judge(command: &str, default_dir: &str) -> Decision {
    // La valvola vale scritta davanti al comando, che è dove il messaggio dice
    // di metterla: letta dall'ambiente del gancio non ci sarebbe mai arrivata.
    if crate::shell::valve_in_front(command, VALVE) {
        return Decision::Pass;
    }
    for (path, action) in targets(command) {
        let repo = PathBuf::from(expand_tilde(path.as_deref().unwrap_or(default_dir)));
        if !repo.is_dir() {
            continue;
        }
        if is_blind(&repo, &action) == Some(true) {
            let directory = hook_dir(&repo).unwrap_or_default();
            return Decision::Deny(message(&repo, &action, &directory));
        }
    }
    Decision::Pass
}

fn expand_tilde(path: &str) -> String {
    match path.strip_prefix("~/") {
        Some(rest) => format!("{}/{rest}", std::env::var("HOME").unwrap_or_default()),
        None => path.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(command: &str) -> Vec<(Option<String>, String)> {
        targets(command)
    }

    fn pair(path: Option<&str>, action: &str) -> (Option<String>, String) {
        (path.map(str::to_string), action.to_string())
    }

    #[test]
    fn it_finds_the_repo_and_the_gesture_of_every_writing_command() {
        assert_eq!(
            pairs(r#"git commit -m "feat: x""#),
            vec![pair(None, "commit")]
        );
        assert_eq!(
            pairs("git -C /tmp/a commit -m x"),
            vec![pair(Some("/tmp/a"), "commit")]
        );
        assert_eq!(
            pairs("git -C /tmp/a push origin ramo"),
            vec![pair(Some("/tmp/a"), "push")]
        );
    }

    #[test]
    fn a_chain_can_carry_more_than_one() {
        assert_eq!(
            pairs("git -C /tmp/a add -A && git -C /tmp/a commit -q -m x"),
            vec![pair(Some("/tmp/a"), "commit")]
        );
        assert_eq!(
            pairs("git -C /tmp/a commit -m x && git -C /tmp/b push origin r"),
            vec![pair(Some("/tmp/a"), "commit"), pair(Some("/tmp/b"), "push")]
        );
    }

    #[test]
    fn it_ignores_the_commands_that_do_not_write() {
        assert!(targets("git -C /tmp/a status --short").is_empty());
        assert!(targets("git log --oneline -3").is_empty());
        assert!(targets("git diff --stat").is_empty());
        assert!(targets(r#"gh pr create --title "feat: x""#).is_empty());
    }

    #[test]
    fn unbalanced_quotes_make_it_keep_quiet_rather_than_guess() {
        assert!(split_words(r#"git commit -m "aperta"#).is_none());
        assert!(targets(r#"git commit -m "aperta"#).is_empty());
    }

    // ── La decisione, non solo l'estrazione ─────────────────────────────────
    //
    // Fino al 18/08/2026 qui si provava soltanto `targets`, cioè quali coppie
    // (repo, gesto) il comando contiene. `is_blind` — che è dove sta il
    // giudizio — non aveva un caso, e per questo il censimento dei ganci lo
    // elencava fra quelli di cui non si sa niente.
    //
    // L'albero si costruisce qui dentro e non da riga di comando: la shell
    // della sessione ha un freno che vieta di scrivere un file chiamato
    // `pre-commit`, e il caso di prova non si costruiva proprio per quello.

    /// Un repo vero in una cartella usa-e-getta, con `core.hooksPath` puntato a
    /// `.husky/_` come fanno i repo in carico.
    fn blank_repo(name: &str) -> PathBuf {
        // La radice porta il pid del processo: due batterie simultanee sono due
        // processi, e con un nome fisso si cancellavano il repo a vicenda.
        let root = hook_io::testing::test_root().join(format!("hooks-off-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".husky")).unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["config", "core.hooksPath", ".husky/_"],
        ] {
            Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
        }
        root
    }

    fn put(path: PathBuf) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "#!/bin/sh\nnpm test\n").unwrap();
    }

    #[test]
    fn a_tree_whose_checks_were_never_generated_is_blind() {
        // `.husky/pre-commit` versionato (il repo I CONTROLLI LI HA), ma
        // `.husky/_` mai generato perché l'installazione non è girata: è il
        // caso che il 14/08/2026 è costato cinque controlli automatici falliti
        // su dodici, tutti dallo stesso albero.
        let repo = blank_repo("blind");
        put(repo.join(".husky").join("pre-commit"));
        assert_eq!(is_blind(&repo, "commit"), Some(true));
        assert!(matches!(
            judge("git commit -m x", repo.to_str().unwrap()),
            Decision::Deny(_)
        ));
    }

    #[test]
    fn a_tree_with_its_checks_installed_passes() {
        let repo = blank_repo("installed");
        put(repo.join(".husky").join("pre-commit"));
        put(repo.join(".husky").join("_").join("pre-commit"));
        assert_eq!(is_blind(&repo, "commit"), Some(false));
        assert!(matches!(
            judge("git commit -m x", repo.to_str().unwrap()),
            Decision::Pass
        ));
    }

    #[test]
    fn a_repo_that_never_had_checks_is_not_this_guards_business() {
        // Nessun `.husky/pre-commit` versionato: quel repo ha fatto una scelta,
        // e imporgliela non è compito di questo gancio. `None` = tace.
        let repo = blank_repo("no-checks");
        assert_eq!(is_blind(&repo, "commit"), None);
        assert!(matches!(
            judge("git commit -m x", repo.to_str().unwrap()),
            Decision::Pass
        ));
    }

    #[test]
    fn push_is_judged_on_its_own_hook() {
        let repo = blank_repo("push");
        put(repo.join(".husky").join("pre-commit"));
        put(repo.join(".husky").join("_").join("pre-commit"));
        // I controlli del commit ci sono, quelli del push no.
        assert_eq!(is_blind(&repo, "push"), Some(true));
    }
}
