//! Dove si conserva chi si è presentato.
//!
//! **UN FILE SUO, ACCANTO AL DEPOSITO E NON DENTRO.** Le sessioni di terminale
//! non sono le corse dei flussi: `state.db` ha la sua `user_version`, che sale
//! quando cambiano le proiezioni delle corse, e infilare qui dentro un'altra
//! ragione di salire vorrebbe dire che due cantieri paralleli scelgono ognuno
//! «la prossima» — la stessa. Il deposito di chi arriva secondo si
//! dichiarerebbe «versione non supportata» su una macchina dove nessuno ha
//! cambiato niente, e nessun controllo lo vedrebbe. Qui la versione è nostra.
//!
//! **DUE TAVOLE, E LA SECONDA NON SI RISCRIVE MAI.** `terminals` è lo stato di
//! adesso, una riga per tty; `terminal_events` è quello che è successo, in
//! coda. La prima si può correggere, la seconda no: è da lì che si ricostruisce
//! quali sessioni si sono succedute sullo stesso terminale.
//!
//! **LO STACCO STA SUL TTY.** `detached_at` vive sulla riga del terminale e
//! **nessuna scrittura di apertura lo tocca**: staccare una finestra la stacca
//! anche per chi ci aprirà una sessione domani. È quello che una persona
//! intende quando dice «lascia stare questa finestra» — non «lascia stare
//! questo processo».

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::{Path, PathBuf};

/// Il file, accanto a quello del deposito.
pub const SESSIONS_FILE: &str = "sessions.db";

/// La forma che questo codice si aspetta, **indipendente da quella delle
/// proiezioni del deposito**. Va alzata insieme alle colonne: il guasto 24 è
/// nato da una costante rimasta indietro rispetto alla migrazione che la
/// doveva far scattare.
const SESSIONS_SCHEMA_VERSION: i64 = 1;

#[derive(Debug)]
pub enum SessionError {
    Sqlite(rusqlite::Error),
    /// Il file è stato scritto da una versione che non conosciamo. Non si
    /// ripara e non si aggira: si dichiara.
    UnsupportedSchema(i64),
    NoDirectory(String),
}

impl fmt::Display for SessionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "sqlite: {error}"),
            Self::UnsupportedSchema(version) => write!(
                formatter,
                "unsupported sessions schema version {version}: questo file lo ha scritto \
                 una versione più nuova di sessions.db"
            ),
            Self::NoDirectory(reason) => write!(formatter, "{reason}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<rusqlite::Error> for SessionError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// L'ancora del tracciamento, e l'unica cosa che identifica un terminale:
/// **il tty, l'albero e il capostipite**. Nessun prodotto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    /// L'oggetto del kernel. È già il nome neutro che il sistema dà a «un
    /// terminale», ed è la chiave: due finestre diverse non lo condividono.
    pub tty: String,
    /// L'albero di lavoro in cui si trova chi si è presentato.
    pub worktree: String,
    /// Chi ha disegnato la finestra. **Solo etichetta**: si stampa e si
    /// registra, nessuna decisione la legge. `None` è «non lo sappiamo».
    pub ancestor: Option<String>,
}

/// Chi si presenta, con quello che ha.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrival {
    pub anchor: Anchor,
    /// L'identificativo che l'agente si porta dietro, quando ne ha uno.
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub at: i64,
}

/// Una riga del terminale, com'è adesso.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRow {
    pub tty: String,
    pub worktree: String,
    pub ancestor: Option<String>,
    pub session_id: Option<String>,
    pub transcript_path: Option<String>,
    pub opened_at: i64,
    /// `None` = c'è ancora una sessione aperta su questo terminale. Può restare
    /// `None` per sempre: un terminale ucciso non chiude niente, e questo è un
    /// fatto da mostrare, non da nascondere.
    pub closed_at: Option<i64>,
    /// `None` = attaccato. Sopravvive a ogni apertura successiva.
    pub detached_at: Option<i64>,
}

impl TerminalRow {
    pub fn is_open(&self) -> bool {
        self.closed_at.is_none()
    }

    pub fn is_detached(&self) -> bool {
        self.detached_at.is_some()
    }
}

/// Un fatto accaduto su un terminale, in coda e mai riscritto.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEvent {
    pub tty: String,
    pub session_id: Option<String>,
    pub worktree: Option<String>,
    pub ancestor: Option<String>,
    /// Come si chiama il fatto. Arriva dal payload (`hook_event_name`) quando
    /// c'è, altrimenti lo mette chi lo registra.
    pub name: String,
    pub transcript_path: Option<String>,
    pub occurred_at: i64,
    /// Il payload come è arrivato, per non perdere quello che oggi non
    /// leggiamo.
    pub payload: Option<String>,
}

pub struct Sessions {
    connection: Connection,
    path: PathBuf,
}

impl Sessions {
    /// Dove sta il file su questa macchina: **accanto al deposito**, nella
    /// cartella che `ledger::default_directory()` restituisce. La regola su
    /// dov'è la casa non si ricopia — è il guasto 19.
    pub fn default_path() -> Result<PathBuf, SessionError> {
        ledger::default_directory()
            .map(|directory| directory.join(SESSIONS_FILE))
            .ok_or_else(|| {
                SessionError::NoDirectory(
                    "non so dove tenere le sessioni: né SAILOR_LEDGER né HOME sono dichiarate"
                        .to_owned(),
                )
            })
    }

    /// Apre il file, creandolo se non c'è.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, SessionError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                SessionError::NoDirectory(format!("creare {}: {error}", parent.display()))
            })?;
        }
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SESSIONS_SCHEMA_VERSION {
            return Err(SessionError::UnsupportedSchema(version));
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS terminals (
                 tty TEXT PRIMARY KEY,
                 worktree TEXT NOT NULL,
                 ancestor TEXT,
                 session_id TEXT,
                 transcript_path TEXT,
                 opened_at INTEGER NOT NULL,
                 closed_at INTEGER,
                 detached_at INTEGER
             );
             CREATE TABLE IF NOT EXISTS terminal_events (
                 id INTEGER PRIMARY KEY AUTOINCREMENT,
                 tty TEXT NOT NULL,
                 session_id TEXT,
                 worktree TEXT,
                 ancestor TEXT,
                 name TEXT NOT NULL,
                 transcript_path TEXT,
                 occurred_at INTEGER NOT NULL,
                 payload TEXT
             );
             CREATE INDEX IF NOT EXISTS terminal_events_by_terminal
                 ON terminal_events (tty, id);
             CREATE INDEX IF NOT EXISTS terminal_events_by_session
                 ON terminal_events (session_id, id);",
        )?;
        if version < SESSIONS_SCHEMA_VERSION {
            connection.pragma_update(None, "user_version", SESSIONS_SCHEMA_VERSION)?;
        }
        Ok(Self { connection, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// La versione che questo file dichiara. Serve alle prove per tenere ferma
    /// l'indipendenza da quella delle proiezioni del deposito.
    pub fn schema_version(&self) -> Result<i64, SessionError> {
        Ok(self
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Qualcuno apre una sessione su un terminale.
    ///
    /// **`detached_at` NON COMPARE FRA LE COLONNE AGGIORNATE**, ed è il punto:
    /// una finestra staccata resta staccata anche per l'agente che ci arriva
    /// dopo. Se questa riga cambiasse, lo stacco durerebbe quanto una sessione,
    /// che è esattamente ciò che nessuno intende dicendolo.
    pub fn open_terminal(&self, arrival: &Arrival) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminals
                 (tty, worktree, ancestor, session_id, transcript_path, opened_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(tty) DO UPDATE SET
                 worktree = excluded.worktree,
                 ancestor = COALESCE(excluded.ancestor, terminals.ancestor),
                 session_id = excluded.session_id,
                 transcript_path = COALESCE(excluded.transcript_path, terminals.transcript_path),
                 opened_at = CASE
                     WHEN terminals.session_id IS excluded.session_id THEN terminals.opened_at
                     ELSE excluded.opened_at
                 END,
                 closed_at = NULL",
            params![
                arrival.anchor.tty,
                arrival.anchor.worktree,
                arrival.anchor.ancestor,
                arrival.session_id,
                arrival.transcript_path,
                arrival.at,
            ],
        )?;
        Ok(())
    }

    /// Un evento arriva da un terminale di cui nessuno ha annunciato
    /// l'apertura: la riga si crea lo stesso, con quello che si sa.
    ///
    /// Non tocca `closed_at`: un evento non riapre niente, e non tocca
    /// `detached_at` per la ragione di sopra.
    pub fn remember_terminal(&self, arrival: &Arrival) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminals
                 (tty, worktree, ancestor, session_id, transcript_path, opened_at, closed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL)
             ON CONFLICT(tty) DO UPDATE SET
                 worktree = excluded.worktree,
                 ancestor = COALESCE(excluded.ancestor, terminals.ancestor),
                 session_id = COALESCE(excluded.session_id, terminals.session_id),
                 transcript_path = COALESCE(excluded.transcript_path, terminals.transcript_path)",
            params![
                arrival.anchor.tty,
                arrival.anchor.worktree,
                arrival.anchor.ancestor,
                arrival.session_id,
                arrival.transcript_path,
                arrival.at,
            ],
        )?;
        Ok(())
    }

    pub fn record_event(&self, event: &TerminalEvent) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminal_events
                 (tty, session_id, worktree, ancestor, name, transcript_path, occurred_at, payload)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                event.tty,
                event.session_id,
                event.worktree,
                event.ancestor,
                event.name,
                event.transcript_path,
                event.occurred_at,
                event.payload,
            ],
        )?;
        Ok(())
    }

    /// Chiude la riga aperta su un tty. Torna `false` se non ce n'era una: una
    /// chiusura che non chiude niente si dice, non si finge.
    pub fn close_terminal(&self, tty: &str, at: i64) -> Result<bool, SessionError> {
        let changed = self.connection.execute(
            "UPDATE terminals SET closed_at = ?2 WHERE tty = ?1 AND closed_at IS NULL",
            params![tty, at],
        )?;
        Ok(changed > 0)
    }

    /// Stacca un terminale. Se non lo conoscevamo, lo si registra staccato: uno
    /// stacco che si perde perché nessuno si era ancora presentato è uno stacco
    /// che non ha fatto niente.
    pub fn detach(&self, anchor: &Anchor, at: i64) -> Result<(), SessionError> {
        self.connection.execute(
            "INSERT INTO terminals
                 (tty, worktree, ancestor, session_id, transcript_path, opened_at, closed_at,
                  detached_at)
             VALUES (?1, ?2, ?3, NULL, NULL, ?4, ?4, ?4)
             ON CONFLICT(tty) DO UPDATE SET
                 detached_at = ?4,
                 ancestor = COALESCE(excluded.ancestor, terminals.ancestor)",
            params![anchor.tty, anchor.worktree, anchor.ancestor, at],
        )?;
        Ok(())
    }

    /// Riattacca. Torna `false` se quel tty non era staccato.
    pub fn attach(&self, tty: &str) -> Result<bool, SessionError> {
        let changed = self.connection.execute(
            "UPDATE terminals SET detached_at = NULL WHERE tty = ?1 AND detached_at IS NOT NULL",
            params![tty],
        )?;
        Ok(changed > 0)
    }

    pub fn terminal(&self, tty: &str) -> Result<Option<TerminalRow>, SessionError> {
        Ok(self
            .connection
            .query_row(
                "SELECT tty, worktree, ancestor, session_id, transcript_path, opened_at,
                        closed_at, detached_at
                 FROM terminals WHERE tty = ?1",
                params![tty],
                read_terminal,
            )
            .optional()?)
    }

    pub fn terminals(&self) -> Result<Vec<TerminalRow>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT tty, worktree, ancestor, session_id, transcript_path, opened_at,
                    closed_at, detached_at
             FROM terminals ORDER BY tty",
        )?;
        let rows = statement
            .query_map([], read_terminal)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Gli eventi di un terminale, nell'ordine in cui sono arrivati.
    pub fn events_on(&self, tty: &str) -> Result<Vec<TerminalEvent>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT tty, session_id, worktree, ancestor, name, transcript_path, occurred_at,
                    payload
             FROM terminal_events WHERE tty = ?1 ORDER BY id",
        )?;
        let rows = statement
            .query_map(params![tty], read_event)?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Quali sessioni si sono succedute su un terminale, nell'ordine in cui si
    /// sono viste. La riga di `terminals` porta solo l'ultima: chi vuole la
    /// successione la chiede alla coda, che non si riscrive.
    pub fn sessions_on(&self, tty: &str) -> Result<Vec<String>, SessionError> {
        let mut statement = self.connection.prepare(
            "SELECT session_id FROM terminal_events
             WHERE tty = ?1 AND session_id IS NOT NULL
             GROUP BY session_id ORDER BY MIN(id)",
        )?;
        let rows = statement
            .query_map(params![tty], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }
}

fn read_terminal(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerminalRow> {
    Ok(TerminalRow {
        tty: row.get(0)?,
        worktree: row.get(1)?,
        ancestor: row.get(2)?,
        session_id: row.get(3)?,
        transcript_path: row.get(4)?,
        opened_at: row.get(5)?,
        closed_at: row.get(6)?,
        detached_at: row.get(7)?,
    })
}

fn read_event(row: &rusqlite::Row<'_>) -> rusqlite::Result<TerminalEvent> {
    Ok(TerminalEvent {
        tty: row.get(0)?,
        session_id: row.get(1)?,
        worktree: row.get(2)?,
        ancestor: row.get(3)?,
        name: row.get(4)?,
        transcript_path: row.get(5)?,
        occurred_at: row.get(6)?,
        payload: row.get(7)?,
    })
}
