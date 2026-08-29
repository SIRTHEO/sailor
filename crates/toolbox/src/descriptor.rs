//! Il formato del descrittore e il suo caricamento.
//!
//! UN DESCRITTORE È UN DATO, NON UN RAMO DI CODICE. Qui non compare il nome di
//! nessuno strumento: il codice sa eseguire *una forma* di verifica, mai una
//! verifica in particolare. Aggiungere una riga di comando — la CLI di
//! OpenRouter, quella di domani — è scrivere un oggetto JSON, non ricompilare.
//!
//! UN FILE ROTTO NON DEVE FAR CADERE IL RILEVAMENTO. Ogni elemento si legge da
//! solo: quello che non si legge diventa una segnalazione con dentro il perché,
//! e gli altri passano. Un inventario che tace perché una riga era sbagliata è
//! peggio di un inventario incompleto, perché sembra vuoto invece che parziale.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

/// I descrittori che il prodotto si porta dietro.
///
/// Sono incorporati nel binario, non cercati in una cartella di installazione:
/// un binario copiato altrove continua a rispondere, e non c'è nessun percorso
/// da indovinare. Restano comunque dati — chi non li vuole li spegne per `id`,
/// chi li vuole diversi li riscrive per `id`, senza toccare il codice.
pub const BUILTIN: &str = include_str!("../descriptors/default.json");

/// I cataloghi spediti col prodotto, per nome.
///
/// **PERCHÉ PIÙ DI UNO, DAL 29/08/2026.** Un descrittore risponde a «questa cosa
/// c'è?» e a «quali voci dichiara questo file?»: sono due domande buone per
/// molte cose diverse, non solo per gli strumenti che un passo può invocare. La
/// migrazione a Sailor — trovare i ganci, i servizi e gli script che una persona
/// ha già — è la stessa domanda posta su altri percorsi, e riscriverne il
/// meccanismo sarebbe una seconda copia che diverge dalla prima.
///
/// **RESTANO SEPARATI, E NON È PIGNOLERIA.** Un'automazione altrui non è uno
/// strumento che un passo può invocare: se stesse nello stesso catalogo, il suo
/// `id` comparirebbe fra quelli che [`crate::Tools::resolve`] offre a chi ha
/// scritto un nome sbagliato, e un passo potrebbe perfino nominarla. Il catalogo
/// è l'unità di separazione, e chi vuole un catalogo lo chiede per nome.
pub const BUILTIN_CATALOGS: &[(&str, &str)] = &[
    ("tools", BUILTIN),
    ("automations", include_str!("../descriptors/automations.json")),
];

/// Come si dice a chi legge da dove viene un descrittore.
pub const BUILTIN_SOURCE: &str = "incorporato";

/// Da dove si prendono i descrittori.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// Quelli spediti col prodotto: il catalogo `tools`.
    Builtin,
    /// Un altro catalogo spedito col prodotto, per nome. Un nome che nessun
    /// catalogo porta diventa una segnalazione: sbagliarlo in un flusso darebbe
    /// altrimenti un elenco vuoto indistinguibile da «qui non c'è niente».
    BuiltinNamed(String),
    /// Un singolo file JSON.
    File(PathBuf),
    /// Ogni `*.json` dentro una cartella, in ordine di nome.
    Dir(PathBuf),
}

/// Come si verifica che una cosa ci sia.
///
/// Due forme, e bastano entrambe: una riga di comando si riconosce da un
/// eseguibile raggiungibile, un server MCP spesso non ha un eseguibile suo — lo
/// avvia chi lo ospita — e si riconosce dal file che lo dichiara.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Probe {
    /// Il nome di un eseguibile da cercare nelle cartelle del percorso.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Un percorso che deve esistere. Ammette `~/`, `$VAR` e `*`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Una o più sonde. Un JSON con un oggetto solo si scrive senza le parentesi
/// quadre: chi aggiunge uno strumento nel caso semplice non deve conoscere il
/// caso complicato.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Probes {
    One(Probe),
    Many(Vec<Probe>),
}

impl Probes {
    pub fn as_slice(&self) -> &[Probe] {
        match self {
            Probes::One(probe) => std::slice::from_ref(probe),
            Probes::Many(probes) => probes,
        }
    }
}

/// Come si chiede la versione: gli argomenti da passare all'eseguibile trovato.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VersionProbe {
    pub args: Vec<String>,
    /// Il tetto di tempo. Un binario che si mette ad aspettare qualcosa
    /// sull'ingresso non deve poter fermare il rilevamento degli altri.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
    /// La riga giusta è quella che contiene questo testo.
    ///
    /// SERVE, E SI È VISTO SU QUESTA MACCHINA: `ollama --version` stampa prima
    /// un avvertimento su un servizio non raggiungibile, e prendere la prima
    /// riga registrava quell'avvertimento come se fosse un numero di versione.
    /// Il rimedio sta nel dato — chi scrive il descrittore sa che forma ha la
    /// risposta del suo binario — non in un ramo di codice per quel binario.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub must_contain: String,
}

fn default_timeout() -> u64 {
    10
}

/// Come si fa una domanda secca a un motore, e come quel motore dice di non
/// poter lavorare.
///
/// **PERCHÉ STA NEL DESCRITTORE E NON NEL FLUSSO.** Finché `-p` per uno e
/// `--mode plan --print` per un altro stanno scritti dentro i passi, un flusso
/// è legato al motore per cui è stato scritto, e «indipendente dal modello»
/// resta una frase. Qui la differenza fra due motori è un dato, e un motore che
/// non esiste ancora si aggiunge scrivendo un descrittore — senza ricompilare
/// niente, e senza che nessun flusso cambi.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Ask {
    /// Le opzioni che vogliono una risposta sola e non una conversazione,
    /// **senza** il testo della domanda.
    #[serde(default)]
    pub args: Vec<String>,
    /// Dove va il testo della domanda: `stdin` o `last_arg`.
    #[serde(default)]
    pub prompt: PromptPlace,
    /// Come questo motore dice di **non poter lavorare** — quota finita,
    /// credenziali mancanti — invece di dire che il lavoro era sbagliato.
    ///
    /// È ciò che permette a un passo con una catena di motori di passare al
    /// successivo. Chi non lo dichiara non fa scattare nessun ripiego: si
    /// funziona peggio, mai in silenzio. E si dichiarano **le parole del
    /// fornitore**, non una regola generale: «errore» combacerebbe con
    /// qualunque fallimento e manderebbe un mandato sbagliato giù per tutta la
    /// catena finché qualcuno non risponde comunque.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unusable_when: Vec<String>,
}

/// Dove va il testo della domanda.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptPlace {
    /// Sull'ingresso standard. È il caso più comune, e il valore predefinito.
    #[default]
    Stdin,
    /// Come ultimo argomento della riga di comando. Misurato su `agy` il
    /// 28/08/2026: il prompt va in un argomento, e le opzioni vanno prima.
    LastArg,
}

/// Un descrittore che, invece di dire «questa cosa o c'è o non c'è», **scopre**
/// più voci leggendo un file di configurazione.
///
/// PERCHÉ ESISTE, E PERCHÉ NON È UN CASO SPECIALE PER I SERVER MCP. Elencare a
/// mano i server MCP di una macchina sarebbe l'elenco cablato che questo crate
/// esiste per evitare: cambiano quando l'utente ne aggiunge uno, e nessuno
/// ricompila per questo. Il descrittore dice *dove guardare* e *sotto quale
/// chiave*; il codice apre il file e riporta le chiavi che ci trova, senza
/// sapere che cosa siano. Lo stesso meccanismo elenca i profili di un altro
/// strumento il giorno che qualcuno scrive quel descrittore.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct JsonKeys {
    /// I file da leggere. Ammettono `~/`, `$VAR` e `*`.
    pub files: Vec<String>,
    /// Il cammino fino all'oggetto le cui chiavi sono le voci. Un `*` sta per
    /// «tutte le chiavi di questo livello»: `["projects", "*", "mcpServers"]`
    /// raccoglie i server dichiarati progetto per progetto.
    pub pointer: Vec<String>,
}

/// Come si scoprono più voci invece di rispondere «c'è o non c'è».
///
/// Due forme, e la seconda è nata col catalogo delle automazioni: le chiavi di
/// un file JSON dicono quali ganci una riga di comando dichiara, ma un servizio
/// del sistema operativo è **un file per servizio** in una cartella, e sapere
/// che la cartella non è vuota non serve a niente a chi deve decidere cosa
/// migrare. Chi legge vuole i nomi.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Enumerate {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json_keys: Option<JsonKeys>,
    /// Gli schemi di percorso i cui file esistenti sono le voci. Ammettono
    /// `~/`, `$VAR` e `*`. Una voce è il percorso stesso, per intero: due file
    /// con lo stesso nome in due cartelle diverse sono due automazioni diverse,
    /// e chiamarle allo stesso modo le farebbe contare per una.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paths: Option<Vec<String>>,
}

impl Enumerate {
    /// Vero se non dice in nessun modo dove guardare.
    pub fn is_empty(&self) -> bool {
        self.json_keys.is_none() && self.paths.is_none()
    }
}

/// Una riga dell'elenco di cosa cercare.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    /// L'identità della riga. Due descrittori con lo stesso `id` non
    /// convivono: l'ultimo caricato vince, ed è così che un utente riscrive un
    /// descrittore spedito senza doverlo cancellare.
    pub id: String,
    /// A quale famiglia appartiene: `ai_cli`, `mcp_server`, `tool`, o qualunque
    /// altra parola. Il codice non ne conosce nessuna — la usa solo per
    /// raggruppare e filtrare, e un nome nuovo funziona il giorno che qualcuno
    /// lo scrive.
    pub family: String,
    /// Come si chiama per chi legge.
    #[serde(default)]
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detect: Option<Probes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enumerate: Option<Enumerate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionProbe>,
    /// Come gli si fa una domanda secca, per chi ne accetta una. Senza questo,
    /// un passo che lo vuole usare deve scrivere da sé le opzioni.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<Ask>,
    /// Dove vive la sua configurazione. Ammette `~/`, `$VAR` e `*`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub config: Vec<String>,
    /// Una nota per chi legge l'elenco: da dove si installa, come si chiama il
    /// pacchetto. Non entra in nessuna decisione.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// Spegne un descrittore senza cancellarlo. È il modo per togliere di mezzo
    /// uno di quelli spediti: si riscrive il suo `id` con `disabled: true`.
    #[serde(default)]
    pub disabled: bool,
}

/// Un descrittore caricato, con da dove viene: chi legge il risultato deve
/// poter risalire al file che lo ha prodotto, o «da quale descrittore» non è una
/// risposta verificabile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub descriptor: Descriptor,
    pub source: String,
}

/// Qualcosa che non si è potuto caricare, col perché e col dove.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Problem {
    pub source: String,
    /// L'`id` se si è riusciti a leggerlo, altrimenti la posizione nel file.
    pub about: String,
    pub reason: String,
}

/// L'elenco di cosa cercare, più le righe che non si sono lette.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalog {
    pub descriptors: Vec<Loaded>,
    pub problems: Vec<Problem>,
}

/// Il testo di un catalogo spedito, per nome.
pub fn builtin_catalog(name: &str) -> Option<&'static str> {
    BUILTIN_CATALOGS
        .iter()
        .find(|(catalog, _)| *catalog == name)
        .map(|(_, text)| *text)
}

impl Catalog {
    /// Carica in ordine: chi arriva dopo vince sull'`id` di chi c'era.
    pub fn load(sources: &[Source]) -> Catalog {
        let mut catalog = Catalog::default();
        for source in sources {
            match source {
                Source::Builtin => catalog.absorb(BUILTIN_SOURCE, BUILTIN),
                Source::BuiltinNamed(name) => match builtin_catalog(name) {
                    Some(text) => catalog.absorb(&format!("{BUILTIN_SOURCE}:{name}"), text),
                    None => catalog.problems.push(Problem {
                        source: BUILTIN_SOURCE.to_string(),
                        about: name.clone(),
                        reason: format!(
                            "nessun catalogo spedito si chiama così; quelli spediti sono: {}",
                            BUILTIN_CATALOGS
                                .iter()
                                .map(|(name, _)| *name)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                    }),
                },
                Source::File(path) => catalog.absorb_file(path),
                Source::Dir(dir) => {
                    let Ok(entries) = fs::read_dir(dir) else {
                        // Una cartella che non c'è non è un guasto: è il caso
                        // normale di chi non ha mai aggiunto un descrittore suo.
                        // Una cartella che c'è ma non si legge lo è, e lo si
                        // distingue guardando il disco, non l'errore.
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
                        .map(|e| e.path())
                        .filter(|p| p.extension().map(|e| e == "json").unwrap_or(false))
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

    /// IL TESTO SI LEGGE DUE VOLTE, DI PROPOSITO. Prima come JSON generico, poi
    /// elemento per elemento: leggere l'array intero come `Vec<Descriptor>`
    /// farebbe cadere venti descrittori buoni per una virgola sbagliata nel
    /// ventunesimo, e la segnalazione non direbbe nemmeno quale.
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
        // Un array nudo o `{"tools": [...]}`: chi aggiunge uno strumento scrive
        // la forma che gli viene, e nessuna delle due è sbagliata.
        let items = match &value {
            Value::Array(items) => items.clone(),
            Value::Object(map) => match map.get("tools") {
                Some(Value::Array(items)) => items.clone(),
                _ => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about: "il file".to_string(),
                        reason: "non contiene né un array né un campo `tools`".to_string(),
                    });
                    return;
                }
            },
            _ => {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about: "il file".to_string(),
                    reason: "non contiene né un array né un campo `tools`".to_string(),
                });
                return;
            }
        };
        for (index, item) in items.iter().enumerate() {
            let about = item
                .get("id")
                .and_then(|v| v.as_str())
                .map(|id| id.to_string())
                .unwrap_or_else(|| format!("la voce numero {}", index + 1));
            let descriptor: Descriptor = match serde_json::from_value(item.clone()) {
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
            if descriptor.detect.is_none() && descriptor.enumerate.is_none() {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason: "non dice come si verifica: manca `detect` e manca `enumerate`"
                        .to_string(),
                });
                continue;
            }
            // UN `enumerate` VUOTO NON SCOPRE NIENTE, e senza questa riga
            // risponderebbe «nessuna voce» — che si legge come «qui non c'è
            // niente» invece che come «l'elenco è scritto male».
            if descriptor
                .enumerate
                .as_ref()
                .is_some_and(|enumerate| enumerate.is_empty())
            {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason: "`enumerate` non dice dove guardare: manca `json_keys` e manca `paths`"
                        .to_string(),
                });
                continue;
            }
            self.replace(Loaded {
                descriptor,
                source: source.to_string(),
            });
        }
    }

    fn replace(&mut self, loaded: Loaded) {
        match self
            .descriptors
            .iter_mut()
            .find(|l| l.descriptor.id == loaded.descriptor.id)
        {
            Some(existing) => *existing = loaded,
            None => self.descriptors.push(loaded),
        }
    }

    /// Quelli da eseguire: senza gli spenti, in ordine stabile per `id`, perché
    /// due letture di seguito devono dare la stessa sequenza o il confronto fra
    /// un giorno e l'altro non vale niente.
    pub fn live(&self) -> Vec<&Loaded> {
        let mut out: Vec<&Loaded> = self
            .descriptors
            .iter()
            .filter(|l| !l.descriptor.disabled)
            .collect();
        out.sort_by(|a, b| {
            (&a.descriptor.family, &a.descriptor.id).cmp(&(&b.descriptor.family, &b.descriptor.id))
        });
        out
    }
}
