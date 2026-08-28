//! Il formato del descrittore di un innesco e il suo caricamento.
//!
//! **PERCHÉ NON RIUSA IL CARICATORE DEGLI STRUMENTI.** `toolbox::Catalog` fa
//! gli stessi gesti — leggi in ordine, l'ultimo `id` vince, una riga sbagliata
//! diventa una segnalazione invece di far sparire le altre — ma su un altro
//! corpo. Renderlo generico sull'elemento è la cosa giusta da fare **quando ci
//! sarà un terzo elenco**: farlo con due, oggi, costerebbe un parametro di tipo
//! in ogni firma di quel crate per risparmiare cinquanta righe qui. Le regole
//! sono scritte due volte, e questa riga è il legame fra le due copie: chi ne
//! cambia una guardi l'altra.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// Gli inneschi che il prodotto si porta dietro, incorporati nel binario: non
/// c'è nessun percorso di installazione da indovinare, e restano dati — si
/// riscrivono per `id`, si spengono con `disabled`.
pub const BUILTIN: &str = include_str!("../descriptors/default.json");

pub const BUILTIN_SOURCE: &str = "incorporato";

/// Da dove si prendono i descrittori.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Builtin,
    File(PathBuf),
    /// Ogni `*.json` dentro una cartella, in ordine di nome.
    Dir(PathBuf),
}

/// La forma di una sorgente di segnale. **Due, e il codice non ne conosce
/// altre**: quale terminale, quale finestra, quale prodotto lo dicono i
/// descrittori.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Qualcuno preme e parte, portando un testo. Il segnale è già arrivato:
    /// non c'è niente da attendere, e per questo è l'unica forma che oggi
    /// funziona per davvero.
    Manual,
    /// Il segnale comparirebbe in una sessione di terminale. Oggi si dichiara
    /// e non si ascolta: vedi `action`.
    Terminal,
}

/// Dove si vedrebbe comparire un segnale in una sessione di terminale.
///
/// **LE DUE FORME SONO MISURATE, NON IMMAGINATE** (28/08/2026, su questa
/// macchina): o esiste un file che cresce e non viene mai riscritto, con una
/// riga per messaggio, e allora si legge tenendo il punto raggiunto; oppure
/// esiste un comando che, dato un cursore, stampa ciò che è comparso dopo, e
/// allora si chiama quello. Ciò che *non* è una sorgente onesta è un registro
/// di byte di terminale: sono ridisegni di schermo, non messaggi, e ricostruirne
/// il testo vuol dire riscrivere un emulatore di terminale.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Listen {
    /// Un file in sola aggiunta: una riga JSON per messaggio.
    AppendedLines {
        /// I file da seguire. Ammettono `~/`, `$VAR` e `*`.
        files: Vec<String>,
        /// Dove sta il testo del messaggio dentro una riga.
        text_pointer: Vec<String>,
        /// Dove sta chi l'ha scritto. Vuoto: la sorgente non lo sa.
        #[serde(default)]
        who_pointer: Vec<String>,
        /// Dove sta la sessione o il pannello di provenienza.
        #[serde(default)]
        where_pointer: Vec<String>,
    },
    /// Un comando che stampa ciò che è comparso dopo un cursore.
    CursorCommand {
        /// L'identificativo dello strumento da invocare, non un binario: lo
        /// stesso elenco che risolve i motori dei passi.
        tool: String,
        args: Vec<String>,
        /// L'argomento in cui va scritto il cursore raggiunto.
        cursor_argument: String,
    },
}

/// Una riga dell'elenco delle sorgenti di segnale.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct TriggerDescriptor {
    pub id: String,
    pub kind: Kind,
    #[serde(default)]
    pub label: String,
    /// Come si vedrebbe arrivare il segnale. Obbligatorio per un terminale,
    /// vietato per un innesco manuale: un manuale che dichiara dove ascoltare
    /// sta descrivendo due sorgenti diverse col nome di una.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub listen: Option<Listen>,
    /// Per chi legge l'elenco: cosa si è misurato, cosa manca. Non entra in
    /// nessuna decisione.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub descriptor: TriggerDescriptor,
    pub source: String,
}

/// Qualcosa che non si è potuto caricare, col perché e col dove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub source: String,
    pub about: String,
    pub reason: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub descriptors: Vec<Loaded>,
    pub problems: Vec<Problem>,
}

impl Catalog {
    pub fn load(sources: &[Source]) -> Catalog {
        let mut catalog = Catalog::default();
        for source in sources {
            match source {
                Source::Builtin => catalog.absorb(BUILTIN_SOURCE, BUILTIN),
                Source::File(path) => catalog.absorb_file(path),
                Source::Dir(dir) => {
                    let Ok(entries) = fs::read_dir(dir) else {
                        // Una cartella che non c'è è il caso normale di chi non
                        // ha mai aggiunto un innesco suo; una che c'è e non si
                        // legge è un guasto, e si distinguono guardando il disco.
                        if dir.exists() {
                            catalog.problems.push(Problem {
                                source: dir.to_string_lossy().into_owned(),
                                about: "la cartella".to_string(),
                                reason: "non si è potuta leggere".to_string(),
                            });
                        }
                        continue;
                    };
                    let mut files: Vec<PathBuf> = entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| path.extension().is_some_and(|end| end == "json"))
                        .collect();
                    files.sort();
                    for file in files {
                        catalog.absorb_file(&file);
                    }
                }
            }
        }
        catalog
    }

    fn absorb_file(&mut self, path: &Path) {
        let label = path.to_string_lossy().into_owned();
        match fs::read_to_string(path) {
            Ok(text) => self.absorb(&label, &text),
            Err(error) => self.problems.push(Problem {
                source: label,
                about: "il file".to_string(),
                reason: format!("non si è potuto leggere: {error}"),
            }),
        }
    }

    /// Il testo si legge due volte di proposito: elemento per elemento, così
    /// una virgola sbagliata in fondo non cancella gli inneschi buoni sopra.
    fn absorb(&mut self, source: &str, text: &str) {
        let value: Value = match serde_json::from_str(text) {
            Ok(value) => value,
            Err(error) => {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about: "il file".to_string(),
                    reason: format!("non è JSON valido: {error}"),
                });
                return;
            }
        };
        let items = match &value {
            Value::Array(items) => items.clone(),
            Value::Object(map) => match map.get("triggers") {
                Some(Value::Array(items)) => items.clone(),
                _ => {
                    self.malformed(source);
                    return;
                }
            },
            _ => {
                self.malformed(source);
                return;
            }
        };
        for (index, item) in items.iter().enumerate() {
            let about = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| format!("la voce numero {}", index + 1));
            let descriptor: TriggerDescriptor = match serde_json::from_value(item.clone()) {
                Ok(descriptor) => descriptor,
                Err(error) => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            if let Err(reason) = coherent(&descriptor) {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason,
                });
                continue;
            }
            self.replace(Loaded {
                descriptor,
                source: source.to_string(),
            });
        }
    }

    fn malformed(&mut self, source: &str) {
        self.problems.push(Problem {
            source: source.to_string(),
            about: "il file".to_string(),
            reason: "non contiene né un array né un campo `triggers`".to_string(),
        });
    }

    fn replace(&mut self, loaded: Loaded) {
        match self
            .descriptors
            .iter_mut()
            .find(|found| found.descriptor.id == loaded.descriptor.id)
        {
            Some(existing) => *existing = loaded,
            None => self.descriptors.push(loaded),
        }
    }

    /// Quelli accesi, in ordine stabile per `id`: due letture di seguito devono
    /// dare la stessa sequenza, o l'elenco mostrato non si può confrontare.
    pub fn live(&self) -> Vec<&Loaded> {
        let mut out: Vec<&Loaded> = self
            .descriptors
            .iter()
            .filter(|loaded| !loaded.descriptor.disabled)
            .collect();
        out.sort_by(|left, right| left.descriptor.id.cmp(&right.descriptor.id));
        out
    }

    pub fn find(&self, id: &str) -> Option<&Loaded> {
        self.live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == id)
    }

    /// Gli `id` accesi, per il messaggio di chi ne ha chiesto uno che non c'è:
    /// un errore che non dice quali esistono costringe a cercare il file.
    pub fn known(&self) -> Vec<String> {
        self.live()
            .into_iter()
            .map(|loaded| loaded.descriptor.id.clone())
            .collect()
    }
}

/// Un descrittore che dichiara una forma e ne descrive un'altra non si carica:
/// il giorno in cui qualcuno lo scrive è l'unico giorno in cui è facile
/// accorgersene.
fn coherent(descriptor: &TriggerDescriptor) -> Result<(), String> {
    match (descriptor.kind, descriptor.listen.is_some()) {
        (Kind::Manual, true) => Err(
            "un innesco manuale porta il segnale con sé: non può dichiarare anche dove ascoltare"
                .to_string(),
        ),
        (Kind::Terminal, false) => Err(
            "un innesco da terminale deve dire dove si vedrebbe comparire il segnale: manca `listen`"
                .to_string(),
        ),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_descriptors_all_load() {
        let catalog = Catalog::load(&[Source::Builtin]);
        assert!(
            catalog.problems.is_empty(),
            "i descrittori spediti non si leggono: {:?}",
            catalog.problems
        );
        assert!(!catalog.live().is_empty());
    }

    /// L'innesco manuale è quello su cui si regge la finestra: se sparisce dai
    /// descrittori spediti, il pulsante di lancio non ha più una sorgente.
    #[test]
    fn a_manual_source_is_shipped_with_the_product() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let manual = catalog
            .find("manual")
            .expect("l'innesco manuale è spedito col prodotto");
        assert_eq!(manual.descriptor.kind, Kind::Manual);
    }

    #[test]
    fn a_terminal_source_without_a_place_to_listen_is_refused() {
        let mut catalog = Catalog::default();
        catalog.absorb("prova", r#"[{"id": "vuoto", "kind": "terminal"}]"#);
        assert!(catalog.descriptors.is_empty());
        assert_eq!(catalog.problems.len(), 1);
        assert!(catalog.problems[0].reason.contains("listen"));
    }

    #[test]
    fn a_manual_source_that_also_listens_is_refused() {
        let mut catalog = Catalog::default();
        catalog.absorb(
            "prova",
            r#"[{"id": "confuso", "kind": "manual",
                 "listen": {"kind": "appended_lines", "files": ["~/x.jsonl"],
                            "text_pointer": ["testo"]}}]"#,
        );
        assert!(catalog.descriptors.is_empty());
        assert_eq!(catalog.problems.len(), 1);
    }

    /// Una riga sbagliata non cancella quelle buone: senza questa regola un
    /// elenco parziale sembrerebbe vuoto, che è peggio.
    #[test]
    fn a_broken_entry_does_not_take_the_good_ones_with_it() {
        let mut catalog = Catalog::default();
        catalog.absorb(
            "prova",
            r#"[{"id": "buono", "kind": "manual"},
                {"id": "rotto", "kind": "inventato"}]"#,
        );
        assert_eq!(catalog.live().len(), 1);
        assert_eq!(catalog.problems.len(), 1);
        assert_eq!(catalog.problems[0].about, "rotto");
    }

    /// Lo stesso `id` scritto due volte: vince l'ultimo caricato, ed è così che
    /// un utente riscrive un innesco spedito senza cancellarlo.
    #[test]
    fn the_last_descriptor_with_an_id_wins() {
        let mut catalog = Catalog::default();
        catalog.absorb("spedito", r#"[{"id": "x", "kind": "manual", "label": "primo"}]"#);
        catalog.absorb("mio", r#"[{"id": "x", "kind": "manual", "label": "secondo"}]"#);
        assert_eq!(catalog.live().len(), 1);
        assert_eq!(catalog.live()[0].descriptor.label, "secondo");
        assert_eq!(catalog.live()[0].source, "mio");
    }

    #[test]
    fn a_disabled_descriptor_disappears_from_the_live_list() {
        let mut catalog = Catalog::default();
        catalog.absorb("spedito", r#"[{"id": "x", "kind": "manual"}]"#);
        catalog.absorb("mio", r#"[{"id": "x", "kind": "manual", "disabled": true}]"#);
        assert!(catalog.live().is_empty());
        assert!(catalog.known().is_empty());
    }
}
