//! Chiude i terminali Orca senza padrone e allinea i nomi che non dicono niente.
//!
//! Porto di `scripts/orca-cleanup.py` (792 righe). L'originale resta l'oracolo:
//! qui non si corregge niente e non si migliora niente, si riproduce — compresi
//! i due difetti dichiarati in fondo a questa intestazione.
//!
//! È IL GANCIO PIÙ PERICOLOSO DEL PARCO. Sta su `SessionEnd` in due modalità
//! (`--close --quiet` e `--names --rename --quiet`) e le sue uscite non sono
//! testo: sono `orca terminal close` e `orca worktree set` su terminali e copie
//! di lavoro veri. Un porto che sbagliasse un ramo non stampa una riga diversa,
//! chiude la scheda di qualcun altro. Per questo il confronto d'equivalenza
//! (`tools/compare-orca-cleanup.py`) non guarda solo stdout: guarda **il
//! registro delle chiamate a un finto `orca` messo davanti nel PATH**, che è
//! l'unica traccia di «cosa avrebbe chiuso». Senza quel registro un porto che
//! non chiude niente sembrerebbe uguale a uno che chiude bene.
//!
//! LE QUATTRO MODALITÀ, in ordine di dispatch:
//!   `--test`   le prove interne, 46 righe, nessuna chiamata a Orca;
//!   `--tabs`   rinomina la tab col titolo del terminale che ci gira dentro;
//!   `--names`  i nomi delle copie di lavoro (+ le tab, se non c'è `--align`);
//!   default    elenca/chiude i terminali senza padrone.
//!
//! DUE DIFETTI DELL'ORIGINALE, riprodotti perché il porto non è la sede per
//! correggerli (dettaglio e percorso nella risposta che accompagna il lavoro):
//!   1. l'ordine delle chiusure viene da un `set` di Python, quindi cambia a
//!      ogni esecuzione (l'hash delle stringhe è randomizzato per processo);
//!   2. una copia senza `path` finisce in `orca worktree set --worktree
//!      path:None`, perché l'f-string interpola `None` invece di fermarsi.
//!
//! RESIDUO DICHIARATO: un campo testuale che arriva da Orca come numero
//! (`title`, `handle`, `worktreePath`, `displayName`, `branch`) fa esplodere il
//! Python con un traceback e uscita 1. Qui vale come assente. Riprodurre un
//! traceback byte per byte non è un comportamento, è un'impronta
//! dell'interprete; il confronto non prova quei casi e lo dichiara.

use regex::Regex;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// I titoli che mette Orca, non una persona.
fn anonymous_titles() -> &'static [Regex] {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    RES.get_or_init(|| {
        [
            r"^\s*$",
            r"(?i)^\(untitled\)$",
            r"(?i)^terminal\s*\d*$",
            r"(?i)^setup$",
            r"(?i)^(zsh|bash|sh|shell)$",
            // «Claude Code» dice che c'è un agente, non che lavoro sta facendo.
            r"(?i)^claude code$",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("regex costante"))
        .collect()
    })
}

/// I nomi che mette l'attrezzo a una copia di lavoro.
fn mute_names() -> &'static [Regex] {
    static RES: OnceLock<Vec<Regex>> = OnceLock::new();
    RES.get_or_init(|| {
        [
            r"^\s*$",
            r"(?i)^\(?untitled\)?$",
            r"(?i)^principale$",
            r"(?i)^package$",
            r"(?i)^workspace$",
        ]
        .iter()
        .map(|p| Regex::new(p).expect("regex costante"))
        .collect()
    })
}

fn handoff_title() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)consegna .*in attesa di INVIO").unwrap())
}

fn tab_line() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s+tab ([0-9a-f-]{36})\s+(.*)$").unwrap())
}

fn leading_glyph() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^[\s◐◑◒◓✳✢✻✽·⏳❓?]+").unwrap())
}

const SIGNS_OF_LIFE: &[&str] = &["✳", "◐", "◑", "⏳", "⠂", "✶", "✻", "∗"];

const DEFAULT_IDLE_MIN: f64 = 30.0;
const HANDOFF_IDLE_MIN: f64 = 120.0;
const DONE_IDLE_MIN: f64 = 20.0;

// ── i pezzi di Python che non hanno un equivalente diretto ──────────────────

/// `str.strip()` di Python: la sua idea di spazio comprende i separatori C0
/// (`\x1c`–`\x1f`), che `char::is_whitespace` non considera tali.
fn py_strip(s: &str) -> &str {
    let space = |c: char| c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c);
    s.trim_matches(space)
}

/// `valore or ""` di Python su un campo che ci si aspetta testuale.
///
/// Un campo assente, nullo o vuoto vale `""`. Un campo di **altro tipo** in
/// Python farebbe esplodere il chiamante (`.strip()`, `[:18]`, `basename`); qui
/// vale `""`, ed è il residuo dichiarato in cima al file.
fn text(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

/// La verità di Python: vuoto, zero, nullo e assente sono falsi.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().map(|f| f != 0.0).unwrap_or(true),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// `f"{valore}"` di Python: un campo assente si scrive `None`, e finisce
/// davvero negli argomenti passati a Orca. Vedi il difetto 2.
fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        Some(other) => other.to_string(),
    }
}

/// I primi `n` **caratteri**, non byte: `[:18]` di Python conta caratteri.
fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Gli ultimi `n` caratteri, cioè `[-n:]`.
fn tail(s: &str, n: usize) -> String {
    let count = s.chars().count();
    s.chars().skip(count.saturating_sub(n)).collect()
}

/// `f"{s:<w.w}"`: prima tronca a `w` caratteri, poi riempie fino a `w`.
fn pad(s: &str, w: usize) -> String {
    let cut = head(s, w);
    let n = cut.chars().count();
    format!("{cut}{}", " ".repeat(w.saturating_sub(n)))
}

/// `os.path.basename` su un percorso posix: tutto ciò che segue l'ultima barra.
fn basename(p: &str) -> &str {
    match p.rfind('/') {
        Some(i) => &p[i + 1..],
        None => p,
    }
}

/// `os.path.realpath`: assoluto, con i collegamenti risolti, **senza pretendere
/// che il percorso esista**.
///
/// Non è `fs::canonicalize`, che su un percorso inesistente fallisce: qui i
/// percorsi delle copie di lavoro possono benissimo essere già stati smontati, e
/// il Python in quel caso risponde comunque con la forma normalizzata. `..` si
/// applica al percorso **già risolto**, come in posixpath dalla 3.10.
pub(crate) fn realpath(path: &str) -> String {
    let start = if path.starts_with('/') {
        path.to_string()
    } else {
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{cwd}/{path}")
    };
    let mut rest: Vec<String> = start.split('/').rev().map(String::from).collect();
    let mut resolved = String::new();
    let mut hops = 0;
    while let Some(comp) = rest.pop() {
        if comp.is_empty() || comp == "." {
            continue;
        }
        if comp == ".." {
            if let Some(i) = resolved.rfind('/') {
                resolved.truncate(i);
            }
            continue;
        }
        let candidate = format!("{resolved}/{comp}");
        match std::fs::read_link(&candidate) {
            Ok(target) if hops < 40 => {
                hops += 1;
                let t = target.to_string_lossy().into_owned();
                if t.starts_with('/') {
                    resolved.clear();
                }
                for piece in t.split('/').rev() {
                    rest.push(piece.to_string());
                }
            }
            _ => resolved = candidate,
        }
    }
    if resolved.is_empty() {
        "/".into()
    } else {
        resolved
    }
}

/// `subprocess(..., text=True).stdout.splitlines()`: Python normalizza prima i
/// fine-riga e poi spezza anche su `\v`, `\f`, `\x1c`–`\x1e`, `\x85`, ` `,
/// ` `. `str::lines()` conosce solo `\n`.
fn py_splitlines(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(std::mem::take(&mut cur));
            }
            '\n' | '\u{b}' | '\u{c}' | '\u{1c}' | '\u{1d}' | '\u{1e}' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// ── la CLI di Orca ──────────────────────────────────────────────────────────

/// Lo stdout del comando, o `None` se non si è potuto eseguire o è scaduto.
///
/// Lo stderr va nel nulla come nell'originale (`capture_output=True` lo cattura
/// e lo butta): lasciarlo ereditare lo farebbe finire nello stderr del gancio,
/// che è una delle cose confrontate.
fn capture(args: &[&str]) -> Option<String> {
    let mut child = Command::new("orca")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = child.stdout.take()?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    // `timeout=30` dell'originale. Scaduto, Python uccide il figlio e l'eccezione
    // viene assorbita: qui si fa lo stesso e si risponde `None`.
    let buf = match rx.recv_timeout(Duration::from_secs(30)) {
        Ok(b) => b,
        Err(_) => {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    };
    let _ = child.wait();
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// Il `result` della CLI, o `None` se Orca non risponde.
///
/// Quattro modi di rispondere «niente», tutti uguali per il chiamante: comando
/// non eseguibile, stdout vuoto, JSON illeggibile, `ok` falso. L'esito di
/// processo **non** si guarda: l'originale passa `check=False` e legge solo lo
/// stdout.
fn orca(args: &[&str]) -> Option<Value> {
    let mut full: Vec<&str> = args.to_vec();
    full.push("--json");
    let out = capture(&full)?;
    if out.trim().is_empty() {
        return None;
    }
    let parsed: Value = serde_json::from_str(&out).ok()?;
    let obj = parsed.as_object()?;
    if !truthy(obj.get("ok")) {
        return None;
    }
    match obj.get("result") {
        Some(r) if r.is_object() => Some(r.clone()),
        _ => None,
    }
}

// ── il giudizio sui terminali ───────────────────────────────────────────────

fn is_anonymous_title(title: &str) -> bool {
    let t = py_strip(title);
    anonymous_titles().iter().any(|r| r.is_match(t))
}

fn has_agent(title: &str) -> bool {
    SIGNS_OF_LIFE.iter().any(|s| title.contains(s))
}

fn idle_minutes(term: &Value, now_ms: f64) -> f64 {
    let last = match term.get("lastOutputAt") {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        // `isinstance(True, int)` è vero in Python: `true` vale 1 millisecondo.
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => return f64::INFINITY,
    };
    if last <= 0.0 {
        return f64::INFINITY;
    }
    ((now_ms - last) / 60000.0).max(0.0)
}

/// La scheda da cui sto girando, riconosciuta su due identificativi.
fn is_mine(term: &Value) -> bool {
    let mut mine: BTreeSet<String> = BTreeSet::new();
    for name in ["ORCA_TAB_ID", "ORCA_TERMINAL_HANDLE"] {
        if let Ok(v) = std::env::var(name) {
            mine.insert(v);
        }
    }
    mine.remove("");
    if mine.is_empty() {
        return false;
    }
    mine.contains(&text(term.get("tabId"))) || mine.contains(&text(term.get("handle")))
}

/// `paneKey` → stato dell'agente, letto una volta sola per giro.
///
/// IL GLIFO NEL TITOLO NON DICE SE L'AGENTE HA FINITO: resta lì anche dopo, e
/// una sessione finita risulterebbe indistinguibile da una che lavora. Lo stato
/// vero sta in `orca worktree ps` e si lega al terminale con `<tabId>:<leafId>`.
/// Muto in caso di errore: senza stati si torna all'euristica del titolo, che è
/// più prudente perché non chiude mai una scheda col glifo.
fn agent_states() -> HashMap<String, Value> {
    let mut out = HashMap::new();
    let listing = orca(&["worktree", "ps"]);
    let items = match listing.as_ref().and_then(|r| r.get("worktrees")) {
        Some(Value::Array(a)) => a.clone(),
        _ => Vec::new(),
    };
    for w in items {
        if !w.is_object() {
            continue;
        }
        let Some(Value::Array(agents)) = w.get("agents") else {
            continue;
        };
        for a in agents {
            // `if key:` — una chiave vuota o assente non entra. Una chiave non
            // testuale in Python entrerebbe come chiave di altro tipo, che la
            // ricerca (sempre testuale) non trova mai: qui si salta, ed è la
            // stessa cosa vista da chi cerca.
            if let Some(Value::String(key)) = a.get("paneKey") {
                if !key.is_empty() {
                    out.insert(key.clone(), a.get("state").cloned().unwrap_or(Value::Null));
                }
            }
        }
    }
    out
}

/// `(chiudibile, motivo)`. Il motivo vale sia per il sì che per il no.
pub(crate) fn judge(
    term: &Value,
    idle_min: f64,
    with_handoffs: bool,
    now_ms: f64,
    states: &HashMap<String, Value>,
) -> (bool, String) {
    let title = text(term.get("title"));
    let idle = idle_minutes(term, now_ms);

    if is_mine(term) {
        return (false, "this is the tab I am running in".into());
    }

    // Lo stato vero batte il glifo in entrambi i versi: protegge una scheda che
    // lavora sotto un titolo anonimo, e libera una che ha finito sotto un titolo
    // col marcatore. `waiting` non è finito — è una sessione che aspetta una
    // risposta, e chiuderla butta via ciò che aspettava.
    let key = format!(
        "{}:{}",
        py_str(term.get("tabId")),
        py_str(term.get("leafId"))
    );
    let state = states.get(&key).and_then(|v| v.as_str()).unwrap_or("");
    if state == "working" || state == "waiting" {
        return (false, format!("the agent is {state}"));
    }
    if state == "done" {
        if idle < DONE_IDLE_MIN {
            return (
                false,
                format!("agent done but idle only {idle:.0} min, under {DONE_IDLE_MIN:.0}"),
            );
        }
        return (true, format!("the agent is done, idle {idle:.0} min"));
    }

    if has_agent(&title) {
        return (false, "an agent is working in it".into());
    }

    if handoff_title().is_match(&title) {
        if !with_handoffs {
            return (false, "pending handoff (needs --handoffs)".into());
        }
        if idle < HANDOFF_IDLE_MIN {
            return (
                false,
                format!("handoff pending for {idle:.0} min, under {HANDOFF_IDLE_MIN:.0}"),
            );
        }
        return (
            true,
            format!("handoff never started, idle {:.1} h", idle / 60.0),
        );
    }

    if !is_anonymous_title(&title) {
        return (false, "the title names a job".into());
    }
    if idle < idle_min {
        return (false, format!("idle {idle:.0} min, under {idle_min:.0}"));
    }
    let shown = {
        let t = py_strip(&title);
        if t.is_empty() {
            "(empty)".to_string()
        } else {
            t.to_string()
        }
    };
    (
        true,
        format!("anonymous title '{shown}', idle {idle:.0} min"),
    )
}

/// I terminali, filtrati su una copia di lavoro se richiesto.
///
/// `None` significa «Orca non risponde», che non è la stessa cosa di «nessun
/// terminale»: il chiamante stampa due frasi diverse.
fn collect(worktree: Option<&str>) -> Option<Vec<Value>> {
    let r = orca(&["terminal", "list"])?;
    let Some(Value::Array(items)) = r.get("terminals") else {
        return None;
    };
    let mut terminals = items.clone();
    if let Some(w) = worktree {
        // NOTA: `realpath("")` è la cartella corrente, quindi un terminale senza
        // `worktreePath` finisce nel filtro quando si chiede proprio la cartella
        // da cui si gira. È il comportamento dell'originale.
        let expected = realpath(w);
        terminals.retain(|t| realpath(&text(t.get("worktreePath"))) == expected);
    }
    Some(terminals)
}

/// Chiede la chiusura. Nessun valore di ritorno, di proposito.
///
/// L'esito della CLI non dice se il terminale è sparito: una chiamata ha
/// risposto `tab_not_found` con il terminale sparito davvero, altre
/// `terminal_handle_stale` lasciando tutto dov'era. L'unica verità è rileggere
/// l'elenco dopo. Si prova prima con `--tab` — che toglie la voce dalla barra,
/// non solo il pannello — e poi senza, perché un pannello senza scheda propria
/// risponde `tab_not_found` al primo e si chiude col secondo.
fn close_tab(handle: &str) {
    orca(&["terminal", "close", "--terminal", handle, "--tab"]);
    orca(&["terminal", "close", "--terminal", handle]);
}

// ── il giudizio sui nomi delle copie di lavoro ──────────────────────────────

fn is_mute_name(name: &str) -> bool {
    let n = py_strip(name);
    mute_names().iter().any(|r| r.is_match(n))
}

/// Il repo se è la copia principale, il ramo altrimenti.
///
/// Sulla copia principale il ramo cambia a ogni checkout e il nome ballerebbe;
/// lì serve sapere quale repo si sta guardando. In una copia di lavoro vale il
/// contrario: nasce per un ramo e lo tiene.
fn expected_name(w: &Value) -> String {
    if truthy(w.get("isMainWorktree")) {
        let p = text(w.get("path"));
        basename(p.trim_end_matches('/')).to_string()
    } else {
        text(w.get("branch")).replace("refs/heads/", "")
    }
}

/// `(da rinominare, motivo)`.
///
/// Senza `align` si toccano solo i nomi muti: un nome che dice un mestiere è
/// un'intenzione di qualcuno. Con `align` si toccano anche i nomi parlanti che
/// non sono quelli del ramo — il criterio della sola mutezza trovava zero righe
/// su 27 mentre tredici portavano un nome diverso dal ramo.
fn judge_name(w: &Value, align: bool) -> (bool, String) {
    let name = py_strip(&text(w.get("displayName"))).to_string();
    let expected = py_strip(&expected_name(w)).to_string();
    if expected.is_empty() {
        return (false, "no branch and no path to name it after".into());
    }
    if name.to_lowercase() == expected.to_lowercase() {
        return (false, "already named after it".into());
    }
    if !is_mute_name(&name) {
        if !align {
            return (false, "the name says a job".into());
        }
        return (
            true,
            format!("name '{name}' does not match the branch → '{expected}'"),
        );
    }
    let shown = if name.is_empty() { "(empty)" } else { name.as_str() };
    (true, format!("mute name '{shown}' → '{expected}'"))
}

fn collect_worktrees() -> Option<Vec<Value>> {
    let r = orca(&["worktree", "list"])?;
    match r.get("worktrees") {
        Some(Value::Array(a)) => Some(a.clone()),
        _ => None,
    }
}

/// Chiede la rinomina. Nessun ritorno: la verità è la rilettura.
///
/// `path:{…}` interpola con le regole di Python: una copia senza `path` produce
/// letteralmente `path:None`. È il difetto 2, riprodotto.
fn rename(w: &Value, fresh: &str) {
    let target = format!("path:{}", py_str(w.get("path")));
    orca(&[
        "worktree",
        "set",
        "--worktree",
        &target,
        "--display-name",
        fresh,
    ]);
}

fn fix_names(only_current: bool, apply_it: bool, quiet: bool, align: bool) -> i32 {
    let Some(mut items) = collect_worktrees() else {
        if !quiet {
            println!("orca is not answering: nothing to do.");
        }
        return 0;
    };

    if only_current {
        let here = realpath(
            &std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        items.retain(|w| {
            let p = realpath(&text(w.get("path")));
            // Il secondo ramo dell'originale usa `"/\0"` come sentinella, ma è
            // codice morto: con `path` vuoto il primo confronto è già vero,
            // perché `realpath("")` è la cartella corrente.
            here == p || here.starts_with(&format!("{p}/"))
        });
    }

    let verdicts: Vec<(&Value, bool, String)> = items
        .iter()
        .map(|w| {
            let (ok, why) = judge_name(w, align);
            (w, ok, why)
        })
        .collect();
    let todo: Vec<(&Value, &str)> = verdicts
        .iter()
        .filter(|(_, ok, _)| *ok)
        .map(|(w, _, why)| (*w, why.as_str()))
        .collect();

    if quiet && todo.is_empty() {
        return 0;
    }

    let label = if align { "misnamed" } else { "mute names" };
    println!(
        "worktrees in scope: {} · {label}: {}\n",
        items.len(),
        todo.len()
    );
    for &(w, reason) in &todo {
        let p = text(w.get("path"));
        let shown = if p.is_empty() { "?".to_string() } else { p };
        println!("  RENAME  {}  {reason}", pad(&tail(&shown, 46), 46));
    }
    if !apply_it {
        if !todo.is_empty() {
            println!("\n(nothing renamed: add --rename)");
        }
        return 0;
    }

    for &(w, _) in &todo {
        rename(w, &expected_name(w));
    }

    // Come per le schede: si verifica rileggendo, mai sommando gli esiti della CLI.
    let mut after: HashMap<String, String> = HashMap::new();
    for w in collect_worktrees().unwrap_or_default() {
        after.insert(realpath(&text(w.get("path"))), text(w.get("displayName")));
    }
    let done = todo
        .iter()
        .filter(|(w, _)| after.get(&realpath(&text(w.get("path")))) == Some(&expected_name(w)))
        .count();
    println!(
        "\nrenamed {done} of {} worktrees (verified by re-reading the list).",
        todo.len()
    );
    0
}

// ── il nome delle tab ───────────────────────────────────────────────────────

/// `tabId` → titolo della tab, letto dal «visual layout» del comando testuale.
///
/// NON sta nel JSON: `orca terminal list --json` espone `tabId` ma non il titolo
/// della tab, che compare solo nell'uscita testuale.
fn tab_titles() -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Some(raw) = capture(&["terminal", "list"]) else {
        return out;
    };
    for line in py_splitlines(&raw) {
        if let Some(c) = tab_line().captures(&line) {
            out.insert(c[1].to_string(), py_strip(&c[2]).to_string());
        }
    }
    out
}

/// Il titolo che la tab dovrebbe portare, ripulito dai simboli di stato.
///
/// Il glifo davanti dice se l'agente sta pensando: cambia di secondo in secondo
/// e l'interfaccia ha già la sua icona, quindi in un nome è rumore.
fn wanted_tab_title(term_title: &str) -> String {
    let stripped = leading_glyph().replace(term_title, "");
    head(py_strip(&stripped), 70)
}

/// Il nome nuovo per questa tab, oppure `""` se non va toccata.
///
/// Pura apposta: dentro la funzione che chiama Orca non sarebbe provabile, e il
/// primo mutante contro la guardia sui titoli generici passava lo stesso.
fn tab_rename_to(term_title: &str, tab_title: &str) -> String {
    if !has_agent(term_title) {
        return String::new(); // niente agente: da chiudere, non da nominare
    }
    let wanted = wanted_tab_title(term_title);
    if wanted.is_empty() || is_anonymous_title(&wanted) {
        return String::new(); // generico: peggiorerebbe un nome che dice qualcosa
    }
    if wanted == tab_title {
        String::new()
    } else {
        wanted
    }
}

/// Allinea il nome della tab a quello del terminale che ci gira dentro.
///
/// Orca battezza una tab col primo messaggio scritto dentro, troncato, e non lo
/// aggiorna mai più: nella barra si leggono «Procedi» o «Cosa dice il mockup in»
/// mentre il terminale sa benissimo cosa sta facendo. Rinominare il **terminale**
/// non funziona (Claude Code lo sovrascrive col suo `ai-title`); rinominare la
/// **tab** sì, ed è una cosa diversa.
///
/// LIMITE DICHIARATO: non c'è modo di distinguere una tab battezzata da Orca da
/// una rinominata a mano — il titolo è un campo solo.
fn fix_tabs(apply_it: bool, quiet: bool) -> i32 {
    let Some(terms) = collect(None) else {
        if !quiet {
            println!("Orca does not answer: no tab renamed");
        }
        return 1;
    };
    let titles = tab_titles();
    let mut touched = 0;
    for t in &terms {
        let tab = text(t.get("tabId"));
        let known = titles.get(&tab).map(String::as_str);
        let wanted = tab_rename_to(&text(t.get("title")), known.unwrap_or(""));
        if wanted.is_empty() {
            continue;
        }
        touched += 1;
        // Il ripiego è `'?'` qui e `''` dentro la decisione: sono due default
        // diversi nello stesso giro, e si conservano com'erano.
        let shown = pad(&head(known.unwrap_or("?"), 34), 34);
        if !apply_it {
            println!("  WOULD RENAME  {shown} -> {wanted}");
            continue;
        }
        let ok = orca(&[
            "terminal",
            "rename",
            "--terminal",
            &text(t.get("handle")),
            "--title",
            &wanted,
        ]);
        let esito = if ok.is_some() { "renamed" } else { "FAILED " };
        println!("  {esito}  {shown} -> {wanted}");
    }
    if touched == 0 && !quiet {
        println!("every tab already says what runs inside it");
    }
    0
}

// ── argomenti ───────────────────────────────────────────────────────────────

#[derive(Default)]
struct Args {
    close: bool,
    worktree: Option<String>,
    handoffs: bool,
    idle: Option<f64>,
    quiet: bool,
    names: bool,
    rename: bool,
    all: bool,
    align: bool,
    tabs: bool,
    test: bool,
}

/// Solo le forme che la configurazione usa davvero.
///
/// NON è argparse: le abbreviazioni non ambigue (`--clo`), `--help` e i suoi
/// messaggi d'errore restano fuori, e il confronto lo dichiara invece di
/// fingere. Un argomento sconosciuto qui esce 2 come farebbe argparse, ma con
/// un messaggio proprio.
fn value_for(
    argv: &[String],
    i: &mut usize,
    inline: Option<String>,
    flag: &str,
) -> Result<String, String> {
    match inline {
        Some(v) => Ok(v),
        None => {
            *i += 1;
            argv.get(*i)
                .cloned()
                .ok_or_else(|| format!("{flag}: expected one argument"))
        }
    }
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut i = 0;
    while i < argv.len() {
        let raw = argv[i].clone();
        let (name, inline) = match raw.split_once('=') {
            Some((n, v)) => (n.to_string(), Some(v.to_string())),
            None => (raw.clone(), None),
        };
        match name.as_str() {
            "--list" => {}
            "--close" => a.close = true,
            "--worktree" => a.worktree = Some(value_for(argv, &mut i, inline, "--worktree")?),
            "--handoffs" => a.handoffs = true,
            "--idle" => {
                let v = value_for(argv, &mut i, inline, "--idle")?;
                a.idle = Some(
                    v.trim()
                        .parse::<f64>()
                        .map_err(|_| format!("--idle: invalid float value: '{v}'"))?,
                );
            }
            "--quiet" => a.quiet = true,
            "--names" => a.names = true,
            "--rename" => a.rename = true,
            "--all" => a.all = true,
            "--align" => a.align = true,
            "--tabs" => a.tabs = true,
            "--test" => a.test = true,
            other => return Err(format!("unrecognized arguments: {other}")),
        }
        i += 1;
    }
    Ok(a)
}

pub fn run() -> i32 {
    let argv: Vec<String> = std::env::args().skip(2).collect();
    let args = match parse(&argv) {
        Ok(a) => a,
        Err(why) => {
            eprintln!("orca-cleanup: {why}");
            return 2;
        }
    };

    if args.test {
        return selftest();
    }
    if args.tabs {
        return fix_tabs(args.rename, args.quiet);
    }
    if args.names {
        // `--names` copre entrambe le superfici di nome che Orca mostra: la
        // copia di lavoro e la tab. È lo stesso mestiere, e tenerle separate
        // vorrebbe dire una riga in più in `settings.json` — che il 16/08/2026
        // si è rivelato il punto fragile del sistema.
        //
        // `--names` e `--close` restano invece due invocazioni distinte in
        // `settings.json`, e non è una svista: misurato il 19/08/2026, avviare
        // questo binario costa **0,00 s** (`--test` non tocca la rete), mentre
        // le due strade pesano 0,37 s e 0,28 s — cioè tutte chiamate alla CLI di
        // Orca, che restano due anche in un processo solo perché interrogano
        // cose diverse (`worktree list` di qua, `terminal list` di là).
        let outcome = fix_names(!args.all, args.rename, args.quiet, args.align);
        // Con `--align` si sistemano le righe della barra, non le tab:
        // mescolarci la rinomina delle tab vorrebbe dire, nello stesso comando,
        // sovrascrivere nomi scritti a mano con titoli di agente.
        if args.align {
            return outcome;
        }
        let tabs = fix_tabs(args.rename, args.quiet);
        return if tabs != 0 { tabs } else { outcome };
    }

    let Some(terminals) = collect(args.worktree.as_deref()) else {
        if !args.quiet {
            println!("orca is not answering: nothing to do.");
        }
        return 0;
    };
    if terminals.is_empty() {
        if !args.quiet {
            println!("no terminal in the requested scope.");
        }
        return 0;
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64() * 1000.0)
        .unwrap_or(0.0);
    // Una lettura per giro, non una per terminale: `worktree ps` costa una
    // chiamata alla CLI, e trenta schede la farebbero trenta volte.
    let states = agent_states();
    let idle_min = args.idle.unwrap_or(DEFAULT_IDLE_MIN);
    let verdicts: Vec<(&Value, bool, String)> = terminals
        .iter()
        .map(|t| {
            let (ok, why) = judge(t, idle_min, args.handoffs, now, &states);
            (t, ok, why)
        })
        .collect();
    let to_close: Vec<(&Value, &str)> = verdicts
        .iter()
        .filter(|(_, ok, _)| *ok)
        .map(|(t, _, w)| (*t, w.as_str()))
        .collect();
    let kept: Vec<(&Value, &str)> = verdicts
        .iter()
        .filter(|(_, ok, _)| !*ok)
        .map(|(t, _, w)| (*t, w.as_str()))
        .collect();

    if args.quiet && to_close.is_empty() {
        return 0;
    }

    println!(
        "terminals in scope: {} · to close: {} · kept: {}\n",
        terminals.len(),
        to_close.len(),
        kept.len()
    );
    let riga = |t: &Value, verbo: &str, reason: &str| {
        // `os.path.basename(percorso or "?")`: senza percorso il ripiego è «?»,
        // e un percorso che finisce con la barra dà stringa vuota — non il nome
        // della cartella, perché qui nessuno toglie la barra finale.
        let path = text(t.get("worktreePath"));
        let where_ = if path.is_empty() { "?" } else { basename(&path) };
        let handle = text(t.get("handle"));
        let handle = if handle.is_empty() {
            "?".to_string()
        } else {
            handle
        };
        println!(
            "  {verbo}  {}  {}  {reason}",
            head(&handle, 18),
            pad(&where_, 28)
        );
    };
    for &(t, reason) in &to_close {
        riga(t, "CLOSE", reason);
    }
    if !kept.is_empty() && !args.close {
        println!();
        for &(t, reason) in &kept {
            riga(t, "keep ", reason);
        }
    }

    if !args.close {
        println!("\n(nothing closed: add --close)");
        return 0;
    }

    // Un insieme, come nell'originale: lo stesso manico su due pannelli si chiude
    // una volta sola. Qui è ordinato, perché quello di Python non lo è affatto.
    let targets: BTreeSet<String> = to_close
        .iter()
        .map(|(t, _)| text(t.get("handle")))
        .collect();
    for handle in &targets {
        close_tab(handle);
    }

    // La verifica è una rilettura, non la somma degli esiti: vedi `close_tab`.
    let after = collect(args.worktree.as_deref()).unwrap_or_default();
    let left: BTreeSet<String> = after
        .iter()
        .map(|t| text(t.get("handle")))
        .filter(|h| targets.contains(h))
        .collect();
    println!(
        "\nclosed {} of {} terminals (verified by re-reading the list).",
        targets.len() - left.len(),
        targets.len()
    );
    if !left.is_empty() {
        println!(
            "still there — the runtime calls their handle stale, the CLI cannot \
close them; they belong to an earlier Orca incarnation:"
        );
        for handle in &left {
            println!("  {}", head(handle, 18));
        }
    }
    0
}

// ── le prove interne (`--test`) ─────────────────────────────────────────────

/// Ogni caso è uno dei modi in cui questo strumento può fare danno.
///
/// Porto riga per riga del `selftest()` dell'originale, comprese le sue
/// etichette: è l'unico modo in cui il confronto d'equivalenza può verificare i
/// **motivi** di cinquanta verdetti senza mai avvicinarsi a un terminale vero.
fn selftest() -> i32 {
    let now = 1_000_000_000_000.0_f64;
    let fa = |minutes: f64| now - minutes * 60000.0;
    let empty: HashMap<String, Value> = HashMap::new();

    std::env::set_var("ORCA_TAB_ID", "tab_mine");
    std::env::remove_var("ORCA_TERMINAL_HANDLE");

    let mut rows: Vec<(bool, String, String)> = Vec::new();
    let mut push = |ok: bool, name: &str, reason: &str| {
        rows.push((ok, name.to_string(), reason.to_string()));
    };

    let cases: Vec<(&str, Value, bool)> = vec![
        (
            "anonymous and idle → closed",
            serde_json::json!({"title": "Setup", "handle": "term_a", "lastOutputAt": fa(90.0)}),
            true,
        ),
        (
            "anonymous but just active → kept",
            serde_json::json!({"title": "(untitled)", "handle": "term_b", "lastOutputAt": fa(2.0)}),
            false,
        ),
        (
            "title names a job → kept, however old",
            serde_json::json!({"title": "release to staging", "handle": "term_c", "lastOutputAt": fa(600.0)}),
            false,
        ),
        (
            "agent at work → kept, even with an anonymous title",
            serde_json::json!({"title": "◐ Terminal 2", "handle": "term_d", "lastOutputAt": fa(600.0)}),
            false,
        ),
        (
            "my own tab → never closes itself",
            serde_json::json!({"title": "Terminal 1", "tabId": "tab_mine", "handle": "x", "lastOutputAt": fa(600.0)}),
            false,
        ),
        (
            "never wrote anything and anonymous → closed",
            serde_json::json!({"title": "", "handle": "term_e", "lastOutputAt": 0}),
            true,
        ),
        (
            "numbered Terminal → anonymous",
            serde_json::json!({"title": "Terminal 12", "handle": "term_f", "lastOutputAt": fa(90.0)}),
            true,
        ),
    ];
    for (name, term, expected) in cases {
        let (ok, reason) = judge(&term, DEFAULT_IDLE_MIN, false, now, &empty);
        push(ok == expected, name, &reason);
    }

    // Lo stato dell'agente batte il glifo, in tutti e due i versi. Il caso vero
    // del 17/08/2026: una scheda «✳ Analizzare Swagger…» con l'agente in `done`
    // da ore, tenuta viva perché il glifo c'era ancora.
    let pane = |state: &str, tab: &str, leaf: &str| -> HashMap<String, Value> {
        let mut m = HashMap::new();
        m.insert(format!("{tab}:{leaf}"), Value::String(state.into()));
        m
    };
    let live = serde_json::json!({"title": "◐ un lavoro qualsiasi", "handle": "term_h",
                                  "tabId": "t1", "leafId": "l1", "lastOutputAt": fa(600.0)});
    let con = |patch: Value| -> Value {
        let mut v = live.clone();
        for (k, val) in patch.as_object().unwrap() {
            v[k.as_str()] = val.clone();
        }
        v
    };
    let state_cases: Vec<(&str, Value, HashMap<String, Value>, bool)> = vec![
        (
            "MUTANT: agent done → closed, glyph in the title notwithstanding",
            live.clone(),
            pane("done", "t1", "l1"),
            true,
        ),
        (
            "MUTANT: agent working → kept, even under an anonymous title",
            con(serde_json::json!({"title": "Terminal 4"})),
            pane("working", "t1", "l1"),
            false,
        ),
        // Il titolo anonimo è ciò che rende il caso discriminante: col glifo,
        // togliere `waiting` dalla guardia darebbe lo stesso verdetto per la via
        // del ripiego, e il mutante sopravviverebbe.
        (
            "MUTANT: agent waiting is not agent done, anonymous title and all",
            con(serde_json::json!({"title": "Terminal 7"})),
            pane("waiting", "t1", "l1"),
            false,
        ),
        (
            "done but only just → kept, it may still be writing",
            con(serde_json::json!({"lastOutputAt": fa(5.0)})),
            pane("done", "t1", "l1"),
            false,
        ),
        (
            "MUTANT: a state on another tab does not decide this one",
            live.clone(),
            pane("done", "t9", "l1"),
            false,
        ),
        // Stessa scheda, pannello diverso: è il caso che obbliga la chiave a
        // portare anche il `leafId`.
        (
            "MUTANT: nor does another pane of the same tab",
            con(serde_json::json!({"leafId": "l9"})),
            pane("done", "t1", "l1"),
            false,
        ),
        (
            "no state at all → falls back to the title",
            live.clone(),
            HashMap::new(),
            false,
        ),
        (
            "and the fallback still closes an anonymous idle one",
            con(serde_json::json!({"title": "Setup"})),
            HashMap::new(),
            true,
        ),
        (
            "my own tab is never closed, whatever the agent state says",
            con(serde_json::json!({"tabId": "tab_mine", "leafId": "l1"})),
            pane("done", "tab_mine", "l1"),
            false,
        ),
    ];
    for (name, term, states, expected) in state_cases {
        let (ok, reason) = judge(&term, DEFAULT_IDLE_MIN, false, now, &states);
        push(ok == expected, name, &reason);
    }

    let handoff = serde_json::json!({"title": "consegna raccolta (in attesa di INVIO)",
                                     "handle": "term_g", "lastOutputAt": fa(400.0)});
    let (without, _) = judge(&handoff, DEFAULT_IDLE_MIN, false, now, &empty);
    let (with_, reason_with) = judge(&handoff, DEFAULT_IDLE_MIN, true, now, &empty);
    let mut fresh_term = handoff.clone();
    fresh_term["lastOutputAt"] = serde_json::json!(fa(10.0));
    let (fresh, _) = judge(&fresh_term, DEFAULT_IDLE_MIN, true, now, &empty);
    push(!without, "handoff tab protected without --handoffs", "");
    push(with_, "old handoff tab closed with --handoffs", &reason_with);
    push(!fresh, "recent handoff tab kept even with --handoffs", "");

    // I nomi dei worktree: stesso criterio delle schede, altra superficie.
    let names: Vec<(&str, Value, bool, &str)> = vec![
        (
            "main copy called 'principale' → named after the repo",
            serde_json::json!({"displayName": "principale", "isMainWorktree": true,
                               "path": "/home/someone/other-repo/work/suite", "branch": "refs/heads/feat/x"}),
            true,
            "suite",
        ),
        (
            "main copy already named after the repo → kept",
            serde_json::json!({"displayName": "suite", "isMainWorktree": true,
                               "path": "/home/someone/other-repo/work/suite", "branch": "refs/heads/feat/x"}),
            false,
            "suite",
        ),
        (
            "a name someone wrote → never touched, however odd",
            serde_json::json!({"displayName": "suite - vista altro user", "isMainWorktree": false,
                               "path": "/w/x", "branch": "refs/heads/release/0.1.0"}),
            false,
            "release/0.1.0",
        ),
        (
            "untitled work copy → named after its branch",
            serde_json::json!({"displayName": "(untitled)", "isMainWorktree": false,
                               "path": "/w/y", "branch": "refs/heads/fix/drop-legacy"}),
            true,
            "fix/drop-legacy",
        ),
        (
            "mute name but nothing to name it after → kept",
            serde_json::json!({"displayName": "principale", "isMainWorktree": false, "path": "/w/z", "branch": ""}),
            false,
            "",
        ),
        (
            "main copy keeps the repo name even when the branch moves",
            serde_json::json!({"displayName": "a-service", "isMainWorktree": true,
                               "path": "/home/someone/other-repo/work/a-service",
                               "branch": "refs/heads/fix/matching-response-shapes"}),
            false,
            "a-service",
        ),
    ];
    for (name, w, expected_ok, expected_name_) in names {
        let (ok, _) = judge_name(&w, false);
        let right = ok == expected_ok && (!ok || expected_name(&w) == expected_name_);
        // L'originale stampa `f"{ok} / {expected_name(w)!r}"`: `True`/`False` e
        // l'apice singolo del `repr` di Python.
        let reason = format!(
            "{} / '{}'",
            if ok { "True" } else { "False" },
            expected_name(&w)
        );
        push(right, name, &reason);
    }

    // `--align`: la riga che non porta il nome del suo ramo.
    let aligns: Vec<(&str, Value, bool)> = vec![
        (
            "a talking name that is not the branch → aligned",
            serde_json::json!({"displayName": "rimozione ruolo account legacy", "isMainWorktree": false,
                               "path": "/w/suite-229-tabella-residui",
                               "branch": "refs/heads/fix/drop-legacy-account-role"}),
            true,
        ),
        (
            "already named after its branch → kept",
            serde_json::json!({"displayName": "fix/drop-legacy-account-role", "isMainWorktree": false,
                               "path": "/w/x", "branch": "refs/heads/fix/drop-legacy-account-role"}),
            false,
        ),
        (
            "same name in another case → kept, not rewritten",
            serde_json::json!({"displayName": "Promote-Cookie", "isMainWorktree": false,
                               "path": "/w/x", "branch": "refs/heads/promote-cookie"}),
            false,
        ),
        (
            "a mute name is still caught with --align",
            serde_json::json!({"displayName": "(untitled)", "isMainWorktree": false,
                               "path": "/w/y", "branch": "refs/heads/fix/x"}),
            true,
        ),
        (
            "nothing to name it after → kept even with --align",
            serde_json::json!({"displayName": "qualsiasi cosa", "isMainWorktree": false,
                               "path": "/w/z", "branch": ""}),
            false,
        ),
        (
            "a canonical checkout keeps the repo name, never the branch",
            serde_json::json!({"displayName": "suite", "isMainWorktree": true,
                               "path": "/home/someone/other-repo/work/suite",
                               "branch": "refs/heads/feat/design-residues"}),
            false,
        ),
        (
            "a canonical checkout named after the branch → back to the repo",
            serde_json::json!({"displayName": "feat/job-model", "isMainWorktree": true,
                               "path": "/home/someone/personal/sailor",
                               "branch": "refs/heads/feat/job-model"}),
            true,
        ),
    ];
    for (name, w, expected_ok) in aligns {
        let (ok, reason) = judge_name(&w, true);
        push(ok == expected_ok, &format!("align: {name}"), &reason);
    }

    // Il nome della tab.
    let tabs: Vec<(&str, bool)> = vec![
        (
            "the status glyph is stripped: it changes every second",
            wanted_tab_title("◐ Analizzare i cron") == "Analizzare i cron",
        ),
        (
            "the hourglass is stripped too",
            wanted_tab_title("⏳ general · pulizia") == "general · pulizia",
        ),
        (
            "a plain title survives untouched",
            wanted_tab_title("Release to staging") == "Release to staging",
        ),
        (
            "a long title is cut, not dropped",
            wanted_tab_title(&"x".repeat(200)).chars().count() == 70,
        ),
        // Il caso che il primo giro a secco ha trovato addosso a sé stesso: un
        // titolo generico avrebbe sostituito «consegna raccolta (parte da sola)».
        (
            "MUTANT: a generic agent title is not an improvement",
            is_anonymous_title("Claude Code"),
        ),
        ("and neither is an empty one", is_anonymous_title("")),
        (
            "but a real one is kept",
            !is_anonymous_title("Release to staging"),
        ),
        (
            "the layout line yields id and title",
            tab_line()
                .captures("  tab dc616130-4c66-442a-9c9e-b7553cc6c074  Procedi")
                .map(|c| c[2].to_string())
                == Some("Procedi".to_string()),
        ),
        (
            "a terminal line is not a tab line",
            !tab_line().is_match("    * term_08fe2bb1  qualcosa"),
        ),
        // La DECISIONE, non i suoi pezzi: provando solo `is_anonymous_title` la
        // batteria restava verde anche togliendo la guardia da `fix_tabs`.
        (
            "MUTANT: a generic agent title never renames a tab",
            tab_rename_to("✳ Claude Code", "consegna raccolta (parte da sola)").is_empty(),
        ),
        (
            "a first-prompt name gives way to the real work",
            tab_rename_to("◑ Release to staging", "Procedi") == "Release to staging",
        ),
        (
            "MUTANT: a tab without an agent is left alone",
            tab_rename_to("Setup", "Setup").is_empty(),
        ),
        // Un titolo che NON è anonimo e NON ha agente: è l'unico caso che isola
        // la guardia sull'agente.
        (
            "MUTANT: a plain shell command is not work worth naming a tab after",
            tab_rename_to("npm run dev", "Procedi").is_empty(),
        ),
        (
            "an already aligned tab is not rewritten",
            tab_rename_to("◐ Analizzare i cron", "Analizzare i cron").is_empty(),
        ),
    ];
    for (name, passed) in tabs {
        push(passed, name, "");
    }

    for (passed, name, reason) in &rows {
        let marker = if *passed { "ok " } else { "NO " };
        if *passed {
            println!("  {marker} {name}");
        } else {
            println!("  {marker} {name}  → {reason}");
        }
    }
    let failed = rows.iter().filter(|(p, _, _)| !*p).count();
    println!("\n{}/{} self-tests passed.", rows.len() - failed, rows.len());
    if failed > 0 {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `{:.0}` deve arrotondare al pari come Python, o i motivi divergono di un
    /// minuto proprio sui valori a metà.
    #[test]
    fn l_arrotondamento_e_quello_di_python() {
        assert_eq!(format!("{:.0}", 2.5_f64), "2");
        assert_eq!(format!("{:.0}", 1.5_f64), "2");
        assert_eq!(format!("{:.0}", f64::INFINITY), "inf");
        assert_eq!(format!("{:.1}", f64::INFINITY), "inf");
    }

    /// Un terminale che non ha mai scritto è vecchio all'infinito, non nuovo.
    #[test]
    fn senza_lastoutputat_l_inattivita_e_infinita() {
        assert!(idle_minutes(&serde_json::json!({}), 0.0).is_infinite());
        assert!(idle_minutes(&serde_json::json!({"lastOutputAt": 0}), 0.0).is_infinite());
        assert!(idle_minutes(&serde_json::json!({"lastOutputAt": "ieri"}), 0.0).is_infinite());
    }

    /// `realpath` deve rispondere anche su un percorso che non esiste più.
    #[test]
    fn realpath_normalizza_senza_pretendere_l_esistenza() {
        assert_eq!(realpath("/w/x/../y/"), "/w/y");
        assert_eq!(realpath("/"), "/");
    }

    /// Le 46 righe dell'originale devono passare tutte anche qui.
    ///
    /// La casa isolata serve per il lucchetto, non per i file: `selftest`
    /// dichiara `ORCA_TAB_ID` per provare che una scheda non chiude sé stessa, e
    /// quella è una variabile di **processo** — un caso in parallelo che finiva
    /// la sua `HomeIsolata` gliela toglieva a metà, e il rosso compariva a giri
    /// alterni sul caso «my own tab».
    #[test]
    fn the_originals_own_tests_all_pass() {
        let _home = crate::test_home::HomeIsolata::nuova("orca-cleanup-selftest");
        assert_eq!(selftest(), 0);
    }
}
