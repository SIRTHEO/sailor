//! Ferma i gesti che distruggono lavoro: cancellazioni ricorsive, riazzeramenti
//! e pubblicazioni forzate.
//!
//! PERCHÉ ESISTE, con la misura del 21/08/2026. I due freni di questa macchina
//! sono stati invocati con lo **stesso** ingresso e le due risposte messe a
//! confronto. Su quattro casi veri il freno di terze parti chiedeva conferma o
//! negava, e il nostro **lasciava passare in silenzio**. Rimisurato qui prima di
//! scrivere una riga, su un albero fuori dal perimetro di `bash-guard.mjs`:
//! nessuno dei nove ganci `PreToolUse/Bash` della configurazione di casa diceva
//! niente su nessuna delle trenta righe di prova.
//!
//! Il buco è nostro e non dipende da nessuna decisione sul componente altrui:
//! oggi quella protezione vive **interamente** in un pezzo di terze parti che
//! torna com'era a ogni aggiornamento, e in `bash-guard.mjs`, che vale solo
//! dentro i repo Gyver — cioè non dove gira la maggior parte delle sessioni.
//!
//! UN DIVIETO SCRITTO PER PREFISSO SI AGGIRA SPOSTANDO UNA PAROLA, ed è la
//! ragione per cui questo freno guarda il **gesto** e non l'inizio della riga.
//! Nella lista dei permessi il divieto sulla pubblicazione forzata è
//! `Bash(git push --force:*)`: `git -C /qualunque/cosa push --force` è la stessa
//! identica azione e non lo tocca, e `git push origin +ramo` nemmeno. Qui il
//! comando si spezza nei suoi pezzi, si toglie ciò che avvolge il verbo, e si
//! giudica il verbo.
//!
//! I FALSI POSITIVI SONO IL RISCHIO, NON I FALSI NEGATIVI. Il difetto che si sta
//! togliendo a monte è un freno che nega **nominando un file che il comando
//! stava soltanto leggendo**: sei occorrenze in un giorno su sessioni vere. Qui
//! il bersaglio nominato è sempre un argomento vero di *quel* pezzo, e un
//! comando che si limita a nominare — `bash -n script.sh`, `grep -n rm file`,
//! `echo "rm -rf /"` — non è un gesto e non viene negato.
//!
//! LE VIE D'USCITA ESISTENTI NON SI CHIUDONO. Il materiale usa-e-getta dentro
//! una copia di lavoro continua a passare, e il permesso lo decide la stessa
//! funzione che lo concedeva già (`worktree_deletes::inside_worktree`): riusata,
//! non ricopiata. `git push origin --delete <ramo>` resta autorizzato, perché ha
//! una via legittima in questa casa.
//!
//! COSA QUESTO FRENO NON VEDE, scritto qui perché un elenco taciuto sembra
//! completo: `git clean -fd`, `git checkout --force`, `git branch -D`,
//! `rsync --delete`, `truncate`, `shred`, `dd`, la riscrittura della storia
//! (`rebase`, `filter-branch`), e qualunque cancellazione dentro un programma
//! che il gancio non legge (uno script già sul disco, un `Makefile`, un
//! `npm run`). Le prime tre sono le più vicine e le più facili da aggiungere.

use crate::shell::{split_segments, split_words, valve_in_front};
use crate::worktree_deletes::{assignments, expand, inside_worktree};
use hook_io::Decision;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Quanto in profondità si segue un interprete che riceve del codice (`sh -c …`).
const MAX_NESTING: usize = 3;

/// I rami su cui un riazzeramento forzato distrugge lavoro che può non essere tuo.
///
/// **QUESTO È UN ELENCO, e come tutti gli elenchi di nomi invecchia.** In questa
/// casa un elenco di famiglie ha già mancato la stessa cosa in tre occasioni
/// diverse. Va riletto quando nasce un ramo condiviso nuovo: qui non c'è nessuna
/// deduzione che lo trovi da sola. Quella che ci sarebbe — «è protetto sul
/// remoto» — costa una chiamata di rete dentro un gancio che deve rispondere in
/// millisecondi, e sarebbe cieca sui repo senza remoto.
pub const SHARED_BRANCHES: &[&str] = &[
    "main",
    "master",
    "develop",
    "staging",
    "integration",
    "production",
    "release",
];

/// Parole che precedono il comando vero senza esserlo.
///
/// **ELENCO, non deduzione**: `timeout` era la scorciatoia più corta per
/// aggirare la versione precedente del freno di Linear, e la stessa scorciatoia
/// vale qui. Chi ne trova una che manca la aggiunge invece di rifare il freno.
const PREFIXES: &[&str] = &[
    "sudo", "doas", "env", "time", "nohup", "command", "exec", "xargs", "timeout", "gtimeout",
    "stdbuf", "nice", "ionice", "caffeinate", "setsid", "arch", "builtin", "eval", "then", "else",
    "elif", "do", "while", "until", "for",
];

/// Chi riceve del **codice** come argomento: la stringa va guardata, non il verbo.
///
/// È la sola via per cui `bash -c "rm -rf /x"` viene riconosciuto. Senza,
/// basterebbe scrivere il gesto dentro una stringa. E `bash -n script.sh`, che
/// il codice lo *legge* soltanto, continua a passare: lì non c'è nessun `-c`.
const INTERPRETERS: &[&str] = &["bash", "sh", "zsh", "dash", "ksh", "busybox"];

/// Le radici sotto cui una cancellazione ricorsiva non distrugge lavoro.
///
/// Misurato sui transcript dei sette giorni al 21/08/2026: su 29.874 comandi
/// Bash veri, 126 cancellano ricorsivamente, e la fetta più grossa — 15 — punta
/// qui dentro. Negarle sarebbe attrito puro su un gesto senza pericolo.
const SCRATCH_ROOTS: &[&str] =
    &["/tmp/", "/private/tmp/", "/private/var/folders/", "/var/folders/"];

/// Le opzioni di `git` che stanno **prima** del sottocomando e portano un valore.
const GIT_GLOBALS_WITH_VALUE: &[&str] = &["-C", "-c", "--git-dir", "--work-tree", "--namespace"];

/// Un `rm` con la ricorsione accesa, riconosciuto sul **testo grezzo**.
///
/// Serve solo al ramo di riserva, quando le virgolette di un segmento non si
/// chiudono e le parole non si possono leggere. Un freno che nega non può
/// rispondere «non ho capito, passa pure» proprio sul comando che non ha capito.
fn raw_recursive_rm() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?:^|[\s;&|])(?:sudo\s+|doas\s+)?rm\s+(?:-\S*[rR]|--recursive)").unwrap()
    })
}

/// `find … -delete` sul testo grezzo, per lo stesso ramo di riserva.
fn raw_find_delete() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|[\s;&|])find\s.*(?:-delete|-exec\w*\s+rm)").unwrap())
}

/// Redirezioni: non sono bersagli, e prenderle per tali fa nominare `1`.
fn redirection() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d*(>>?|<)&?\d*|&>>?|\d*>&\d*)$").unwrap())
}

/// I fatti che il giudizio non può inventarsi.
pub struct Facts<'a> {
    pub command: &'a str,
    /// Da dove parte il comando: serve a rendere assoluto un bersaglio relativo.
    pub cwd: &'a str,
    /// La radice delle copie di lavoro usa-e-getta.
    pub workspaces: &'a str,
    pub home: &'a str,
    /// «Questo pezzo di percorso è un collegamento?» — iniettata, come in
    /// `worktree_deletes`: seguire un collegamento vuol dire decidere in base a
    /// dove punta *ora*, e a tavolino non si sa.
    pub is_link: &'a dyn Fn(&str) -> bool,
    /// Il ramo su cui quel repository gira **davvero**.
    ///
    /// Si legge il repository invece del testo del comando: la regola di terze
    /// parti guarda il testo, quindi un `git reset --hard <sha>` che non nomina
    /// il ramo le sfugge anche mentre ci si trova sopra — ed è il caso che fa
    /// danno, non quello che scrive `origin/main` per esteso.
    pub current_branch: &'a dyn Fn(&str) -> Option<String>,
}

/// Il gesto riconosciuto, con il bersaglio che il rifiuto potrà nominare.
#[derive(Debug, PartialEq, Eq)]
enum Danger {
    RecursiveDelete { target: String, privileged: bool },
    UnreadableDelete { segment: String },
    HardReset { branch: String, repo: String },
    ForcePush { how: String },
}

pub fn judge(f: &Facts) -> Decision {
    // La valvola sta **sulla riga di comando**, non nell'ambiente: esportata una
    // volta disarmerebbe in silenzio tutto ciò che viene dopo, scritta qui vale
    // per quel comando e resta nel registro. Esiste perché questo freno è nuovo
    // e non ancora provato sul traffico vero: si toglie quando la misura dirà
    // che non sbaglia, e finché c'è, ogni uso è una misura.
    if valve_in_front(f.command, "FRENO_DISTRUTTIVO=off") {
        return Decision::Pass;
    }
    match first_danger(f.command, f, 0) {
        Some(d) => Decision::Block(explain(&d)),
        None => Decision::Pass,
    }
}

fn first_danger(command: &str, f: &Facts, depth: usize) -> Option<Danger> {
    if depth > MAX_NESTING {
        return None;
    }
    let variables = assignments(command);
    for segment in split_segments(command) {
        if let Some(d) = judge_segment(&segment, &variables, f, depth) {
            return Some(d);
        }
    }
    None
}

fn judge_segment(
    segment: &str,
    variables: &HashMap<String, String>,
    f: &Facts,
    depth: usize,
) -> Option<Danger> {
    let Some(words) = split_words(segment) else {
        // Virgolette aperte: le parole non si leggono. Qui la risposta di riserva
        // è l'opposto di quella di `worktree_deletes` — là tacere significa «non
        // concedo», qui significherebbe «passa pure».
        if raw_recursive_rm().is_match(segment) || raw_find_delete().is_match(segment) {
            return Some(Danger::UnreadableDelete {
                segment: segment.trim().to_string(),
            });
        }
        return None;
    };
    let rest = strip_prefixes(&words);
    let (verb, args) = rest.split_first()?;
    let privileged = words.iter().any(|w| w == "sudo" || w == "doas");

    // Un interprete che riceve del codice: si guarda il codice, non il verbo.
    if INTERPRETERS.iter().any(|i| ends_with_command(verb, i)) {
        let code = args
            .iter()
            .position(|a| a == "-c")
            .and_then(|i| args.get(i + 1))?;
        return first_danger(code, f, depth + 1);
    }

    if ends_with_command(verb, "rm") {
        return recursive_rm(args, variables, f, privileged);
    }
    if ends_with_command(verb, "find") {
        return find_delete(args, variables, f, privileged);
    }
    if ends_with_command(verb, "git") {
        return git_gesture(args, f);
    }
    None
}

/// `rm` e `/bin/rm` sono lo stesso comando.
fn ends_with_command(word: &str, name: &str) -> bool {
    word == name || word.ends_with(&format!("/{name}"))
}

/// Toglie le assegnazioni in testa e i verbi che ne avvolgono un altro.
fn strip_prefixes(words: &[String]) -> Vec<String> {
    let mut i = 0;
    while i < words.len() {
        let w = &words[i];
        // `A=1 rm -rf x`: l'assegnazione precede il comando, non è il comando.
        if w.contains('=') && !w.starts_with('-') && w.split('=').next().is_some_and(is_name) {
            i += 1;
            continue;
        }
        if PREFIXES.iter().any(|p| ends_with_command(w, p)) {
            i += 1;
            // `timeout 30 rm -rf x`, `env -C /a rm -rf x`: si saltano le opzioni
            // dell'involucro e il loro valore, altrimenti il verbo vero non si
            // trova mai.
            while i < words.len() && (words[i].starts_with('-') || is_bare_duration(&words[i])) {
                let takes_value = matches!(words[i].as_str(), "-C" | "-u" | "-S" | "-n" | "-k");
                i += 1;
                if takes_value && i < words.len() {
                    i += 1;
                }
            }
            continue;
        }
        break;
    }
    words[i..].to_vec()
}

fn is_name(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// `timeout 30 …`, `timeout 1.5m …`: la durata sta fra l'involucro e il verbo.
fn is_bare_duration(s: &str) -> bool {
    let core = s.trim_end_matches(['s', 'm', 'h', 'd']);
    !core.is_empty() && core.chars().all(|c| c.is_ascii_digit() || c == '.')
}

/// La ricorsione è accesa? `-rf`, `-fr`, `-Rf`, `-r -f`, `--recursive`.
///
/// Si guarda **ogni** opzione carattere per carattere invece di elencare le
/// combinazioni: le combinazioni sono un elenco, le lettere no.
fn is_recursive(args: &[String]) -> bool {
    args.iter().any(|a| {
        a == "--recursive"
            || (a.starts_with('-')
                && !a.starts_with("--")
                && a.chars().skip(1).any(|c| c == 'r' || c == 'R'))
    })
}

/// I bersagli veri di un comando: niente opzioni, niente redirezioni.
fn operands(args: &[String], variables: &HashMap<String, String>) -> Vec<Option<String>> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for a in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if a == "--" || a.starts_with('-') || redirection().is_match(a) {
            continue;
        }
        // `> /tmp/log` scritto staccato: il nome che segue è la destinazione
        // della redirezione, non un bersaglio.
        if a.ends_with('>') || a.ends_with('<') {
            skip_next = true;
            continue;
        }
        out.push(expand(a, variables));
    }
    out
}

/// Il percorso assoluto di un bersaglio. `None` se non si è potuto leggere.
fn absolute(target: &str, f: &Facts) -> Option<String> {
    if target.is_empty() {
        return None;
    }
    // Un glob non si risolve senza guardare il disco, e ciò che colpisce non si
    // sa: resta illeggibile, cioè non passa.
    if target.contains('*') || target.contains('?') || target.contains('$') {
        return None;
    }
    if let Some(tail) = target.strip_prefix("~/") {
        return Some(format!("{}/{tail}", f.home));
    }
    if target == "~" {
        return Some(f.home.to_string());
    }
    if target.starts_with('/') {
        return Some(target.to_string());
    }
    if f.cwd.is_empty() {
        return None;
    }
    Some(format!("{}/{}", f.cwd.trim_end_matches('/'), target))
}

/// Il bersaglio è materiale che si può rifare senza perdere niente?
fn is_disposable(path: &str, f: &Facts) -> bool {
    for root in SCRATCH_ROOTS {
        let Some(rest) = path.strip_prefix(root) else {
            continue;
        };
        // Solo qualcosa **dentro**: la radice nuda si cancella con la macchina.
        // La profondità si misura da **questa** radice e non contando le barre
        // dall'inizio, che è come era scritta prima — e `/private/tmp/` ne ha due
        // per conto suo, quindi passava per svuotare la radice di tutti.
        return !rest.trim_matches('/').is_empty();
    }
    inside_worktree(path, f.workspaces, f.is_link)
}

fn recursive_rm(
    args: &[String],
    variables: &HashMap<String, String>,
    f: &Facts,
    privileged: bool,
) -> Option<Danger> {
    if !is_recursive(args) {
        return None;
    }
    let found = operands(args, variables);
    if found.is_empty() {
        // `cat lista | xargs rm -rf`: la ricorsione c'è, i bersagli arrivano da
        // fuori e nessuno può dire cosa colpiranno.
        return Some(Danger::RecursiveDelete {
            target: "(bersagli non scritti nel comando)".to_string(),
            privileged,
        });
    }
    for target in found {
        let Some(raw) = target else {
            return Some(Danger::RecursiveDelete {
                target: "(bersaglio non leggibile)".to_string(),
                privileged,
            });
        };
        let Some(abs) = absolute(&raw, f) else {
            return Some(Danger::RecursiveDelete {
                target: raw,
                privileged,
            });
        };
        // Con i privilegi di amministratore non c'è materiale usa-e-getta che
        // tenga: `sudo` esce dai perimetri che rendono sicura la concessione.
        if !privileged && is_disposable(&abs, f) {
            continue;
        }
        return Some(Danger::RecursiveDelete {
            target: abs,
            privileged,
        });
    }
    None
}

/// `find … -delete` e `find … -exec rm …`: la stessa azione con un'altra sintassi.
///
/// Il caso è arrivato **misurato addosso a una sessione viva** il 21/08/2026:
/// negata su `rm -rf`, ha cancellato cinque file e quattro cartelle con due
/// `find … -print -delete`, e nessuno dei due freni ha detto niente. Ci è
/// arrivata in trenta secondi cercando una strada non negata, che è quello che
/// farà chiunque venga fermato.
fn find_delete(
    args: &[String],
    variables: &HashMap<String, String>,
    f: &Facts,
    privileged: bool,
) -> Option<Danger> {
    let deletes = args.iter().enumerate().any(|(i, a)| {
        a == "-delete"
            || (matches!(a.as_str(), "-exec" | "-execdir" | "-ok" | "-okdir")
                && args.get(i + 1).is_some_and(|n| ends_with_command(n, "rm")))
    });
    if !deletes {
        return None;
    }
    // In `find` i percorsi vengono prima dell'espressione: si fermano alla prima
    // parola che comincia per trattino.
    let roots: Vec<&String> = args
        .iter()
        .take_while(|a| !a.starts_with('-'))
        .filter(|a| !redirection().is_match(a))
        .collect();
    if roots.is_empty() {
        // `find -delete` senza percorso lavora sulla cartella corrente.
        let target = absolute(".", f).unwrap_or_else(|| ".".to_string());
        if !privileged && is_disposable(&target, f) {
            return None;
        }
        return Some(Danger::RecursiveDelete { target, privileged });
    }
    for root in roots {
        let Some(abs) = expand(root, variables).and_then(|r| absolute(&r, f)) else {
            return Some(Danger::RecursiveDelete {
                target: root.clone(),
                privileged,
            });
        };
        if !privileged && is_disposable(&abs, f) {
            continue;
        }
        return Some(Danger::RecursiveDelete {
            target: abs,
            privileged,
        });
    }
    None
}

/// Il sottocomando di git e ciò che lo segue, saltate le opzioni globali.
///
/// È il pezzo che rende inutile spostare una parola: `git -C /x push --force` e
/// `git push --force` arrivano qui identici.
fn git_subcommand(args: &[String]) -> Option<(&str, &[String])> {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if !a.starts_with('-') {
            return Some((a.as_str(), &args[i + 1..]));
        }
        let named = a.split('=').next().unwrap_or(a);
        // `-C /percorso` porta il valore staccato; `--git-dir=/x` attaccato.
        if GIT_GLOBALS_WITH_VALUE.contains(&named) && !a.contains('=') {
            i += 2;
        } else {
            i += 1;
        }
    }
    None
}

fn git_gesture(args: &[String], f: &Facts) -> Option<Danger> {
    let (sub, tail) = git_subcommand(args)?;
    match sub {
        "reset" => hard_reset(args, tail, f),
        "push" => force_push(tail),
        _ => None,
    }
}

fn hard_reset(args: &[String], tail: &[String], f: &Facts) -> Option<Danger> {
    if !tail.iter().any(|a| a == "--hard") {
        return None;
    }
    let repo = args
        .iter()
        .position(|a| a == "-C")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| f.cwd.to_string());
    let repo = absolute(&repo, f)?;
    let branch = (f.current_branch)(&repo)?;
    if !SHARED_BRANCHES
        .iter()
        .any(|b| branch == *b || branch.starts_with(&format!("{b}/")))
    {
        return None;
    }
    Some(Danger::HardReset { branch, repo })
}

/// Riscrivere la storia pubblicata, in qualunque forma la si scriva.
///
/// Le tre forme non sono equivalenti per pericolosità — `--force-with-lease`
/// controlla di non sovrascrivere il lavoro di un altro — ma in questa casa sono
/// vietate tutte e tre, e un freno che ne distinguesse due insegnerebbe la
/// scorciatoia. La via d'uscita è la valvola, che resta scritta nel registro.
fn force_push(tail: &[String]) -> Option<Danger> {
    for a in tail {
        let named = a.split('=').next().unwrap_or(a);
        if matches!(
            named,
            "-f" | "--force" | "--force-with-lease" | "--force-if-includes"
        ) {
            return Some(Danger::ForcePush { how: a.clone() });
        }
        // `git push origin +ramo`: il segno è una pubblicazione forzata
        // travestita da riferimento, e nessun divieto scritto per prefisso lo
        // vede. `--delete` resta autorizzato e non passa di qui.
        if a.starts_with('+') && a.len() > 1 {
            return Some(Danger::ForcePush { how: a.clone() });
        }
    }
    None
}

fn explain(d: &Danger) -> String {
    match d {
        Danger::RecursiveDelete { target, privileged } => {
            let head = if *privileged {
                format!("cancellazione ricorsiva con i privilegi di amministratore su «{target}»")
            } else {
                format!("cancellazione ricorsiva su «{target}»")
            };
            format!(
                "{head}.\n  · Non esiste, in questa casa, una via più sicura del comando nudo: i \
                 rami hanno `rami.py` e le copie di lavoro hanno `orca`, una cartella qualunque \
                 non ha niente. Il freno lo dice invece di inventarla.\n  · Se la cancellazione \
                 serve davvero, la fa una persona a mano fuori da una sessione, dopo aver \
                 guardato cosa c'è dentro.\n  · Il materiale usa-e-getta dentro una copia di \
                 lavoro e sotto /tmp continua a passare: se il bersaglio era uno di quelli, il \
                 percorso qui sopra dice perché non lo sembrava.\n  · Consapevole e necessario: \
                 `FRENO_DISTRUTTIVO=off <comando>`, che resta scritto nel registro."
            )
        }
        Danger::UnreadableDelete { segment } => format!(
            "una cancellazione ricorsiva dentro un comando che non so leggere per intero — le \
             virgolette non si chiudono: «{segment}».\n  · Un freno che nega non può rispondere \
             «non ho capito, passa pure» proprio sul comando che non ha capito.\n  · Riscrivilo \
             con le virgolette in pari e il rifiuto tornerà a nominare il bersaglio vero."
        ),
        Danger::HardReset { branch, repo } => format!(
            "riazzeramento forzato mentre «{repo}» si trova su «{branch}», che è un ramo \
             condiviso.\n  · `git reset --hard` butta via ogni cosa non pubblicata, e su un ramo \
             condiviso quel lavoro può non essere tuo.\n  · Il ramo si legge dal repository, non \
             dal comando: cambiare il testo non cambia il verdetto.\n  · Vie d'uscita: \
             `git -C {repo} stash push -u` prima; oppure un ramo di salvataggio \
             (`git -C {repo} branch salvataggio/<data> HEAD`); oppure il riazzeramento su un ramo \
             di lavoro invece che qui."
        ),
        Danger::ForcePush { how } => format!(
            "pubblicazione forzata («{how}»).\n  · Riscrive la storia già pubblicata, e chi \
             l'aveva presa si ritrova con un ramo che non esiste più.\n  · Il gesto si riconosce \
             ovunque stia la parola: `git -C /percorso push --force` e `git push origin +ramo` \
             sono lo stesso gesto, e il divieto scritto per prefisso non li vedeva.\n  · Vie \
             d'uscita: un commit che corregge invece di riscrivere; oppure un ramo nuovo e una \
             richiesta di modifica.\n  · `git push origin --delete <ramo>` non è questo gesto e \
             resta autorizzato."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "/Users/theo/orca/workspaces";
    const HOME: &str = "/Users/theo";

    fn on(command: &str, cwd: &str, branch: Option<&str>) -> Decision {
        let b = branch.map(|s| s.to_string());
        judge(&Facts {
            command,
            cwd,
            workspaces: W,
            home: HOME,
            is_link: &|_| false,
            current_branch: &|_| b.clone(),
        })
    }

    fn blocked(command: &str) -> bool {
        matches!(
            on(command, "/Users/theo/orca/general", None),
            Decision::Block(_)
        )
    }

    fn message(command: &str) -> String {
        match on(command, "/Users/theo/orca/general", None) {
            Decision::Block(m) => m,
            other => panic!("expected a block, got {other:?}"),
        }
    }

    // ── I quattro casi del mandato ──────────────────────────────────────────

    /// MUTANTE: in `is_recursive`, sostituire `c == 'r' || c == 'R'` con
    /// `c == 'R'`. Eseguito: questa prova diventa rossa perché `-rf` non è più
    /// riconosciuto. Ripristinato.
    #[test]
    fn a_plain_recursive_delete_is_blocked_and_names_its_target() {
        let m = message("rm -rf /Users/theo/personal/sailor/src");
        assert!(m.contains("/Users/theo/personal/sailor/src"), "{m}");
        assert!(m.contains("cancellazione ricorsiva"), "{m}");
        // Le combinazioni di lettere non sono un elenco: si guarda la lettera.
        for form in ["-rf", "-fr", "-Rf", "-r -f", "--recursive -f"] {
            assert!(
                blocked(&format!("rm {form} /Users/theo/personal/sailor/src")),
                "forma {form}"
            );
        }
    }

    /// MUTANTE: in `first_danger`, sostituire il ciclo su `split_segments` con
    /// il solo primo segmento (`.into_iter().take(1)`). Eseguito: questa prova
    /// diventa rossa, le altre restano verdi. Ripristinato.
    #[test]
    fn a_delete_at_the_tail_of_a_compound_command_is_blocked() {
        for sep in ["&&", ";", "||", "|", "\n"] {
            let cmd = format!("npm run build {sep} rm -rf /Users/theo/personal/sailor/src");
            assert!(blocked(&cmd), "separatore {sep}");
        }
        // E nomina il **suo** bersaglio, non i comandi che lo precedono: è il
        // difetto misurato sei volte nel freno di terze parti.
        let m = message("npm run build && rm -rf /Users/theo/personal/sailor/src");
        assert!(m.contains("/Users/theo/personal/sailor/src"), "{m}");
        assert!(!m.contains("npm"), "il rifiuto non accusa il comando accanto: {m}");
    }

    /// MUTANTE: togliere `"sudo"` da `PREFIXES`. Eseguito: questa prova diventa
    /// rossa perché il verbo resta `sudo` e non `rm`. Ripristinato.
    #[test]
    fn a_privileged_recursive_delete_is_blocked_and_says_so() {
        let m = message("sudo rm -rf /Users/theo/personal/sailor/src");
        assert!(m.contains("privilegi di amministratore"), "{m}");
        // Nemmeno sotto /tmp: `sudo` esce dai perimetri che rendono sicura la
        // concessione. Le due righe insieme isolano la condizione `!privileged`.
        assert!(blocked("sudo rm -rf /tmp/qualcosa"));
        assert!(!blocked("rm -rf /tmp/qualcosa"));
    }

    /// MUTANTE: in `hard_reset`, sostituire `(f.current_branch)(&repo)` con
    /// `Some("feature/x".to_string())`. Eseguito: questa prova diventa rossa.
    /// Ripristinato.
    #[test]
    fn a_hard_reset_on_a_shared_branch_is_blocked_and_names_the_branch() {
        let d = on(
            "git reset --hard origin/main",
            "/Users/theo/gyver/work/suite",
            Some("main"),
        );
        let Decision::Block(m) = d else {
            panic!("expected a block");
        };
        assert!(m.contains("«main»"), "{m}");
        // Il ramo si legge dal repository, non dal testo: un riazzeramento che
        // NON nomina il ramo viene fermato lo stesso. È il caso che sfugge alla
        // regola di terze parti, che guarda il testo del comando.
        for arg in ["HEAD~3", "abc1234", "@{u}"] {
            assert!(
                matches!(
                    on(
                        &format!("git reset --hard {arg}"),
                        "/Users/theo/gyver/work/suite",
                        Some("develop")
                    ),
                    Decision::Block(_)
                ),
                "argomento {arg}"
            );
        }
    }

    /// MUTANTE: in `hard_reset`, ignorare `-C` e usare sempre `f.cwd`.
    /// Eseguito: questa prova diventa rossa. Ripristinato.
    #[test]
    fn a_hard_reset_reads_the_repository_named_by_dash_c() {
        let seen = std::cell::RefCell::new(String::new());
        let d = judge(&Facts {
            command: "git -C /Users/theo/gyver/work/whatsapp reset --hard origin/develop",
            cwd: "/Users/theo/orca/general",
            workspaces: W,
            home: HOME,
            is_link: &|_| false,
            current_branch: &|repo| {
                *seen.borrow_mut() = repo.to_string();
                Some("develop".to_string())
            },
        });
        assert!(matches!(d, Decision::Block(_)));
        assert_eq!(*seen.borrow(), "/Users/theo/gyver/work/whatsapp");
    }

    // ── Il quinto caso: cancellare per criterio ─────────────────────────────

    /// MUTANTE: in `find_delete`, togliere `a == "-delete"` dalla condizione.
    /// Eseguito: le tre righe su `-delete` diventano rosse, quelle su `-exec rm`
    /// restano verdi. Ripristinato.
    #[test]
    fn deleting_by_criterion_with_find_is_the_same_action_and_is_blocked() {
        // I due comandi veri con cui una sessione ha aggirato il freno il
        // 21/08/2026, dopo essersi vista negare `rm -rf`.
        let m = message("find /Users/theo/.banco-marcatori -type f -print -delete");
        assert!(m.contains("/Users/theo/.banco-marcatori"), "{m}");
        assert!(blocked(
            "find /Users/theo/.banco-marcatori -depth -type d -print -delete"
        ));
        assert!(blocked("find /Users/theo/personal/sailor -delete"));
        assert!(blocked(
            "find /Users/theo/personal/sailor -name '*.ts' -exec rm -f {} ;"
        ));
        assert!(blocked(
            "find /Users/theo/personal/sailor -type d -execdir rm -rf {} +"
        ));
    }

    #[test]
    fn a_find_that_only_reads_is_not_a_deletion() {
        assert!(!blocked("find /Users/theo/personal/sailor -name '*.ts' -print"));
        assert!(!blocked(
            "find /Users/theo/personal/sailor -type f -exec grep -n rm {} ;"
        ));
        assert!(!blocked("find /Users/theo/personal/sailor -newer /tmp/x -ls"));
    }

    // ── Il sesto caso: pubblicare a forza ───────────────────────────────────

    /// MUTANTE: in `git_subcommand`, togliere il salto del valore di `-C`
    /// (`i += 2` diventa `i += 1`). Eseguito: questa prova e quella del `-C` sul
    /// riazzeramento diventano rosse — il sottocomando letto diventa il percorso
    /// — e le altre ventuno restano verdi. Ripristinato.
    #[test]
    fn a_force_push_is_the_same_gesture_wherever_the_word_sits() {
        // Le forme che il divieto scritto per prefisso NON tocca: misurato il
        // 21/08/2026, `Bash(git push --force:*)` non vede niente di tutto questo.
        for cmd in [
            "git push --force",
            "git push -f",
            "git push --force-with-lease",
            "git push --force-with-lease=main:abc123",
            "git -C /Users/theo/personal/sailor push --force",
            "git -C /Users/theo/personal/sailor push -f",
            "git --git-dir=/Users/theo/personal/sailor/.git push --force",
            "git push origin +main",
            "git push origin +refs/heads/x:refs/heads/x",
            "cd /Users/theo/personal/sailor; git push --force",
            "git -c user.name=x push --force",
        ] {
            assert!(blocked(cmd), "forma non riconosciuta: {cmd}");
        }
        let m = message("git -C /Users/theo/personal/sailor push --force");
        assert!(m.contains("pubblicazione forzata"), "{m}");
    }

    /// La via legittima non si chiude: cancellare un ramo remoto è autorizzato
    /// in questa casa, e un freno che lo negasse romperebbe ciò che funzionava.
    #[test]
    fn deleting_a_remote_branch_stays_authorised() {
        assert!(!blocked("git push origin --delete vecchio-ramo"));
        assert!(!blocked("git push origin :vecchio-ramo"));
        assert!(!blocked("git -C /Users/theo/personal/sailor push origin --delete x"));
        assert!(!blocked("git push origin main"));
        assert!(!blocked("git push --follow-tags"));
        assert!(!blocked("git push -u origin feat/x"));
    }

    // ── I falsi positivi: il rischio vero di questo freno ───────────────────

    /// Il difetto che si sta togliendo a monte, e che qui non va ripetuto: un
    /// comando che **nomina** un percorso senza toccarlo veniva negato accusando
    /// quel percorso. Sei occorrenze in un giorno su sessioni vere.
    ///
    /// MUTANTE: in `judge_segment`, prima di leggere le parole, giudicare il
    /// testo grezzo del segmento con
    /// `rm\s+-\S*[rR]|find\s.*-delete|push\s+(-f|--force)` — cioè il difetto di
    /// terze parti riprodotto. Eseguito: questa prova e quella qui sotto
    /// diventano rosse, insieme ad altre otto. Ripristinato.
    ///
    /// Un mutante più debole — la stessa regex ma con il confine `(^|[\s;&|])`
    /// davanti — **non uccide questa prova**, ed è scritto perché si veda:
    /// `echo "rm -rf /"` ha una virgoletta prima di `rm`, quindi il confine non
    /// combacia e il caso passa per la ragione sbagliata.
    #[test]
    fn a_command_that_only_names_a_gesture_is_never_denied() {
        assert!(!blocked("bash -n /Users/theo/.claude/scripts/deploy.sh"));
        assert!(!blocked("grep -n rm /Users/theo/.claude/scripts/deploy.sh"));
        assert!(!blocked("grep -rn 'rm -rf' /Users/theo/.claude/scripts"));
        assert!(!blocked(
            "grep -n 'push --force' /Users/theo/.claude/scripts/deploy.sh"
        ));
        assert!(!blocked("echo \"rm -rf /\""));
        assert!(!blocked("echo \"git push --force\""));
        assert!(!blocked("echo 'find /Users/theo -delete'"));
        assert!(!blocked("cat /Users/theo/.claude/scripts/rm-tutto.sh"));
        assert!(!blocked(
            "rg --files-with-matches 'find . -delete' /Users/theo/.claude"
        ));
    }

    /// I casi veri su cui il freno di terze parti ha sbagliato il 21/08/2026.
    ///
    /// Quel giorno ha prodotto 40 dei 97 blocchi della macchina e ne ha sbagliati
    /// 35. Tre hanno impedito di **scrivere una segnalazione e una consegna**:
    /// erano righe di prosa che nominavano una pubblicazione forzata. Il freno ha
    /// impedito di documentare il freno.
    ///
    /// L'ultima riga è la più istruttiva: là il bersaglio era stato estratto dal
    /// **nome di un caso di prova** scritto apposta per dimostrare che nominare
    /// non è toccare.
    #[test]
    fn the_cases_the_third_party_brake_got_wrong_today_all_pass_here() {
        assert!(!blocked(
            "cat >> /Users/theo/.claude/state/plancia/segnalazioni/nota.md"
        ));
        assert!(!blocked(
            "echo 'the ban on git push --force is easy to walk around' >> /tmp/nota.md"
        ));
        assert!(!blocked("cargo test -p guards names_not_touches"));
        assert!(!blocked("cargo test names-not-touches -- --nocapture"));
        // Un `rm` che il comando sta soltanto cercando dentro un sorgente.
        assert!(!blocked(
            "grep -rn 'rm -rf' /Users/theo/.claude/rust/crates/guards/src"
        ));
    }

    #[test]
    fn ordinary_work_is_not_touched() {
        assert!(!blocked("git log --oneline -5"));
        assert!(!blocked("git reset --soft HEAD~1"));
        assert!(!blocked("git rm -r --cached node_modules"));
        assert!(!blocked("npm run build"));
        assert!(!blocked("rm /Users/theo/personal/sailor/src/uno.txt"));
        assert!(!blocked("rm -f /Users/theo/personal/sailor/src/uno.txt"));
        assert!(!blocked("npm run rm-rf-helper"));
        assert!(!blocked("ls -R /Users/theo/personal/sailor"));
        assert!(!blocked("cp -r /Users/theo/a /Users/theo/b"));
    }

    /// La via d'uscita che esisteva prima non si chiude: è la stessa funzione a
    /// deciderlo, non una copia dell'elenco.
    #[test]
    fn throwaway_material_inside_a_worktree_still_passes() {
        assert!(!blocked(&format!("rm -rf {W}/suite/tautog/dist")));
        assert!(!blocked(&format!("rm -rf {W}/suite/tautog/node_modules/.vite")));
        // Ma non la copia di lavoro intera, né il repo: la profondità minima è
        // quella di `worktree_deletes`, riusata.
        assert!(blocked(&format!("rm -rf {W}/suite/tautog")));
        assert!(blocked(&format!("rm -rf {W}/suite")));
    }

    /// MUTANTE: in `is_disposable`, togliere il controllo sulla profondità
    /// (`return true` subito dopo il prefisso). Eseguito: **la prima volta non
    /// ha ucciso**, e la riga qui sotto su `/tmp/` è nata da lì. Il motivo è che
    /// le radici portano la barra finale, quindi `/tmp` senza barra è già
    /// escluso dal confronto sul prefisso e il caso non arrivava mai alla
    /// profondità. `/tmp/` invece ci arriva: aggiunto quello, il mutante uccide.
    /// Ripristinato.
    #[test]
    fn scratch_directories_still_pass_but_their_roots_do_not() {
        assert!(!blocked("rm -rf /tmp/costruzione-123"));
        assert!(!blocked("rm -rf /private/tmp/claude-501/x/scratchpad"));
        assert!(blocked("rm -rf /tmp"));
        assert!(blocked("rm -rf /tmp/"));
        assert!(blocked("rm -rf /private/tmp/"));
        assert!(blocked("rm -rf /"));
        assert!(blocked("rm -rf ~"));
    }

    // ── Le vie laterali ─────────────────────────────────────────────────────

    /// MUTANTE: in `judge_segment`, disattivare il ramo degli interpreti
    /// (`if false && …`). Eseguito: questa prova diventa rossa e **solo** questa.
    /// Ripristinato.
    #[test]
    fn code_handed_to_an_interpreter_is_read_as_code() {
        assert!(blocked("bash -c \"rm -rf /Users/theo/personal/sailor/src\""));
        // Ma leggere uno script che *contiene* una cancellazione non lo è.
        assert!(!blocked("bash -n /Users/theo/.claude/scripts/deploy.sh"));
        assert!(!blocked("sh /Users/theo/.claude/scripts/deploy.sh"));
    }

    #[test]
    fn a_wrapper_does_not_hide_the_verb() {
        assert!(blocked("timeout 30 rm -rf /Users/theo/personal/sailor/src"));
        assert!(blocked("env -C /tmp rm -rf /Users/theo/personal/sailor/src"));
        assert!(blocked("nohup rm -rf /Users/theo/personal/sailor/src"));
        assert!(blocked("/bin/rm -rf /Users/theo/personal/sailor/src"));
        assert!(blocked("A=1 rm -rf /Users/theo/personal/sailor/src"));
    }

    #[test]
    fn targets_that_arrive_from_outside_the_command_are_not_readable() {
        assert!(blocked("cat lista | xargs rm -rf"));
        assert!(blocked("rm -rf $(cat lista)"));
        assert!(blocked("rm -rf /Users/theo/personal/sailor/*"));
        assert!(blocked("rm -rf \"$NONDEFINITA/x\""));
    }

    /// Una variabile che il comando stesso definisce si risolve, ed è così che
    /// il rifiuto nomina il percorso vero invece di `$S`.
    #[test]
    fn a_variable_the_command_defines_is_resolved_before_judging() {
        let m = message("S=/Users/theo/personal/sailor\nrm -rf \"$S/src\"");
        assert!(m.contains("/Users/theo/personal/sailor/src"), "{m}");
        // E se punta a materiale usa-e-getta, passa.
        assert!(!blocked(&format!("T={W}/suite/tautog\nrm -rf \"$T/dist\"")));
    }

    /// MUTANTE: nel ramo in cui `split_words` fallisce, togliere il controllo
    /// sul testo grezzo e tornare `None`. Eseguito: questa prova diventa rossa e
    /// **solo** questa. Ripristinato.
    #[test]
    fn a_command_it_cannot_read_is_refused_instead_of_waved_through() {
        let m = message("rm -rf \"/Users/theo/personal/sailor/src");
        assert!(m.contains("non so leggere"), "{m}");
        // Ma un comando illeggibile che non distrugge niente passa lo stesso.
        assert!(!blocked("echo \"aperta"));
    }

    /// La redirezione non è un bersaglio: prenderla per tale è il modo esatto in
    /// cui il freno di terze parti nomina il file sbagliato.
    #[test]
    fn a_redirection_is_never_named_as_the_target() {
        let m = message("rm -rf /Users/theo/personal/sailor/src > /tmp/log 2>&1");
        assert!(m.contains("/Users/theo/personal/sailor/src"), "{m}");
        assert!(!m.contains("/tmp/log"), "{m}");
    }

    #[test]
    fn a_relative_target_is_resolved_against_the_working_directory() {
        let d = on(
            "rm -rf node_modules/.vite",
            &format!("{W}/suite/tautog"),
            None,
        );
        assert!(matches!(d, Decision::Pass), "materiale usa-e-getta: {d:?}");
        let d = on("rm -rf src", "/Users/theo/personal/sailor", None);
        let Decision::Block(m) = d else {
            panic!("expected a block");
        };
        assert!(m.contains("/Users/theo/personal/sailor/src"), "{m}");
    }

    #[test]
    fn the_valve_is_read_in_front_of_the_command_and_nowhere_else() {
        assert!(!blocked(
            "FRENO_DISTRUTTIVO=off rm -rf /Users/theo/personal/sailor/src"
        ));
        assert!(blocked(
            "rm -rf /Users/theo/personal/sailor/src --nota FRENO_DISTRUTTIVO=off"
        ));
    }

    /// Un ramo di lavoro non è un tronco: il riazzeramento lì è mestiere normale,
    /// e negarlo fermerebbe 19 comandi veri su 21 misurati in sette giorni.
    #[test]
    fn a_hard_reset_on_a_working_branch_is_ordinary_work() {
        assert!(matches!(
            on(
                "git reset --hard origin/fix/x",
                "/Users/theo/gyver/work/suite",
                Some("fix/x")
            ),
            Decision::Pass
        ));
        // E un repo di cui non si legge il ramo non si giudica a indovinare.
        assert!(matches!(
            on("git reset --hard HEAD~1", "/Users/theo/orca/general", None),
            Decision::Pass
        ));
    }
}
