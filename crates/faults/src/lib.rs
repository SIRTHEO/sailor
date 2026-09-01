//! I guasti incontrati costruendo Sailor, come **dati**.
//!
//! **LA COSA CHE CAMBIA È CHI ASSEGNA IL NUMERO.** Finché la tabella era un file
//! markdown, il numero lo sceglieva chi scriveva guardando l'ultima riga — e due
//! rami non si vedono. L'01/09/2026 il 43, il 47 e il 48 sono stati contesi in
//! un pomeriggio, ogni volta scoperti alla fusione. È il guasto 42, e la sua
//! colonna «cosa lo impedirebbe» diceva che nessuna prova può bastare, perché
//! una prova guarda un ramo alla volta. Qui il numero lo assegna il deposito,
//! che è uno solo: la collisione non è più improbabile, è impossibile.
//!
//! **LE SEI COLONNE SI CONSERVANO COM'ERANO SCRITTE.** Lo stato non è un enum
//! ma il testo intero della cella — `**aperto**`, `**chiuso** il 01/09 — con
//! mutante`, `**chiuso in parte** il 01/09, riaperto il 02/09` — perché la
//! sfumatura *è* l'informazione, e ridurla a tre casi butterebbe via proprio la
//! metà che racconta quale metà della cura è fatta. Se «aperto» conta si decide
//! leggendo il testo, in [`Fault::still_open`], che è dove la regola stava già.

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Il file, accanto al deposito.
pub const FAULTS_FILE: &str = "faults.db";

/// La forma che questo codice si aspetta. Indipendente dalle proiezioni del
/// deposito, per la ragione scritta nel `Cargo.toml`.
const FAULTS_SCHEMA_VERSION: i64 = 1;

#[derive(Debug)]
pub enum FaultError {
    Database(rusqlite::Error),
    NoDirectory(String),
    UnsupportedSchema(i64),
    Unknown(i64),
}

impl std::fmt::Display for FaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FaultError::Database(error) => write!(f, "{error}"),
            FaultError::NoDirectory(what) => write!(f, "{what}"),
            FaultError::UnsupportedSchema(found) => write!(
                f,
                "questo deposito dei guasti è alla versione {found} e questo binario \
                 conosce la {FAULTS_SCHEMA_VERSION}: non è rotto, è più nuovo"
            ),
            FaultError::Unknown(number) => write!(f, "il guasto {number} non esiste"),
        }
    }
}

impl std::error::Error for FaultError {}

impl From<rusqlite::Error> for FaultError {
    fn from(error: rusqlite::Error) -> Self {
        FaultError::Database(error)
    }
}

/// Un guasto vero, con la data. Le sei colonne sono quelle che la tabella aveva
/// dal 28/08/2026, e **una voce senza «cosa lo impedirebbe» non è finita**: è la
/// riga che separa questo da un diario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fault {
    pub number: i64,
    pub happened_on: String,
    pub what_happened: String,
    pub how_it_showed: String,
    pub what_would_prevent: String,
    pub status: String,
}

/// Un guasto da registrare: tutto tranne il numero, **che non si sceglie**.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Draft {
    pub happened_on: String,
    pub what_happened: String,
    pub how_it_showed: String,
    pub what_would_prevent: String,
    pub status: String,
}

impl Fault {
    /// Un guasto conta come aperto finché la cura che dichiara non è fatta.
    ///
    /// **«CHIUSO IN PARTE» È APERTO.** Uno stato di mezzo racconta quale metà è
    /// fatta, non toglie la riga dal conto: chi legge «undici aperti» crede che
    /// ne restino undici, e invece ne restano dodici. È il guasto 40.
    ///
    /// E basta che *cominci* con «aperto»: un confronto esatto su un campo di
    /// prosa si rompe alla prima sfumatura, e si rompe **verso il basso** — cioè
    /// nella direzione che tranquillizza. È il guasto 42, scoperto scrivendo la
    /// riga che descriveva sé stessa.
    pub fn still_open(&self) -> bool {
        self.status.starts_with("**aperto**") || self.status.contains("chiuso in parte")
    }

    fn cells(&self) -> [&str; 6] {
        // Il numero manca apposta: si formatta a parte, ed è l'unico campo che
        // non viene da chi scrive.
        [
            &self.happened_on,
            &self.what_happened,
            &self.how_it_showed,
            &self.what_would_prevent,
            &self.status,
            "",
        ]
    }
}

pub struct Faults {
    connection: Connection,
    path: PathBuf,
}

impl Faults {
    /// Dove tenerli su questa macchina: accanto al deposito, che è l'unico posto
    /// che sa dov'è la casa — ricopiare quel percorso è il guasto 19.
    pub fn default_path() -> Result<PathBuf, FaultError> {
        ledger::default_directory()
            .map(|directory| directory.join(FAULTS_FILE))
            .ok_or_else(|| {
                FaultError::NoDirectory(
                    "non so dove tenere i guasti: né SAILOR_LEDGER né HOME sono dichiarate"
                        .to_owned(),
                )
            })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self, FaultError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                FaultError::NoDirectory(format!("creare {}: {error}", parent.display()))
            })?;
        }
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "synchronous", "FULL")?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > FAULTS_SCHEMA_VERSION {
            return Err(FaultError::UnsupportedSchema(version));
        }
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS faults (
                 number INTEGER PRIMARY KEY,
                 happened_on TEXT NOT NULL,
                 what_happened TEXT NOT NULL,
                 how_it_showed TEXT NOT NULL,
                 what_would_prevent TEXT NOT NULL,
                 status TEXT NOT NULL
             );",
        )?;
        connection.pragma_update(None, "user_version", FAULTS_SCHEMA_VERSION)?;
        Ok(Faults { connection, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Registra un guasto e **gli assegna il numero**.
    ///
    /// Il numero è `MAX(number) + 1` calcolato dentro la stessa istruzione che
    /// inserisce: due sessioni che registrano nello stesso momento ne prendono
    /// due diversi, perché a decidere è il deposito e non chi scrive. È l'unico
    /// modo di chiudere il guasto 42 — una prova non può, perché guarda un ramo
    /// alla volta e i rami non si vedono.
    pub fn record(&self, draft: &Draft) -> Result<Fault, FaultError> {
        self.connection.execute(
            "INSERT INTO faults
                 (number, happened_on, what_happened, how_it_showed, what_would_prevent, status)
             VALUES (
                 (SELECT COALESCE(MAX(number), 0) + 1 FROM faults),
                 ?1, ?2, ?3, ?4, ?5
             )",
            params![
                draft.happened_on,
                draft.what_happened,
                draft.how_it_showed,
                draft.what_would_prevent,
                draft.status,
            ],
        )?;
        let number = self.connection.last_insert_rowid();
        self.get(number)
    }

    /// Rimette un guasto **col suo numero**: serve solo alla migrazione dal
    /// markdown, dove i numeri esistevano già e cambiarli spezzerebbe i rinvii
    /// che altri documenti e altri commenti fanno a quelli.
    pub fn restore(&self, fault: &Fault) -> Result<(), FaultError> {
        self.connection.execute(
            "INSERT OR REPLACE INTO faults
                 (number, happened_on, what_happened, how_it_showed, what_would_prevent, status)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                fault.number,
                fault.happened_on,
                fault.what_happened,
                fault.how_it_showed,
                fault.what_would_prevent,
                fault.status,
            ],
        )?;
        Ok(())
    }

    pub fn get(&self, number: i64) -> Result<Fault, FaultError> {
        self.all()?
            .into_iter()
            .find(|fault| fault.number == number)
            .ok_or(FaultError::Unknown(number))
    }

    pub fn all(&self) -> Result<Vec<Fault>, FaultError> {
        let mut statement = self.connection.prepare(
            "SELECT number, happened_on, what_happened, how_it_showed, what_would_prevent, status
             FROM faults ORDER BY number",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Fault {
                number: row.get(0)?,
                happened_on: row.get(1)?,
                what_happened: row.get(2)?,
                how_it_showed: row.get(3)?,
                what_would_prevent: row.get(4)?,
                status: row.get(5)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    /// Cambia lo stato di un guasto, che è l'unica cosa che cambia dopo.
    pub fn set_status(&self, number: i64, status: &str) -> Result<Fault, FaultError> {
        let touched = self.connection.execute(
            "UPDATE faults SET status = ?2 WHERE number = ?1",
            params![number, status],
        )?;
        if touched == 0 {
            return Err(FaultError::Unknown(number));
        }
        self.get(number)
    }

    /// Quanti restano aperti, **contati** e non ricopiati. Erano sbagliati in
    /// quattro documenti su quattro il 31/08/2026, ed è il difetto per cui la
    /// prova sui conti in prosa era stata scritta: qui non serve più, perché non
    /// esiste più un secondo posto dove scriverlo.
    pub fn still_open(&self) -> Result<usize, FaultError> {
        Ok(self.all()?.iter().filter(|f| f.still_open()).count())
    }
}

// ── Il markdown: una resa, e una porta d'ingresso una volta sola ──────────

/// Legge la tabella di `docs/guasti-incontrati.md`.
///
/// **ESISTE PER LA MIGRAZIONE, E POI PER SMENTIRLA.** Serve a portare dentro le
/// righe che c'erano; e serve alla prova che le riscrive e le confronta con
/// l'originale, che è l'unico modo di sapere che non se ne è persa nessuna. Una
/// migrazione che perde una riga e non lo dice è esattamente il genere di cosa
/// che questa tabella esiste per registrare.
pub fn parse(markdown: &str) -> Vec<Fault> {
    markdown
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            if !trimmed.starts_with('|') {
                return None;
            }
            let cells: Vec<&str> = trimmed.trim_matches('|').split(" | ").collect();
            if cells.len() != 6 {
                return None;
            }
            let number: i64 = cells[0].trim().parse().ok()?;
            Some(Fault {
                number,
                happened_on: cells[1].trim().to_owned(),
                what_happened: cells[2].trim().to_owned(),
                how_it_showed: cells[3].trim().to_owned(),
                what_would_prevent: cells[4].trim().to_owned(),
                status: cells[5].trim().to_owned(),
            })
        })
        .collect()
}

/// Riscrive le righe come le scriveva la tabella, per chi vuole leggerle così.
pub fn render(faults: &[Fault]) -> String {
    let mut out = String::new();
    for fault in faults {
        let [on, what, how, prevent, status, _] = fault.cells();
        out.push_str(&format!(
            "| {} | {on} | {what} | {how} | {prevent} | {status} |\n",
            fault.number
        ));
    }
    out
}
