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
use std::collections::BTreeMap;
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
pub struct Ask {
    /// Le opzioni che vogliono una risposta sola e non una conversazione,
    /// **senza** il testo della domanda.
    #[serde(default)]
    pub args: Vec<String>,
    /// Dove va il testo della domanda: `stdin` o `last_arg`.
    #[serde(default)]
    pub prompt: PromptPlace,
    /// Le opzioni che devono stare **subito prima della domanda**, dopo tutto
    /// il resto — comprese quelle che servono a farsi dire il consumo.
    ///
    /// **PERCHÉ NON BASTA `args`.** La riga si compone mettendo `args`, poi le
    /// opzioni di `usage`, poi la domanda. Per un motore che prende la domanda
    /// come ultimo argomento questo infila le opzioni del consumo **fra** la
    /// bandiera che introduce la domanda e la domanda stessa: `agy` risponde
    /// «--print took "--output-format" as its prompt», e il testo vero viene
    /// ignorato. È il guasto 1 ricomparso da un'altra porta — allora l'ordine
    /// sbagliato erano due opzioni di `ask` fra loro, oggi è una di `ask`
    /// contro una di `usage` — e non si cura riordinando `args`, perché il
    /// vincolo non è fra le opzioni di uno stesso blocco.
    ///
    /// Un ordine globale non lo risolve: `codex` ha `exec` come sottocomando e
    /// vuole stare **primo**, `agy` vuole `--print` **ultimo**. Sono due
    /// vincoli opposti sullo stesso posto, quindi li dichiara il descrittore.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args_before_prompt: Vec<String>,
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
    /// I campi non capiti, per la stessa ragione di `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Come si legge **quanto ha consumato** un motore, dichiarato dal descrittore.
///
/// **PERCHÉ STA QUI E NON NEL CODICE.** È la stessa ragione di `Ask`, applicata
/// al conto invece che alla domanda: finché «chiedi `--output-format json` e
/// guarda sotto la chiave `usage`» sta scritto in un ramo `if` per un
/// fornitore, misurare un motore nuovo vuol dire ricompilare Sailor. Qui la
/// differenza fra due motori è un dato, e chi ne aggiunge uno scrive un file.
///
/// **CHI NON LO DICHIARA NON PEGGIORA.** Il campo è facoltativo: un motore
/// senza `usage` si invoca esattamente come prima, produce la stessa uscita, e
/// lascia i propri token a **sconosciuto** — mai a zero. Uno zero scritto al
/// posto di «non lo so» è una bugia che nessuna vista a valle può correggere.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Usage {
    /// Le opzioni da aggiungere alla domanda per farsi dire il consumo — per
    /// esempio `["--output-format", "json"]`.
    ///
    /// Si accodano a quelle di `ask`, e **solo** quando è la ricetta del
    /// descrittore a dettare la riga di comando: un passo che scrive i propri
    /// argomenti sta dicendo qualcosa di preciso su *quella* chiamata, e
    /// allungargliela alle spalle sarebbe decidere al posto suo. In quel caso
    /// il consumo resta sconosciuto, che è il prezzo giusto da pagare.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    /// In che forma leggere l'uscita: `json` (i puntatori sono cammini di
    /// chiavi) o `text` (i puntatori sono espressioni regolari con un gruppo di
    /// cattura).
    #[serde(default)]
    pub read: ReadAs,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<Where>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<Where>,
    /// I token d'ingresso **letti dalla cache**. Vanno dichiarati a parte
    /// perché costano a parte, spesso un ordine di grandezza meno: un solo
    /// numero d'ingresso renderebbe la misura falsa proprio dove conta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<Where>,
    /// I token d'ingresso **scritti nella cache**. Non è la stessa cosa che
    /// leggerla: scrivere costa **più** dell'ingresso normale, leggere molto
    /// meno. Su una chiamata misurata il 30/08/2026 questa sola voce era il 96%
    /// della spesa — chi non la dichiara sbaglia il conto verso il basso.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_tokens: Option<Where>,
    /// I token scritti in una cache **a lunga durata**, dove esiste e ha un
    /// prezzo suo, più alto di quella breve.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write_long_tokens: Option<Where>,
    /// Il totale, per i motori che dicono solo quello senza separare i lati.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_tokens: Option<Where>,
    /// Dove il motore dichiara quanto ha fatto pagare. Si registra come
    /// confronto: il listino locale resta la fonte di verità.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Where>,
    /// Dove il motore nomina il modello che ha davvero servito la chiamata. È
    /// l'unico legame onesto fra una riga di comando e una voce di listino: chi
    /// non lo dichiara lascia il costo sconosciuto, e va bene così.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<Where>,
    /// Dove sta il testo della risposta dentro l'involucro.
    ///
    /// **VA DICHIARATO DA CHI CHIEDE UN INVOLUCRO.** Farsi dire i token in JSON
    /// avvolge anche la risposta: senza questo puntatore un passo a valle
    /// riceverebbe l'involucro al posto del testo, e un flusso che dichiara la
    /// forma della propria risposta diventerebbe rosso per una misura che non
    /// ha chiesto. Misurare non deve cambiare ciò che si misura.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Where>,
    /// I campi non capiti, per la stessa ragione di `Descriptor::extra`.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// In che forma un motore dice il proprio consumo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadAs {
    /// L'uscita è un involucro JSON.
    #[default]
    Json,
    /// L'uscita è testo in chiaro.
    Text,
}

/// Dove sta un valore dentro ciò che il motore ha detto: un cammino di chiavi
/// (`["usage", "input_tokens"]`) se si legge JSON, un'espressione regolare col
/// valore nel primo gruppo se si legge testo.
///
/// Un puntatore della forma sbagliata non trova niente e lascia il valore
/// sconosciuto. È il modo giusto di sbagliare: un descrittore impreciso
/// peggiora la misura, non rompe la chiamata che stava misurando, e non
/// inventa mai un numero al posto di quello che non ha trovato.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum Where {
    Path(Vec<String>),
    Pattern(String),
    /// `{"first_key_of": ["modelUsage"]}` — il valore è **il nome della prima
    /// chiave** dell'oggetto che sta a quel cammino.
    ///
    /// Serve ai motori che il modello non lo mettono in un campo ma lo usano
    /// come chiave. Senza questa forma quel nome è inarrivabile, e senza il nome
    /// nessuna voce di listino si trova: il costo resta sconosciuto anche
    /// avendo tutti i token e un listino giusto.
    FirstKeyOf { first_key_of: Vec<String> },
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
///
/// **NON RIFIUTA I CAMPI CHE NON CONOSCE, E PRIMA SÌ.** Fino al 31/08/2026
/// portava `deny_unknown_fields`: un descrittore scritto per una versione più
/// nuova di Sailor — o copiato da un esempio più recente — veniva **scartato
/// intero** per un campo in più, e con lui spariva lo strumento. È il guasto 8.
///
/// Adesso il campo di troppo finisce in `extra`, la voce vive, e chi carica
/// lascia una nota che lo nomina. `extra` è anche serializzato: chi rilegge e
/// riscrive un descrittore non perde i campi che questa versione non capisce —
/// il difetto opposto, e altrettanto silenzioso.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
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
    /// Come si legge quanto ha consumato. Facoltativo, e deve restarlo: un
    /// descrittore scritto prima che questo campo esistesse deve continuare a
    /// caricarsi identico, altrimenti una versione nuova di Sailor spegnerebbe
    /// gli strumenti dichiarati con la vecchia.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
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
    /// I campi che questa versione di Sailor non conosce.
    ///
    /// Vivono qui invece di far cadere la voce, e vengono riscritti tali e
    /// quali quando il descrittore si serializza. Chi carica li nomina in una
    /// nota: ignorare in silenzio sarebbe il guasto 20 su un altro oggetto.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Descriptor {
    /// I campi che non sono stati capiti, col percorso per trovarli.
    ///
    /// Guarda anche dentro `ask` e `usage`: un campo ignoto là sotto faceva
    /// cadere la voce esattamente come uno di primo livello, e chi legge la nota
    /// deve sapere **dove** cercarlo, non solo che c'è.
    pub fn unknown_fields(&self) -> Vec<String> {
        let mut found: Vec<String> = self.extra.keys().cloned().collect();
        if let Some(ask) = &self.ask {
            found.extend(ask.extra.keys().map(|key| format!("ask.{key}")));
        }
        if let Some(usage) = &self.usage {
            found.extend(usage.extra.keys().map(|key| format!("usage.{key}")));
        }
        found
    }
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
    /// Le voci **perse**: non sono nel catalogo, e qui c'è il perché.
    pub problems: Vec<Problem>,
    /// Le voci **tenute**, con qualcosa che è stato ignorato.
    ///
    /// **NON STANNO IN `problems`, E LA DIFFERENZA È TUTTA.** Un problema dice
    /// «questo strumento non c'è»; una nota dice «c'è, e di lui ho ignorato un
    /// campo». Metterle insieme farebbe contare come guasti delle voci che
    /// funzionano — e ci sono prove che contano `problems` una per una, che
    /// diventerebbero rosse per un descrittore perfettamente vivo.
    pub notes: Vec<Problem>,
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
            // **UN CAMPO IGNOTO È UNA NOTA, NON UN RIFIUTO.** Prima faceva
            // cadere la voce intera, e con lei spariva lo strumento: guasto 8.
            // La nota va in `notes` e non in `problems`, perché il descrittore
            // c'è ed è vivo — chi conta i problemi conta le voci perse.
            let unknown = descriptor.unknown_fields();
            if !unknown.is_empty() {
                self.notes.push(Problem {
                    source: source.to_string(),
                    about: about.clone(),
                    reason: format!(
                        "campi che questa versione non conosce, ignorati: {}",
                        unknown.join(", ")
                    ),
                });
            }
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

#[cfg(test)]
mod the_new_field_is_optional {
    //! Che cosa succede a un descrittore quando questa versione di Sailor
    //! impara un campo che la precedente non conosceva.

    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-descrittori-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("cartella di lavoro");
        dir
    }

    fn loaded(name: &str, text: &str) -> Catalog {
        let dir = scratch(name);
        let file = dir.join("descrittori.json");
        std::fs::write(&file, text).expect("scrivere i descrittori");
        Catalog::load(&[Source::File(file)])
    }

    /// **IL CRITERIO (d) DEL MANDATO.** Un descrittore scritto prima che `usage`
    /// esistesse si carica identico. Questo crate ha un guasto aperto e noto —
    /// un campo che questa versione non conosce scarta il descrittore intero —
    /// e il campo nuovo non deve peggiorarlo: chi non ce l'ha continua a
    /// funzionare, altrimenti una versione nuova di Sailor spegnerebbe in
    /// silenzio gli strumenti dichiarati con la vecchia.
    #[test]
    fn a_descriptor_written_before_usage_existed_still_loads() {
        let catalog = loaded(
            "senza-usage",
            r#"[{
              "id": "vecchio", "family": "ai_cli", "label": "Vecchio",
              "detect": { "command": "vecchio" },
              "ask": { "args": ["-p"], "prompt": "stdin", "unusable_when": ["quota"] }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        assert_eq!(catalog.descriptors.len(), 1);
        let descriptor = &catalog.descriptors[0].descriptor;
        assert!(descriptor.usage.is_none(), "assente vuol dire assente");
        assert!(descriptor.ask.is_some(), "e il resto arriva intatto");
    }

    /// Ogni descrittore spedito col prodotto si carica: se il campo nuovo
    /// rendesse illeggibile anche uno solo di loro, quello strumento sparirebbe
    /// dalla macchina di chiunque aggiorni.
    #[test]
    fn every_shipped_descriptor_still_loads() {
        let catalog = Catalog::load(&[Source::Builtin]);
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        assert!(catalog.descriptors.len() > 5);
    }

    /// Il blocco `usage` nella forma con i cammini di chiavi.
    #[test]
    fn a_json_usage_block_is_read_pointer_by_pointer() {
        let catalog = loaded(
            "usage-json",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "ask": { "args": ["-p"], "prompt": "stdin" },
              "usage": {
                "args": ["--output-format", "json"],
                "read": "json",
                "input_tokens": ["usage", "input_tokens"],
                "cached_tokens": ["usage", "cache_read_input_tokens"],
                "cost": ["total_cost_usd"],
                "model": ["model"],
                "answer": ["result"]
              }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        let usage = catalog.descriptors[0]
            .descriptor
            .usage
            .as_ref()
            .expect("il blocco c'è");
        assert_eq!(usage.args, vec!["--output-format", "json"]);
        assert_eq!(usage.read, ReadAs::Json);
        assert_eq!(
            usage.input_tokens,
            Some(Where::Path(vec!["usage".into(), "input_tokens".into()]))
        );
        assert_eq!(
            usage.cached_tokens,
            Some(Where::Path(vec![
                "usage".into(),
                "cache_read_input_tokens".into()
            ])),
            "la cache ha un puntatore suo, separato dall'ingresso"
        );
        assert_eq!(usage.answer, Some(Where::Path(vec!["result".into()])));
        assert_eq!(usage.output_tokens, None, "ciò che non è scritto non c'è");
    }

    /// La forma testuale: i puntatori sono espressioni regolari.
    #[test]
    fn a_text_usage_block_reads_its_pointers_as_patterns() {
        let catalog = loaded(
            "usage-text",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "usage": { "read": "text", "total_tokens": "tokens used\\s*\\n\\s*([\\d.,]+)" }
            }]"#,
        );
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        let usage = catalog.descriptors[0].descriptor.usage.as_ref().unwrap();
        assert_eq!(usage.read, ReadAs::Text);
        assert_eq!(
            usage.total_tokens,
            Some(Where::Pattern("tokens used\\s*\\n\\s*([\\d.,]+)".to_owned()))
        );
    }

    /// **UN CAMPO INVENTATO SI DICE, MA NON PORTA VIA LO STRUMENTO.**
    ///
    /// **QUESTA PROVA DICEVA IL CONTRARIO, E IL RIBALTAMENTO È DELIBERATO.**
    /// Prima pretendeva `descriptors.len() == 0`: un refuso dentro `usage`
    /// faceva cadere l'intero descrittore, e con lui spariva lo strumento — è il
    /// guasto 8. La ragione scritta allora era buona («non un silenzio che poi
    /// lascia il consumo sconosciuto senza dire perché») ed è **ancora
    /// rispettata**: il campo viene nominato. Quello che cambia è il prezzo —
    /// prima si perdeva il motore, adesso si perde solo il campo che nessuno
    /// sapeva leggere.
    #[test]
    fn an_invented_field_inside_usage_is_named_without_losing_the_tool() {
        let catalog = loaded(
            "usage-sbagliato",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "usage": { "read": "json", "token_di_ingresso": ["a"] }
            }]"#,
        );

        assert_eq!(catalog.descriptors.len(), 1, "lo strumento resta usabile");
        assert!(
            catalog.problems.is_empty(),
            "e non è una voce persa: {:?}",
            catalog.problems
        );
        assert_eq!(catalog.notes.len(), 1, "ma non è nemmeno un silenzio");
        assert!(
            catalog.notes[0].reason.contains("usage.token_di_ingresso"),
            "la nota dice quale campo e dove sta: {}",
            catalog.notes[0].reason
        );
    }

    /// **UN CAMPO INVENTATO AL PRIMO LIVELLO, STESSA REGOLA.** Il caso vero per
    /// cui il guasto 8 esiste: un descrittore scritto per una versione più nuova
    /// di Sailor, o copiato da un esempio più recente.
    #[test]
    fn a_descriptor_from_a_newer_version_still_loads() {
        let catalog = loaded(
            "dal-futuro",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "capabilities": { "vision": true }
            }]"#,
        );

        assert_eq!(catalog.descriptors.len(), 1);
        assert!(catalog.problems.is_empty(), "{:?}", catalog.problems);
        assert_eq!(catalog.notes.len(), 1);
        assert!(
            catalog.notes[0].reason.contains("capabilities"),
            "{}",
            catalog.notes[0].reason
        );
    }

    /// **E CIÒ CHE NON SI CAPISCE NON SI PERDE RISCRIVENDOLO.** Un descrittore
    /// riletto e riscritto da questa versione conserva i campi del futuro:
    /// perderli sarebbe il difetto opposto, e altrettanto silenzioso.
    #[test]
    fn what_this_version_does_not_understand_survives_a_round_trip() {
        let catalog = loaded(
            "andata-e-ritorno",
            r#"[{
              "id": "nuovo", "family": "ai_cli",
              "detect": { "command": "nuovo" },
              "capabilities": { "vision": true }
            }]"#,
        );

        let written = serde_json::to_value(&catalog.descriptors[0].descriptor)
            .expect("un descrittore in memoria si riscrive sempre");

        assert_eq!(
            written["capabilities"],
            serde_json::json!({"vision": true}),
            "il campo ignoto torna fuori com'era: {written}"
        );
    }

    /// Il descrittore di `codex` spedito col prodotto dichiara come si legge il
    /// suo consumo, e lo dichiara nella forma testuale: è l'unico formato che
    /// su questa macchina sia stato davvero misurato.
    #[test]
    fn the_shipped_codex_descriptor_declares_how_to_read_its_tokens() {
        let catalog = Catalog::load(&[Source::Builtin]);
        let codex = catalog
            .live()
            .into_iter()
            .find(|loaded| loaded.descriptor.id == "codex")
            .expect("codex è spedito col prodotto");
        let usage = codex
            .descriptor
            .usage
            .as_ref()
            .expect("codex dichiara il proprio consumo");
        assert_eq!(usage.read, ReadAs::Text);
        assert!(usage.total_tokens.is_some());
        assert!(
            usage.args.is_empty(),
            "codex scrive già i token da sé: chiedergli qualcosa in più \
             cambierebbe la sua riga di comando per niente"
        );
        assert!(
            usage.answer.is_none(),
            "nessun involucro chiesto, quindi niente da spacchettare: \
             l'uscita del passo resta quella di sempre"
        );
    }
}
