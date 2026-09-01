//! I terminali aperti: quali sono, in quale spazio di lavoro, e cosa succede a
//! una riga che ci viene scritta dentro.
//!
//! **QUI SI INCONTRANO LE DUE METÀ.** Sotto c'è lo pseudo-terminale, che tocca
//! il sistema operativo; accanto c'è lo smistamento, che è un elenco di dati. Un
//! [`Terminal`] è il punto in cui la riga scritta dall'utente viene prima
//! guardata e poi — solo se è un comando — eseguita.
//!
//! **QUESTO CRATE NON ESEGUE FLUSSI, E LA RIGA DI CONFINE È VOLUTA.**
//! [`Terminal::submit`] restituisce [`Routed::Flow`] e non fa partire niente.
//! Far partire una corsa vuol dire il motore dei flussi, il deposito e gli
//! inneschi: farli entrare qui dentro renderebbe impossibile aprire un terminale
//! senza portarsi dietro tutto Sailor, e un terminale deve poter aprirsi anche
//! quando i flussi sono rotti. Chi compone il programma prende quel `Flow` e lo
//! consegna all'innesco manuale — la prova
//! `a_routed_request_reaches_the_trigger` fa esattamente questo, ed è lì che si
//! vede il collegamento completo.

use crate::pty::{Pty, PtyError, Size};
use crate::routing::{PathLookup, Routed, Router};
use crate::{Catalog, Ending, Output, Workspace};
use std::ffi::OsString;
use std::io::Read;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Come si apre un terminale: cosa avviare, dove, quanto grande.
///
/// **UNA STRUTTURA E NON SEI ARGOMENTI** perché i valori predefiniti sono la
/// parte che conta: chi apre un terminale normale dice solo lo spazio di lavoro,
/// e chi ne apre uno particolare cambia un campo — senza che l'aggiunta del
/// campo dopo rompa chi chiama.
#[derive(Debug, Clone)]
pub struct Opening {
    /// Il programma da avviare dentro il terminale. Predefinito: la shell di
    /// chi lancia, letta da `SHELL`.
    pub program: OsString,
    pub args: Vec<OsString>,
    pub size: Size,
    /// Variabili aggiunte a quelle ereditate.
    pub environment: Vec<(String, String)>,
}

impl Default for Opening {
    fn default() -> Opening {
        Opening {
            program: std::env::var_os("SHELL").unwrap_or_else(|| OsString::from("/bin/sh")),
            args: Vec::new(),
            size: Size::default(),
            environment: vec![
                // Senza `TERM` un programma non sa che tipo di terminale ha
                // davanti e si comporta come se non ne avesse nessuno: niente
                // colori, niente posizionamento del cursore.
                ("TERM".to_string(), "xterm-256color".to_string()),
                // Chi gira dentro deve poter sapere di essere dentro Sailor:
                // è la sola via perché un programma annidato non riapra un
                // terminale dentro il terminale.
                ("SAILOR_TERMINAL".to_string(), "1".to_string()),
            ],
        }
    }
}

/// Un terminale aperto, legato al proprio spazio di lavoro.
pub struct Terminal {
    id: String,
    workspace: Workspace,
    pty: Pty,
    router: Arc<Router>,
    closed: Mutex<bool>,
}

impl Terminal {
    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    pub fn alive(&self) -> bool {
        !*self.closed.lock().expect("il lucchetto non panica") && self.pty.alive()
    }

    pub fn process_id(&self) -> u32 {
        self.pty.process_id()
    }

    /// **LA RIGA CHE L'UTENTE HA SCRITTO, GUARDATA PRIMA DI ESSERE ESEGUITA.**
    ///
    /// Se è un comando, ci finisce dentro col ritorno a capo, come se fosse
    /// stata digitata, e la risposta dice perché è passata. Se è una richiesta
    /// che riguarda un flusso, **non ci finisce dentro**: la risposta dice quale
    /// regola l'ha riconosciuta, a quale flusso va e con quale testo, e chi
    /// chiama decide cosa farne.
    pub fn submit(&self, line: &str) -> Result<Routed, PtyError> {
        let decision = self.router.route(line);
        if let Routed::Command { line, .. } = &decision {
            let mut typed = line.as_bytes().to_vec();
            typed.push(b'\n');
            self.pty.write(&typed)?;
        }
        Ok(decision)
    }

    /// Byte grezzi sull'ingresso, senza passare dallo smistamento: un Ctrl-C,
    /// una freccia, la risposta a una domanda interattiva.
    ///
    /// **NON SI SMISTA CIÒ CHE NON È UNA RIGA.** Lo smistamento guarda una
    /// richiesta intera; un tasto premuto dentro un editor non è una richiesta,
    /// e passarlo di qui vorrebbe dire farlo esaminare da un elenco di regole
    /// che non lo riguarda.
    pub fn press(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.pty.write(bytes)
    }

    pub fn resize(&self, size: Size) -> Result<(), PtyError> {
        self.pty.resize(size)
    }

    pub fn close(&self) -> Result<(), PtyError> {
        *self.closed.lock().expect("il lucchetto non panica") = true;
        self.pty.close()
    }

    /// Cosa mostrare di questo terminale in un elenco.
    pub fn summary(&self) -> Summary {
        Summary {
            id: self.id.clone(),
            workspace_root: self.workspace.root.to_string_lossy().into_owned(),
            workspace_name: self.workspace.name.clone(),
            alive: self.alive(),
            process_id: self.process_id(),
        }
    }
}

/// Una riga dell'elenco dei terminali aperti.
///
/// **QUESTO TIPO È LA RIGA CHE LA FINESTRA RICEVE, E NON SE NE RICOPIA UNA
/// SECONDA.** `docs/2026-09-01-il-contratto-del-terminale.md` lo dice per
/// esteso: il ponte risponde con questa struttura tale e quale, invece di
/// dichiarare i cinque campi una seconda volta in TypeScript e una terza in un
/// tipo di comodo dentro il guscio. I nomi escono in `camelCase` perché è la
/// forma in cui il contratto li scrive e in cui la finestra li legge; restano
/// identificatori inglesi da tutte e due le parti, che è ciò che `AGENTS.md`
/// chiede.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub workspace_root: String,
    pub workspace_name: String,
    pub alive: bool,
    pub process_id: u32,
}

/// I terminali aperti da questo processo.
///
/// **L'ELENCO STA QUI E NON IN UN FILE**, perché è la risposta alla domanda
/// «quali terminali ho aperto **io**»: un elenco su disco sopravvivrebbe al
/// processo e direbbe che sono aperti terminali che sono morti con lui — che è
/// il guasto 4 raccontato di nuovo, un passo più in là. Il giorno in cui i
/// terminali devono sopravvivere a chi li ha aperti, chi li registra è il
/// deposito, non questa struttura.
pub struct Terminals {
    open: Mutex<Vec<Arc<Terminal>>>,
    router: Arc<Router>,
    next: AtomicU64,
}

impl Terminals {
    /// I terminali di questo processo, con le regole di smistamento spedite col
    /// prodotto e quelle scritte dall'utente.
    pub fn current() -> Terminals {
        Terminals::with_router(Arc::new(Router::current()))
    }

    /// Con un elenco di regole dichiarato da chi chiama: è la forma che usano le
    /// prove, e quella che userà chi vuole regole diverse per terminali diversi.
    pub fn with_catalog(catalog: &Catalog) -> Terminals {
        Terminals::with_router(Arc::new(Router::new(
            catalog,
            Arc::new(PathLookup::current()),
        )))
    }

    pub fn with_router(router: Arc<Router>) -> Terminals {
        Terminals {
            open: Mutex::new(Vec::new()),
            router,
            next: AtomicU64::new(1),
        }
    }

    pub fn router(&self) -> &Arc<Router> {
        &self.router
    }

    /// Apre un terminale dentro `workspace` e consegna la sua uscita **mentre
    /// esce** al destinatario che `make_output` fabbrica.
    ///
    /// Il filo che legge nasce qui e muore quando il terminale finisce: leggere
    /// a richiesta vorrebbe dire o un buffer che cresce senza che nessuno lo
    /// svuoti, o un figlio che si blocca in scrittura quando nessuno chiede.
    ///
    /// **IL DESTINATARIO SI FABBRICA COL NOME DEL TERMINALE IN MANO, E NON LO
    /// RICEVE DOPO.** Chi consegna l'uscita altrove — a una finestra, a una
    /// rete — deve dire *di quale* terminale è ogni pezzo, e l'identificativo lo
    /// assegna questa funzione. Passando un destinatario già fatto restava un
    /// istante in cui i primi byte esistevano e il nome no: l'invito della shell
    /// esce lì dentro, cioè proprio il pezzo che chi guarda si aspetta per
    /// primo. Un `FnOnce(&str)` toglie quell'istante per costruzione, invece di
    /// coprirlo con una coda d'attesa.
    pub fn open(
        &self,
        workspace: Workspace,
        opening: &Opening,
        make_output: impl FnOnce(&str) -> Arc<dyn Output>,
    ) -> Result<Arc<Terminal>, PtyError> {
        let args: Vec<&std::ffi::OsStr> = opening.args.iter().map(AsRef::as_ref).collect();
        let pty = Pty::open(
            &workspace,
            &opening.program,
            &args,
            opening.size,
            &opening.environment,
        )?;
        let mut reader = pty.reader()?;
        let id = format!(
            "{}-{}",
            workspace.name,
            self.next.fetch_add(1, Ordering::Relaxed)
        );
        let output = make_output(&id);
        let terminal = Arc::new(Terminal {
            id,
            workspace,
            pty,
            router: Arc::clone(&self.router),
            closed: Mutex::new(false),
        });
        let draining = Arc::clone(&terminal);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    // Zero byte è la fine del terminale, non qualcosa che è
                    // stato detto: consegnarlo farebbe scrivere una riga a chi
                    // guarda per un fatto che non è accaduto.
                    Ok(0) => break,
                    Ok(read) => output.chunk(&buffer[..read]),
                    // Un segnale arrivato durante la lettura non è la fine
                    // dell'uscita, e trattarlo così troncherebbe il testo di un
                    // terminale sano.
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                    // Su Linux l'ultimo lettore di uno pseudo-terminale il cui
                    // figlio è morto riceve `EIO`, non zero byte: è la fine, e
                    // chiamarla guasto farebbe apparire rotto ogni terminale
                    // chiuso normalmente.
                    Err(_) => break,
                }
            }
            output.ended(how_it_ended(&draining.pty));
        });
        self.open
            .lock()
            .expect("il lucchetto dell'elenco non panica")
            .push(Arc::clone(&terminal));
        Ok(terminal)
    }

    /// Quali terminali sono aperti e in quale spazio di lavoro.
    pub fn list(&self) -> Vec<Summary> {
        self.open
            .lock()
            .expect("il lucchetto dell'elenco non panica")
            .iter()
            .map(|terminal| terminal.summary())
            .collect()
    }

    pub fn find(&self, id: &str) -> Option<Arc<Terminal>> {
        self.open
            .lock()
            .expect("il lucchetto dell'elenco non panica")
            .iter()
            .find(|terminal| terminal.id == id)
            .map(Arc::clone)
    }

    /// Chiude un terminale e lo toglie dall'elenco.
    pub fn close(&self, id: &str) -> Option<Result<(), PtyError>> {
        let mut open = self
            .open
            .lock()
            .expect("il lucchetto dell'elenco non panica");
        let at = open.iter().position(|terminal| terminal.id == id)?;
        let terminal = open.remove(at);
        Some(terminal.close())
    }

    /// Chiude tutto. Chi apre terminali deve avere un gesto solo per la fine,
    /// o ne dimentica uno.
    pub fn close_all(&self) {
        let taken: Vec<Arc<Terminal>> = std::mem::take(
            &mut *self
                .open
                .lock()
                .expect("il lucchetto dell'elenco non panica"),
        );
        for terminal in taken {
            let _ = terminal.close();
        }
    }
}

impl Drop for Terminals {
    fn drop(&mut self) {
        self.close_all();
    }
}

/// Quanto si insiste a chiedere com'è finito, dopo che l'uscita è finita.
///
/// **NON È UN'ATTESA PER SICUREZZA, È IL TEMPO FRA DUE FATTI DIVERSI.** Che
/// l'ultimo descrittore del terminale si chiuda e che il processo sia stato
/// raccolto dal sistema sono due cose, e nell'ordine sbagliato per chi legge: il
/// figlio muore, l'uscita finisce, e solo poco dopo `try_wait` ha un esito da
/// dare. Due secondi sono lunghi rispetto a quella distanza e corti rispetto a
/// chi guarda.
const HOW_LONG_TO_ASK: std::time::Duration = std::time::Duration::from_secs(2);

/// Com'è finito il processo dentro, chiesto senza mai bloccare.
///
/// **SCADUTA LA PAZIENZA SI DICE «ANCORA VIVO», NON «USCITO CON ZERO».** Un
/// programma che chiude i propri descrittori e continua a girare esiste, ed è
/// raro: proprio per questo un esito inventato al posto suo non lo troverebbe
/// nessuno.
fn how_it_ended(pty: &Pty) -> Ending {
    let until = std::time::Instant::now() + HOW_LONG_TO_ASK;
    loop {
        if let Some(ending) = pty.finished() {
            return ending;
        }
        if std::time::Instant::now() >= until {
            return Ending::StillRunning;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
