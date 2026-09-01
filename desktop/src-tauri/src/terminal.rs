//! Il ponte fra il motore dei terminali e la finestra.
//!
//! **QUI NON C'È NESSUN TERMINALE, E NON DEVE ESSERCENE.** Lo pseudo-terminale,
//! lo smistamento e l'elenco degli aperti stanno in `crates/terminal`, che si
//! prova da riga di comando con `cargo test -p terminal`. Questo file fa tre
//! cose e nient'altro: traduce sei chiamate in altrettanti gesti del motore,
//! porta l'uscita alla finestra come evento, e trasforma un errore in una frase.
//! Ogni riga di decisione che finisse qui sarebbe una seconda verità accanto al
//! motore, provabile solo aprendo la finestra — cioè non provata.
//!
//! **I NOMI E LE FORME VENGONO DAL CONTRATTO**, non da questo file:
//! `docs/2026-09-01-il-contratto-del-terminale.md`. Sono sei comandi e due
//! eventi, scritti prima del lavoro perché la metà React li stava costruendo in
//! parallelo. Chi ne cambia uno lo cambia lì e lo dice.
//!
//! **LA RIGA DELL'ELENCO NON SI RICOPIA.** `terminal::Summary` è già
//! `Serialize`, con i nomi che il contratto dichiara: questo modulo la
//! restituisce tale e quale. Dichiarare qui una struttura coi suoi cinque campi
//! sarebbe il guasto 10, che in questo repo si è già ripresentato cinque volte.

use std::sync::{Arc, OnceLock};

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Il canale su cui la finestra riceve ciò che un terminale stampa.
pub const OUTPUT_EVENT: &str = "terminal_output";

/// Il canale su cui la finestra viene a sapere che il processo dentro è finito.
///
/// **SENZA QUESTO EVENTO LA FINESTRA MENTE PER OMISSIONE.** Un terminale che
/// smette di parlare è indistinguibile da uno morto, e resterebbe disegnato come
/// vivo per sempre: è la forma in cui il guasto 12 — un sensore che confonde
/// «zero» con «cieco» — si ripresenta ogni volta che qualcuno la dimentica.
pub const CLOSED_EVENT: &str = "terminal_closed";

/// I terminali di questa finestra.
///
/// **STA IN UNA COSTANTE DI PROCESSO E NON IN `manage`, ED È UN VINCOLO DEL
/// CONTRATTO, NON UNA PREFERENZA.** Il ponte può aggiungere a `main.rs` le sole
/// sei voci dentro `generate_handler!`: un `.manage(...)` sarebbe una riga in
/// più in un file che un altro cantiere sta toccando in parallelo, ed è
/// esattamente il tipo di conflitto che il contratto esiste per evitare. La
/// portata è la stessa — uno per processo, vivo quanto la finestra.
///
/// **COSA QUESTO NON DÀ, E VA DETTO QUI.** Un `static` non viene mai lasciato
/// cadere, quindi `Drop for Terminals` — che chiude tutto — non gira alla
/// chiusura della finestra. I processi dentro muoiono lo stesso, perché il capo
/// dello pseudo-terminale se ne va col processo e una shell che perde il proprio
/// terminale esce; ma è una conseguenza del sistema operativo, non una chiusura
/// ordinata, e nessuno la scrive nel deposito. La sopravvivenza vera è un
/// cantiere a sé: sta scritta nel contratto come prima delle due proprietà, e
/// non è questa.
fn terminals() -> &'static terminal::Terminals {
    static OPEN: OnceLock<terminal::Terminals> = OnceLock::new();
    OPEN.get_or_init(terminal::Terminals::current)
}

// ── quello che la finestra riceve ───────────────────────────────────────

/// Un pezzo di uscita, marcato col terminale da cui viene.
///
/// **I BYTE VIAGGIANO IN BASE64, E IL MOTIVO NON È LA PRUDENZA.** Ciò che esce
/// da uno pseudo-terminale è una sequenza di byte che si spezza dove capita,
/// anche a metà di un carattere multibyte: consegnarla come stringa la
/// corromperebbe, e l'accento perso si vedrebbe solo su una parola italiana in
/// mezzo a un'uscita lunga — cioè quasi mai, e mai dove qualcuno guarda.
// `Clone` perché `Emitter::emit` lo pretende: un evento può andare a più
// finestre, e ognuna riceve la propria copia.
#[derive(Clone, Serialize)]
struct OutputEvent {
    id: String,
    bytes: String,
}

/// Il processo dentro un terminale è finito, e com'è finito.
///
/// `status` è **la frase che il motore produce**, non un codice numerico: i tre
/// casi che `terminal::Ending` distingue — uscito con un codice, fermato da un
/// segnale, uscita finita ma processo ancora vivo — non stanno in un numero, e
/// il terzo diventerebbe uno zero, cioè un successo inventato.
#[derive(Clone, Serialize)]
struct ClosedEvent {
    id: String,
    status: String,
}

/// Chi porta l'uscita di **un** terminale alla finestra.
///
/// Nasce col nome del terminale già in mano — `Terminals::open` lo fabbrica
/// dandogli l'identificativo — quindi non esiste nessun pezzo che arrivi prima
/// di sapere a chi appartiene.
struct ToWindow {
    app: AppHandle,
    id: String,
}

impl terminal::Output for ToWindow {
    fn chunk(&self, bytes: &[u8]) {
        // Un evento perso non ferma il terminale: chi legge è una finestra che
        // può essersi chiusa, e far cadere il filo che drena bloccherebbe in
        // scrittura il processo dentro.
        let _ = self.app.emit(
            OUTPUT_EVENT,
            OutputEvent {
                id: self.id.clone(),
                bytes: base64::engine::general_purpose::STANDARD.encode(bytes),
            },
        );
    }

    fn ended(&self, ending: terminal::Ending) {
        let _ = self.app.emit(
            CLOSED_EVENT,
            ClosedEvent {
                id: self.id.clone(),
                status: ending.to_string(),
            },
        );
    }
}

/// Dove è andata a finire la riga che l'utente ha confermato.
///
/// **DUE ESITI E NON UNO**, perché il motore ne distingue due e la distinzione è
/// il senso di tutto il crate: una riga può non essere stata eseguita affatto.
/// Il campo `rule` porta l'identificativo della regola che ha deciso — chi
/// guarda deve poter risalire alla riga di JSON che ha dirottato il proprio
/// comando, non solo al flusso che se l'è preso.
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Submitted {
    /// È passata al terminale, com'era.
    Command,
    /// È andata a un flusso, e il terminale non l'ha vista.
    Flow {
        flow: String,
        text: String,
        rule: String,
    },
}

// ── i sei comandi ───────────────────────────────────────────────────────

/// Apre un terminale dentro uno spazio di lavoro.
///
/// **LA MISURA SI DICHIARA APRENDO, E NON HA UN PREDEFINITO DI COMODO.** Uno
/// pseudo-terminale nasce a zero righe per zero colonne: un programma che chiede
/// quanto è largo lo schermo si sentirebbe rispondere zero, e si disegnerebbe su
/// una riga sola. Chi apre sa quanto è grande il riquadro; questo modulo no.
#[tauri::command]
pub(crate) fn terminal_open(
    app: AppHandle,
    workspace_root: String,
    program: Option<String>,
    args: Option<Vec<String>>,
    cols: u16,
    rows: u16,
) -> Result<terminal::Summary, String> {
    let workspace = terminal::Workspace::open(&workspace_root)
        .map_err(|error| format!("lo spazio di lavoro «{workspace_root}» non si apre: {error}"))?;

    // I predefiniti — la shell di chi lancia, `TERM`, `SAILOR_TERMINAL` — stanno
    // nel motore: ricopiarli qui li farebbe divergere alla prima aggiunta.
    let mut opening = terminal::Opening {
        size: terminal::Size {
            rows,
            columns: cols,
        },
        ..terminal::Opening::default()
    };
    if let Some(program) = program.filter(|program| !program.trim().is_empty()) {
        opening.program = program.into();
    }
    if let Some(args) = args {
        opening.args = args.into_iter().map(Into::into).collect();
    }

    let opened = terminals()
        .open(workspace, &opening, |id| {
            Arc::new(ToWindow {
                app: app.clone(),
                id: id.to_owned(),
            }) as Arc<dyn terminal::Output>
        })
        .map_err(|error| error.to_string())?;
    Ok(opened.summary())
}

/// La riga che l'utente ha confermato con Invio, **guardata prima di essere
/// eseguita**.
///
/// Ci arriva solo ciò che è una riga intera: un tasto premuto dentro un editor
/// passa da [`terminal_press`], e farlo esaminare da un elenco di regole che non
/// lo riguarda renderebbe inservibile ogni programma interattivo.
#[tauri::command]
pub(crate) fn terminal_submit(id: String, line: String) -> Result<Submitted, String> {
    let terminal = find(&id)?;
    match terminal.submit(&line).map_err(|error| error.to_string())? {
        terminal::Routed::Command { .. } => Ok(Submitted::Command),
        terminal::Routed::Flow { route, flow, text } => Ok(Submitted::Flow {
            flow,
            text,
            rule: route,
        }),
    }
}

/// Byte grezzi sull'ingresso: un Ctrl-C, una freccia, la risposta a una domanda.
#[tauri::command]
pub(crate) fn terminal_press(id: String, bytes: String) -> Result<(), String> {
    let pressed = base64::engine::general_purpose::STANDARD
        .decode(bytes.as_bytes())
        // Un base64 storto non è un tasto: scriverlo lo stesso manderebbe
        // spazzatura dentro il terminale di qualcuno.
        .map_err(|error| format!("i byte da premere non sono base64 valido: {error}"))?;
    find(&id)?
        .press(&pressed)
        .map_err(|error| error.to_string())
}

/// Dice al terminale quanto è grande adesso.
#[tauri::command]
pub(crate) fn terminal_resize(id: String, cols: u16, rows: u16) -> Result<(), String> {
    find(&id)?
        .resize(terminal::Size {
            rows,
            columns: cols,
        })
        .map_err(|error| error.to_string())
}

/// Chiude un terminale e lo toglie dall'elenco.
#[tauri::command]
pub(crate) fn terminal_close(id: String) -> Result<(), String> {
    match terminals().close(&id) {
        Some(outcome) => outcome.map_err(|error| error.to_string()),
        None => Err(unknown(&id)),
    }
}

/// Quali terminali sono aperti e in quale spazio di lavoro.
#[tauri::command]
pub(crate) fn terminal_list() -> Vec<terminal::Summary> {
    terminals().list()
}

// ── il poco che resta ───────────────────────────────────────────────────

fn find(id: &str) -> Result<Arc<terminal::Terminal>, String> {
    terminals().find(id).ok_or_else(|| unknown(id))
}

/// **CHI CHIEDE DI UN TERMINALE CHE NON C'È RICEVE ANCHE L'ELENCO DI QUELLI CHE
/// CI SONO.** Un «terminale sconosciuto» da solo non distingue i due casi che
/// portano lì: un identificativo sbagliato, e una finestra che sta parlando con
/// un processo diverso da quello che ha aperto i suoi terminali — cioè un
/// riavvio del guscio, che è il caso normale finché la sopravvivenza non c'è.
fn unknown(id: &str) -> String {
    let open = terminals().list();
    if open.is_empty() {
        return format!("il terminale «{id}» non esiste: questa finestra non ne ha nessuno aperto");
    }
    let names: Vec<&str> = open.iter().map(|row| row.id.as_str()).collect();
    format!(
        "il terminale «{id}» non esiste; quelli aperti sono: {}",
        names.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **LA RISPOSTA DI `terminal_submit` HA DUE FORME, E LA FINESTRA LE
    /// DISTINGUE DAL CAMPO `kind`.**
    ///
    /// È l'unica parte del contratto che questo file *decide*: `Routed` porta
    /// `route`, la finestra legge `rule`, e la traduzione sta qui. Una prova che
    /// guardasse il tipo Rust invece del JSON non vedrebbe niente — a divergere
    /// sarebbero i nomi, che è la sola cosa che la finestra legge.
    ///
    /// LA MISURA CHE POTEVA VENIRE DIVERSA: rinominando il campo in `route`, o
    /// togliendo `tag = "kind"`, la finestra riceve un oggetto che non sa
    /// leggere e mostra una riga vuota invece di dire quale regola ha dirottato
    /// il comando di qualcuno.
    #[test]
    fn a_command_that_passed_and_one_that_went_to_a_flow_look_different() {
        let passed = serde_json::to_value(Submitted::Command).expect("si serializza");
        assert_eq!(passed, serde_json::json!({ "kind": "command" }));

        let routed = serde_json::to_value(Submitted::Flow {
            flow: "smista-il-lavoro".to_owned(),
            text: "trova i residui".to_owned(),
            rule: "richiesta-in-italiano".to_owned(),
        })
        .expect("si serializza");
        assert_eq!(
            routed,
            serde_json::json!({
                "kind": "flow",
                "flow": "smista-il-lavoro",
                "text": "trova i residui",
                "rule": "richiesta-in-italiano"
            })
        );
    }

    /// **I DUE EVENTI SI CHIAMANO COME DICE IL CONTRATTO**, e la metà React si
    /// mette in ascolto su quelle due stringhe: un refuso qui non rompe niente
    /// che compili, e lascia una finestra muta davanti a un terminale che parla.
    #[test]
    fn the_two_events_keep_the_names_the_window_listens_to() {
        assert_eq!(OUTPUT_EVENT, "terminal_output");
        assert_eq!(CLOSED_EVENT, "terminal_closed");
    }

    /// **I BYTE ESCONO IN BASE64 STANDARD**, che è ciò che `atob` del browser
    /// sa leggere. Il caso che conta è quello che non si vede: un accento
    /// spezzato a metà fra due pezzi resta due byte, e la finestra li rimette
    /// insieme. Passato come stringa sarebbe già perduto qui.
    #[test]
    fn a_chunk_cut_in_the_middle_of_a_letter_survives_the_trip() {
        // I due byte di «à» in UTF-8, separati come li separerebbe una lettura
        // che si ferma dove capita.
        let first = base64::engine::general_purpose::STANDARD.encode([0xC3]);
        let second = base64::engine::general_purpose::STANDARD.encode([0xA0]);
        let mut back = base64::engine::general_purpose::STANDARD
            .decode(first)
            .expect("torna indietro");
        back.extend(
            base64::engine::general_purpose::STANDARD
                .decode(second)
                .expect("torna indietro"),
        );
        assert_eq!(String::from_utf8(back).expect("è «à»"), "à");
    }
}
