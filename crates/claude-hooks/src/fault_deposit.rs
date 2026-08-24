//! Porta in coda i guasti che le automazioni ripetono senza che nessuno legga.
//!
//! Il giudizio — quale riga conta, con quale soglia, che testo esce — sta in
//! `guards::fault_deposit`, che non tocca il disco. Qui c'è tutto il resto: le
//! valvole, il lucchetto che tiene un giro alla volta, la lettura dei registri,
//! la scrittura delle voci e il codice d'uscita.
//!
//! L'USCITA DICE SE IL DEPOSITO È RIUSCITO, non se il programma è arrivato in
//! fondo. Le due cose sembrano la stessa e non lo sono: fino al 21/08/2026 la
//! versione shell usciva sempre zero, quindi una coda non scrivibile veniva
//! annotata da chi la chiama come «il depositatore ha girato». Un guasto vero —
//! disco pieno, permessi, coda irraggiungibile — sparirebbe dietro una riga
//! tranquilla, ed è il caso in cui tacere costa più di tutti gli altri messi
//! insieme.
//!
//! L'ECCEZIONE, DICHIARATA: un giro che trova il lucchetto occupato esce **zero**
//! e lascia una riga **non marcata**, quindi non diventerà mai una voce di coda.
//! È voluto e ha un prezzo. Trovare occupato è la condizione normale quando due
//! giri si sfiorano, e marcarla vorrebbe dire aprire una voce ogni volta che la
//! ronda accavalla se stessa; ma se il lucchetto restasse occupato per davvero,
//! i giri persi resterebbero invisibili fino alla scadenza dei cinque minuti,
//! che è la sola cosa che li fa ripartire.
//!
//! NON ARMA NESSUN LAVORO DI SISTEMA e non ne vuole uno per funzionare: lo
//! chiama la ronda della coda a ogni giro, e chiunque a mano.

use guards::fault_deposit as judge;
use guards::fault_deposit::{Alarm, EntryFacts, RegisterRow, Zone};
use guards::queue_overlap::{is_closed, state_word};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Cinque minuti. Un giro dura meno di cinque secondi: un lucchetto più vecchio
/// di così è di un processo morto, non di uno lento. La scadenza non è
/// facoltativa — senza, un solo `kill -9` fermerebbe il deposito **per sempre e
/// in silenzio**, che è la forma di guasto che questo programma esiste per
/// togliere.
const LOCK_STALE_S: u64 = 300;

/// Cosa chiede chi lo lancia.
#[derive(Debug, PartialEq, Eq, Default)]
pub struct Args {
    /// Senza questo, dice cosa farebbe e non tocca niente.
    pub act: bool,
    /// Scavalca la soglia di **tutti** i registri: serve solo al collaudo,
    /// perché in servizio ogni programma ha la propria e una soglia sola per
    /// tutti è il difetto che la tabella esiste per non ripetere.
    pub forced_threshold: Option<u32>,
    pub queue: Option<PathBuf>,
}

/// UN ARGOMENTO SBAGLIATO ESCE 2, E LO DICE. La versione shell, davanti a
/// `--threshold abc`, dichiarava malformati tutti i registri, li saltava in
/// silenzio e usciva **zero**: chi la chiama legge il codice d'uscita e annota
/// «il depositatore ha girato». Qui la ronda annoterà un fallimento, che è
/// quello che è successo.
pub fn parse(argv: &[String]) -> Result<Args, String> {
    let mut args = Args::default();
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--act" => args.act = true,
            "--threshold" => {
                let v = it.next().ok_or("--threshold vuole un numero")?;
                args.forced_threshold =
                    Some(v.parse().map_err(|_| format!("soglia non valida: {v}"))?);
            }
            "--queue" => {
                let v = it.next().ok_or("--queue vuole una cartella")?;
                args.queue = Some(PathBuf::from(v));
            }
            other => return Err(format!("unknown option: {other}")),
        }
    }
    Ok(args)
}

/// Dove sta ciò che questo programma legge e scrive.
///
/// LE VALVOLE SONO QUATTRO, E SI USANO TUTTE INSIEME. `FAULT_QUEUE` da sola
/// lascia il registro proprio e il lucchetto veri, e un collaudo finisce per
/// contendere il lucchetto a un giro autentico — già successo il 21/08/2026. Il
/// giro autentico lo trova occupato, annota di saltare, ed esce: un giro perso in
/// silenzio, cioè la classe di guasto che questo programma esiste per non far
/// succedere altrove.
#[derive(Clone)]
pub struct Places {
    pub queue: PathBuf,
    pub state: PathBuf,
    pub own_log: PathBuf,
    pub registers: String,
    pub caps: String,
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).map(PathBuf::from)
}

/// Una tabella letta da un file, o quella di serie.
///
/// UN FILE VUOTO È UNA TABELLA VUOTA, non l'assenza di una tabella. La
/// distinzione decide dove va a finire un collaudo: chi punta la valvola a un
/// file vuoto sta dicendo «nessun registro», e ricadere sui quattro di serie
/// vorrebbe dire leggere i registri **veri** — fra cui quello della staffetta,
/// vivo e da un mega — e depositarci sopra con `--act`. Ricade sulla tabella di
/// serie solo il file che non si riesce a leggere affatto, che è un guasto
/// dell'ambiente e non una dichiarazione.
fn table(var: &str, default: &str) -> String {
    match env_path(var) {
        Some(p) => std::fs::read_to_string(p).unwrap_or_else(|_| default.to_string()),
        None => default.to_string(),
    }
}

impl Places {
    pub fn from_env(args: &Args) -> Places {
        let home = home();
        let state = env_path("FAULT_STATE").unwrap_or_else(|| home.join(".claude/state"));
        let own_log = state.join("deposito-guasti.log");
        let queue = args
            .queue
            .clone()
            .or_else(|| env_path("FAULT_QUEUE"))
            .unwrap_or_else(|| home.join(".claude/state/plancia/segnalazioni"));
        let registers = table(
            "FAULT_REGISTERS",
            &judge::DEFAULT_REGISTERS
                .replace("{HOME}", &home.to_string_lossy())
                .replace("{OWN_LOG}", &own_log.to_string_lossy()),
        );
        Places {
            queue,
            state,
            own_log,
            registers,
            caps: table("FAULT_CAPS", judge::DEFAULT_CAPS),
        }
    }
}

pub fn run() -> i32 {
    let argv: Vec<String> = std::env::args().skip(2).collect();
    let args = match parse(&argv) {
        Ok(a) => a,
        Err(why) => {
            eprintln!("{why}");
            return 2;
        }
    };
    let places = Places::from_env(&args);
    run_with(&args, &places, now())
}

/// Il giro, con tutto ciò che dipende dal mondo già deciso da chi chiama: è la
/// forma che si prova senza toccare né l'ambiente né l'orologio.
pub fn run_with(args: &Args, places: &Places, now: i64) -> i32 {
    // Il deposito è la porta di casa per una segnalazione: se qui torna
    // «operation not permitted», chi ha appena trovato un guasto ne trova due, e
    // il secondo non c'è. Servono tutte e due le cartelle anche al giro a vuoto,
    // che scrive comunque il proprio registro.
    for dir in [&places.state, &places.queue] {
        if let Some(why) = not_writable(dir) {
            eprintln!("deposito-guasti: {why}");
            return 78;
        }
    }

    // Il lucchetto si prende **solo quando si scrive**: un giro a secco è
    // diagnostico e deve poter girare mentre l'altro lavora.
    let _lock = if args.act {
        match Lock::take(&places.state.join("deposito-guasti.lock"), now) {
            Ok((l, taken_over)) => {
                if let Some(why) = taken_over {
                    note(&places.own_log, now, &why);
                }
                Some(l)
            }
            Err(why) => {
                note(&places.own_log, now, &why);
                return 0;
            }
        }
    } else {
        None
    };

    let caps = judge::parse_caps(&places.caps);
    // Le spie della tabella escono prima di qualunque registro: una soglia che
    // non può scattare va detta anche quando nessun registro esiste.
    for a in judge::cap_table_alarms(&caps) {
        alarm(&places.own_log, now, &a);
    }

    let mut failed = 0usize;
    for row in judge::parse_registers(&places.registers) {
        // PRIMA «IL FILE C'È», POI «LA RIGA SI CAPISCE», nell'ordine della
        // versione shell. Una riga sbagliata che punta a un registro inesistente
        // non ha nessuno da avvisare, e marcarla come guasto aprirebbe in coda
        // una voce che nessuno può chiudere: il registro proprio è sorvegliato,
        // e tre giri della ronda bastano a farla nascere.
        let Some(log) = read_lossy(Path::new(row.path())) else {
            note(
                &places.own_log,
                now,
                &format!("register missing, skipped: {}", row.path()),
            );
            continue;
        };
        let mut r = match row {
            RegisterRow::Ok(r) => r,
            RegisterRow::Malformed {
                source,
                path,
                threshold,
                per_day,
                window_h,
            } => {
                note_fault(
                    &places.own_log,
                    now,
                    "registro-malformato",
                    &format!(
                        "{source} ({path}): threshold='{threshold}' per_day='{per_day}' \
                         window_h='{window_h}' — skipped"
                    ),
                );
                continue;
            }
        };
        if let Some(f) = args.forced_threshold {
            r.threshold = f;
        }
        if let Some(a) = judge::unreachable_register_alarm(&r) {
            alarm(&places.own_log, now, &a);
        }
        for a in
            judge::raised_threshold_alarms(&caps, &r.source, r.threshold, args.forced_threshold)
        {
            alarm(&places.own_log, now, &a);
        }

        let since = stamp(now - i64::from(r.window_h) * 3600, r.zone);
        for g in judge::extract(&log, &since) {
            let key = judge::key_of(&r.source, &g.name, &g.subject);
            let t = judge::threshold_for(&caps, &r.source, &g.name, r.threshold, args.forced_threshold);
            let note_cap = judge::cap_note(judge::cap_for(&caps, &r.source, &g.name));

            // C'è già una voce per questo guasto? Si cerca la chiave, non il
            // nome del file: il nome può cambiare, la chiave no.
            if let Some(existing) = existing_entry(&places.queue, &key) {
                let Some(text) = read_lossy(&existing) else {
                    continue;
                };
                // LO STATO LO LEGGE IL LETTORE DELLA CODA, non una copia locale:
                // guarda solo dentro il frontmatter e prende la prima parola. La
                // versione shell scandiva tutto il file e pretendeva la colonna
                // zero, quindi su una voce indentata i due non erano d'accordo —
                // e chi sceglie quale voce svegliare usa questo, non quello.
                let closed = state_word(&text).as_deref().is_some_and(is_closed);
                if !closed {
                    if g.count < t {
                        continue;
                    }
                    if !args.act {
                        println!("  would update {}: {} repetitions", name_of(&existing), g.count);
                        continue;
                    }
                    let new = judge::update_header(&text, g.count, &g.last, t);
                    // LA RIGA DI REGISTRO SEGUE L'ESITO DELLA SCRITTURA, non lo
                    // precede: la versione shell annunciava l'aggiornamento e poi
                    // lo falliva in silenzio.
                    if swap(&existing, &new).is_ok() {
                        note(
                            &places.own_log,
                            now,
                            &format!("updated {}: {key}, {} repetitions", name_of(&existing), g.count),
                        );
                    } else {
                        note_fault(
                            &places.own_log,
                            now,
                            "coda-non-scrivibile",
                            &format!("could not update {} for {key}", existing.display()),
                        );
                        eprintln!("  FAILED to update {} for {key}", existing.display());
                        failed += 1;
                    }
                    continue;
                }

                // Voce già chiusa: si guarda solo ciò che è successo DOPO la
                // chiusura. Senza questo taglio la voce appena chiusa
                // rinascerebbe al giro dopo sulle stesse righe di registro, e chi
                // la consuma chiuderebbe la stessa cosa all'infinito.
                let closed_at = modified_at(&existing);
                let after = stamp(closed_at, r.zone);
                // SI RIPRENDE TUTTA LA RIGA, non solo il numero: riportando il
                // solo conteggio, la voce riaperta diceva «2 volte, fra le 09:17
                // e le 09:23» — il numero della finestra nuova e le date di
                // quella vecchia.
                let Some(fresh) = judge::extract(&log, &after)
                    .into_iter()
                    .find(|x| x.name == g.name && x.subject == g.subject)
                else {
                    continue;
                };
                if fresh.count < t {
                    continue;
                }
                if !args.act {
                    println!(
                        "  would reopen {}: {key}, {} repetitions",
                        name_of(&existing),
                        fresh.count
                    );
                    continue;
                }
                let returns = judge::returns_of(&text) + 1;
                let back_at = minute(now);
                let mut new = judge::reopen_header(
                    &text,
                    fresh.count,
                    &fresh.first,
                    &fresh.last,
                    t,
                    returns,
                    &back_at,
                );
                new.push_str(&judge::return_note(
                    fresh.count,
                    t,
                    &note_cap,
                    &fresh.first,
                    &fresh.last,
                    &fresh.text,
                    &back_at,
                    returns,
                ));
                if swap(&existing, &new).is_ok() {
                    note(
                        &places.own_log,
                        now,
                        &format!(
                            "reopened {}: {key}, {} repetitions, return {returns}",
                            name_of(&existing),
                            fresh.count
                        ),
                    );
                } else {
                    note_fault(
                        &places.own_log,
                        now,
                        "coda-non-scrivibile",
                        &format!("could not reopen {} for {key}", existing.display()),
                    );
                    eprintln!("  FAILED to reopen {} for {key}", existing.display());
                    failed += 1;
                }
                continue;
            }

            if g.count < t {
                continue;
            }
            let dest = free_name(
                &places.queue,
                &judge::file_stem(&stamp(now, Zone::Local)[..10], &r.source, &g.name, &g.subject),
            );
            if !args.act {
                println!("  would open {}: {key}, {} repetitions", name_of(&dest), g.count);
                continue;
            }
            let entry = judge::new_entry(&EntryFacts {
                key: &key,
                source: &r.source,
                name: &g.name,
                subject: &g.subject,
                count: g.count,
                threshold: t,
                cap_note: &note_cap,
                first: &g.first,
                last: &g.last,
                text: &g.text,
                log_path: &r.path,
                per_day: r.per_day,
                window_h: r.window_h,
                turns: r.turns(),
                when: &second(now),
            });
            if std::fs::write(&dest, entry).is_ok() {
                note(
                    &places.own_log,
                    now,
                    &format!("opened {}: {key}, {} repetitions", name_of(&dest), g.count),
                );
            } else {
                // Il deposito che non riesce a depositare è il caso in cui tacere
                // sarebbe peggio di tutto: senza questa riga sembrerebbe che non
                // ci fosse niente da segnalare.
                note_fault(
                    &places.own_log,
                    now,
                    "coda-non-scrivibile",
                    &format!("could not write {} for {key}", dest.display()),
                );
                eprintln!("  FAILED to write {} for {key}", dest.display());
                failed += 1;
            }
        }
    }

    if !args.act {
        println!("  (dry run: nothing was written — add --act)");
    }
    if failed > 0 {
        eprintln!(
            "  {failed} queue entries could not be written to {}",
            places.queue.display()
        );
        return 1;
    }
    0
}

// ── Il mondo ────────────────────────────────────────────────────────────────

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Un istante nel fuso del registro che lo scrive, nella forma che si confronta.
fn stamp(epoch: i64, zone: Zone) -> String {
    match zone {
        Zone::Utc => hook_io::local_time::utc_iso_seconds(epoch),
        Zone::Local => hook_io::local_time::local_iso_seconds(epoch),
    }
}

/// `2026-08-24 21:05:00`, l'ora locale come la scrivono le voci di coda.
fn second(epoch: i64) -> String {
    hook_io::local_time::local_iso_seconds(epoch).replace('T', " ")
}

/// Lo stesso, al minuto: è la precisione con cui una voce dichiara un ritorno.
fn minute(epoch: i64) -> String {
    let s = second(epoch);
    s[..16].to_string()
}

fn name_of(p: &Path) -> String {
    p.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

fn modified_at(p: &Path) -> i64 {
    (modified_nanos(p) / 1_000_000_000) as i64
}

/// La stessa data a piena risoluzione, per l'unico posto in cui serve:
/// scegliere la più recente fra due voci. Al secondo, due file scritti nello
/// stesso istante si sarebbero pareggiati e la scelta sarebbe caduta sull'ordine
/// del filesystem.
fn modified_nanos(p: &Path) -> u128 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// Il contenuto di un file, anche se dentro c'è un byte che non è testo.
///
/// PERCHÉ NON `read_to_string`. Un registro è un file in cui altri programmi
/// riversano quello che capita: in quello della staffetta ci sono già 539 righe
/// di uscita grezza di un comando. Con la lettura stretta, un byte fuori posto
/// avrebbe fatto sparire il registro **intero** dietro la riga «register
/// missing», che è falsa — il file c'è. `awk` legge quei file senza batter
/// ciglio, e la sostituzione dei byte illeggibili non tocca le righe buone.
fn read_lossy(p: &Path) -> Option<String> {
    std::fs::read(p)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Il messaggio da dare a chi lancia il deposito dove non può scrivere, o `None`
/// se lì si scrive davvero.
///
/// La prova è una scrittura vera e non un controllo dei permessi: dentro il
/// perimetro di una sessione i bit dicono di sì e la scrittura torna «operation
/// not permitted», e la differenza fra le due risposte è tutto ciò che separa un
/// guasto della macchina da un guasto di questo programma.
fn not_writable(dir: &Path) -> Option<String> {
    let probe = dir.join(".deposito-probe");
    let done = std::fs::create_dir_all(dir).and_then(|_| std::fs::write(&probe, b"x"));
    let _ = std::fs::remove_file(&probe);
    match done {
        Ok(()) => None,
        // I DUE RAMI SONO DUE CONSIGLI DIVERSI, e darne uno solo è il difetto per
        // cui questo messaggio esiste: mandare fuori dal perimetro chi ha il
        // disco pieno o una cartella di un altro utente non ripara niente e fa
        // cercare il guasto dove non è.
        Err(e) if sandbox_protects(dir) => Some(format!(
            "cannot write in {} ({e}) -- operation not permitted.\n  \
             This is NOT a fault in this program. That directory is one of the paths \
             Claude Code's sandbox protects, and listing it under \
             sandbox.filesystem.allowWrite does not reach it.\n  \
             WHAT TO DO: run the very same command again outside the sandbox. With the \
             Bash tool, pass dangerouslyDisableSandbox: true. The Write and Edit tools \
             are never sandboxed and reach these paths too.\n  \
             Do NOT edit the settings file to widen the perimeter: the perimeter is \
             deliberate, and only Theo changes it.",
            dir.display()
        )),
        Err(e) => Some(format!(
            "cannot write in {} ({e}) -- check that it exists and is yours.",
            dir.display()
        )),
    }
}

/// I nomi che il prodotto protegge sotto la cartella di configurazione, presi
/// dal binario in servizio e non da una prova a campione: un elenco ricavato
/// provando le cartelle che esistono oggi salta quelle che nascono domani.
const PROTECTED: &[&str] = &[
    "agents",
    "backups",
    "bridge-spawn",
    "commands",
    "daemon",
    "hooks",
    "ide",
    "jobs",
    "local",
    "output-styles",
    "plugins",
    "projects",
    "routines",
    "rules",
    "session-env",
    "shell-snapshots",
    "skills",
    "state",
    "workflows",
];

fn sandbox_protects(dir: &Path) -> bool {
    let root = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".claude"));
    dir.strip_prefix(&root)
        .ok()
        .and_then(|rest| rest.components().next())
        .is_some_and(|first| PROTECTED.contains(&&*first.as_os_str().to_string_lossy()))
}

/// Un nome libero per una voce nuova. Il suffisso numerico resta solo come rete
/// contro una collisione di nomi: un guasto che torna riapre la propria voce,
/// non ne crea una accanto.
fn free_name(queue: &Path, stem: &str) -> PathBuf {
    let first = queue.join(format!("{stem}.md"));
    if !first.exists() {
        return first;
    }
    (2..)
        .map(|i| queue.join(format!("{stem}-{i}.md")))
        .find(|p| !p.exists())
        .unwrap_or(first)
}

/// La voce che porta questa chiave, la più recente se sono più d'una.
fn existing_entry(queue: &Path, key: &str) -> Option<PathBuf> {
    let line = judge::key_line(key);
    let entries = std::fs::read_dir(queue).ok()?;
    let mut best: Option<(u128, PathBuf)> = None;
    for path in entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "md"))
    {
        let Some(text) = read_lossy(&path) else {
            continue;
        };
        if !text.lines().any(|l| l == line) {
            continue;
        }
        let when = modified_nanos(&path);
        if best.as_ref().is_none_or(|(w, _)| when > *w) {
            best = Some((when, path));
        }
    }
    best.map(|(_, p)| p)
}

/// Sostituisce una voce senza mai lasciarla a metà.
///
/// IL FILE DI PASSAGGIO NASCE ACCANTO AL BERSAGLIO, e le ragioni sono due. La
/// prima è che uno spostamento è atomico solo dentro lo stesso filesystem: dalla
/// cartella temporanea alla coda è una copia, e una copia interrotta lascia una
/// voce mezza scritta. La seconda è che dentro il perimetro di una sessione la
/// cartella temporanea di sistema può essere negata del tutto — nella versione
/// shell `mktemp` nudo rispondeva «operation not permitted», il percorso restava
/// vuoto, e l'aggiornamento falliva in silenzio.
/// IL NOME DEL FILE DI PASSAGGIO NON FINISCE IN `.md`, e non è una finezza: chi
/// enumera la coda filtra per estensione — la ricerca per chiave qui sotto e la
/// passata di freschezza in `queue_freshness` — quindi un `.deposito.…live.md`
/// **è una voce** per loro. Porta dentro la riga `chiave:`, e se lo scambio
/// fallisse senza riuscire a cancellarlo, il giro dopo aggiornerebbe il file di
/// passaggio invece della voce vera.
fn swap(target: &Path, content: &str) -> std::io::Result<()> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let tmp = dir.join(format!(
        ".deposito.{}.{}.tmp",
        std::process::id(),
        target
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    std::fs::write(&tmp, content)?;
    match std::fs::rename(&tmp, target) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

// ── Il registro proprio ─────────────────────────────────────────────────────

fn note(log: &Path, now: i64, message: &str) {
    if let Some(dir) = log.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log) {
        let _ = writeln!(f, "{} {message}", stamp(now, Zone::Local));
    }
}

/// Anche questo programma marca i propri guasti, con la stessa forma che legge:
/// altrimenti sarebbe l'unica automazione della casa a gridare davanti a
/// nessuno. La riga rientra dalla porta principale, perché il proprio registro
/// sta nella tabella dei registri sorvegliati.
fn note_fault(log: &Path, now: i64, fault: &str, message: &str) {
    note(log, now, &format!("[guasto={fault}] {message}"));
}

fn alarm(log: &Path, now: i64, a: &Alarm) {
    note_fault(log, now, a.fault, &a.note);
    if !a.warning.is_empty() {
        eprintln!("{}", a.warning);
    }
}

// ── Un giro alla volta ──────────────────────────────────────────────────────

/// Il lucchetto che serializza i giri che scrivono.
///
/// Fra la lettura della coda e la scrittura c'è un istante in cui un secondo
/// giro vedrebbe la coda di prima: due voci per lo stesso guasto, cioè proprio il
/// difetto che la chiave esiste per togliere. Finché lo lanciava una persona non
/// era un rischio; da quando lo chiama il giro che sorveglia la coda, due
/// esecuzioni possono sovrapporsi.
///
/// La cartella è la primitiva atomica che c'è ovunque, anche dove `flock` non
/// arriva — su macOS è il caso. La forma è quella già in servizio nel
/// raccoglitore delle copie di lavoro, presa di lì invece che inventata.
struct Lock(PathBuf);

/// Cosa è successo al lucchetto, oltre ad averlo preso: una presa forzata è
/// l'unica prova che un giro precedente è stato ucciso, e senza questa riga
/// quella morte non la racconta più nessuno.
type Taken = (Lock, Option<String>);

impl Lock {
    fn take(path: &Path, now: i64) -> Result<Taken, String> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::create_dir(path).is_ok() {
            return Ok((Lock(path.to_path_buf()), None));
        }
        let age = (now - modified_at(path)).max(0) as u64;
        if age < LOCK_STALE_S {
            return Err(format!(
                "another run holds the lock ({age}s old) — skipping this one"
            ));
        }
        let taken_over = Some(format!("stale lock after {age}s — taking it over"));
        // Prenderlo davvero, non solo dirlo. La rimozione prova tutte e due le
        // forme perché il lucchetto dev'essere rimovibile qualunque cosa sia
        // diventato: con un file al posto della cartella, una rimozione sola
        // fallisce e il deposito si ferma per sempre.
        let _ = std::fs::remove_dir(path);
        let _ = std::fs::remove_file(path);
        match std::fs::create_dir(path) {
            Ok(()) => Ok((Lock(path.to_path_buf()), taken_over)),
            Err(_) => Err("could not take the lock — skipping".to_string()),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hook_io::testing::test_dir;

    /// Un banco: coda, registro sorvegliato e stato proprio, tutti nuovi. Scrive
    /// `n` righe marcate col guasto chiesto, una al minuto all'indietro.
    struct Bench {
        dir: PathBuf,
        places: Places,
        now: i64,
    }

    /// Un istante fisso, così le date scritte nelle voci non dipendono dal
    /// giorno in cui gira la batteria: le 21:00 del 24/08/2026 a Roma, cioè le
    /// 19:00 UTC. Il fuso non conta comunque — ogni data di questi casi passa
    /// da `stamp(…, Zone::Local)`, la stessa funzione dei due lati.
    const NOW: i64 = 1_787_598_000;

    fn bench(name: &str, fault: &str, lines: u32) -> Bench {
        bench_with(name, fault, lines, None)
    }

    fn bench_with(name: &str, fault: &str, lines: u32, caps: Option<&str>) -> Bench {
        let dir = test_dir(name);
        let queue = dir.join("queue");
        let state = dir.join("state");
        std::fs::create_dir_all(&queue).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        let log = dir.join("log");
        let mut text = String::new();
        for i in 0..lines {
            let at = stamp(NOW - i64::from(i) * 60, Zone::Local).replace('T', " ");
            text.push_str(&format!("{at} [guasto={fault}] sess=abc12345 stuck\n"));
        }
        std::fs::write(&log, text).unwrap();
        Bench {
            places: Places {
                queue,
                own_log: state.join("deposito-guasti.log"),
                state,
                registers: format!("{}|staffetta|local|30|1440|24", log.display()),
                caps: caps.unwrap_or(judge::DEFAULT_CAPS).to_string(),
            },
            dir,
            now: NOW,
        }
    }

    impl Bench {
        fn run(&self, args: Args) -> i32 {
            run_with(&args, &self.places, self.now)
        }
        fn act(&self) -> i32 {
            self.run(Args {
                act: true,
                ..Args::default()
            })
        }
        fn born(&self) -> bool {
            std::fs::read_dir(&self.places.queue)
                .unwrap()
                .flatten()
                .any(|e| e.path().extension().is_some_and(|x| x == "md"))
        }
        fn entry(&self, name: &str) -> String {
            std::fs::read_to_string(self.places.queue.join(name)).unwrap()
        }
        fn own_log(&self) -> String {
            std::fs::read_to_string(&self.places.own_log).unwrap_or_default()
        }
    }

    /// Alla propria soglia la voce nasce: `pannello-non-letto` sta a 3 contro le
    /// 30 del registro.
    #[test]
    fn at_its_own_threshold_the_entry_is_born() {
        let b = bench("deposito-soglia", "pannello-non-letto", 3);
        assert_eq!(b.act(), 0);
        assert!(b.born());
    }

    /// Il caso che rende valido il primo: una riga sotto soglia non scrive
    /// niente. Senza, un verde direbbe soltanto che la soglia è stata tolta.
    #[test]
    fn one_line_below_the_threshold_nothing_is_written() {
        let b = bench("deposito-sotto-soglia", "pannello-non-letto", 2);
        assert_eq!(b.act(), 0);
        assert!(!b.born());
    }

    /// Chi non ha una soglia propria tiene quella del registro: muto a 3, apre a
    /// 31.
    #[test]
    fn a_fault_with_no_cap_keeps_the_register_threshold() {
        let quiet = bench("deposito-senza-tetto-3", "avvio-non-marcato", 3);
        assert_eq!(quiet.act(), 0);
        assert!(!quiet.born());
        let loud = bench("deposito-senza-tetto-31", "avvio-non-marcato", 31);
        assert_eq!(loud.act(), 0);
        assert!(loud.born());
    }

    /// La forzatura del collaudo batte la tabella dei tetti.
    #[test]
    fn the_forced_threshold_overrides_the_table() {
        let b = bench("deposito-forzata", "pannello-non-letto", 3);
        assert_eq!(
            b.run(Args {
                act: true,
                forced_threshold: Some(20),
                ..Args::default()
            }),
            0
        );
        assert!(!b.born());
    }

    /// A secco non si scrive niente, e si dice cosa si farebbe.
    #[test]
    fn a_dry_run_writes_nothing() {
        let b = bench("deposito-a-secco", "pannello-non-letto", 3);
        assert_eq!(b.run(Args::default()), 0);
        assert!(!b.born());
        assert!(
            !b.places.state.join("deposito-guasti.lock").exists(),
            "un giro a secco non prende nemmeno il lucchetto"
        );
    }

    /// Una voce viva si aggiorna, e la soglia in testa segue: il corpo porta i
    /// numeri del giorno in cui la voce è nata.
    #[test]
    fn a_live_entry_is_updated_and_its_body_untouched() {
        let b = bench("deposito-aggiorna", "pannello-non-letto", 4);
        std::fs::write(
            b.places.queue.join("live.md"),
            "---\nsessione: bench\nstato: aperta\nchiave: staffetta/pannello-non-letto/sess=abc12345\nripetizioni: 1\nsoglia: 99\nultima: 2026-01-01 00:00\n---\n\nCorpo che deve restare intatto.\n",
        )
        .unwrap();
        assert_eq!(b.act(), 0);
        let entry = b.entry("live.md");
        assert!(entry.contains("\nripetizioni: 4\n"));
        assert!(entry.contains("\nsoglia: 3\n"));
        assert!(entry.contains("Corpo che deve restare intatto"));
        assert_eq!(
            std::fs::read_dir(&b.places.queue).unwrap().count(),
            1,
            "si aggiorna quella, non se ne apre una seconda"
        );
    }

    /// Una voce chiusa si riapre — quella, non una gemella — e il ritorno
    /// dichiara la soglia viva.
    #[test]
    fn a_closed_entry_is_reopened_in_place() {
        let b = bench("deposito-riapre", "pannello-non-letto", 4);
        let path = b.places.queue.join("closed.md");
        std::fs::write(
            &path,
            "---\nsessione: bench\nstato: chiusa\nchiave: staffetta/pannello-non-letto/sess=abc12345\nripetizioni: 1\nsoglia: 99\nprima: 2026-01-01 00:00\nultima: 2026-01-01 00:00\nritorni: 0\n---\n\nCorpo che deve restare intatto.\n",
        )
        .unwrap();
        // Chiusa molto prima delle righe di registro, o il taglio le lascerebbe
        // tutte fuori: è la data del file a dire da quando si riconta.
        set_mtime(&path, NOW - 86_400);
        assert_eq!(b.act(), 0);
        let entry = b.entry("closed.md");
        assert!(entry.contains("stato: aperta — riaperta il "));
        assert!(entry.contains("\nsoglia: 3\n"));
        assert!(entry.contains("\nritorni: 1\n"));
        assert!(entry.contains("soglia di **3**"));
        assert_eq!(std::fs::read_dir(&b.places.queue).unwrap().count(), 1);
    }

    /// IL TAGLIO È LA METÀ CHE CONTA. Una voce appena chiusa non deve rinascere
    /// sulle righe che l'avevano fatta nascere.
    #[test]
    fn a_just_closed_entry_does_not_rise_again_on_the_old_lines() {
        let b = bench("deposito-chiusa-adesso", "pannello-non-letto", 4);
        let path = b.places.queue.join("closed.md");
        std::fs::write(
            &path,
            "---\nstato: chiusa\nchiave: staffetta/pannello-non-letto/sess=abc12345\nripetizioni: 4\nsoglia: 3\nprima: x\nultima: y\nritorni: 0\n---\n\nCorpo.\n",
        )
        .unwrap();
        set_mtime(&path, NOW + 60); // chiusa dopo l'ultima riga del registro
        assert_eq!(b.act(), 0);
        assert!(
            b.entry("closed.md").contains("stato: chiusa"),
            "niente di nuovo dopo la chiusura: la voce resta chiusa"
        );
    }

    /// Il tetto arriva a chi legge la voce, non resta nella tabella.
    #[test]
    fn the_entry_names_the_cap_it_is_measured_against() {
        let b = bench("deposito-denominatore", "pannello-non-letto", 3);
        assert_eq!(b.act(), 0);
        let name = first_entry(&b);
        assert!(b.entry(&name).contains("tetto di 11 al giorno"));
    }

    /// I due guasti che dicono «mi sono arreso» hanno una soglia loro: con
    /// quella del registro non aprirebbero una voce mai.
    #[test]
    fn the_give_up_faults_fire_at_their_own_threshold() {
        let b = bench("deposito-resa", "rinvio-senza-scadenza", 2);
        assert_eq!(b.act(), 0);
        assert!(b.born());
    }

    /// Una riga di tabella malformata si dichiara e **non si applica**: con la
    /// soglia di 40 applicata, 35 righe non aprirebbero niente.
    #[test]
    fn a_malformed_cap_row_is_reported_and_not_applied() {
        let b = bench_with(
            "deposito-tetto-mancante",
            "pannello-non-letto",
            35,
            Some("staffetta|pannello-non-letto|40"),
        );
        assert_eq!(b.act(), 0);
        assert!(b.born(), "vale la soglia del registro, e 35 la superano");
        assert!(b.own_log().contains("[guasto=tabella-tetti-malformata]"));
    }

    /// Le spie di questo programma finiscono nel suo registro **marcate**, cioè
    /// nella forma che rientra dalla porta principale.
    #[test]
    fn its_own_alarms_are_marked_the_way_it_reads_them() {
        let b = bench_with(
            "deposito-spie-proprie",
            "pannello-non-letto",
            1,
            Some("staffetta|pannello-non-letto|20|12"),
        );
        assert_eq!(b.act(), 0);
        let log = b.own_log();
        assert!(log.contains("[guasto=soglia-guasto-irraggiungibile]"));
        assert!(
            judge::extract(&log, "2026-01-01T00:00:00").len() == 1,
            "la riga che scrive è la stessa che sa rileggere"
        );
    }

    /// Il registro proprio sta nella tabella DI SERIE, non solo in quella che il
    /// collaudo si porta: qui la tabella non si sostituisce.
    #[test]
    fn the_shipped_table_watches_its_own_register() {
        let dir = test_dir("deposito-tabella-di-serie");
        let queue = dir.join("queue");
        let state = dir.join("state");
        std::fs::create_dir_all(&queue).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        let own_log = state.join("deposito-guasti.log");
        let mut text = String::new();
        for i in 0..3 {
            let at = stamp(NOW - i * 60, Zone::Local);
            text.push_str(&format!(
                "{at} [guasto=soglia-guasto-irraggiungibile] staffetta/y: threshold above cap\n"
            ));
        }
        std::fs::write(&own_log, text).unwrap();
        let places = Places {
            queue,
            registers: judge::DEFAULT_REGISTERS
                .replace("{HOME}", &dir.to_string_lossy())
                .replace("{OWN_LOG}", &own_log.to_string_lossy()),
            caps: judge::DEFAULT_CAPS.to_string(),
            own_log,
            state,
        };
        // A secco: senza scrivere, e con i registri veri fuori portata perché
        // `{HOME}` punta al banco.
        assert_eq!(run_with(&Args::default(), &places, NOW), 0);
        assert!(
            std::fs::read_dir(&places.queue).unwrap().count() == 0,
            "a secco non nasce niente"
        );
    }

    /// Una coda che non si lascia scrivere non esce zero: chi chiama legge il
    /// codice e, vedendo zero, annota che il giro è andato bene.
    #[test]
    fn a_queue_that_cannot_be_written_does_not_exit_zero() {
        let b = bench("deposito-coda-negata", "pannello-non-letto", 3);
        let missing = b.dir.join("nessuna-cartella/che-non-si-crea");
        std::fs::write(b.dir.join("nessuna-cartella"), "sono un file, non una cartella").unwrap();
        let places = Places {
            queue: missing,
            state: b.places.state.clone(),
            own_log: b.places.own_log.clone(),
            registers: b.places.registers.clone(),
            caps: b.places.caps.clone(),
        };
        assert_eq!(
            run_with(
                &Args {
                    act: true,
                    ..Args::default()
                },
                &places,
                NOW
            ),
            78,
            "una coda irraggiungibile è l'ambiente, non un difetto: 78, mai 0"
        );
    }

    /// Il lucchetto tiene un giro alla volta, e scade: senza scadenza un solo
    /// processo ucciso fermerebbe il deposito per sempre e in silenzio.
    #[test]
    fn the_lock_serialises_and_expires() {
        let dir = test_dir("deposito-lucchetto");
        let path = dir.join("lock");
        let (first, fresh) = Lock::take(&path, NOW).expect("il primo lo prende");
        assert!(fresh.is_none(), "un lucchetto libero non racconta niente");
        // L'ETÀ SI MISURA CONTRO L'OROLOGIO DEL FILE, non contro `NOW`: appena
        // creata, la cartella porta la data vera della macchina, che sta avanti
        // all'istante fisso di questa batteria. Senza questa riga l'età viene
        // negativa, il `.max(0)` la porta a zero, e il ramo «scaduto» non si
        // raggiunge mai — il caso passerebbe senza aver provato niente.
        set_mtime(&path, NOW);
        assert!(
            Lock::take(&path, NOW).is_err(),
            "il secondo trova occupato e salta il giro"
        );
        let (_late, taken_over) = Lock::take(&path, NOW + LOCK_STALE_S as i64 + 1)
            .expect("dopo la scadenza è di un processo morto: si prende");
        assert!(
            taken_over.is_some_and(|m| m.contains("taking it over")),
            "una presa forzata è l'unica prova che un giro è stato ucciso"
        );
        drop(first);
    }

    /// Un lucchetto diventato un file si toglie lo stesso: con una sola forma di
    /// rimozione il deposito si fermerebbe per sempre.
    #[test]
    fn a_lock_that_became_a_file_is_still_removable() {
        let dir = test_dir("deposito-lucchetto-file");
        let path = dir.join("lock");
        std::fs::write(&path, "non sono una cartella").unwrap();
        set_mtime(&path, NOW - 86_400);
        assert!(Lock::take(&path, NOW).is_ok());
    }

    /// Un file di tabella **vuoto** dice «nessun registro», e non deve ricadere
    /// su quelli di serie: là ci sono i registri veri, e un collaudo con `--act`
    /// ci depositerebbe sopra.
    #[test]
    fn an_empty_table_file_means_no_registers_not_the_shipped_ones() {
        let dir = test_dir("deposito-tabella-vuota");
        let empty = dir.join("vuota.txt");
        std::fs::write(&empty, "").unwrap();
        std::env::set_var("FAULT_TABLE_PROVA", &empty);
        assert_eq!(table("FAULT_TABLE_PROVA", judge::DEFAULT_REGISTERS), "");
        std::env::remove_var("FAULT_TABLE_PROVA");
        // e una valvola che non punta a niente tiene la tabella di serie
        assert_eq!(
            table("FAULT_TABLE_PROVA", judge::DEFAULT_CAPS),
            judge::DEFAULT_CAPS
        );
    }

    /// Un registro che non esiste si salta **senza marcare un guasto**, anche
    /// quando la sua riga di tabella è pure sbagliata: il proprio registro è
    /// sorvegliato, e una riga marcata qui aprirebbe in coda una voce che
    /// nessuno può chiudere.
    #[test]
    fn a_missing_register_is_skipped_without_marking_a_fault() {
        let b = bench("deposito-registro-assente", "pannello-non-letto", 3);
        let places = Places {
            registers: format!("{}/non-esiste.log|staffetta|local||1440|24", b.dir.display()),
            ..b.places.clone()
        };
        assert_eq!(run_with(&Args { act: true, ..Args::default() }, &places, NOW), 0);
        let log = std::fs::read_to_string(&places.own_log).unwrap_or_default();
        assert!(log.contains("register missing, skipped:"));
        assert!(
            !log.contains("[guasto=registro-malformato]"),
            "l'ordine dei due controlli: prima il file, poi la riga"
        );
        // SI NEGA IL MARCATORE, NON UN NOME. Negare `[guasto=registro-malformato]`
        // misura l'ordine dei controlli e lascia scoperta la promessa del titolo:
        // una marcatura qualunque, in questo punto, aprirebbe in coda una voce
        // che nessuno può chiudere. In questo giro non esiste nessuna riga
        // marcata legittima — un solo registro, tabella dei tetti coerente.
        assert!(
            !log.contains("[guasto="),
            "un registro che non c'è non ha nessuno da avvisare: {log}"
        );
    }

    /// Una riga di tabella sbagliata su un registro che **esiste** si marca, e
    /// porta i tre numeri come sono scritti.
    #[test]
    fn a_malformed_row_on_a_real_register_is_marked_with_its_fields() {
        let b = bench("deposito-riga-sbagliata", "pannello-non-letto", 3);
        let places = Places {
            registers: format!("{}/log|staffetta|local||1440|24", b.dir.display()),
            ..b.places.clone()
        };
        assert_eq!(run_with(&Args { act: true, ..Args::default() }, &places, NOW), 0);
        let log = std::fs::read_to_string(&places.own_log).unwrap();
        assert!(log.contains("[guasto=registro-malformato]"));
        assert!(log.contains("threshold='' per_day='1440' window_h='24'"));
    }

    /// Un byte che non è testo non fa sparire il registro intero: `awk` legge
    /// quei file senza batter ciglio, e il registro della staffetta ha già
    /// ingoiato l'uscita grezza di un comando.
    #[test]
    fn a_register_with_a_stray_byte_is_still_read() {
        let b = bench("deposito-byte-strano", "pannello-non-letto", 3);
        let log = b.dir.join("log");
        let mut bytes = std::fs::read(&log).unwrap();
        bytes.extend_from_slice(&[0xff, 0xfe, b'\n']);
        std::fs::write(&log, bytes).unwrap();
        assert_eq!(b.act(), 0);
        assert!(b.born(), "le righe buone si contano lo stesso");
    }

    /// Uno scambio non lascia residui dietro di sé.
    #[test]
    fn a_swap_leaves_nothing_behind() {
        let dir = test_dir("deposito-scambio");
        let target = dir.join("voce.md");
        std::fs::write(&target, "prima").unwrap();
        swap(&target, "dopo").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "dopo");
        let leftovers: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n != "voce.md")
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
    }

    /// Un file di passaggio rimasto indietro **non è una voce**, e la prova sta
    /// nel comportamento di chi cerca per chiave — non nella forma del suo nome.
    ///
    /// Lo scenario che copre: uno scambio interrotto lascia un file con la
    /// stessa `chiave:` dentro; se chi enumera la coda lo scambiasse per la voce
    /// viva, il giro dopo aggiornerebbe **quello** invece di lei, e la voce vera
    /// resterebbe ferma per sempre senza che niente lo dica.
    #[test]
    fn a_leftover_swap_file_is_not_mistaken_for_an_entry() {
        let b = bench("deposito-file-di-passaggio", "pannello-non-letto", 4);
        let key = judge::key_of("staffetta", "pannello-non-letto", "sess=abc12345");
        std::fs::write(
            b.places
                .queue
                .join(format!(".deposito.{}.voce.md.tmp", std::process::id())),
            format!("---\nstato: aperta\n{}\n---\n\nCorpo.\n", judge::key_line(&key)),
        )
        .unwrap();
        assert!(
            existing_entry(&b.places.queue, &key).is_none(),
            "un file di passaggio non è una voce"
        );
        assert_eq!(b.act(), 0);
        assert!(
            b.born(),
            "nasce la voce, invece di aggiornare il file di passaggio"
        );
    }


    /// Un giro che scrive rilascia il lucchetto uscendo, anche quando non ha
    /// depositato niente.
    #[test]
    fn the_lock_is_released_when_the_run_ends() {
        let b = bench("deposito-lucchetto-rilascio", "pannello-non-letto", 3);
        assert_eq!(b.act(), 0);
        assert!(!b.places.state.join("deposito-guasti.lock").exists());
    }

    #[test]
    fn the_arguments_are_read_the_way_the_shell_read_them() {
        assert_eq!(
            parse(&["--act".into(), "--threshold".into(), "7".into()]).unwrap(),
            Args {
                act: true,
                forced_threshold: Some(7),
                queue: None
            }
        );
        assert!(parse(&["--sconosciuta".into()]).is_err());
        assert!(parse(&["--threshold".into()]).is_err());
    }

    fn first_entry(b: &Bench) -> String {
        std::fs::read_dir(&b.places.queue)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n.ends_with(".md"))
            .expect("una voce")
    }

    /// `touch -t` invece di una dipendenza per spostare indietro una data: è la
    /// forma già in uso nella raccolta delle radici di prova.
    fn set_mtime(path: &Path, epoch: i64) {
        let when = hook_io::local_time::local_iso_seconds(epoch);
        let stamp = format!(
            "{}{}{}{}{}",
            &when[0..4],
            &when[5..7],
            &when[8..10],
            &when[11..13],
            &when[14..16]
        );
        std::process::Command::new("touch")
            .arg("-t")
            .arg(stamp)
            .arg(path)
            .output()
            .unwrap();
    }
}
