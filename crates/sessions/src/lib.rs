//! Il tracciamento dei terminali.
//!
//! **IL PRINCIPIO: SAILOR NON ENTRA NEL TERMINALE.** È l'agente — o la shell —
//! che si presenta a Sailor. Non c'è nessun codice specifico di prodotto qui
//! dentro, e non ce ne può entrare: la prova
//! `no_product_name_decides_anything` lo tiene fermo.
//!
//! **L'ANCORA È `(tty, albero, capostipite)`.** `ttys004` è un oggetto del
//! kernel: è già il nome neutro che il sistema dà a «un terminale», e non
//! bisogna inventarne un altro. L'albero è la cartella in cui si lavora. Il
//! capostipite — chi ha disegnato la finestra — si ottiene risalendo la catena
//! dei genitori e **serve solo da etichetta**: si stampa, si registra, e
//! nessuna condizione lo legge.
//!
//! **LA REGOLA DI FERRO.** Il nome di un prodotto può comparire in
//! un'etichetta, mai in una condizione. Stampare «gira in Orca» va bene;
//! `if host == "orca"` è vietato, e c'è una prova che lo impedisce.
//!
//! **IL CENSIMENTO È INNESCATO, NON A OROLOGIO.** Non c'è nessun timer, nessun
//! ciclo, nessuna attesa: [`census::Census::of`] si chiama quando arriva un
//! evento, e in nessun altro momento.

pub mod census;
pub mod store;
pub mod tty;

pub use census::{Census, Inhabitant, LocalMachine, Machine, Refusal, Terminal};
pub use store::{
    Anchor, Arrival, SessionError, Sessions, TerminalEvent, TerminalRow, SESSIONS_FILE,
};

use serde::Deserialize;

/// Il payload che arriva su standard input.
///
/// **QUATTRO CAMPI, E TUTTI FACOLTATIVI.** È la forma dei ganci di Claude Code,
/// ma nessuno di questi nomi appartiene a un prodotto: sono l'identificativo di
/// una sessione, dove sta la sua trascrizione, in che cartella gira e come si
/// chiama il fatto. Chiunque scriva quel JSON viene tracciato allo stesso modo,
/// e chi ne manda uno vuoto viene tracciato lo stesso — con meno informazione.
///
/// I campi che non conosciamo si ignorano: **un payload con un campo in più non
/// è un payload rotto**, e rifiutarlo è il guasto 8.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Payload {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub transcript_path: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub hook_event_name: Option<String>,
}

impl Payload {
    /// Legge il testo. Un testo vuoto è un payload vuoto, non un errore: chi
    /// invoca `sailor session` a mano da un terminale non ha niente da mandare,
    /// e ha comunque un tty e una cartella.
    pub fn parse(text: &str) -> Result<Payload, String> {
        if text.trim().is_empty() {
            return Ok(Payload::default());
        }
        serde_json::from_str(text).map_err(|error| format!("il payload non è JSON: {error}"))
    }
}

/// L'ancora costruita con quello che si ha in mano.
///
/// Il capostipite si chiede al censimento, che può non saperlo — e allora resta
/// `None`, che è diverso da una stringa vuota: `None` è «non lo sappiamo».
pub fn anchor_from(payload: &Payload, tty: String, census: &Census) -> Anchor {
    let ancestor = census.ancestor_of(&tty).map(str::to_owned);
    let worktree = payload
        .cwd
        .clone()
        .filter(|found| !found.is_empty())
        .unwrap_or_else(working_directory);
    Anchor {
        tty,
        worktree,
        ancestor,
    }
}

/// La cartella di lavoro di questo processo, o `.` se il sistema non la dice.
pub fn working_directory() -> String {
    std::env::current_dir()
        .map(|found| found.display().to_string())
        .unwrap_or_else(|_| ".".to_owned())
}

/// Adesso, in secondi. Lo stesso orologio del deposito.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs() as i64)
        .unwrap_or_default()
}
