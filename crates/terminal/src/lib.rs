//! Il motore dei terminali di Sailor.
//!
//! **UN TERMINALE NASCE DENTRO UNO SPAZIO DI LAVORO.** Non è un terminale
//! generico a cui poi si dice dove andare: la cartella è parte di cosa il
//! terminale *è*, e si dichiara aprendolo. Questa non è una comodità — è la
//! condizione perché lo smistamento sappia di quale progetto si sta parlando.
//! Un terminale che scopre la propria cartella dopo essere nato è un terminale
//! che, per un istante, appartiene a un posto sbagliato; e quell'istante è
//! esattamente quello in cui l'utente scrive la prima riga.
//!
//! **CHE COSA FA E CHE COSA NON FA.** Apre uno pseudo-terminale vero, gli
//! scrive sull'ingresso, ne consegna l'uscita **mentre esce**, lo ridimensiona,
//! lo chiude, e dice quali terminali sono aperti e in quale spazio. Non disegna
//! niente, non interpreta le sequenze ANSI e non decide cosa un flusso debba
//! fare: consegna byte grezzi e una decisione, e chi lo usa fa il resto.
//!
//! **LO SMISTAMENTO STA IN [`routing`], E LE SUE REGOLE SONO DATI.** Ciò che
//! l'utente scrive viene guardato prima di essere eseguito: se ha la forma di
//! una richiesta che riguarda un flusso va al flusso, altrimenti passa al
//! terminale. Quali forme, e verso quale flusso, lo dicono i descrittori — non
//! un `match` in questo crate. Il valore predefinito è sempre il terminale: lo
//! smistamento è un'aggiunta, e nel dubbio non scatta.
//!
//! **NIENTE TAURI QUI DENTRO.** Il motore si prova da riga di comando —
//! `cargo test -p terminal` — e la finestra ci si attacca sopra. Un motore
//! dentro la finestra sarebbe un motore che nessuno può provare senza aprirla.

pub mod pty;
pub mod routing;
pub mod session;

pub use pty::{Pty, PtyError, Size};
pub use routing::{
    default_sources, Catalog, CommandLookup, Loaded, Match, Passed, PathLookup, Problem, Route,
    Routed, Router, Source,
};
pub use session::{Opening, Summary, Terminal, Terminals};

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Lo spazio di lavoro a cui un terminale appartiene: una repo, la cartella di
/// un progetto.
///
/// **È UNA CARTELLA CHE ESISTE, VERIFICATA ALL'APERTURA.** Un percorso che non
/// c'è diventerebbe un `spawn` fallito con un messaggio del sistema operativo
/// su un binario che invece esiste — il guasto si sposterebbe di un gradino, e
/// chi legge cercherebbe la shell invece della cartella.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workspace {
    /// La radice, resa assoluta e senza collegamenti simbolici: due terminali
    /// aperti su `~/x` e su `/Users/tizio/x` stanno nello stesso posto, e
    /// l'elenco deve dirlo.
    pub root: PathBuf,
    /// Come lo si chiama parlando: l'ultimo segmento della radice.
    pub name: String,
}

impl Workspace {
    pub fn open(root: impl AsRef<Path>) -> io::Result<Workspace> {
        let root = root.as_ref();
        let root = std::fs::canonicalize(root)?;
        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                format!(
                    "uno spazio di lavoro è una cartella, e {} non lo è",
                    root.display()
                ),
            ));
        }
        let name = root
            .file_name()
            .map(|part| part.to_string_lossy().into_owned())
            // La radice del disco non ha un ultimo segmento, e resta se stessa.
            .unwrap_or_else(|| root.to_string_lossy().into_owned());
        Ok(Workspace { root, name })
    }
}

/// Chi riceve l'uscita di un terminale **mentre** esce, invece che quando il
/// processo è morto.
///
/// **SI CONSEGNANO BYTE GREZZI, E LA SCELTA NON È NUOVA.** È la stessa di
/// `actions::LiveSink`, e per le stesse ragioni scritte lì: una lettura si ferma
/// dove capita, anche a metà di una sequenza UTF-8 multibyte, e decodificare qui
/// sostituirebbe l'accento spezzato al bordo con un carattere di sostituzione —
/// un guasto invisibile e permanente — oppure obbligherebbe a trattenere i byte
/// incompleti fino al pezzo dopo, cioè a rimettere il ritardo che il meccanismo
/// esiste per togliere. La decodifica è di chi guarda, che è l'unico a sapere
/// cosa vuole farne. In un terminale la ragione pesa di più che altrove: qui i
/// byte non sono solo testo, sono anche sequenze di controllo, e un emulatore
/// che le riceva mutilate ridisegna male lo schermo.
///
/// **PERCHÉ NON È LO STESSO TRATTO E NON SI RIUSA `actions`.** `LiveSink::chunk`
/// prende un [`actions::Pipe`] perché un figlio ordinario ha due uscite
/// separate; uno pseudo-terminale ne ha **una**, che è ciò che il terminale
/// mostra — stdout e stderr arrivano già mescolati dal sistema operativo, e
/// passare `Pipe::Stdout` sarebbe dichiarare una distinzione che qui non esiste.
/// L'altra ragione è la direzione delle dipendenze: `actions` porta con sé il
/// motore dei flussi e il deposito, e il motore dei terminali non deve
/// dipendere da loro per aprire una shell.
///
/// `chunk` non deve bloccare a lungo né panicare: lo chiama il filo che drena il
/// terminale, e un filo fermo è un processo bloccato in scrittura. E non riceve
/// mai un pezzo vuoto: «zero byte» è la fine del terminale, non qualcosa che è
/// stato detto.
pub trait Output: Send + Sync {
    fn chunk(&self, bytes: &[u8]);
}

/// Una closure basta: un destinatario semplice non deve costare un tipo.
impl<F> Output for F
where
    F: Fn(&[u8]) + Send + Sync,
{
    fn chunk(&self, bytes: &[u8]) {
        self(bytes)
    }
}

/// Un destinatario che accumula tutto, per chi vuole guardare dopo.
///
/// Sta nella libreria e non nelle prove perché serve a tutte e due, e due copie
/// di un accumulatore divergono sul dettaglio che conta: se `text()` decodifichi
/// o no in modo tollerante.
#[derive(Debug, Default)]
pub struct Buffer {
    bytes: Mutex<Vec<u8>>,
}

impl Buffer {
    pub fn new() -> Buffer {
        Buffer::default()
    }

    pub fn bytes(&self) -> Vec<u8> {
        self.bytes.lock().expect("il buffer non panica").clone()
    }

    /// Il testo accumulato, con i byte non decodificabili sostituiti: qui la
    /// perdita è accettabile perché si guarda, non si ritrasmette.
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.bytes()).into_owned()
    }

    /// Attende che `needle` compaia, fino a `limit`. Torna `true` se è comparso.
    ///
    /// **UNA PROVA SU UN PROCESSO VERO ASPETTA, NON DORME UNA VOLTA SOLA.** Un
    /// `sleep` fisso è o troppo corto — e la prova diventa capricciosa — o
    /// troppo lungo, e la batteria rallenta a ogni caso.
    pub fn wait_for(&self, needle: &str, limit: std::time::Duration) -> bool {
        let until = std::time::Instant::now() + limit;
        loop {
            if self.text().contains(needle) {
                return true;
            }
            if std::time::Instant::now() >= until {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }
}

impl Output for Buffer {
    fn chunk(&self, bytes: &[u8]) {
        self.bytes
            .lock()
            .expect("il buffer non panica")
            .extend_from_slice(bytes);
    }
}
