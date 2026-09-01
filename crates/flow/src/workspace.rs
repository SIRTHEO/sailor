//! Dove sta la radice del progetto, e cosa il progetto dichiara di sé.
//!
//! **PERCHÉ ESISTE.** Il guasto 25: `flows/sviluppa-sailor.flow.json` aveva la
//! casa di chi scriveva come `"workdir"`, in chiaro su sette passi.
//! Lanciato da un clone lavorava — e commetteva — nel repository principale,
//! senza dire niente. Un flusso non deve sapere dove sta il repository: la
//! radice viene da chi lancia, e un percorso assoluto dentro un flusso è un
//! flusso che si può eseguire in un posto solo.
//!
//! **PERCHÉ QUI E NON IN UN CRATE NUOVO.** La risalita gemella — quella che
//! cerca una cartella `flows/` — sta in `flow::system`, e le due rispondono
//! alla stessa domanda: «di quale progetto sto parlando». Tenerle a due passi
//! l'una dall'altra è ciò che impedisce che diventino due risposte diverse,
//! che è esattamente il guasto 19. `actions`, `registry`, `sailor` e `ui`
//! dipendono già da `flow`: nessuno deve aggiungere una dipendenza per usarle.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Il file che dichiara «qui comincia un progetto Sailor».
///
/// **È UN MARCATORE, NON UNA CONFIGURAZIONE OBBLIGATORIA.** Può essere `{}`:
/// ciò che conta è che esista, perché è la sua posizione a rispondere alla
/// domanda. Il contenuto serve a chi vuole dichiarare qualcosa in più.
pub const MARKER: &str = "sailor.json";

/// L'origine dei flussi di un progetto che si è dichiarato con [`MARKER`].
pub const ORIGIN_DECLARED: &str = "del progetto";

/// L'origine dei flussi di un progetto **indovinato** risalendo per una
/// cartella `flows/`, senza nessun marcatore.
///
/// **L'AVVISO STA NELL'ORIGINE, E NON È PIGRIZIA.** L'origine è già stampata su
/// ogni riga da `sailor flow list` e mostrata accanto a ogni sorgente dalla
/// finestra: è il solo posto che chi guarda legge davvero, e ce n'è **uno**.
/// Un avviso scritto altrove sarebbe una seconda verità da tenere allineata a
/// questa — il guasto 10 — e comparirebbe una volta sola, mentre questo resta
/// sotto gli occhi finché il progetto non si dichiara.
pub const ORIGIN_GUESSED: &str = "del progetto (nessun sailor.json: radice indovinata)";

/// Ciò che un progetto dichiara di sé nel proprio [`MARKER`].
///
/// **I CAMPI IGNOTI SI TENGONO, NON FANNO SCARTARE IL FILE.** È il guasto 8:
/// un descrittore con un campo che questa versione non conosce veniva scartato
/// intero. `deny_unknown_fields` qui vorrebbe dire che un progetto aperto con
/// un Sailor più vecchio di quello che l'ha scritto smette di funzionare — e
/// smette in silenzio, perché la radice sparirebbe e i percorsi tornerebbero a
/// risolversi dove sta il processo. Ciò che non si conosce finisce in `extra`,
/// e chi vuole avvisare interroga [`Declaration::unknown_fields`].
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct Declaration {
    /// Come si chiama il progetto per chi legge. Vuoto vuol dire «non l'ha
    /// detto», e chi mostra userà il nome della cartella.
    #[serde(default)]
    pub name: String,
    /// I documenti che chi lavora qui deve leggere prima di toccare qualcosa.
    #[serde(default)]
    pub rules: Vec<String>,
    /// Le verifiche del progetto, per nome. **Resta vuoto finché qualcuno non
    /// lo riempie a mano**: indovinare `cargo test` per un progetto qualunque
    /// è la stessa presunzione del percorso assoluto che il guasto 25 racconta.
    #[serde(default)]
    pub checks: BTreeMap<String, String>,
    /// Dove sta la dotazione propria del progetto, se ne ha una.
    #[serde(default)]
    pub equipment: Option<String>,
    /// Ciò che questa versione non riconosce, tenuto invece che rifiutato.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Declaration {
    /// I nomi dei campi che questa versione non conosce.
    ///
    /// Chi mostra ne fa un avviso **sul campo**, mai il rifiuto del file: è la
    /// forma che il guasto 8 ha lasciato scritta.
    pub fn unknown_fields(&self) -> Vec<String> {
        self.extra.keys().cloned().collect()
    }
}

/// La radice del progetto: la prima cartella che, risalendo da `from`, contiene
/// un [`MARKER`].
///
/// **SI RISALE PER LA STESSA RAGIONE DI `project_flows_from`.** Un programma non
/// viene quasi mai avviato dalla radice: la finestra parte da
/// `desktop/src-tauri`, un terminale da dove si trovava chi lo ha aperto.
/// Guardare solo la cartella corrente vorrebbe dire non trovare niente quasi
/// sempre.
///
/// **`None` NON È «LA CARTELLA CORRENTE».** Sono due risposte opposte: `None`
/// vuol dire che nessuno ha dichiarato una radice, e chi ha bisogno di una
/// radice deve fallire dicendolo. Rispondere con la cartella corrente
/// rimetterebbe in piedi il guasto 25 da un'altra porta — un flusso che lavora
/// dove capita, in silenzio.
pub fn find_root(from: &Path) -> Option<PathBuf> {
    let mut here = Some(from);
    while let Some(directory) = here {
        if directory.join(MARKER).is_file() {
            return Some(directory.to_path_buf());
        }
        here = directory.parent();
    }
    None
}

/// Legge la dichiarazione di una radice.
///
/// Un marcatore vuoto o illeggibile non è un progetto rotto: `{}` è una
/// dichiarazione legittima, e il file che non si legge lascia comunque in piedi
/// la radice — è la sua **posizione** a rispondere alla domanda, non il suo
/// contenuto.
pub fn declaration_at(root: &Path) -> Result<Declaration, String> {
    let path = root.join(MARKER);
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("non riesco a leggere {}: {error}", path.display()))?;
    serde_json::from_str(&text)
        .map_err(|error| format!("{} non è una dichiarazione valida: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-workspace-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("cartella di prova");
        dir
    }

    /// Un contatore nel nome, non il solo orologio: è il guasto 21 — `cargo
    /// test` manda le prove sullo stesso processo e si rubavano la cartella.
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    fn put_marker(dir: &Path, text: &str) {
        fs::create_dir_all(dir).expect("cartella");
        fs::write(dir.join(MARKER), text).expect("marcatore");
    }

    /// Il caso di tutti i giorni: si lavora tre cartelle sotto la radice.
    #[test]
    fn the_root_is_the_folder_with_the_marker() {
        let root = scratch("risalita");
        put_marker(&root, "{}");
        let deep = root.join("crates").join("flow").join("src");
        fs::create_dir_all(&deep).expect("sottocartella");

        assert_eq!(find_root(&deep), Some(root.clone()));

        let _ = fs::remove_dir_all(&root);
    }

    /// **SENZA MARCATORE LA RISPOSTA È `None`, NON LA CARTELLA CORRENTE.**
    /// Rispondere con la cartella corrente farebbe lavorare un flusso dove
    /// capita senza dirlo, che è il guasto 25 rimesso in piedi.
    #[test]
    fn without_a_marker_there_is_no_root() {
        let orphan = scratch("senza-marcatore");
        let deep = orphan.join("una").join("due");
        fs::create_dir_all(&deep).expect("sottocartella");

        assert_eq!(find_root(&deep), None);

        let _ = fs::remove_dir_all(&orphan);
    }

    /// Il marcatore più vicino vince: un progetto dentro un progetto è suo.
    #[test]
    fn the_nearest_marker_wins() {
        let outer = scratch("annidati");
        put_marker(&outer, "{}");
        let inner = outer.join("dentro");
        put_marker(&inner, "{}");

        assert_eq!(find_root(&inner), Some(inner.clone()));

        let _ = fs::remove_dir_all(&outer);
    }

    /// IL GUASTO 8, SU QUESTO FILE. Un campo che questa versione non conosce
    /// non fa scartare la dichiarazione: resta in `extra` e si può avvisare.
    #[test]
    fn an_unknown_field_is_kept_not_refused() {
        let root = scratch("campo-ignoto");
        put_marker(
            &root,
            r#"{"name": "sailor", "rules": ["AGENTS.md"], "domani": 3}"#,
        );

        let declared = declaration_at(&root).expect("si legge lo stesso");

        assert_eq!(declared.name, "sailor");
        assert_eq!(declared.rules, vec!["AGENTS.md".to_owned()]);
        assert_eq!(declared.unknown_fields(), vec!["domani".to_owned()]);

        let _ = fs::remove_dir_all(&root);
    }

    /// Un marcatore vuoto è una dichiarazione legittima: conta la posizione.
    #[test]
    fn an_empty_marker_is_a_valid_declaration() {
        let root = scratch("vuoto");
        put_marker(&root, "{}");

        let declared = declaration_at(&root).expect("dichiarazione vuota");

        assert_eq!(declared, Declaration::default());
        assert!(declared.checks.is_empty(), "checks non si indovina");

        let _ = fs::remove_dir_all(&root);
    }
}
