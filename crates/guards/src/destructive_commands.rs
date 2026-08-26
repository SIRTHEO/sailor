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
//! dentro i repo Other-repo — cioè non dove gira la maggior parte delle sessioni.
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
//! `echo "rm -rf /"` — non è un gesto e non viene negato. Dal 24/08/2026 vale
//! anche per il **corpo di un messaggio di commit**, che non esegue niente e
//! che le righe di uno heredoc facevano leggere come comandi.
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

use crate::interpreters::{DELEGATES, INTERPRETERS};
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

// `INTERPRETERS` e `DELEGATES` vivono in `crate::interpreters`, condivisi con
// `linear_readonly.rs`: fino al 25/08/2026 questo file ne aveva una copia
// propria, larga solo quanto le shell POSIX (`bash`, `sh`, `zsh`, `dash`,
// `ksh`, `busybox`), e `python3 -c "…rm -rf…"` non veniva mai riletto — buco
// misurato in diretta lo stesso giorno. `linear_readonly.rs` non importa
// ancora da qui: è protetto e non si modifica dall'interno di una sessione
// (segnalazione in coda), quindi tiene la propria copia, già larga.

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

/// La redirezione in testa a un token: `>`, `2>>`, `&>`, `2>&1`, con la
/// destinazione attaccata (`>/dev/null`) o senza.
///
/// Non è mai un bersaglio, e prenderla per tale fa nominare `1` o `/dev/null` al
/// posto della cartella che il comando sta cancellando — il difetto che questo
/// freno esiste per non ripetere.
fn redirection() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d*(>>?|<)&?\d*|&>>?)").unwrap())
}

/// Un'assegnazione il cui valore è una sostituzione di comando: `X=$(…)` o
/// `` X=`…` ``. `assignments()` la scarta di proposito — il valore non si
/// legge — ma qui serve isolarla per riconoscere il solo caso in cui il
/// comando dentro è `mktemp`.
fn command_substitution_assignment() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^\s*([A-Za-z_][A-Za-z0-9_]*)=(?:\$\(([^()]*)\)|`([^`]*)`)\s*$").unwrap()
    })
}

/// Misurato sui sette giorni al 22/08/2026: 90 delle 127 cancellazioni
/// ricorsive avevano il bersaglio in una variabile, quasi sempre una cartella
/// nata da `mktemp`/`mktemp -d` nella stessa riga. Il valore vero non si
/// conosce a tavolino, ma la sua posizione sì: `mktemp` **senza** un modello
/// di percorso esplicito scrive per costruzione sotto la cartella temporanea
/// del sistema. Un modello (`mktemp /altrove/x.XXXXXX`) o `-p`/`--tmpdir`
/// scavalcano quella garanzia, e allora si resta senza risposta.
fn is_pure_mktemp(inner: &str) -> bool {
    let Some(words) = split_words(inner) else {
        return false;
    };
    let Some(head) = words.first() else {
        return false;
    };
    if !ends_with_command(head, "mktemp") {
        return false;
    }
    let mut i = 1;
    while i < words.len() {
        let w = &words[i];
        // Ridondanza voluta e misurata: da sola questa riga non fa cadere
        // nessuna prova — il ramo di riserva del `match` rifiuta comunque
        // `-p`, `--tmpdir` e un modello senza barra — e da sola nemmeno
        // quel ramo basta, perché tolto lui il token successivo (il
        // percorso vero) arriva comunque qui. Tolte insieme,
        // `a_non_scratch_path_via_a_variable_stays_blocked` cade: è un gate
        // che concede, e lì la prima linea dev'essere esplicita.
        if w.contains('/') {
            return false;
        }
        match w.as_str() {
            "-d" | "-q" | "-u" | "--directory" => i += 1,
            // `-t prefisso` (macOS) resta sotto la cartella temporanea: solo
            // il prefisso cambia nome, non la radice. Il valore consumato qui
            // sfugge al controllo sulla barra fatto in testa al ciclo — va
            // ripetuto, o un prefisso che nomina un percorso passerebbe per
            // un nome nudo.
            "-t" => {
                i += 1;
                match words.get(i) {
                    Some(prefix) if prefix.starts_with('-') => {}
                    Some(prefix) if prefix.contains('/') => return false,
                    Some(_) => i += 1,
                    None => {}
                }
            }
            _ => return false,
        }
    }
    true
}

/// La cartella in cui un `mktemp` **con modello assoluto** (o `-p`/`--tmpdir`)
/// scriverà: il genitore del modello. Dentro il perimetro di sessione la
/// cartella temporanea di sistema non è scrivibile, quindi questa è l'unica
/// forma di `mktemp` che funziona lì — e va giudicata sul percorso, come ogni
/// altro bersaglio. `None` appena compare qualcosa che non si legge a tavolino:
/// un modello relativo, una variabile o una sostituzione dentro il percorso.
fn mktemp_template_dir(inner: &str) -> Option<String> {
    let words = split_words(inner)?;
    if !ends_with_command(words.first()?, "mktemp") {
        return None;
    }
    let mut dir: Option<String> = None;
    let mut i = 1;
    while i < words.len() {
        let w = words[i].as_str();
        match w {
            "-d" | "-q" | "-u" | "--directory" => {}
            "-t" => i += 1,
            "-p" => {
                i += 1;
                dir = Some(words.get(i)?.clone());
            }
            _ if w.starts_with("--tmpdir=") => dir = Some(w["--tmpdir=".len()..].to_string()),
            _ if w.starts_with('-') => return None,
            // Un modello con la barra dice da sé dove sta; uno senza è un nome
            // dentro la cartella di `-p`/`--tmpdir`, se c'è — altrimenti è
            // relativo a dove gira il comando, e non si legge.
            _ => match w.rsplit_once('/') {
                Some((parent, _)) => dir = Some(parent.to_string()),
                None if dir.is_some() => {}
                None => return None,
            },
        }
        i += 1;
    }
    let dir = dir?;
    if !dir.starts_with('/') || dir.contains('$') || dir.contains('`') {
        return None;
    }
    Some(dir)
}

/// Le variabili che il comando assegna da un `mktemp`: non risolvibili a un
/// valore letterale, ma risolvibili a «sotto una cartella nota» — la radice di
/// riserva se puro, il genitore del modello se ne ha uno — che è tutto ciò che
/// serve per giudicarle.
fn mktemp_variables(command: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for line in command.replace(';', "\n").split('\n') {
        let Some(c) = command_substitution_assignment().captures(line) else {
            continue;
        };
        let inner = c.get(2).or_else(|| c.get(3)).map(|m| m.as_str()).unwrap_or("");
        let name = &c[1];
        if is_pure_mktemp(inner) {
            out.insert(name.to_string(), format!("{}mktemp-{name}", SCRATCH_ROOTS[0]));
        } else if let Some(dir) = mktemp_template_dir(inner) {
            out.insert(name.to_string(), format!("{dir}/mktemp-{name}"));
        }
    }
    out
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
    /// C'è del lavoro non salvato che quel bersaglio perderebbe?
    ///
    /// Letta da git (`diff --quiet -- <bersaglio>`), non dedotta dal testo: un
    /// `git checkout -- <path>` su un albero pulito è il modo normale di
    /// scartare una modifica che non esiste, e negarlo sarebbe attrito ogni
    /// giorno per niente. Il repository e il bersaglio arrivano assoluti.
    pub has_pending_changes: &'a dyn Fn(&str, &str) -> bool,
}

/// Il gesto riconosciuto, con il bersaglio che il rifiuto potrà nominare.
#[derive(Debug, PartialEq, Eq)]
enum Danger {
    RecursiveDelete { target: String, privileged: bool },
    UnreadableDelete { segment: String },
    HardReset { branch: String, repo: String },
    ForcePush { how: String },
    CheckoutDiscardsChanges { path: String, repo: String },
}

pub fn judge(f: &Facts) -> Decision {
    // La valvola sta **sulla riga di comando**, non nell'ambiente: esportata una
    // volta disarmerebbe in silenzio tutto ciò che viene dopo, scritta qui vale
    // per quel comando e resta nel registro. Esiste perché questo freno è nuovo
    // e non ancora provato sul traffico vero: si toglie quando la misura dirà
    // che non sbaglia, e finché c'è, ogni uso è una misura.
    match first_danger(f.command, f, 0) {
        Some(d) if every_dangerous_segment_is_waived(f) => {
            let _ = &d;
            Decision::Pass
        }
        Some(d) => Decision::Block(explain(&d)),
        // Nessun pericolo: resta da dire se il comando ha passato il lavoro a
        // un delegato che questo freno non può leggere. Non si nega — Codex
        // serve a lavoro vero — ma si rende visibile nel registro, con
        // `Decision::Warn` che `hook_io::emit` scrive e lascia passare.
        None => match named_delegate(f.command) {
            Some(verb) => Decision::Warn(format!(
                "delegato non ispezionabile: {verb}.\n  · Questo freno legge comandi di \
                 shell, non ciò che un altro processo farà: non vede né vieta i suoi gesti.\n  \
                 · Non è negato — {verb} è in uso legittimo — ma resta scritto nel registro."
            )),
            None => Decision::Pass,
        },
    }
}

/// Il primo delegato nominato nel comando, se ce n'è uno.
fn named_delegate(command: &str) -> Option<String> {
    for segment in split_segments(command) {
        let Some(words) = split_words(&segment) else {
            continue;
        };
        let rest = strip_prefixes(&words);
        let Some(verb) = rest.first() else {
            continue;
        };
        if let Some(name) = DELEGATES.iter().find(|d| ends_with_command(verb, d)) {
            return Some((*name).to_string());
        }
    }
    None
}

/// La valvola disarma **il comando che la porta davanti**, non la riga intera.
///
/// PERCHÉ NON BASTA CERCARLA NELLA RIGA. Chiedere «c'è una valvola da qualche
/// parte?» lascia disarmare il freno da un pezzo di contorno: in un
/// `rm -rf <bersaglio>` seguito da `; for x in 1; do FRENO_DISTRUTTIVO=off :; done`
/// la valvola non governa niente di ciò che cancella, e il comando è bash
/// ordinario. Trovato da un revisore il 24/08/2026 addosso alla correzione dello
/// stesso giorno, che aveva allargato la testa del segmento senza legare la
/// valvola al gesto: una valvola che si può mettere altrove non è una valvola, è
/// una parola d'ordine.
///
/// COSA CHIEDE, ESATTAMENTE. Ogni segmento che porta un gesto pericoloso deve
/// portare **anche** la valvola davanti a sé. Un solo segmento pericoloso senza
/// valvola fa negare tutto il comando: chi vuole cancellare due cose lo dichiara
/// due volte.
///
/// IL VERSO SICURO. Se il pericolo si vede solo guardando il comando intero — un
/// heredoc, uno script nominato per percorso — nessun singolo segmento risulta
/// pericoloso, la funzione torna `false` e il freno **nega**. Un dubbio qui
/// costa un rifiuto da spiegare; il dubbio dall'altra parte costa il lavoro di
/// qualcuno.
fn every_dangerous_segment_is_waived(f: &Facts) -> bool {
    let mut any = false;
    for segment in split_segments(f.command) {
        if first_danger(&segment, f, 0).is_none() {
            continue;
        }
        any = true;
        if !valve_in_front(&segment, "FRENO_DISTRUTTIVO=off") {
            return false;
        }
    }
    any
}

/// Le parole di un segmento, con una lettura di ripiego quando le virgolette
/// non si chiudono.
///
/// Serve **solo** a riconoscere dove nasce un messaggio di commit: nell'idioma
/// `git commit -m "$(cat <<'EOF'` la virgoletta si chiude tre righe più in
/// basso, e senza ripiego quella riga non si riconoscerebbe come un commit. Chi
/// giudica un gesto non passa di qui: là una parola che non si legge deve
/// restare non letta, ed è il ramo che nega invece di indovinare.
fn words_or_split(segment: &str) -> Vec<String> {
    split_words(segment).unwrap_or_else(|| segment.split_whitespace().map(str::to_string).collect())
}

/// Il segmento è il posto in cui **nasce** un messaggio di commit?
fn is_commit_context(words: &[String]) -> bool {
    // Il file che git apre nell'editor: ciò che ci finisce dentro è testo.
    if words.iter().any(|w| w.ends_with("COMMIT_EDITMSG")) {
        return true;
    }
    let rest = strip_prefixes(words);
    let Some((verb, args)) = rest.split_first() else {
        return false;
    };
    ends_with_command(verb, "git") && git_subcommand(args).is_some_and(|(sub, _)| sub == "commit")
}

/// Chi riceve uno heredoc **senza eseguirlo**: `git commit -F-`, e il `cat`
/// dell'idioma qui sopra. `bash <<EOF` no — quel corpo viene eseguito davvero,
/// e resta letto come codice anche sulla riga di un commit.
fn receives_text(words: &[String]) -> bool {
    if is_commit_context(words) {
        return true;
    }
    let rest = strip_prefixes(words);
    let Some((verb, args)) = rest.split_first() else {
        return false;
    };
    // Un `cat` che manda il testo altrove sta scrivendo un file, e un file si
    // esegue: solo quello senza destinazione porta il corpo di un messaggio.
    ends_with_command(verb, "cat") && !args.iter().any(|a| a.contains('>'))
}

/// I terminatori degli heredoc aperti da un segmento: `<<EOF`, `<<-EOF`,
/// `<<'EOF'` (le virgolette le toglie `split_words`), `<< EOF`. `<<<` è una
/// stringa, non uno heredoc, e non entra: dopo i due segni ne trova un terzo.
fn heredoc_tags(words: &[String]) -> Vec<String> {
    let mut tags = Vec::new();
    for (i, w) in words.iter().enumerate() {
        let Some(rest) = w.strip_prefix("<<") else {
            continue;
        };
        let rest = rest.strip_prefix('-').unwrap_or(rest);
        if rest.is_empty() {
            // `<< EOF`: il terminatore è la parola dopo. Non la si salta: da
            // sola non comincia per `<<`, quindi il giro successivo la ignora.
            tags.extend(words.get(i + 1).cloned());
        } else if !rest.starts_with('<') {
            tags.push(rest.to_string());
        }
    }
    tags
}

/// Il terminatore dello heredoc che questa riga apre **per farne un messaggio**.
/// Servono tutte e due le condizioni: che sulla riga ci sia un commit, e che a
/// ricevere il testo sia qualcosa che non lo esegue.
fn commit_message_heredoc(line: &str) -> Option<String> {
    let segments: Vec<Vec<String>> =
        split_segments(line).iter().map(|s| words_or_split(s)).collect();
    if !segments.iter().any(|w| is_commit_context(w)) {
        return None;
    }
    segments
        .iter()
        .filter(|w| receives_text(w))
        .flat_map(|w| heredoc_tags(w))
        .next()
}

/// Il comando con il **corpo dei messaggi di commit** svuotato.
///
/// Un messaggio di commit non esegue niente, ma `split_segments` legge ogni riga
/// di uno heredoc come un comando: la notte del 24/08/2026 un commit è stato
/// negato perché il messaggio nominava un riazzeramento forzato, e chi lo
/// scriveva ha cambiato forma al comando invece di segnalarlo. Si svuota il
/// corpo e nient'altro: la riga che lo apre resta giudicata, e dopo il
/// terminatore si torna a comandi veri.
fn without_commit_messages(command: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut terminator: Option<String> = None;
    for line in command.split('\n') {
        match &terminator {
            Some(tag) => {
                if line.trim() == tag.as_str() {
                    terminator = None;
                    out.push(line);
                } else {
                    out.push("");
                }
            }
            None => {
                out.push(line);
                terminator = commit_message_heredoc(line);
            }
        }
    }
    out.join("\n")
}

fn first_danger(command: &str, f: &Facts, depth: usize) -> Option<Danger> {
    if depth > MAX_NESTING {
        return None;
    }
    let command = &without_commit_messages(command);
    let mut variables = assignments(command);
    variables.extend(mktemp_variables(command));
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
    judge_words(&words, variables, f, depth)
}

/// Le lettere che portano codice inline, per interprete: `-c` per
/// bash/sh/python, `-e`/`--eval` per node/ruby/perl/deno.
fn inline_code_flag(a: &str) -> bool {
    matches!(a, "-c" | "-e" | "--eval")
}

/// Le stringhe letterali di un codice che non è sintassi di shell — Python,
/// JavaScript — nell'ordine in cui compaiono.
///
/// `subprocess.run(['rm','-rf','x'])` non si legge come `rm -rf x`:
/// parentesi, virgole e parentesi quadre non sono sintassi di shell, quindi
/// `split_words` le fonde in un'unica parola illeggibile. Ma gli argomenti
/// veri di quella chiamata sono le stringhe fra virgolette, nell'ordine in cui
/// il codice le scrive — ed è tutto ciò che serve per ricostruire l'elenco che
/// il processo eseguirà davvero.
fn quoted_literals(code: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = code.chars();
    while let Some(c) = chars.next() {
        if c != '\'' && c != '"' {
            continue;
        }
        let mut s = String::new();
        for c2 in chars.by_ref() {
            if c2 == c {
                break;
            }
            s.push(c2);
        }
        out.push(s);
    }
    out
}

/// Il codice nomina una funzione nota per uscire verso una shell?
///
/// SENZA QUESTO CANCELLO, `quoted_literals` legge come comando anche una
/// stringa mai eseguita: `print('rm -rf is dangerous, never run it')` non
/// tocca niente, ed è la stessa classe di caso per cui il freno principale
/// non nega chi si limita a *nominare* un gesto (vedi
/// `a_command_that_only_names_a_gesture_is_never_denied`). Il fallback sui
/// letterali si applica solo quando il codice porta uno di questi nomi vicino
/// — non capisce la semantica di Python o JavaScript, ma un `print(...)`
/// senza nessuno di questi non ha modo di eseguire ciò che stampa.
///
/// NON È PRECISO: un marcatore e una stringa innocua nello stesso blocco di
/// codice, senza nessun legame vero fra i due, restano un falso positivo
/// possibile — il prezzo di non poter analizzare la sintassi vera del
/// linguaggio ospite. È il compromesso scritto qui, non taciuto.
fn looks_like_a_shell_escape(code: &str) -> bool {
    // Confronto testuale: si cerca la sottostringa nel codice altrui, non si
    // chiama `eval`/`exec` di nessun linguaggio da qui.
    let lower = code.to_lowercase();
    const MARKERS: &[&str] = &[
        "os.system",
        "os.popen",
        "subprocess.run",
        "subprocess.call",
        "subprocess.check_call",
        "subprocess.check_output",
        "subprocess.popen",
        "execsync",
        "spawnsync",
        "child_process",
        "system(",
        "popen(",
        "shell_exec",
        "eval(",
        "exec(",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

fn judge_words(
    words: &[String],
    variables: &HashMap<String, String>,
    f: &Facts,
    depth: usize,
) -> Option<Danger> {
    if depth > MAX_NESTING {
        return None;
    }
    let rest = strip_prefixes(words);
    let (verb, args) = rest.split_first()?;
    let privileged = words.iter().any(|w| w == "sudo" || w == "doas");

    // Un interprete che riceve del codice: si guarda il codice, non il verbo.
    if INTERPRETERS.iter().any(|i| ends_with_command(verb, i)) {
        let code = args.iter().position(|a| inline_code_flag(a)).and_then(|i| args.get(i + 1))?;
        if let Some(d) = first_danger(code, f, depth + 1) {
            return Some(d);
        }
        // Il codice non è sintassi di shell: se nomina una funzione nota per
        // uscire verso una shell, si prova a leggerlo come una sequenza di
        // argomenti (`subprocess.run(['rm','-rf','x'])`) e, se dentro c'è una
        // stringa che è essa stessa un comando di shell intero
        // (`execSync('git push --force')`), come quella stringa a sé. Senza
        // quel nome, una stringa fra virgolette è testo quanto lo è per il
        // freno principale (`echo "rm -rf /"` non è negato).
        if looks_like_a_shell_escape(code) {
            let literals = quoted_literals(code);
            for lit in &literals {
                if let Some(d) = first_danger(lit, f, depth + 1) {
                    return Some(d);
                }
            }
            if !literals.is_empty() {
                let empty = HashMap::new();
                return judge_words(&literals, &empty, f, depth + 1);
            }
        }
        return None;
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
        if a == "--" || a.starts_with('-') {
            continue;
        }
        // La destinazione staccata (`> /dev/null`) è il token dopo, e
        // giudicarlo significa negare `rm -rf /tmp/x > /dev/null` accusando
        // `/dev/null`. Attaccata (`>/dev/null`) se ne va tutta insieme, e
        // `2>&1` non porta via niente: prenderlo per un bersaglio fa nominare
        // `1`.
        if let Some(m) = redirection().find(a) {
            skip_next = m.len() == a.len() && (a.ends_with('>') || a.ends_with('<'));
            continue;
        }
        // `cartella>destinazione` scritti attaccati: davanti c'è un bersaglio
        // vero, dietro la destinazione, e solo il primo si giudica.
        if let Some(i) = a.find(['>', '<']) {
            skip_next = a.ends_with('>') || a.ends_with('<');
            out.push(expand(&a[..i], variables));
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
        "checkout" => checkout_gesture(args, tail, f),
        _ => None,
    }
}

/// `git checkout -- <path>` e `git checkout .`: scartano le modifiche non
/// salvate di un percorso, non cambiano ramo — e il mandato che ha fatto
/// perdere lavoro vero il 20/08/2026 (`checkout-porta-via-lavoro-altrui`) è
/// esattamente questo gesto.
///
/// SOLO SE C'È DAVVERO QUALCOSA DA PERDERE. `git checkout -- <path>` su un
/// albero pulito è il modo normale di scartare una modifica che non esiste —
/// negarlo sarebbe un attrito quotidiano senza motivo. Si condiziona a `git
/// diff --quiet -- <path>`, come `hard_reset` condiziona al ramo vero: il
/// fatto si legge dal repository, non si indovina dal testo.
///
/// `git checkout <ramo>` senza `--` e senza bersaglio nudo (`.`) resta fuori
/// di proposito: è un cambio di ramo, non uno scarto di modifiche.
fn checkout_gesture(args: &[String], tail: &[String], f: &Facts) -> Option<Danger> {
    let paths: Vec<&String> = if let Some(sep) = tail.iter().position(|a| a == "--") {
        tail[sep + 1..].iter().collect()
    } else if tail == ["."] {
        // `.` non è mai un nome di ramo valido: senza `--` è comunque un
        // percorso, e scarta ogni modifica non salvata nella cartella corrente.
        tail.iter().collect()
    } else {
        return None;
    };
    if paths.is_empty() {
        return None;
    }
    let repo = args
        .iter()
        .position(|a| a == "-C")
        .and_then(|i| args.get(i + 1))
        .cloned()
        .unwrap_or_else(|| f.cwd.to_string());
    let repo = absolute(&repo, f)?;
    for path in paths {
        if (f.has_pending_changes)(&repo, path) {
            return Some(Danger::CheckoutDiscardsChanges {
                path: path.clone(),
                repo,
            });
        }
    }
    None
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

/// L'avvertenza da appendere quando il bersaglio nominato **non è un percorso**
/// ma il testo che lo produrrà.
///
/// Il freno legge il comando come testo e non espande niente: in
/// `d="$T/roba"; rm -rf "$d"` il bersaglio che stampa è `$d`, che non comincia
/// per `/tmp` né per `/var/folders` e quindi non supera il riconoscimento della
/// zona usa-e-getta — anche quando a valle ci finiva davvero. Il 24/08/2026 è
/// costato un diniego su quattro copie abbandonate da `cargo mutants`.
///
/// **Perché dirlo invece di espandere.** Espandere le variabili sposterebbe il
/// verso dell'errore dalla parte sbagliata: un'espansione approssimata farebbe
/// *passare* cancellazioni che oggi si fermano, e questo freno esiste per il
/// caso in cui sbagliare costa il lavoro di qualcuno. Chi legge il rifiuto,
/// invece, ha bisogno di sapere che quel testo non è il percorso: senza,
/// conclude che il freno non riconosce `/tmp` e va a riscrivere il comando
/// finché il freno non lo vede più — che è la lezione peggiore.
fn unexpanded_note(target: &str) -> &'static str {
    if target.contains('$') || target.contains('`') {
        "\n  · **Quel bersaglio non è un percorso**: contiene variabili, e il freno legge il \
         comando come testo senza espanderle. Non sa dove finisce davvero la cancellazione, \
         quindi non può riconoscerla come zona usa-e-getta."
    } else {
        ""
    }
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
                "{head}.{}\n  · Non esiste, in questa casa, una via più sicura del comando nudo: i \
                 rami hanno `rami.py` e le copie di lavoro hanno `orca`, una cartella qualunque \
                 non ha niente. Il freno lo dice invece di inventarla.\n  · Se la cancellazione \
                 serve davvero, la fa una persona a mano fuori da una sessione, dopo aver \
                 guardato cosa c'è dentro.\n  · Il materiale usa-e-getta dentro una copia di \
                 lavoro e sotto /tmp continua a passare: se il bersaglio era uno di quelli, il \
                 percorso qui sopra dice perché non lo sembrava.\n  · Consapevole e necessario: \
                 `FRENO_DISTRUTTIVO=off <comando>`, che resta scritto nel registro.",
                unexpanded_note(target)
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
        Danger::CheckoutDiscardsChanges { path, repo } => format!(
            "riazzeramento di «{path}» in «{repo}», che ha modifiche non salvate.\n  · \
             `git checkout -- <percorso>` (e `git checkout .`) riporta il file alla versione \
             dell'indice e butta via ogni modifica nel mezzo, in silenzio: è il gesto che ha già \
             fatto perdere lavoro vero.\n  · Le modifiche si leggono dal repository (`git diff \
             --quiet -- {path}`), non dal testo del comando.\n  · Vie d'uscita: \
             `git -C {repo} stash push -- {path}` prima; oppure `git -C {repo} diff -- {path}` \
             per guardare cosa si perderebbe."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const W: &str = "/home/someone/orca/workspaces";
    const HOME: &str = "/home/someone";

    fn on(command: &str, cwd: &str, branch: Option<&str>) -> Decision {
        on_dirty(command, cwd, branch, &[])
    }

    /// Come `on`, ma con `dirty` a dire quali bersagli `git diff --quiet`
    /// troverebbe sporchi — serve solo ai casi di `git checkout`.
    fn on_dirty(command: &str, cwd: &str, branch: Option<&str>, dirty: &[&str]) -> Decision {
        let b = branch.map(|s| s.to_string());
        let dirty: Vec<String> = dirty.iter().map(|s| s.to_string()).collect();
        judge(&Facts {
            command,
            cwd,
            workspaces: W,
            home: HOME,
            is_link: &|_| false,
            current_branch: &|_| b.clone(),
            has_pending_changes: &|_repo, path| dirty.iter().any(|d| d == path),
        })
    }

    fn blocked(command: &str) -> bool {
        matches!(
            on(command, "/home/someone/orca/general", None),
            Decision::Block(_)
        )
    }

    fn message(command: &str) -> String {
        match on(command, "/home/someone/orca/general", None) {
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
        let m = message("rm -rf /home/someone/personal/sailor/src");
        assert!(m.contains("/home/someone/personal/sailor/src"), "{m}");
        assert!(m.contains("cancellazione ricorsiva"), "{m}");
        // Le combinazioni di lettere non sono un elenco: si guarda la lettera.
        for form in ["-rf", "-fr", "-Rf", "-r -f", "--recursive -f"] {
            assert!(
                blocked(&format!("rm {form} /home/someone/personal/sailor/src")),
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
            let cmd = format!("npm run build {sep} rm -rf /home/someone/personal/sailor/src");
            assert!(blocked(&cmd), "separatore {sep}");
        }
        // E nomina il **suo** bersaglio, non i comandi che lo precedono: è il
        // difetto misurato sei volte nel freno di terze parti.
        let m = message("npm run build && rm -rf /home/someone/personal/sailor/src");
        assert!(m.contains("/home/someone/personal/sailor/src"), "{m}");
        assert!(!m.contains("npm"), "il rifiuto non accusa il comando accanto: {m}");
    }

    /// MUTANTE: togliere `"sudo"` da `PREFIXES`. Eseguito: questa prova diventa
    /// rossa perché il verbo resta `sudo` e non `rm`. Ripristinato.
    #[test]
    fn a_privileged_recursive_delete_is_blocked_and_says_so() {
        let m = message("sudo rm -rf /home/someone/personal/sailor/src");
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
            "/home/someone/other-repo/work/suite",
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
                        "/home/someone/other-repo/work/suite",
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
            command: "git -C /home/someone/other-repo/work/a-client reset --hard origin/develop",
            cwd: "/home/someone/orca/general",
            workspaces: W,
            home: HOME,
            is_link: &|_| false,
            current_branch: &|repo| {
                *seen.borrow_mut() = repo.to_string();
                Some("develop".to_string())
            },
            has_pending_changes: &|_, _| false,
        });
        assert!(matches!(d, Decision::Block(_)));
        assert_eq!(*seen.borrow(), "/home/someone/other-repo/work/a-client");
    }

    // ── Il quinto caso: cancellare per criterio ─────────────────────────────

    /// MUTANTE: in `find_delete`, togliere `a == "-delete"` dalla condizione.
    /// Eseguito: le tre righe su `-delete` diventano rosse, quelle su `-exec rm`
    /// restano verdi. Ripristinato.
    #[test]
    fn deleting_by_criterion_with_find_is_the_same_action_and_is_blocked() {
        // I due comandi veri con cui una sessione ha aggirato il freno il
        // 21/08/2026, dopo essersi vista negare `rm -rf`.
        let m = message("find /home/someone/.banco-marcatori -type f -print -delete");
        assert!(m.contains("/home/someone/.banco-marcatori"), "{m}");
        assert!(blocked(
            "find /home/someone/.banco-marcatori -depth -type d -print -delete"
        ));
        assert!(blocked("find /home/someone/personal/sailor -delete"));
        assert!(blocked(
            "find /home/someone/personal/sailor -name '*.ts' -exec rm -f {} ;"
        ));
        assert!(blocked(
            "find /home/someone/personal/sailor -type d -execdir rm -rf {} +"
        ));
    }

    #[test]
    fn a_find_that_only_reads_is_not_a_deletion() {
        assert!(!blocked("find /home/someone/personal/sailor -name '*.ts' -print"));
        assert!(!blocked(
            "find /home/someone/personal/sailor -type f -exec grep -n rm {} ;"
        ));
        assert!(!blocked("find /home/someone/personal/sailor -newer /tmp/x -ls"));
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
            "git -C /home/someone/personal/sailor push --force",
            "git -C /home/someone/personal/sailor push -f",
            "git --git-dir=/home/someone/personal/sailor/.git push --force",
            "git push origin +main",
            "git push origin +refs/heads/x:refs/heads/x",
            "cd /home/someone/personal/sailor; git push --force",
            "git -c user.name=x push --force",
        ] {
            assert!(blocked(cmd), "forma non riconosciuta: {cmd}");
        }
        let m = message("git -C /home/someone/personal/sailor push --force");
        assert!(m.contains("pubblicazione forzata"), "{m}");
    }

    /// La via legittima non si chiude: cancellare un ramo remoto è autorizzato
    /// in questa casa, e un freno che lo negasse romperebbe ciò che funzionava.
    #[test]
    fn deleting_a_remote_branch_stays_authorised() {
        assert!(!blocked("git push origin --delete vecchio-ramo"));
        assert!(!blocked("git push origin :vecchio-ramo"));
        assert!(!blocked("git -C /home/someone/personal/sailor push origin --delete x"));
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
        assert!(!blocked("bash -n /home/someone/.claude/scripts/deploy.sh"));
        assert!(!blocked("grep -n rm /home/someone/.claude/scripts/deploy.sh"));
        assert!(!blocked("grep -rn 'rm -rf' /home/someone/.claude/scripts"));
        assert!(!blocked(
            "grep -n 'push --force' /home/someone/.claude/scripts/deploy.sh"
        ));
        assert!(!blocked("echo \"rm -rf /\""));
        assert!(!blocked("echo \"git push --force\""));
        assert!(!blocked("echo 'find /home/someone -delete'"));
        assert!(!blocked("cat /home/someone/.claude/scripts/rm-tutto.sh"));
        assert!(!blocked(
            "rg --files-with-matches 'find . -delete' /home/someone/.claude"
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
            "cat >> /home/someone/.claude/state/plancia/segnalazioni/nota.md"
        ));
        assert!(!blocked(
            "echo 'the ban on git push --force is easy to walk around' >> /tmp/nota.md"
        ));
        assert!(!blocked("cargo test -p guards names_not_touches"));
        assert!(!blocked("cargo test names-not-touches -- --nocapture"));
        // Un `rm` che il comando sta soltanto cercando dentro un sorgente.
        assert!(!blocked(
            "grep -rn 'rm -rf' /home/someone/.claude/rust/crates/guards/src"
        ));
    }

    #[test]
    fn ordinary_work_is_not_touched() {
        assert!(!blocked("git log --oneline -5"));
        assert!(!blocked("git reset --soft HEAD~1"));
        assert!(!blocked("git rm -r --cached node_modules"));
        assert!(!blocked("npm run build"));
        assert!(!blocked("rm /home/someone/personal/sailor/src/uno.txt"));
        assert!(!blocked("rm -f /home/someone/personal/sailor/src/uno.txt"));
        assert!(!blocked("npm run rm-rf-helper"));
        assert!(!blocked("ls -R /home/someone/personal/sailor"));
        assert!(!blocked("cp -r /home/someone/a /home/someone/b"));
    }

    // ── Il settimo caso: la variabile nata da `mktemp` ──────────────────────

    /// Il difetto misurato il 22/08/2026: 90 delle 127 cancellazioni ricorsive
    /// di sette giorni avevano il bersaglio in una variabile, e il valore era
    /// quasi sempre una cartella temporanea appena creata.
    ///
    /// MUTANTE: in `is_pure_mktemp`, sostituire `ends_with_command(head,
    /// "mktemp")` con `true`. Eseguito: questa prova resta verde (il verbo
    /// resta comunque `mktemp`), ma `a_non_scratch_path_via_a_variable_stays_
    /// blocked` diventa rossa, perché `D=$(qualcosa)` verrebbe letto come
    /// scratch. Ripristinato.
    ///
    /// UN SECONDO MUTANTE, PIÙ DEBOLE, NON UCCIDE NIENTE E LO SCRIVO PERCHÉ SI
    /// VEDA: togliere il controllo sulla barra in testa al ciclo — quello
    /// prima del `match` — non fa arrossire nessuna prova di questo file,
    /// nemmeno `a_non_scratch_path_via_a_variable_stays_blocked`: ogni parola
    /// coi due punti che nomina un percorso finisce comunque nel ramo di
    /// riserva del `match`, che non la riconosce come `-d`, `-q`, `-u`,
    /// `--directory` o `-t`. È ridondanza voluta (vedi il commento lì), non
    /// una riga che regge una prova. Il controllo che regge davvero è quello
    /// dentro il ramo `-t`, isolato da `a_slash_after_dash_t_is_not_a_bare_
    /// prefix` qui sotto.
    #[test]
    fn a_variable_assigned_from_a_bare_mktemp_call_is_disposable() {
        assert!(!blocked("D=$(mktemp -d); rm -rf \"$D\""));
        assert!(!blocked("D=$(mktemp -d)\nrm -rf \"$D\""));
        assert!(!blocked("D=$(mktemp -d); rm -rf $D"));
        assert!(!blocked("D=$(mktemp -d); rm -rf \"${D}/sub\""));
        assert!(!blocked("D=`mktemp -d`; rm -rf \"$D\""));
        assert!(!blocked("F=$(mktemp); rm -rf \"$F\""));
        assert!(!blocked("D=$(mktemp -d -t sessione); rm -rf \"$D\""));
    }

    /// Un modello di percorso esplicito, o `-p`/`--tmpdir`, tolgono la
    /// garanzia: `mktemp` può scrivere dove gli si dice. Resta bloccato come
    /// prima di questa correzione.
    ///
    /// MISURATO, non un solo mutante: le prime tre righe sono protette due
    /// volte (il controllo sulla barra in testa al ciclo, e il ramo di
    /// riserva del `match`), e nessuno dei due da solo le fa cadere — tolti
    /// insieme sì. Le ultime due dipendono da un controllo diverso, isolato
    /// da `a_variable_assigned_from_a_bare_mktemp_call_is_disposable`.
    #[test]
    fn a_non_scratch_path_via_a_variable_stays_blocked() {
        assert!(blocked(
            "D=$(mktemp -d /home/someone/personal/sailor/x.XXXXXX); rm -rf \"$D\""
        ));
        assert!(blocked("D=$(mktemp -p /home/someone/.claude); rm -rf \"$D\""));
        assert!(blocked("D=$(mktemp --tmpdir=/home/someone/.claude); rm -rf \"$D\""));
        // Un valore reale, non da mktemp, assegnato per esteso: già coperto da
        // `a_variable_the_command_defines_is_resolved_before_judging`, qui si
        // isola il caso in cui il comando dentro le parentesi non è leggibile.
        assert!(blocked("D=$(uuidgen); rm -rf \"$D\""));
        assert!(blocked("D=$(cat /home/someone/percorso-segreto); rm -rf \"$D\""));
    }

    /// Dentro il perimetro `mktemp` senza modello non scrive (la cartella di
    /// sistema non è fra le scrivibili), quindi in sessione si usa sempre un
    /// modello nello scratchpad: il genitore del modello è noto e si giudica
    /// come un percorso qualunque. Un modello fuori dalle radici di riserva
    /// resta bloccato (`a_non_scratch_path_via_a_variable_stays_blocked`).
    ///
    /// MUTANTE: in `mktemp_variables`, togliere il ramo `else if let Some(dir)`.
    /// Eseguito: questa prova diventa rossa e solo questa. Ripristinato.
    #[test]
    fn a_variable_from_mktemp_with_a_scratch_template_is_disposable() {
        let s = "/private/tmp/claude-501/-Users-theo-orca-general/abc/scratchpad";
        assert!(!blocked(&format!("D=$(mktemp -d {s}/prova.XXXXXX); rm -rf \"$D\"")));
        assert!(!blocked(&format!("D=$(mktemp -d -p {s}); rm -rf \"$D\"")));
        assert!(!blocked(&format!("D=$(mktemp --tmpdir={s} x.XXXXXX); rm -rf \"$D\"")));
        assert!(!blocked(&format!("F=$(mktemp {s}/f.XXXXXX); rm -rf \"$F\"")));
    }

    /// Ciò che non si legge a tavolino resta non letto: modello relativo,
    /// variabile o sostituzione nel percorso.
    #[test]
    fn a_mktemp_template_that_cannot_be_read_stays_blocked() {
        assert!(blocked("D=$(mktemp -d prova.XXXXXX); rm -rf \"$D\""));
        assert!(blocked("D=$(mktemp -d $TMPDIR/prova.XXXXXX); rm -rf \"$D\""));
        assert!(blocked("D=$(mktemp -d -p $HOME); rm -rf \"$D\""));
        assert!(blocked("D=$(mktemp -d --tmpdir=/home/someone/.claude x.XXXXXX); rm -rf \"$D\""));
    }

    /// Isola il controllo sulla barra: senza, il valore di `-t` verrebbe
    /// consumato senza guardarlo, e un prefisso che nomina un percorso
    /// passerebbe come se fosse un nome nudo.
    ///
    /// MUTANTE: in `is_pure_mktemp`, nel ramo `-t`, togliere
    /// `Some(prefix) if prefix.contains('/') => return false`. Eseguito:
    /// questa prova diventa rossa e **solo** questa. Ripristinato.
    #[test]
    fn a_slash_after_dash_t_is_not_a_bare_prefix() {
        assert!(blocked("D=$(mktemp -t /home/someone/.claude); rm -rf \"$D\""));
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
        assert!(blocked("bash -c \"rm -rf /home/someone/personal/sailor/src\""));
        // Ma leggere uno script che *contiene* una cancellazione non lo è.
        assert!(!blocked("bash -n /home/someone/.claude/scripts/deploy.sh"));
        assert!(!blocked("sh /home/someone/.claude/scripts/deploy.sh"));
    }

    #[test]
    fn a_wrapper_does_not_hide_the_verb() {
        assert!(blocked("timeout 30 rm -rf /home/someone/personal/sailor/src"));
        assert!(blocked("env -C /tmp rm -rf /home/someone/personal/sailor/src"));
        assert!(blocked("nohup rm -rf /home/someone/personal/sailor/src"));
        assert!(blocked("/bin/rm -rf /home/someone/personal/sailor/src"));
        assert!(blocked("A=1 rm -rf /home/someone/personal/sailor/src"));
    }

    #[test]
    fn targets_that_arrive_from_outside_the_command_are_not_readable() {
        assert!(blocked("cat lista | xargs rm -rf"));
        assert!(blocked("rm -rf $(cat lista)"));
        assert!(blocked("rm -rf /home/someone/personal/sailor/*"));
        assert!(blocked("rm -rf \"$NONDEFINITA/x\""));
    }

    /// Una variabile che il comando stesso definisce si risolve, ed è così che
    /// il rifiuto nomina il percorso vero invece di `$S`.
    #[test]
    fn a_variable_the_command_defines_is_resolved_before_judging() {
        let m = message("S=/home/someone/personal/sailor\nrm -rf \"$S/src\"");
        assert!(m.contains("/home/someone/personal/sailor/src"), "{m}");
        // E se punta a materiale usa-e-getta, passa.
        assert!(!blocked(&format!("T={W}/suite/tautog\nrm -rf \"$T/dist\"")));
    }

    /// MUTANTE: nel ramo in cui `split_words` fallisce, togliere il controllo
    /// sul testo grezzo e tornare `None`. Eseguito: questa prova diventa rossa e
    /// **solo** questa. Ripristinato.
    #[test]
    fn a_command_it_cannot_read_is_refused_instead_of_waved_through() {
        let m = message("rm -rf \"/home/someone/personal/sailor/src");
        assert!(m.contains("non so leggere"), "{m}");
        // Ma un comando illeggibile che non distrugge niente passa lo stesso.
        assert!(!blocked("echo \"aperta"));
    }

    /// La redirezione non è un bersaglio: prenderla per tale è il modo esatto in
    /// cui il freno di terze parti nomina il file sbagliato.
    #[test]
    fn a_redirection_is_never_named_as_the_target() {
        let m = message("rm -rf /home/someone/personal/sailor/src > /tmp/log 2>&1");
        assert!(m.contains("/home/someone/personal/sailor/src"), "{m}");
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
        let d = on("rm -rf src", "/home/someone/personal/sailor", None);
        let Decision::Block(m) = d else {
            panic!("expected a block");
        };
        assert!(m.contains("/home/someone/personal/sailor/src"), "{m}");
    }

    #[test]
    fn the_valve_is_read_in_front_of_the_command_and_nowhere_else() {
        assert!(!blocked(
            "FRENO_DISTRUTTIVO=off rm -rf /home/someone/personal/sailor/src"
        ));
        assert!(blocked(
            "rm -rf /home/someone/personal/sailor/src --nota FRENO_DISTRUTTIVO=off"
        ));
    }

    /// La valvola apre **il comando che la porta davanti**, non la riga.
    ///
    /// Trovato da un revisore il 24/08/2026, addosso alla correzione dello
    /// stesso giorno: allargare la testa del segmento senza legare la valvola al
    /// gesto pericoloso lascia disarmare il freno da un pezzo di contorno.
    /// `rm -rf <bersaglio>; for x in 1; do FRENO_DISTRUTTIVO=off :; done` è un
    /// comando bash ordinario, e la valvola non governa niente di ciò che
    /// cancella.
    #[test]
    fn a_valve_on_another_segment_does_not_open_the_dangerous_one() {
        let target = "/home/someone/personal/sailor/src";
        assert!(blocked(&format!(
            "rm -rf {target}; for x in 1; do FRENO_DISTRUTTIVO=off :; done"
        )));
        assert!(blocked(&format!("FRENO_DISTRUTTIVO=off :; rm -rf {target}")));
        assert!(blocked(&format!("rm -rf {target}; FRENO_DISTRUTTIVO=off :")));
        assert!(blocked(&format!(
            "FRENO_DISTRUTTIVO=off echo pulizia && rm -rf {target}"
        )));
        // E la valvola davanti al gesto pericoloso continua ad aprire, anche
        // quando la riga porta altro: è il caso per cui la valvola esiste.
        assert!(!blocked(&format!("echo pulizia; FRENO_DISTRUTTIVO=off rm -rf {target}")));
    }

    /// Il caso vero del 24/08/2026, che la valvola non apriva: quattro copie
    /// abbandonate da `cargo mutants`, cancellate dentro un ciclo, con la
    /// valvola scritta dove il rifiuto dice di scriverla.
    #[test]
    fn the_valve_opens_inside_a_loop_too() {
        assert!(!blocked(
            "for n in A B; do\n  d=\"$T/cargo-mutants-$n.tmp\"\n  \
             FRENO_DISTRUTTIVO=off rm -rf \"$d\"\ndone"
        ));
        // Senza valvola lo stesso ciclo resta bloccato: è la valvola che apre,
        // non il ciclo che nasconde.
        assert!(blocked(
            "for n in A B; do\n  d=\"$T/cargo-mutants-$n.tmp\"\n  rm -rf \"$d\"\ndone"
        ));
    }

    /// Un bersaglio che è testo, non un percorso: il rifiuto deve dirlo, o chi
    /// legge conclude che il freno non riconosce le zone usa-e-getta e va a
    /// riscrivere il comando finché il freno non lo vede più.
    #[test]
    fn a_target_still_carrying_variables_says_it_is_not_a_path() {
        let Decision::Block(m) = on(
            "for n in A B; do\n  d=\"$T/cargo-mutants-$n.tmp\"\n  rm -rf \"$d\"\ndone",
            "/home/someone/orca/general",
            None,
        ) else {
            panic!("expected a block");
        };
        assert!(m.contains("$T/cargo-mutants-$n.tmp"), "{m}");
        assert!(m.contains("non è un percorso"), "{m}");
        // E un bersaglio vero non porta quell'avvertenza addosso.
        let Decision::Block(m) = on("rm -rf src", "/home/someone/personal/sailor", None) else {
            panic!("expected a block");
        };
        assert!(!m.contains("non è un percorso"), "{m}");
    }

    // ── I gesti che la misura dei mutanti ha trovato scoperti ───────────────

    /// Il riazzeramento **nudo**. Ogni prova qui sopra passa un bersaglio
    /// (`origin/main`, `HEAD~3`), quindi la forma più corta e più frequente non
    /// era provata da nessuna: bastava leggere `--hard` al contrario perché il
    /// divieto sparisse proprio sul gesto che si digita ogni giorno.
    #[test]
    fn a_hard_reset_with_no_target_at_all_is_still_blocked() {
        assert!(matches!(
            on(
                "git reset --hard",
                "/home/someone/other-repo/work/suite",
                Some("main")
            ),
            Decision::Block(_)
        ));
        assert!(matches!(
            on(
                "git reset --hard",
                "/home/someone/other-repo/work/suite",
                Some("fix/x")
            ),
            Decision::Pass
        ));
    }

    /// `find` senza percorso lavora sulla cartella corrente: il permesso lo
    /// decide dove gira il comando, e i privilegi lo tolgono comunque.
    #[test]
    fn a_find_without_a_path_is_judged_on_the_working_directory() {
        assert!(matches!(
            on(
                "find -type f -delete",
                &format!("{W}/suite/tautog/dist"),
                None
            ),
            Decision::Pass
        ));
        assert!(blocked("find -type f -delete"));
        assert!(matches!(
            on(
                "sudo find -type f -delete",
                &format!("{W}/suite/tautog/dist"),
                None
            ),
            Decision::Block(_)
        ));
    }

    /// E con il percorso scritto: la via d'uscita vale per `find` quanto per
    /// `rm`, e si spegne davanti ai privilegi allo stesso modo.
    #[test]
    fn a_find_that_deletes_throwaway_material_passes_unless_privileged() {
        assert!(!blocked("find /tmp/costruzione-123 -type f -delete"));
        assert!(!blocked(&format!("find {W}/suite/tautog/dist -delete")));
        assert!(blocked("sudo find /tmp/costruzione-123 -type f -delete"));
    }

    /// Il nome della variabile non cambia il gesto: l'assegnazione in testa si
    /// toglie comunque, e dietro resta `rm`.
    #[test]
    fn an_assignment_in_front_is_stripped_whatever_the_variable_is_called() {
        for name in ["A", "MY_VAR", "_TMP", "x2"] {
            assert!(
                blocked(&format!("{name}=1 rm -rf /home/someone/personal/sailor/src")),
                "{name}"
            );
        }
    }

    /// Un comando che finisce prima del verbo non deve far cadere il freno: se
    /// cade, il gancio muore e ogni comando dopo di lui passa senza giudizio.
    #[test]
    fn a_command_that_ends_before_its_verb_does_not_bring_the_brake_down() {
        for cmd in [
            "sudo -v",
            "env -i",
            "timeout -k",
            "timeout 30",
            "nice -n 10",
            // E `git` con le sole opzioni globali: dietro non c'è nessun
            // sottocomando da leggere.
            "git --version",
            "git -C /home/someone/personal/sailor",
        ] {
            assert!(!blocked(cmd), "{cmd}");
        }
    }

    /// Un nome di variabile che la shell non accetta non è un'assegnazione: la
    /// shell cerca quella parola fra i comandi, e `rm` resta un argomento che
    /// nessuno esegue. Trattarla come un involucro è un falso positivo.
    #[test]
    fn a_word_that_only_looks_like_an_assignment_is_the_command_itself() {
        for cmd in ["1BAD=x", "=x", "no-name=x"] {
            assert!(
                !blocked(&format!("{cmd} rm -rf /home/someone/personal/sailor/src")),
                "{cmd}"
            );
        }
    }

    /// Un glob non si legge a tavolino, e `/tmp/*` non è «una cartella
    /// temporanea»: è la cartella temporanea di tutti.
    #[test]
    fn a_glob_is_never_read_as_a_scratch_path() {
        assert!(blocked("rm -rf /tmp/*"));
        assert!(blocked("rm -rf /tmp/costruzione-?"));
        assert!(blocked(&format!("rm -rf {W}/suite/tautog/*")));
    }

    /// La destinazione di una redirezione non è un bersaglio nemmeno quando il
    /// bersaglio vero è materiale usa-e-getta: `rm -rf /tmp/x > /dev/null` è il
    /// modo normale di scrivere quel comando.
    #[test]
    fn a_redirection_target_is_not_a_target_even_when_the_command_passes() {
        assert!(!blocked("rm -rf /tmp/costruzione-123 > /dev/null 2>&1"));
        assert!(!blocked("rm -rf /tmp/costruzione-123 >/dev/null"));
        assert!(!blocked(&format!(
            "rm -rf {W}/suite/tautog/dist > /home/someone/nota.log"
        )));
        // Ma il bersaglio vero resta giudicato: la redirezione non lo copre.
        assert!(blocked(
            "rm -rf /home/someone/personal/sailor/src > /dev/null 2>&1"
        ));
        // Una redirezione che si chiude da sé non porta via il token che la
        // segue: lì il bersaglio c'è, e il rifiuto deve nominare lui.
        let m = message("rm -rf 2>&1 /home/someone/personal/sailor/src");
        assert!(m.contains("/home/someone/personal/sailor/src"), "{m}");
        // Attaccata al bersaglio, invece, se ne porta via solo la destinazione.
        assert!(!blocked("rm -rf /tmp/costruzione-123> /home/someone/nota.log"));
    }

    /// `-p` dice dove: il suo valore va letto, non saltato.
    #[test]
    fn a_scratch_root_given_with_dash_p_is_read_as_the_directory() {
        assert!(!blocked("D=$(mktemp -d -p /tmp); rm -rf \"$D\""));
        assert!(blocked(
            "D=$(mktemp -d -p /home/someone/.claude); rm -rf \"$D\""
        ));
    }

    /// `-t prefisso` non toglie il modello: se il modello c'è, è lui a dire
    /// dove si scrive.
    #[test]
    fn a_template_after_a_dash_t_prefix_still_says_where() {
        assert!(!blocked(
            "D=$(mktemp -t prova /tmp/prova.XXXXXX); rm -rf \"$D\""
        ));
        assert!(blocked(
            "D=$(mktemp -t prova /home/someone/.claude/x.XXXXXX); rm -rf \"$D\""
        ));
        // Il valore di `-t` è un prefisso, non un'opzione: in `-t -t prova` il
        // secondo `-t` resta un'opzione e `prova` è il suo prefisso.
        assert!(!blocked("D=$(mktemp -t -t prova); rm -rf \"$D\""));
    }

    /// Un'opzione che il freno non conosce toglie la garanzia: dove scriverà
    /// quel `mktemp` non si sa più, e ciò che non si legge non passa.
    #[test]
    fn an_option_the_brake_does_not_know_takes_the_guarantee_away() {
        assert!(blocked(
            "D=$(mktemp -d -p /tmp --suffix=.log); rm -rf \"$D\""
        ));
        // L'ordine fra modello relativo e `-p` lo decide `mktemp`, non questo
        // freno: due letture possibili, quindi nessuna.
        assert!(blocked(
            "D=$(mktemp -d prova.XXXXXX -p /tmp); rm -rf \"$D\""
        ));
    }

    /// Un percorso che si compone mentre gira — relativo, o con dentro una
    /// sostituzione di comando — non è un percorso noto.
    #[test]
    fn a_template_that_is_built_while_it_runs_is_not_a_known_directory() {
        assert!(matches!(
            on(
                "D=$(mktemp -d sotto/prova.XXXXXX); rm -rf \"$D\"",
                &format!("{W}/suite/tautog"),
                None
            ),
            Decision::Block(_)
        ));
        assert!(blocked(
            "D=$(mktemp -d /tmp/`whoami`/x.XXXXXX); rm -rf \"$D\""
        ));
    }

    /// Tre involucri uno dentro l'altro: `MAX_NESTING` dichiara di seguirne
    /// tre, e il terzo livello dev'essere ancora dentro.
    #[test]
    fn code_nested_three_interpreters_deep_is_still_read() {
        assert!(blocked(
            "bash -c \"bash -c 'sh -c \\\"rm -rf /home/someone/personal/sailor/src\\\"'\""
        ));
    }

    /// Un ramo di lavoro non è un tronco: il riazzeramento lì è mestiere normale,
    /// e negarlo fermerebbe 19 comandi veri su 21 misurati in sette giorni.
    #[test]
    fn a_hard_reset_on_a_working_branch_is_ordinary_work() {
        assert!(matches!(
            on(
                "git reset --hard origin/fix/x",
                "/home/someone/other-repo/work/suite",
                Some("fix/x")
            ),
            Decision::Pass
        ));
        // E un repo di cui non si legge il ramo non si giudica a indovinare.
        assert!(matches!(
            on("git reset --hard HEAD~1", "/home/someone/orca/general", None),
            Decision::Pass
        ));
    }

    // ── Il messaggio di commit: testo, non un gesto ─────────────────────────

    /// Il caso vero della notte del 24/08/2026. `split_segments` legge le righe
    /// di uno heredoc come comandi, quindi un messaggio che **descriveva** un
    /// riazzeramento forzato è stato negato — e chi lo scriveva ha cambiato
    /// forma al comando invece di segnalarlo.
    ///
    /// MUTANTE: in `first_danger`, togliere la riga che svuota i messaggi (cioè
    /// `without_commit_messages` ridotta all'identità). Eseguito: questa prova
    /// diventa rossa e le altre restano verdi. Ripristinato.
    #[test]
    fn a_commit_message_is_text_and_never_a_gesture() {
        // L'idioma con cui questa casa scrive i messaggi lunghi.
        let idiom = "git commit -m \"$(cat <<'EOF'\n\
                     test(guards): close the mutants left alive on the two brakes\n\
                     \n\
                     The branch is read from the repository, so a bare\n\
                     `git reset --hard HEAD~3` is blocked too. Same for\n\
                     `rm -rf /home/someone/personal/sailor` and `git push --force`.\n\
                     EOF\n\
                     )\"";
        assert!(matches!(
            on(idiom, "/home/someone/other-repo/work/suite", Some("main")),
            Decision::Pass
        ));
        // `-F-`: lo heredoc arriva a git, che non lo esegue.
        assert!(!blocked(
            "git commit -F- <<'EOF'\nrm -rf /home/someone/personal/sailor\nEOF"
        ));
        // Il file che git apre nell'editor: ciò che ci finisce dentro è testo.
        assert!(!blocked(
            "cat > .git/COMMIT_EDITMSG <<'EOF'\nrm -rf /home/someone/personal/sailor\nEOF"
        ));
        // Un apostrofo nel testo non rende il comando «illeggibile»: prima
        // spariva il corpo del messaggio e il rifiuto nominava un gesto che
        // nessuno stava facendo.
        assert!(!blocked(
            "git commit -F- <<'EOF'\nl'errore era rm -rf /home/someone\nEOF"
        ));
        // Il messaggio scritto sulla riga era già testo, e resta tale.
        assert!(!blocked(
            "git commit -m 'rm -rf / e git push --force, spiegati'"
        ));
        assert!(!blocked("git commit --message=\"rm -rf /home/someone\""));
        assert!(!blocked("git commit -F /tmp/messaggio.txt"));
    }

    /// IL VINCOLO: l'esenzione vale per il **messaggio**, mai per il comando che
    /// gli sta accanto.
    ///
    /// MUTANTE: in `receives_text`, tornare `true` sempre. Eseguito: la riga su
    /// `bash <<EOF` e quella su `cat > script.sh` diventano rosse. Ripristinato.
    #[test]
    fn the_exemption_covers_the_message_and_nothing_beside_it() {
        let t = "/home/someone/personal/sailor";
        assert!(blocked(&format!("git commit -m \"x\" && rm -rf {t}")));
        assert!(blocked(&format!("git commit -F /tmp/msg.txt; rm -rf {t}")));
        // Dopo il terminatore si torna a comandi veri.
        assert!(blocked(&format!(
            "git commit -F- <<'EOF'\nun messaggio\nEOF\nrm -rf {t}"
        )));
        // Uno heredoc che qualcuno **esegue** resta letto come codice, anche
        // sulla riga di un commit.
        assert!(blocked(&format!(
            "git commit -m x && bash <<'EOF'\nrm -rf {t}\nEOF"
        )));
        // E un `cat` che manda il testo in un file sta scrivendo un file, che
        // qualcuno eseguirà: non è un messaggio.
        assert!(blocked(&format!(
            "git commit -m x && cat > script.sh <<'EOF'\nrm -rf {t}\nEOF"
        )));
        // Senza un commit sulla riga non c'è nessuna esenzione.
        assert!(blocked(&format!("cat <<'EOF'\nrm -rf {t}\nEOF")));
        // Un git che non è un commit non esenta niente...
        assert!(blocked(&format!("git stash list <<'EOF'\nrm -rf {t}\nEOF")));
        // ...e un «commit» che non è di git nemmeno.
        assert!(blocked(&format!("jj commit <<'EOF'\nrm -rf {t}\nEOF")));
    }

    /// Le forme dello heredoc, e ciò che non lo è.
    ///
    /// MUTANTE: in `heredoc_tags`, togliere il ramo `rest.is_empty()` — quello
    /// del terminatore staccato (`<< EOF`). Eseguito: cade **la riga della
    /// coda** di quella forma, e solo quella. Il corpo resta verde perché il
    /// terminatore diventa la stringa vuota, che non combacia con nessuna riga:
    /// il messaggio sparisce comunque, e con lui tutto ciò che segue. È la
    /// ragione per cui ogni forma qui ha due righe invece di una.
    /// Ripristinato.
    #[test]
    fn every_shape_of_heredoc_is_read_and_a_here_string_is_not_one() {
        let t = "/home/someone/personal/sailor";
        for open in ["<<EOF", "<< EOF", "<<'EOF'", "<<\"EOF\""] {
            assert!(
                !blocked(&format!("git commit -F- {open}\nrm -rf {t}\nEOF")),
                "corpo di {open}"
            );
            assert!(
                blocked(&format!(
                    "git commit -F- {open}\nun messaggio\nEOF\nrm -rf {t}"
                )),
                "coda di {open}"
            );
        }
        // `<<-` accetta il terminatore rientrato: senza toglierne il segno, il
        // terminatore non combacerebbe più e la coda passerebbe.
        assert!(!blocked(&format!(
            "git commit -F- <<-EOF\n\trm -rf {t}\n\tEOF"
        )));
        assert!(blocked(&format!(
            "git commit -F- <<-EOF\n\tun messaggio\n\tEOF\nrm -rf {t}"
        )));
        // `<<<` è una stringa, non uno heredoc: la riga dopo è un comando vero.
        assert!(blocked(&format!(
            "git commit -F- <<<\"messaggio\"\nrm -rf {t}"
        )));
    }

    // ── Il buco misurato il 25/08/2026: interpreti non-shell, delegati, checkout ──

    fn warned(command: &str) -> Option<String> {
        match on(command, "/home/someone/orca/general", None) {
            Decision::Warn(m) => Some(m),
            _ => None,
        }
    }

    /// Il caso vero, misurato in diretta: `python3 -c "…subprocess.run([…])"`
    /// passava con `exit=0` perché `python3` non era fra gli interpreti
    /// riconosciuti. Non basta allargare l'elenco: il codice Python non è
    /// sintassi di shell, e il bersaglio vero sono le stringhe fra virgolette.
    ///
    /// MUTANTE: in `interpreters.rs`, restringere `INTERPRETERS` alle sole
    /// shell POSIX (`bash`, `sh`, `zsh`, `dash`, `ksh`, `busybox`). Eseguito
    /// (con una ricompilazione vera del binario `claude-hooks`, non solo di
    /// questo test): il PoC torna a passare con `exit=0`. Ripristinato, e
    /// l'impronta MD5 del file combacia con quella di prima della mutazione.
    #[test]
    fn python_code_with_a_list_argv_is_read_as_the_command_it_will_run() {
        let m = message(
            "python3 -c \"import subprocess; subprocess.run(['rm','-rf','./una-cartella-finta'])\"",
        );
        assert!(m.contains("una-cartella-finta"), "{m}");
        // `os.system`/`subprocess.call` con una stringa intera valgono quanto
        // `bash -c`: la stringa È shell, e il primo tentativo (senza il
        // fallback sui letterali) la coglie già.
        assert!(blocked(
            "python3 -c \"import os; os.system('rm -rf /home/someone/personal/sailor/src')\""
        ));
    }

    /// La stessa classe di buco con `node -e`: il codice non è shell, ma la
    /// stringa passata a `execSync` lo È, per intero.
    #[test]
    fn node_inline_code_that_shells_out_is_read_too() {
        let m = message(
            "node -e \"require('child_process').execSync('git push --force')\"",
        );
        assert!(m.contains("pubblicazione forzata"), "{m}");
        // `-e` funziona quanto `-c`: prima solo `-c` era riconosciuto.
        assert!(blocked(
            "ruby -e \"system('rm -rf /home/someone/personal/sailor/src')\""
        ));
    }

    /// Il rischio dichiarato del fallback sui letterali: una stringa che è
    /// solo testo (mai eseguita) può contenere per caso un verbo pericoloso
    /// come prima parola. Qui non capita — «print» non è mai `rm`/`git` — ma
    /// la prova esiste per dire dove passa il confine: non si prova a
    /// capire la semantica del Python, si legge cosa il codice esegue quando
    /// il primo pezzo è già un verbo riconosciuto.
    #[test]
    fn a_literal_that_only_describes_a_gesture_does_not_trigger_the_fallback_alone() {
        assert!(!blocked("python3 -c \"print('rm -rf is dangerous, never run it')\""));
    }

    /// Il delegato non si vieta: passa, ma il freno lo rende visibile invece
    /// di tacere — è l'unica cosa che può fare, perché non legge cosa farà
    /// quel processo.
    ///
    /// MUTANTE: in `named_delegate`, tornare sempre `None`. Eseguito: questa
    /// prova diventa rossa e basta. Ripristinato.
    #[test]
    fn a_delegate_is_not_blocked_but_becomes_visible() {
        for (cmd, verb) in [
            ("codex exec \"fai qualcosa\"", "codex"),
            ("gemini -p 'fai qualcosa'", "gemini"),
            ("aider --message 'fai qualcosa'", "aider"),
        ] {
            let m = warned(cmd).unwrap_or_else(|| panic!("atteso un avviso per: {cmd}"));
            assert!(m.contains("delegato non ispezionabile"), "{m}");
            assert!(m.contains(verb), "{m}");
            assert!(!blocked(cmd), "un delegato non si nega: {cmd}");
        }
        // Un comando ordinario non genera nessun avviso.
        assert!(warned("npm run build").is_none());
        // Un delegato che porta comunque un gesto riconoscibile resta
        // giudicato su quello, prima di arrivare al ramo dell'avviso: qui non
        // c'è nessun modo di leggere dentro `codex exec`, quindi passa con
        // l'avviso e basta — la riga è qui per dire che l'ordine dei rami non
        // nasconde un pericolo che *si può* leggere.
        assert!(blocked("rm -rf /home/someone/personal/sailor/src && codex exec x"));
    }

    /// `git checkout -- <path>` e `git checkout .`: il caso vero che ha fatto
    /// perdere lavoro il 20/08/2026. Bloccato **solo** se c'è davvero
    /// qualcosa da perdere — letto dal repository, non dal testo.
    ///
    /// MUTANTE: in `checkout_gesture`, tornare sempre `None`. Eseguito: le
    /// prime tre righe diventano rosse. Ripristinato.
    #[test]
    fn checkout_that_discards_real_changes_is_blocked() {
        let repo = "/home/someone/personal/sailor";
        let m = match on_dirty("git checkout -- src/main.rs", repo, None, &["src/main.rs"]) {
            Decision::Block(m) => m,
            other => panic!("atteso un blocco, ottenuto {other:?}"),
        };
        assert!(m.contains("src/main.rs"), "{m}");
        assert!(m.contains("modifiche non salvate"), "{m}");
        assert!(matches!(
            on_dirty("git checkout .", repo, None, &["."]),
            Decision::Block(_)
        ));
        // Più bersagli: basta che uno sia sporco.
        assert!(matches!(
            on_dirty(
                "git checkout -- a.txt b.txt",
                repo,
                None,
                &["b.txt"]
            ),
            Decision::Block(_)
        ));
    }

    /// L'ALBERO PULITO NON FA RUMORE: è il falso positivo da cui questo
    /// gesto va protetto, o l'attrito quotidiano vince sulla protezione.
    ///
    /// MUTANTE: in `checkout_gesture`, ignorare `f.has_pending_changes` e
    /// bloccare sempre. Eseguito: questa prova diventa rossa. Ripristinato.
    #[test]
    fn checkout_on_a_clean_tree_passes_without_noise() {
        let repo = "/home/someone/personal/sailor";
        assert!(matches!(
            on_dirty("git checkout -- src/main.rs", repo, None, &[]),
            Decision::Pass
        ));
        assert!(matches!(
            on_dirty("git checkout .", repo, None, &[]),
            Decision::Pass
        ));
    }

    /// Un cambio di ramo non è uno scarto di modifiche: senza `--` e senza un
    /// bersaglio nudo (`.`), il gesto resta fuori — anche con l'albero sporco.
    #[test]
    fn switching_branches_is_not_touched_even_with_a_dirty_tree() {
        let repo = "/home/someone/personal/sailor";
        assert!(matches!(
            on_dirty("git checkout main", repo, None, &["main"]),
            Decision::Pass
        ));
        assert!(matches!(
            on_dirty("git checkout -b nuovo-ramo", repo, None, &["nuovo-ramo"]),
            Decision::Pass
        ));
    }
}
