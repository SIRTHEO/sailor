//! `sailor session`: **la porta unica** del tracciamento dei terminali.
//!
//! **IL PRINCIPIO.** Sailor non entra nel terminale: è l'agente — o la shell —
//! che si presenta. Un gancio manda il proprio payload su standard input, e
//! questo comando lo registra. Non c'è nessun altro modo di entrare, e non c'è
//! nessun codice specifico di prodotto: **questo comando non legge nessuna
//! variabile d'ambiente di nessun programma e non nomina nessun terminale.**
//!
//! **L'ANCORA È `(tty, albero, capostipite)`.** Il tty lo si chiede al proprio
//! descrittore, l'albero al payload (o alla cartella corrente), il capostipite
//! al censimento — e il capostipite **è un'etichetta**: si stampa e si
//! registra, nessuna condizione lo legge. La prova
//! `no_product_name_decides_anything` tiene ferma la regola di ferro: il nome
//! di un prodotto può comparire in un'etichetta, mai in una condizione.
//!
//! **IL CENSIMENTO È INNESCATO, NON A OROLOGIO.** Si guarda la macchina quando
//! arriva un evento, e in nessun altro momento: qui dentro non c'è nessun
//! timer, nessun ciclo e nessuna attesa.
//!
//! **UN CENSIMENTO NEGATO NON FA FALLIRE UNA REGISTRAZIONE.** Un gancio che
//! esce male è un gancio che disturba chi lavora: se non abbiamo potuto
//! guardare la macchina, il capostipite resta ignoto e la riga si scrive lo
//! stesso. È solo `sailor session census` che il diniego fa uscire con 3,
//! perché è l'unico la cui risposta *è* il censimento.

use sessions::census::{Census, LocalMachine};
use sessions::{anchor_from, now, Anchor, Arrival, Payload, Sessions, TerminalEvent};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

/// Le forme di `sailor session`, una per riga.
///
/// **È UN ELENCO E NON UN BLOCCO DI TESTO PERCHÉ LA LEGGE ANCHE LA FINESTRA.**
/// `Command::usage` in `lib.rs` vuole righe interrogabili da un programma: una
/// stringa sola costringerebbe la pagina d'aiuto della finestra a spezzarla da
/// sé, cioè a tenere una seconda idea di dove finisce una forma.
pub const USAGE: &[&str] = &[
    "sailor session open      < payload.json   registra un terminale e chi ci è arrivato",
    "sailor session event     < payload.json   registra un fatto sulla sessione",
    "sailor session close     [--tty <nome>]   chiude la riga di un terminale",
    "sailor session list      [--json]         cosa risulta tracciato",
    "sailor session detach    [--tty <nome>]   lascia stare questa finestra",
    "sailor session attach    [--tty <nome>]   torna a seguirla",
    "sailor session census    [--json]         cosa c'è adesso sulla macchina",
    "sailor session install   [--settings <file>]  innesta i ganci, senza toccare quelli altrui",
];

/// Le opzioni che valgono per più forme, fuori dall'elenco perché non sono
/// forme: metterle lì le farebbe contare come tali da chi conta le righe.
const COMMON_OPTIONS: &str = "opzioni comuni: --tty <nome> per dire il terminale invece di dedurlo,\n\
                              \x20               --store <file> per scrivere altrove che accanto al deposito";

/// L'aiuto come lo legge chi digita, costruito dall'elenco invece che
/// ricopiato accanto.
fn usage_text() -> String {
    format!("uso: {}\n\n{COMMON_OPTIONS}", USAGE.join("\n     "))
}

/// Le forme che questo comando conosce, in un posto solo: l'elenco che
/// `--help` stampa e quello che il dispatch accetta devono essere lo stesso,
/// altrimenti una forma documentata e non accettata si scopre in mano a chi la
/// digita.
const FORMS: &[&str] = &[
    "open", "event", "close", "list", "detach", "attach", "census", "install",
];

/// Le forme che parlano di **un** terminale, e quindi devono saperne il nome.
/// `list` e `census` non ci sono: parlano di tutti.
const NEEDS_A_TERMINAL: &[&str] = &["open", "event", "close", "detach", "attach"];

/// Le opzioni che non vogliono un valore dopo di sé.
const WITHOUT_VALUE: &[&str] = &["json"];

/// Cosa dire e con che codice uscire.
#[derive(Debug, PartialEq, Eq)]
pub struct Report {
    pub message: String,
    pub code: i32,
}

impl Report {
    fn spoken(message: impl Into<String>) -> Report {
        Report {
            message: message.into(),
            code: 0,
        }
    }
}

/// Il codice d'uscita quando il censimento non è stato permesso. Non è un
/// errore del comando: è la risposta, e vale la pena poterla riconoscere da
/// uno script senza leggere il testo.
pub const REFUSED: i32 = 3;

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(report) => {
            if !report.message.is_empty() {
                println!("{}", report.message);
            }
            report.code
        }
        Err(message) => {
            eprintln!("sailor session: {message}");
            1
        }
    }
}

fn dispatch(args: &[String]) -> Result<Report, String> {
    let Some(verb) = args.first().map(String::as_str) else {
        return Err(usage_text());
    };
    if !FORMS.contains(&verb) {
        return Err(format!(
            "«{verb}» non è una forma di questo comando; ci sono {}\n{}",
            FORMS.join(", "),
            usage_text()
        ));
    }
    let options = options_of(&args[1..])?;

    // Standard input si legge **solo** dove serve: leggerlo per `list` da un
    // terminale interattivo bloccherebbe il comando senza dire perché.
    let raw = if verb == "open" || verb == "event" {
        std::io::read_to_string(std::io::stdin())
            .map_err(|error| format!("non riesco a leggere il payload: {error}"))?
    } else {
        String::new()
    };
    let payload = Payload::parse(&raw)?;

    // **SOLO CHI PARLA DI UN TERMINALE NE PRETENDE UNO.** `list` e `census`
    // parlano di tutti: chiedere loro un tty li fa fallire ovunque l'uscita sia
    // catturata, cioè in ogni script e in ogni gancio.
    let tty = match options.get("tty") {
        Some(declared) => declared.clone(),
        None if NEEDS_A_TERMINAL.contains(&verb) => sessions::tty::current().ok_or_else(|| {
            "non so su quale terminale gira questo processo: nessuno dei suoi tre \
             descrittori è un tty. Dillo con --tty <nome>"
                .to_owned()
        })?,
        None => String::new(),
    };

    // **SOLO CHI LEGGE IL DEPOSITO LO APRE**, per la stessa ragione della riga
    // qui sopra sul terminale — e questa riga quella ragione non l'aveva
    // ricevuta, perché era scritta come commento accanto all'altra. Un principio
    // vale dove c'è un elenco che lo applica.
    let store = if NEEDS_THE_STORE.contains(&verb) {
        let path = match options.get("store") {
            Some(declared) => PathBuf::from(declared),
            None => Sessions::default_path().map_err(|error| error.to_string())?,
        };
        Some(Sessions::open(&path).map_err(|error| format!("{}: {error}", path.display()))?)
    } else {
        None
    };

    // Qui, e solo qui, si guarda la macchina: un evento è arrivato.
    let census = Census::of(&LocalMachine);

    act(&Request {
        verb,
        options: &options,
        payload: &payload,
        raw: &raw,
        store: store.as_ref(),
        census: &census,
        tty: &tty,
        at: now(),
    })
}

/// Tutto quello che serve per agire, già in mano.
///
/// **ESISTE PERCHÉ [`act`] SI POSSA PROVARE.** `dispatch` legge standard input,
/// apre il file vero e interroga la macchina vera: tre cose che una prova non
/// può avere senza misurare la macchina di chi la esegue. Con questa struttura
/// le stesse decisioni si provano su un file usa-e-getta, un payload scritto a
/// mano e un censimento costruito — compreso quello negato.
struct Request<'a> {
    verb: &'a str,
    options: &'a BTreeMap<String, String>,
    payload: &'a Payload,
    raw: &'a str,
    /// **`None` QUANDO NESSUNO NE HA BISOGNO**, e non è un dettaglio di
    /// comodità: `census` deve poter rispondere «non lo so» anche dove il
    /// deposito non si apre. Finché era obbligatorio, l'unica forma che esiste
    /// per non mentire moriva prima di poter parlare.
    store: Option<&'a Sessions>,
    census: &'a Census,
    tty: &'a str,
    at: i64,
}

impl<'a> Request<'a> {
    /// Il deposito di questa forma.
    ///
    /// **UN ERRORE QUI È UN DIFETTO DI QUESTO FILE, NON UN GUASTO DI CHI
    /// DIGITA**, e il messaggio lo dice: vuol dire che una forma legge il
    /// deposito senza essere elencata in [`NEEDS_THE_STORE`].
    fn store(&self) -> Result<&'a Sessions, String> {
        self.store.ok_or_else(|| {
            format!(
                "«{}» legge il deposito ma non è in NEEDS_THE_STORE: è un difetto \
                 di session_cmd.rs, non della riga che hai scritto",
                self.verb
            )
        })
    }

    /// Se chi parla è un gancio d'avvio di Claude Code.
    ///
    /// **LA FORMA DELLA RISPOSTA SEGUE CHI DOMANDA**, e chi domanda sta nel
    /// payload: a un `SessionStart` si risponde con l'involucro che Claude Code
    /// inietta nel contesto, a una persona con una frase.
    fn is_a_session_start(&self) -> bool {
        self.payload.hook_event_name.as_deref() == Some("SessionStart")
    }
}

/// Le forme che il deposito lo leggono davvero.
///
/// **È UN ELENCO DI CHI NE HA BISOGNO, NON DELLE ECCEZIONI**, e la differenza
/// si vede sulla forma aggiunta domani: un elenco di eccezioni la lascia
/// passare in silenzio, questo no. Fino al 01/09/2026 `dispatch` apriva
/// `sessions.db` prima di sapere quale forma fosse stata chiesta, quindi
/// `census` — che il deposito non lo tocca — moriva con l'errore del file **al
/// posto della propria risposta**, e la sua risposta è proprio «non lo so».
///
/// Sorvegliata da `a_form_that_never_reads_the_store_survives_a_store_that_will_not_open`,
/// che gira su **ogni** forma non elencata qui.
const NEEDS_THE_STORE: &[&str] = &["open", "event", "close", "list", "detach", "attach"];

/// I ganci che Sailor innesta, e cosa chiama ciascuno.
///
/// **QUATTRO, E NON DI PIÙ.** Un gancio in più è un processo in più a ogni
/// evento di ogni sessione della macchina: si mette solo dove la risposta serve
/// davvero. `SessionStart` porta il benvenuto ed è l'unico evento in cui ciò che
/// scriviamo entra nel contesto dell'agente; gli altri tre dicono che la
/// sessione è viva, cosa le è stato chiesto e quando il contesto sta per essere
/// compattato — cioè i tre momenti da cui si capisce se tocca passare il
/// testimone.
const HOOKS: &[(&str, &str)] = &[
    ("SessionStart", "open"),
    ("Stop", "event"),
    ("UserPromptSubmit", "event"),
    ("PreCompact", "event"),
];

/// Come si riconosce un gancio nostro fra quelli di chiunque altro: dal fatto
/// che invoca **questo** comando. Non da un nome scritto accanto, che si può
/// cambiare senza cambiare cosa fa.
const MARK: &str = " session ";

/// Dove Claude Code tiene le impostazioni dell'utente.
///
/// **NON È UN NOME DI PRODOTTO IN UNA CONDIZIONE**, è l'indirizzo di ciò che
/// stiamo innestando: un innesto deve sapere dove innesta. Nessun ramo di
/// questo comando cambia comportamento a seconda di cosa trova lì.
fn default_settings() -> Result<PathBuf, String> {
    let home = std::env::var("HOME").map_err(|_| "HOME non è definita".to_owned())?;
    Ok(PathBuf::from(home).join(".claude").join("settings.json"))
}

fn install_hooks(request: &Request<'_>) -> Result<Report, String> {
    let settings = match request.options.get("settings") {
        Some(declared) => PathBuf::from(declared),
        None => default_settings()?,
    };
    let hooks = installed(&settings)?;
    let commands = settings
        .parent()
        .ok_or_else(|| format!("{}: non ha una cartella", settings.display()))?
        .join("commands");
    let written = wrote_the_two_commands(&commands)?;
    Ok(Report::spoken(format!("{hooks}\n{written}")))
}

/// Le due parole che il benvenuto promette, scritte dove Claude Code le cerca.
///
/// **SE MANCANO, IL BENVENUTO MENTE.** Il saluto dice «per staccarlo:
/// /sailor-off», e una parola promessa che non esiste è peggio di una non
/// promessa: chi la digita crede di essersi staccato. La prova
/// `the_welcome_only_promises_words_that_exist` tiene insieme le due cose.
fn wrote_the_two_commands(directory: &std::path::Path) -> Result<String, String> {
    std::fs::create_dir_all(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    for (name, verb, what) in [
        (
            "sailor-off",
            "detach",
            "Stacca questo terminale da Sailor: smette di essere tracciato, e lo \
             restano anche le sessioni che si apriranno qui dopo.",
        ),
        (
            "sailor-on",
            "attach",
            "Ricollega questo terminale a Sailor, se era stato staccato.",
        ),
    ] {
        let body = format!(
            "---\ndescription: {what}\nallowed-tools: Bash(sailor session {verb}:*)\n---\n\n\
             Esegui `sailor session {verb}` e riferisci in una riga cosa ha risposto. \
             Non fare altro.\n"
        );
        let path = directory.join(format!("{name}.md"));
        std::fs::write(&path, body).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    Ok(format!(
        "/sailor-off e /sailor-on scritti in {}",
        directory.display()
    ))
}

/// Innesta i ganci in un file di impostazioni, **aggiungendo**.
///
/// Il percorso del binario è quello che sta girando adesso
/// (`current_exe`): un innesto che scrivesse `sailor` e basta funzionerebbe
/// solo dove quel nome è già nel `PATH` di chi apre il terminale, che non è
/// una cosa che si può sapere da qui.
fn installed(settings: &std::path::Path) -> Result<String, String> {
    let mut root: serde_json::Value = match std::fs::read_to_string(settings) {
        Ok(text) if text.trim().is_empty() => serde_json::json!({}),
        // **UN FILE CHE NON SI CAPISCE NON SI RISCRIVE.** Sostituirlo con la
        // sola parte nostra cancellerebbe la configurazione di chi lo usa, per
        // un errore di battitura.
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("{}: non è JSON valido ({error})", settings.display()))?,
        Err(_) => serde_json::json!({}),
    };

    let binary = std::env::current_exe()
        .map_err(|error| format!("non so dove sono: {error}"))?
        .display()
        .to_string();

    let hooks = root
        .as_object_mut()
        .ok_or_else(|| format!("{}: la radice non è un oggetto", settings.display()))?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| format!("{}: «hooks» non è un oggetto", settings.display()))?;

    let mut added = Vec::new();
    for (event, verb) in HOOKS {
        let command = format!("{binary} session {verb}");
        let list = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| format!("{}: «{event}» non è un vettore", settings.display()))?;

        // Già innestato: si riconosce dal comando, non dalla posizione.
        let ours = list.iter().any(|entry| {
            serde_json::to_string(entry)
                .map(|text| text.contains(MARK) && text.contains("sailor"))
                .unwrap_or(false)
        });
        if ours {
            continue;
        }
        list.push(serde_json::json!({
            "hooks": [{"type": "command", "command": command}]
        }));
        added.push(*event);
    }

    if added.is_empty() {
        return Ok(format!("già innestato in {}", settings.display()));
    }
    if let Some(parent) = settings.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    std::fs::write(settings, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", settings.display()))?;
    Ok(format!(
        "innestato in {}: {}",
        settings.display(),
        added.join(", ")
    ))
}

fn act(request: &Request<'_>) -> Result<Report, String> {
    match request.verb {
        "open" => open_terminal(request),
        "event" => record_event(request),
        "close" => close_terminal(request),
        "detach" => detach_terminal(request),
        "attach" => attach_terminal(request),
        "list" => list_terminals(request),
        "census" => report_census(request),
        "install" => install_hooks(request),
        other => Err(format!("«{other}» non è una forma di questo comando")),
    }
}

fn anchor_of(request: &Request<'_>) -> Anchor {
    anchor_from(request.payload, request.tty.to_owned(), request.census)
}

fn arrival_of(request: &Request<'_>) -> Arrival {
    Arrival {
        anchor: anchor_of(request),
        session_id: request.payload.session_id.clone(),
        transcript_path: request.payload.transcript_path.clone(),
        at: request.at,
    }
}

/// Il nome del fatto: quello che dichiara il payload, o quello del verbo.
fn event_named(request: &Request<'_>, fallback: &str) -> TerminalEvent {
    let anchor = anchor_of(request);
    TerminalEvent {
        tty: anchor.tty.clone(),
        session_id: request.payload.session_id.clone(),
        worktree: Some(anchor.worktree.clone()),
        ancestor: anchor.ancestor.clone(),
        name: request
            .payload
            .hook_event_name
            .clone()
            .filter(|found| !found.is_empty())
            .unwrap_or_else(|| fallback.to_owned()),
        transcript_path: request.payload.transcript_path.clone(),
        occurred_at: request.at,
        // Quello che oggi non leggiamo si conserva com'è arrivato: un campo
        // buttato via non si recupera guardando meglio domani.
        payload: (!request.raw.trim().is_empty()).then(|| request.raw.to_owned()),
    }
}

fn open_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let arrival = arrival_of(request);

    // **STACCATO VUOL DIRE STACCATO**, e vale per i fatti prima che per il
    // testo: niente riga, niente evento, niente saluto. Uno stacco che
    // registrasse comunque sarebbe un silenzio di facciata.
    let detached = store
        .terminal(&arrival.anchor.tty)
        .map_err(|error| error.to_string())?
        .is_some_and(|row| row.is_detached());
    if detached {
        return Ok(Report::spoken(String::new()));
    }

    store
        .open_terminal(&arrival)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "open"))
        .map_err(|error| error.to_string())?;

    if request.is_a_session_start() {
        return Ok(Report::spoken(welcome(&arrival)));
    }
    Ok(Report::spoken(described(&arrival)))
}

/// Il benvenuto, nell'involucro che Claude Code inietta nel contesto della
/// sessione.
///
/// **PERCHÉ È UN INVOLUCRO E NON UNA RIGA STAMPATA.** `SessionStart` è uno dei
/// quattro eventi in cui ciò che il gancio scrive diventa contesto che l'agente
/// legge — verificato sulla documentazione il 01/09/2026. Con una riga normale
/// il saluto lo leggerebbe la persona davanti allo schermo e non l'agente, e lo
/// stacco resterebbe una cosa che esiste e che non sa nessuno.
///
/// Dice tre cose e basta: dove sei, che sei tracciato, e come smettere.
fn welcome(arrival: &Arrival) -> String {
    let text = format!(
        "Sei collegato a Sailor.\n\
         terminale {} · albero {}\n\
         Questo terminale è tracciato: Sailor sa che esisti, in che albero lavori \
         e quando avrai bisogno di passare il testimone.\n\
         Per staccarlo: /sailor-off",
        arrival.anchor.tty, arrival.anchor.worktree,
    );
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    })
    .to_string()
}

fn record_event(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let arrival = arrival_of(request);
    store
        .remember_terminal(&arrival)
        .map_err(|error| error.to_string())?;
    let happened = event_named(request, "event");
    store
        .record_event(&happened)
        .map_err(|error| error.to_string())?;
    Ok(Report::spoken(format!(
        "{} su {}",
        happened.name, happened.tty
    )))
}

fn close_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let closed = store
        .close_terminal(request.tty, request.at)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "close"))
        .map_err(|error| error.to_string())?;
    Ok(Report::spoken(if closed {
        format!("chiuso {}", request.tty)
    } else {
        format!(
            "{} non aveva nessuna riga aperta: il fatto è registrato lo stesso",
            request.tty
        )
    }))
}

fn detach_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    store
        .detach(&anchor_of(request), request.at)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "detach"))
        .map_err(|error| error.to_string())?;
    Ok(Report::spoken(format!(
        "{} è staccato: lo resta anche per chi ci arriverà dopo",
        request.tty
    )))
}

fn attach_terminal(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let was_detached = store
        .attach(request.tty)
        .map_err(|error| error.to_string())?;
    store
        .record_event(&event_named(request, "attach"))
        .map_err(|error| error.to_string())?;
    Ok(Report::spoken(if was_detached {
        format!("{} è di nuovo seguito", request.tty)
    } else {
        format!("{} non era staccato", request.tty)
    }))
}

fn list_terminals(request: &Request<'_>) -> Result<Report, String> {
    let store = request.store()?;
    let rows = store
        .terminals()
        .map_err(|error| error.to_string())?;
    if request.options.contains_key("json") {
        let text = serde_json::to_string_pretty(&rows).map_err(|error| error.to_string())?;
        return Ok(Report::spoken(text));
    }
    if rows.is_empty() {
        return Ok(Report::spoken(
            "nessun terminale si è ancora presentato".to_owned(),
        ));
    }
    let mut text = String::new();
    for row in &rows {
        let howmany = store
            .events_on(&row.tty)
            .map(|found| found.len())
            .unwrap_or_default();
        let _ = writeln!(
            text,
            "{:<10} {:<14} {:<8} {:<11} eventi={:<4} {} {}",
            row.tty,
            row.ancestor.as_deref().unwrap_or("?"),
            if row.is_open() { "aperto" } else { "chiuso" },
            if row.is_detached() {
                "staccato"
            } else {
                "attaccato"
            },
            howmany,
            row.session_id.as_deref().unwrap_or("-"),
            row.worktree,
        );
    }
    Ok(Report::spoken(text.trim_end().to_owned()))
}

fn report_census(request: &Request<'_>) -> Result<Report, String> {
    if request.options.contains_key("json") {
        let text =
            serde_json::to_string_pretty(request.census).map_err(|error| error.to_string())?;
        return Ok(Report {
            code: refusal_code(request.census),
            message: text,
        });
    }
    let message = match request.census {
        Census::Refused(refusal) => format!(
            "NON LO SO: non mi è stato permesso guardare la macchina ({refusal}). \
             Questo non è «nessun terminale»"
        ),
        Census::NoTerminal => {
            "nessun processo ha un terminale, e l'ho potuto chiedere".to_owned()
        }
        Census::Terminals(terminals) => {
            let mut text = String::new();
            for terminal in terminals {
                let _ = writeln!(
                    text,
                    "{} ({}), {} processi",
                    terminal.tty,
                    terminal.ancestor.as_deref().unwrap_or("capostipite ignoto"),
                    terminal.inhabitants.len()
                );
                for inhabitant in &terminal.inhabitants {
                    let _ = writeln!(
                        text,
                        "  {:<8} {:<12} {:<40} {}",
                        inhabitant.pid,
                        inhabitant.uptime,
                        inhabitant.command,
                        inhabitant.working_directory.as_deref().unwrap_or("?"),
                    );
                }
            }
            text.trim_end().to_owned()
        }
    };
    Ok(Report {
        code: refusal_code(request.census),
        message,
    })
}

fn refusal_code(census: &Census) -> i32 {
    match census {
        Census::Refused(_) => REFUSED,
        Census::NoTerminal | Census::Terminals(_) => 0,
    }
}

fn described(arrival: &Arrival) -> String {
    format!(
        "{} in {} ({}), sessione {}",
        arrival.anchor.tty,
        arrival.anchor.worktree,
        arrival.anchor.ancestor.as_deref().unwrap_or("capostipite ignoto"),
        arrival.session_id.as_deref().unwrap_or("senza identificativo"),
    )
}

/// Le opzioni scritte sulla riga. Un'opzione che vuole un valore e non ce l'ha
/// è un errore, non un vuoto: prenderebbe per valore l'opzione successiva.
fn options_of(args: &[String]) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    let mut rest = args.iter();
    while let Some(word) = rest.next() {
        let Some(name) = word.strip_prefix("--") else {
            return Err(format!("non capisco «{word}»\n{}", usage_text()));
        };
        if WITHOUT_VALUE.contains(&name) {
            found.insert(name.to_owned(), "true".to_owned());
            continue;
        }
        let value = rest
            .next()
            .ok_or_else(|| format!("«--{name}» vuole un valore dopo di sé"))?;
        if value.starts_with("--") {
            return Err(format!(
                "«--{name}» ha preso «{value}» per un valore: manca il valore vero"
            ));
        }
        found.insert(name.to_owned(), value.clone());
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sessions::census::{Inhabitant, Refusal, Terminal};
    use sessions::SESSIONS_FILE;

    struct Scratch {
        directory: PathBuf,
    }

    impl Scratch {
        fn new(label: &str) -> Scratch {
            let directory = std::env::temp_dir().join(format!(
                "sailor-session-cmd-{label}-{}-{}",
                std::process::id(),
                now()
            ));
            std::fs::create_dir_all(&directory).expect("creare la cartella");
            Scratch { directory }
        }

        fn store(&self) -> Sessions {
            Sessions::open(self.directory.join(SESSIONS_FILE)).expect("aprire")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }

    fn no_options() -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    fn one_terminal() -> Census {
        Census::Terminals(vec![Terminal {
            tty: "ttys004".to_owned(),
            ancestor: Some("Whatever".to_owned()),
            inhabitants: vec![Inhabitant {
                pid: 10,
                parent_pid: 1,
                tty: "ttys004".to_owned(),
                uptime: "01:00".to_owned(),
                command: "/bin/zsh".to_owned(),
                working_directory: Some("/here".to_owned()),
            }],
        }])
    }

    fn refused() -> Census {
        Census::Refused(Refusal {
            tool: "ps".to_owned(),
            reason: "operation not permitted: ps".to_owned(),
        })
    }

    fn ask(
        verb: &str,
        raw: &str,
        store: &Sessions,
        census: &Census,
        options: &BTreeMap<String, String>,
    ) -> Result<Report, String> {
        let payload = Payload::parse(raw).expect("il payload della prova è JSON");
        act(&Request {
            verb,
            options,
            payload: &payload,
            raw,
            store: Some(store),
            census,
            tty: "ttys004",
            at: 1_000,
        })
    }

    /// **LA PROVA GIRA SU OGNI FORMA CHE NON DICHIARA DI VOLERE IL DEPOSITO**,
    /// non solo su `census`: una forma aggiunta domani e lasciata fuori
    /// dall'elenco viene provata qui senza che nessuno se ne ricordi.
    ///
    /// **E OGNI FORMA RICEVE ANCHE `--settings` DENTRO LA CARTELLA USA-E-GETTA.**
    /// Non è una precauzione teorica: `install`, aggiunta il 01/09/2026, senza
    /// quella riga scriverebbe nel `settings.json` **vero** di chi esegue la
    /// batteria — cioè una prova che riconfigura la macchina di chi la lancia.
    /// Una prova generica lo diventa: copre anche ciò che non esisteva quando è
    /// stata scritta, e questo vale per il bene e per il male.
    ///
    /// *Mutante eseguito*: aprire il deposito incondizionatamente in `dispatch`
    /// fa tornare rossa questa prova nominando la forma e l'errore del file.
    #[test]
    fn a_form_that_never_reads_the_store_survives_a_store_that_will_not_open() {
        let scratch = Scratch::new("senza-deposito");
        // Una **cartella** dove ci si aspetta un file: SQLite non la apre, e il
        // fallimento è dello stesso genere di quelli veri — permessi, disco
        // pieno, un file di una versione più nuova — senza doverli fabbricare.
        let impossible = scratch.directory.join("non-e-un-file");
        std::fs::create_dir_all(&impossible).expect("la cartella di prova");
        let settings = scratch.directory.join("settings-di-prova.json");

        for form in FORMS.iter().filter(|form| !NEEDS_THE_STORE.contains(form)) {
            let words: Vec<String> = vec![
                (*form).to_owned(),
                "--store".to_owned(),
                impossible.display().to_string(),
                "--settings".to_owned(),
                settings.display().to_string(),
            ];
            let report = dispatch(&words).unwrap_or_else(|error| {
                panic!(
                    "«session {form}» non legge il deposito, eppure è morto perché non \
                     si apriva: {error}"
                )
            });
            assert!(
                !report.message.is_empty(),
                "«session {form}» ha risposto senza dire niente"
            );
        }
    }

    /// I ganci di un altro prodotto, già installati, come stanno davvero in
    /// `~/.claude/settings.json` su questa macchina.
    fn settings_of_someone_else() -> &'static str {
        r#"{
          "model": "opusplan",
          "hooks": {
            "Stop": [
              {"hooks": [{"type": "command", "command": "/Users/qualcuno/.altro/gancio.sh"}]}
            ],
            "PreToolUse": [
              {"hooks": [{"type": "command", "command": "/Users/qualcuno/.altro/gancio.sh"}]}
            ]
          }
        }"#
    }

    /// **SI AGGIUNGE, NON SI SOSTITUISCE.** Su `~/.claude/settings.json`
    /// scrivono in cinque, e un innesto che riscrive il vettore dei ganci
    /// spegne in silenzio quelli di chi c'era prima — che è il modo in cui uno
    /// strumento di tracciamento diventa il guasto che doveva prevenire.
    #[test]
    fn installing_leaves_the_hooks_that_were_already_there() {
        let scratch = Scratch::new("innesto");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, settings_of_someone_else()).expect("scrivere");

        installed(&settings).expect("l'innesto riesce");

        let after: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).expect("rileggere"))
                .expect("resta JSON valido");

        assert_eq!(after["model"], "opusplan", "l'innesto non tocca il resto");
        let stops = after["hooks"]["Stop"].as_array().expect("Stop è un vettore");
        assert_eq!(stops.len(), 2, "il gancio di prima è ancora lì, e il nostro è in più");
        assert!(
            serde_json::to_string(&after).unwrap().contains("gancio.sh"),
            "il gancio di chi c'era prima non è stato cancellato"
        );
        assert!(
            after["hooks"]["SessionStart"].is_array(),
            "l'evento che porta il benvenuto dev'esserci"
        );
    }

    /// Un innesto ripetuto è un innesto solo: chi lo lancia due volte per
    /// sicurezza non deve trovarsi due ganci che registrano lo stesso fatto.
    #[test]
    fn installing_twice_does_not_double_anything() {
        let scratch = Scratch::new("innesto-doppio");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, settings_of_someone_else()).expect("scrivere");

        installed(&settings).expect("primo innesto");
        let once = std::fs::read_to_string(&settings).expect("rileggere");
        installed(&settings).expect("secondo innesto");
        let twice = std::fs::read_to_string(&settings).expect("rileggere");

        assert_eq!(once, twice, "il secondo innesto non deve cambiare niente");
    }

    /// **UN FILE CHE NON SI CAPISCE NON SI RISCRIVE.** Sovrascriverlo con la
    /// sola parte nostra cancellerebbe la configurazione di chi lo usa, e per
    /// un errore di battitura.
    #[test]
    fn a_settings_file_that_does_not_parse_is_left_alone() {
        let scratch = Scratch::new("innesto-rotto");
        let settings = scratch.directory.join("settings.json");
        std::fs::write(&settings, "{ questo non è JSON").expect("scrivere");

        let refused = installed(&settings).expect_err("un file illeggibile ferma l'innesto");
        assert!(refused.contains("settings.json"), "{refused}");
        assert_eq!(
            std::fs::read_to_string(&settings).expect("rileggere"),
            "{ questo non è JSON",
            "il file resta esattamente com'era"
        );
    }

    /// La regola di ferro vale anche per ciò che l'innesto **scrive**: il
    /// comando che finisce nei ganci non nomina nessun prodotto.
    #[test]
    fn what_the_install_writes_names_no_product() {
        let scratch = Scratch::new("innesto-neutro");
        let settings = scratch.directory.join("settings.json");
        installed(&settings).expect("l'innesto riesce anche su un file che non c'era");

        let written = std::fs::read_to_string(&settings).expect("rileggere");
        for product in ["orca", "warp", "vscode", "iterm", "tmux"] {
            assert!(
                !written.to_lowercase().contains(product),
                "l'innesto ha scritto «{product}» in settings.json: {written}"
            );
        }
    }

    /// **IL BENVENUTO PROMETTE SOLO PAROLE CHE ESISTONO.** Il saluto dice «per
    /// staccarlo: /sailor-off»; se l'innesto non scrivesse quel comando, chi lo
    /// digita si crederebbe staccato e non lo sarebbe. Le due cose stanno in
    /// due file diversi e nessun compilatore le lega: le lega questa prova.
    #[test]
    fn the_welcome_only_promises_words_that_exist() {
        let scratch = Scratch::new("parola-mantenuta");
        let settings = scratch.directory.join("settings.json");
        let request = Request {
            verb: "install",
            options: &BTreeMap::from([(
                "settings".to_owned(),
                settings.display().to_string(),
            )]),
            payload: &Payload::parse("{}").expect("payload vuoto"),
            raw: "",
            store: None,
            census: &one_terminal(),
            tty: "",
            at: 1_000,
        };
        act(&request).expect("l'innesto riesce");

        let saluto = welcome(&Arrival {
            anchor: sessions::Anchor {
                tty: "ttys004".to_owned(),
                worktree: "/qui".to_owned(),
                ancestor: None,
            },
            session_id: None,
            transcript_path: None,
            at: 1_000,
        });

        for word in ["/sailor-off", "/sailor-on"] {
            if !saluto.contains(word) {
                continue;
            }
            let file = scratch
                .directory
                .join("commands")
                .join(format!("{}.md", word.trim_start_matches('/')));
            assert!(
                file.exists(),
                "il benvenuto promette «{word}» e l'innesto non lo scrive: {}",
                file.display()
            );
        }
        assert!(
            saluto.contains("/sailor-off"),
            "il saluto deve promettere lo stacco, o lo stacco non lo sa nessuno"
        );
    }

    /// Il payload di un `SessionStart` vero, come lo manda Claude Code.
    fn a_session_start(session: &str) -> String {
        format!(
            r#"{{"session_id":"{session}","hook_event_name":"SessionStart",
                 "startup_reason":"startup","cwd":"/qui/dentro"}}"#
        )
    }

    /// **IL BENVENUTO ENTRA NEL CONTESTO DELL'AGENTE, NON NEL TERMINALE.**
    /// `SessionStart` è uno dei quattro eventi in cui Claude Code aggiunge al
    /// contesto ciò che il gancio scrive: è il pilastro su cui sta in piedi la
    /// promessa piena. Se questo smette di valere, il saluto diventa una riga
    /// che legge la persona e non l'agente — funziona lo stesso, ma è un'altra
    /// cosa, e chi la legge deve saperlo prima.
    #[test]
    fn the_welcome_enters_the_context_of_the_agent() {
        let scratch = Scratch::new("benvenuto");
        let store = scratch.store();
        let report = ask(
            "open",
            &a_session_start("s-1"),
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("l'apertura riesce");

        let spoken: serde_json::Value =
            serde_json::from_str(&report.message).expect("un gancio SessionStart risponde in JSON");
        let context = spoken["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .expect("il saluto viaggia in hookSpecificOutput.additionalContext");

        assert!(context.contains("Sailor"), "{context}");
        assert!(
            context.contains("/sailor-off"),
            "il saluto deve dire come staccarsi, o lo stacco esiste e non lo sa nessuno: {context}"
        );
        assert!(
            context.contains("ttys004"),
            "il saluto nomina il terminale di cui parla: {context}"
        );
        assert_eq!(
            spoken["hookSpecificOutput"]["hookEventName"], "SessionStart",
            "l'involucro dichiara di quale evento è la risposta"
        );
    }

    /// **STACCATO VUOL DIRE STACCATO.** Nessun saluto, e nessuna riga scritta:
    /// se l'apertura registrasse comunque, «lascia stare questa finestra»
    /// varrebbe solo per il testo e non per i fatti.
    #[test]
    fn a_detached_terminal_is_greeted_by_silence() {
        let scratch = Scratch::new("staccato");
        let store = scratch.store();
        let census = one_terminal();

        ask("detach", "{}", &store, &census, &no_options()).expect("lo stacco riesce");
        let report = ask(
            "open",
            &a_session_start("s-2"),
            &store,
            &census,
            &no_options(),
        )
        .expect("l'apertura su un terminale staccato non è un errore");

        assert_eq!(report.message, "", "un terminale staccato non riceve saluti");
        assert!(
            store
                .events_on("ttys004")
                .expect("leggere gli eventi")
                .iter()
                .all(|event| event.name != "SessionStart"),
            "un terminale staccato non lascia eventi: staccato vale per i fatti, non solo per il testo"
        );
    }

    /// **IL CENSIMENTO NON HA BISOGNO DI UN DEPOSITO**, e finché ne aveva
    /// bisogno poteva morire prima di poter dire «non lo so» — cioè proprio la
    /// cosa per cui esiste. Visto eseguendo il binario nel perimetro il
    /// 01/09/2026: apriva `sessions.db` per ogni forma, e con il deposito
    /// predefinito non scrivibile usciva 1 con un errore di SQLite invece di 3
    /// con la frase giusta.
    #[test]
    fn the_census_answers_even_without_a_store() {
        let refused = Census::Refused(Refusal {
            tool: "ps".to_owned(),
            reason: "Operation not permitted".to_owned(),
        });
        let report = act(&Request {
            verb: "census",
            options: &no_options(),
            payload: &Payload::parse("{}").expect("payload vuoto"),
            raw: "",
            store: None,
            census: &refused,
            tty: "",
            at: 1_000,
        })
        .expect("il censimento risponde");

        assert_eq!(report.code, REFUSED);
        assert!(report.message.contains("NON LO SO"), "{}", report.message);
    }

    /// L'elenco stampato e quello accettato sono lo stesso: una forma
    /// documentata e non accettata si scopre solo digitandola.
    #[test]
    fn the_usage_names_every_form_the_dispatch_accepts() {
        for form in FORMS {
            assert!(
                USAGE.iter().any(|line| line.contains(&format!("session {form}"))),
                "«{form}» è accettata e non è scritta in USAGE"
            );
        }
    }

    /// L'ancora è `(tty, albero, capostipite)`, e si vede nella riga scritta.
    #[test]
    fn an_arrival_is_anchored_to_the_tty_the_tree_and_the_ancestor() {
        let scratch = Scratch::new("anchor");
        let store = scratch.store();
        let report = ask(
            "open",
            r#"{"session_id":"abc","cwd":"/Users/theo/personal/sailor",
                "transcript_path":"/tmp/abc.jsonl","hook_event_name":"SessionStart"}"#,
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("registrare");
        assert_eq!(report.code, 0);

        let row = store.terminal("ttys004").expect("leggere").expect("c'è");
        assert_eq!(row.worktree, "/Users/theo/personal/sailor");
        assert_eq!(row.ancestor.as_deref(), Some("Whatever"));
        assert_eq!(row.session_id.as_deref(), Some("abc"));
        let events = store.events_on("ttys004").expect("gli eventi");
        assert_eq!(events[0].name, "SessionStart", "il nome viene dal payload");
        assert!(events[0].payload.is_some(), "il payload si conserva com'è arrivato");
    }

    /// **UN CAMPO CHE MANCA NON FA FALLIRE NIENTE**: ci si arrangia con quello
    /// che si ha. Un payload vuoto ha comunque un tty.
    #[test]
    fn a_payload_with_nothing_in_it_still_registers_the_terminal() {
        let scratch = Scratch::new("empty-payload");
        let store = scratch.store();
        ask("open", "{}", &store, &one_terminal(), &no_options()).expect("registrare");
        let row = store.terminal("ttys004").expect("leggere").expect("c'è");
        assert_eq!(row.session_id, None);
        assert!(!row.worktree.is_empty(), "l'albero cade sulla cartella corrente");
        assert_eq!(store.events_on("ttys004").expect("gli eventi")[0].name, "open");
    }

    /// **UN CENSIMENTO NEGATO NON ROMPE UN GANCIO.** La riga si scrive lo
    /// stesso, e il capostipite resta ignoto invece di essere inventato.
    #[test]
    fn a_refused_census_does_not_stop_the_registration() {
        let scratch = Scratch::new("refused-open");
        let store = scratch.store();
        let report = ask(
            "open",
            r#"{"session_id":"abc","cwd":"/here"}"#,
            &store,
            &refused(),
            &no_options(),
        )
        .expect("un censimento negato non deve far fallire la registrazione");
        assert_eq!(report.code, 0);
        let row = store.terminal("ttys004").expect("leggere").expect("c'è");
        assert_eq!(
            row.ancestor, None,
            "un capostipite che non si è potuto leggere resta ignoto, non inventato"
        );
    }

    /// Ma `census` sì: la sua risposta *è* il censimento, e un diniego si
    /// riconosce anche senza leggere il testo.
    #[test]
    fn the_census_says_it_does_not_know_and_says_it_with_its_own_code() {
        let scratch = Scratch::new("refused-census");
        let store = scratch.store();
        let report = ask("census", "", &store, &refused(), &no_options()).expect("censire");
        assert_eq!(report.code, REFUSED);
        assert!(
            report.message.contains("NON LO SO"),
            "un diniego va detto, non trasformato in un elenco vuoto: {}",
            report.message
        );

        let empty = Census::NoTerminal;
        let other = ask("census", "", &store, &empty, &no_options()).expect("censire");
        assert_eq!(other.code, 0);
        assert!(other.message.contains("nessun processo"), "{}", other.message);
    }

    /// Lo stacco vive sul tty: la porta unica lo scrive e lo toglie, e in mezzo
    /// una sessione nuova non lo cancella.
    #[test]
    fn detaching_through_the_command_holds_across_a_new_session() {
        let scratch = Scratch::new("detach");
        let store = scratch.store();
        ask("detach", "", &store, &one_terminal(), &no_options()).expect("staccare");
        ask(
            "open",
            r#"{"session_id":"nuova","cwd":"/here"}"#,
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("aprire dopo");
        assert!(store
            .terminal("ttys004")
            .expect("leggere")
            .expect("c'è")
            .is_detached());
        ask("attach", "", &store, &one_terminal(), &no_options()).expect("riattaccare");
        assert!(!store
            .terminal("ttys004")
            .expect("leggere")
            .expect("c'è")
            .is_detached());
    }

    #[test]
    fn the_list_says_what_is_open_and_what_is_detached() {
        let scratch = Scratch::new("list");
        let store = scratch.store();
        ask(
            "open",
            r#"{"session_id":"abc","cwd":"/here"}"#,
            &store,
            &one_terminal(),
            &no_options(),
        )
        .expect("aprire");
        ask("detach", "", &store, &one_terminal(), &no_options()).expect("staccare");
        let report = ask("list", "", &store, &one_terminal(), &no_options()).expect("elencare");
        assert!(report.message.contains("ttys004"), "{}", report.message);
        assert!(report.message.contains("aperto"), "{}", report.message);
        assert!(report.message.contains("staccato"), "{}", report.message);
        assert!(report.message.contains("Whatever"), "{}", report.message);
    }

    /// **CHIEDERE COSA RISULTA NON RICHIEDE UN TERMINALE PROPRIO.** `list` e
    /// `census` non parlano del terminale da cui li si invoca: parlano di tutti.
    /// Pretendere un tty li fa fallire ovunque l'uscita sia catturata — cioè in
    /// ogni script, in ogni gancio e in ogni prova — e il messaggio manda a
    /// cercare un'opzione invece del difetto. Visto eseguendo il binario vero:
    /// `sailor session list` usciva 1 su una macchina dove tutto funzionava.
    #[test]
    fn asking_what_is_tracked_does_not_need_a_terminal_of_its_own() {
        let scratch = Scratch::new("no-tty");
        let path = scratch.directory.join(SESSIONS_FILE);
        for form in ["list", "census"] {
            let words: Vec<String> = vec![
                form.to_owned(),
                "--store".to_owned(),
                path.display().to_string(),
            ];
            dispatch(&words)
                .unwrap_or_else(|error| panic!("«session {form}» ha preteso un tty: {error}"));
        }
    }

    #[test]
    fn an_unknown_form_names_the_ones_that_exist() {
        let message = dispatch(&["sweep".to_owned()]).expect_err("una forma ignota è un errore");
        for form in FORMS {
            assert!(message.contains(form), "{message} non nomina «{form}»");
        }
    }

    #[test]
    fn an_option_without_its_value_is_an_error() {
        let words: Vec<String> = ["--tty".to_owned()].into();
        assert!(options_of(&words).is_err());
        let pair: Vec<String> = ["--tty".to_owned(), "--json".to_owned()].into();
        assert!(options_of(&pair).is_err());
        let bare: Vec<String> = ["--json".to_owned()].into();
        assert_eq!(options_of(&bare).expect("--json non vuole valori")["json"], "true");
    }
}
