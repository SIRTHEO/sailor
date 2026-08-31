//! Lo smistamento: ciò che l'utente scrive viene guardato **prima** di essere
//! eseguito, e va al flusso o al terminale.
//!
//! **IL VALORE PREDEFINITO È IL TERMINALE, SEMPRE.** Lo smistamento è
//! un'aggiunta a un terminale che funziona, non un livello che il terminale
//! attraversa. Ogni via di uscita di [`Router::route`] che non sia una regola
//! che ha scattato porta a [`Routed::Command`]: nessuna regola, regola
//! sbagliata, nessun descrittore, elenco vuoto — passa. Un terminale che ogni
//! tanto non esegue quello che scrivi è peggio di uno che non smista affatto,
//! perché diventa imprevedibile, e l'imprevedibilità di un terminale si paga su
//! ogni riga che si scriverà dopo, non solo su quella.
//!
//! **COME SI DECIDE È UN DATO, NON UN RAMO DI CODICE.** In questo file non
//! compare il nome di nessun flusso e nessuna parola da cercare: le regole sono
//! descrittori — spediti col binario, riscrivibili in `~/.config/sailor/routes.d/`
//! — e cambiarle non ricompila niente. È il vincolo permanente «programmiamo a
//! codice solo ciò che tocca il mondo»: qui il codice è la guardia, che misura
//! la macchina; il resto è elenco.
//!
//! **LA GUARDIA È CODICE E NON SI PUÒ SPEGNERE DA UN DESCRITTORE.** Sta qui, e
//! il verso in cui pende è dichiarato: i dati possono solo chiedere di
//! smistare, la guardia può solo far passare. Una regola scritta male manca uno
//! smistamento; non si mangia un `git status`. L'unica eccezione è una regola
//! `explicit`, che l'utente ha marcato lui con un prefisso che nessuna shell
//! eseguirebbe — e il caricamento rifiuta le regole esplicite il cui prefisso
//! potrebbe iniziare un comando, così l'eccezione non si allarga da sola.
//!
//! **PERCHÉ `toolbox::probe::look_up` E NON UN `which` SCRITTO QUI.** Perché sa
//! rispondere «non so»: se anche una sola cartella del percorso non si è potuta
//! leggere, la risposta non è «non c'è». È esattamente la forma che serve alla
//! regola «nel dubbio, passa» — un dubbio sulla macchina diventa un comando
//! eseguito, mai una richiesta dirottata.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use toolbox::{Look, Machine};

/// Le regole spedite col prodotto, incorporate nel binario: non c'è nessun
/// percorso di installazione da indovinare, e restano dati.
pub const BUILTIN: &str = include_str!("../descriptors/default.json");

pub const BUILTIN_SOURCE: &str = "incorporato";

/// Da dove si prendono le regole. Stesse tre forme dei descrittori degli
/// strumenti e degli inneschi, e di proposito: chi ha imparato dove si aggiunge
/// una riga non deve impararlo una seconda volta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Builtin,
    File(PathBuf),
    /// Ogni `*.json` dentro una cartella, in ordine di nome.
    Dir(PathBuf),
}

/// Quale forma di riga una regola riconosce.
///
/// **DUE, E NESSUNA È UN'ESPRESSIONE REGOLARE.** Una regola di smistamento la
/// scrive chi usa Sailor, in un file JSON, senza provarla: un'espressione
/// regolare sbagliata è muta, cattura più di quanto crede, e il modo in cui si
/// accorge di aver sbagliato è un comando che non è stato eseguito. Queste due
/// forme si leggono a voce e non hanno angoli.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Match {
    /// La riga comincia con questo testo. Il confronto ignora le maiuscole.
    StartsWith { text: String },
    /// Tutte queste parole compaiono nella riga, come parole intere.
    ContainsAll { words: Vec<String> },
}

/// Una riga dell'elenco delle regole di smistamento.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub id: String,
    /// L'identificativo del flusso a cui mandare la richiesta. Resta in
    /// italiano quando il flusso si chiama così: è un dato, e i dati non si
    /// rinominano per stile.
    pub flow: String,
    #[serde(default)]
    pub label: String,
    pub when: Match,
    /// L'utente ha marcato lui questa riga come richiesta: la guardia non si
    /// applica. Ammesso solo su un marcatore che non può iniziare un comando.
    #[serde(default)]
    pub explicit: bool,
    /// Toglie il testo riconosciuto da ciò che si consegna al flusso.
    #[serde(default)]
    pub strip_match: bool,
    /// Sotto questo numero di parole la regola non scatta. La seconda difesa,
    /// per le regole per parole: una richiesta in lingua è lunga, una riga di
    /// comando è corta.
    #[serde(default)]
    pub minimum_words: usize,
    /// Per chi legge l'elenco. Non entra in nessuna decisione.
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    pub route: Route,
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
    pub routes: Vec<Loaded>,
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
                        // ha mai scritto una regola sua; una che c'è e non si
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
    /// una virgola sbagliata in fondo non cancella le regole buone sopra.
    pub fn absorb(&mut self, source: &str, text: &str) {
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
            Value::Object(map) => match map.get("routes") {
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
            let route: Route = match serde_json::from_value(item.clone()) {
                Ok(route) => route,
                Err(error) => {
                    self.problems.push(Problem {
                        source: source.to_string(),
                        about,
                        reason: error.to_string(),
                    });
                    continue;
                }
            };
            if let Err(reason) = coherent(&route) {
                self.problems.push(Problem {
                    source: source.to_string(),
                    about,
                    reason,
                });
                continue;
            }
            self.replace(Loaded {
                route,
                source: source.to_string(),
            });
        }
    }

    fn malformed(&mut self, source: &str) {
        self.problems.push(Problem {
            source: source.to_string(),
            about: "il file".to_string(),
            reason: "non contiene né un array né un campo `routes`".to_string(),
        });
    }

    fn replace(&mut self, loaded: Loaded) {
        match self
            .routes
            .iter_mut()
            .find(|found| found.route.id == loaded.route.id)
        {
            Some(existing) => *existing = loaded,
            None => self.routes.push(loaded),
        }
    }

    /// Quelle accese, in ordine stabile per `id`: due letture di seguito devono
    /// dare la stessa sequenza, o due terminali smisterebbero diversamente la
    /// stessa riga quando due regole la riconoscono entrambe.
    pub fn live(&self) -> Vec<&Loaded> {
        let mut out: Vec<&Loaded> = self
            .routes
            .iter()
            .filter(|loaded| !loaded.route.disabled)
            .collect();
        out.sort_by(|left, right| left.route.id.cmp(&right.route.id));
        out
    }

    pub fn known(&self) -> Vec<String> {
        self.live()
            .into_iter()
            .map(|loaded| loaded.route.id.clone())
            .collect()
    }
}

/// Una regola esplicita che non è un marcatore non si carica.
///
/// **È QUI CHE L'ECCEZIONE ALLA GUARDIA SI TIENE PICCOLA.** `explicit` esiste
/// perché l'utente possa dire «questa è una richiesta» su una frase che
/// assomiglia a un comando. Se si potesse marcare `explicit` una regola che
/// comincia per `git`, il descrittore avrebbe il potere di mangiarsi un comando
/// — cioè esattamente il potere che questo crate non gli dà. Il giorno in cui
/// qualcuno la scrive è l'unico giorno in cui è facile accorgersene.
fn coherent(route: &Route) -> Result<(), String> {
    if route.flow.trim().is_empty() {
        return Err("una regola deve dire a quale flusso manda: `flow` è vuoto".to_string());
    }
    if route.strip_match && !matches!(route.when, Match::StartsWith { .. }) {
        return Err(
            "`strip_match` toglie un prefisso, quindi vale solo con `starts_with`: da una regola per parole non c'è niente di preciso da togliere"
                .to_string(),
        );
    }
    if !route.explicit {
        return Ok(());
    }
    let Match::StartsWith { text } = &route.when else {
        return Err(
            "una regola esplicita scavalca la guardia, e può farlo solo se è un marcatore: serve `starts_with`, non una regola per parole"
                .to_string(),
        );
    };
    match text.chars().next() {
        None => Err("una regola esplicita ha un marcatore vuoto, che sta all'inizio di ogni riga: smisterebbe tutto".to_string()),
        Some(first) if can_start_a_command(first) => Err(format!(
            "il marcatore «{text}» comincia con «{first}», che può iniziare un comando: una regola esplicita scavalca la guardia, quindi il suo marcatore deve essere qualcosa che nessuna shell eseguirebbe"
        )),
        Some(_) => Ok(()),
    }
}

/// I caratteri con cui una riga di comando può cominciare: una lettera, una
/// cifra, e i pochi segni con cui si nomina un file o una variabile.
fn can_start_a_command(first: char) -> bool {
    first.is_alphanumeric()
        || matches!(first, '.' | '/' | '~' | '_' | '-' | '$' | '\\' | '\'' | '"')
}

/// Chi sa dire se una parola è un comando **su questa macchina**.
///
/// È un tratto e non una funzione perché è la parte che tocca il mondo, e una
/// prova deve poter dichiarare un mondo suo: senza, «la guardia ferma `git`»
/// sarebbe vero solo dove `git` è installato, e la batteria racconterebbe della
/// macchina invece che del codice.
pub trait CommandLookup: Send + Sync {
    /// **`true` VUOL DIRE ANCHE «NON SO».** Chi implementa questo tratto deve
    /// rispondere `true` quando non è riuscito a guardare ovunque: un dubbio
    /// sulla macchina deve diventare un comando eseguito, mai una richiesta
    /// dirottata.
    fn is_command(&self, word: &str) -> bool;
}

/// La macchina vera: le cartelle del percorso, come le guarda la shell.
pub struct PathLookup {
    machine: Machine,
}

impl PathLookup {
    pub fn current() -> PathLookup {
        PathLookup {
            machine: Machine::current(),
        }
    }

    pub fn on(machine: Machine) -> PathLookup {
        PathLookup { machine }
    }
}

impl CommandLookup for PathLookup {
    fn is_command(&self, word: &str) -> bool {
        match toolbox::probe::look_up(word, &self.machine) {
            Look::Found(_) => true,
            // «Non ho potuto guardare ovunque» pesa come «c'è»: è la regola
            // «nel dubbio, passa», scritta dove si decide.
            Look::Blocked(_) => true,
            Look::Missing => false,
        }
    }
}

/// Le parole che una shell esegue senza cercare nessun binario.
///
/// **STANNO IN CODICE E NON NEI DESCRITTORI, ED È VOLUTO.** Non sono una scelta
/// di configurazione: sono un fatto sulla shell, della stessa specie del
/// percorso di ricerca. Metterle fra i dati darebbe a un descrittore il potere
/// di *indebolire* la guardia cancellando `cd` dall'elenco — e la proprietà per
/// cui questo file è sicuro è che i dati possano solo chiedere di smistare.
const SHELL_BUILTINS: &[&str] = &[
    ".", ":", "[", "alias", "bg", "bind", "break", "builtin", "case", "cd", "command", "continue",
    "declare", "dirs", "disown", "do", "done", "echo", "elif", "else", "esac", "eval", "exec",
    "exit", "export", "false", "fc", "fg", "fi", "for", "getopts", "hash", "help", "history", "if",
    "jobs", "kill", "let", "local", "logout", "popd", "printf", "pushd", "pwd", "read", "readonly",
    "return", "set", "shift", "shopt", "source", "test", "then", "time", "times", "trap", "true",
    "type", "typeset", "ulimit", "umask", "unalias", "unset", "until", "wait", "where", "which",
    "while",
];

/// I segni che rendono una riga sintassi di shell, e mai una frase.
///
/// Vale la stessa asimmetria di tutto il resto: la loro presenza fa **passare**
/// una riga, quindi un segno di troppo in questo elenco costa uno smistamento
/// mancato, non un comando mangiato.
const SHELL_SIGNS: &[&str] = &["|", "&&", "||", ">>", "<<", ">", "<", ";", "$(", "`", "&"];

/// Perché una riga è passata al terminale invece di andare a un flusso.
///
/// **OGNI PASSAGGIO DICE IL PROPRIO MOTIVO**, e non è cosmesi: uno smistamento
/// che non scatta è muto per definizione — la riga viene eseguita, e sembra
/// tutto normale. Senza il motivo, chi scrive una regola che non funziona non ha
/// niente da guardare.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Passed {
    /// Non c'era niente da smistare.
    Empty,
    /// La prima parola è un comando su questa macchina — o non si è potuto
    /// escludere che lo fosse.
    RunnableFirstWord(String),
    /// Una parola della shell: `cd`, `export`, `exit`.
    ShellWord(String),
    /// Un segno che solo una shell interpreta.
    ShellSign(String),
    /// La prima parola nomina un file: `./x`, `/usr/bin/x`, `~/x`.
    PathLike(String),
    /// Un'assegnazione di variabile: `FOO=1 comando`.
    Assignment(String),
    /// La guardia l'avrebbe lasciata smistare, ma nessuna regola l'ha
    /// riconosciuta.
    NoRuleMatched,
}

/// Dove va una riga scritta in un terminale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Passa: il terminale la esegue com'è.
    Command { line: String, why: Passed },
    /// Va al flusso, con la richiesta da consegnargli.
    Flow {
        /// L'`id` della regola che ha riconosciuto la riga: chi guarda deve
        /// poter risalire alla riga di JSON che ha deciso, non solo al flusso.
        route: String,
        flow: String,
        /// Il testo da consegnare al flusso.
        text: String,
    },
}

/// Le regole caricate, più la macchina su cui si misura la guardia.
pub struct Router {
    routes: Vec<Route>,
    lookup: Arc<dyn CommandLookup>,
}

impl Router {
    pub fn new(catalog: &Catalog, lookup: Arc<dyn CommandLookup>) -> Router {
        Router {
            routes: catalog
                .live()
                .into_iter()
                .map(|loaded| loaded.route.clone())
                .collect(),
            lookup,
        }
    }

    /// Le regole spedite col prodotto, su questa macchina.
    pub fn current() -> Router {
        let machine = Machine::current();
        let catalog = Catalog::load(&default_sources(&machine));
        Router::new(&catalog, Arc::new(PathLookup::on(machine)))
    }

    /// Un terminale senza nessuna regola: smista niente, esegue tutto.
    pub fn without_routes(lookup: Arc<dyn CommandLookup>) -> Router {
        Router {
            routes: Vec::new(),
            lookup,
        }
    }

    pub fn routes(&self) -> &[Route] {
        &self.routes
    }

    /// **DOVE VA QUESTA RIGA.** Tre passaggi, in quest'ordine, e l'ordine è la
    /// difesa:
    ///
    /// 1. i marcatori espliciti — l'utente ha già detto lui che non è un
    ///    comando, e il caricamento ha già garantito che nessuna shell
    ///    eseguirebbe una riga che comincia così;
    /// 2. la guardia: se la riga ha una qualunque forma di comando, passa, e
    ///    nessuna regola la vede;
    /// 3. le regole rimaste.
    ///
    /// Fuori da questi tre, si passa.
    pub fn route(&self, line: &str) -> Routed {
        let text = line.trim();
        if text.is_empty() {
            return Routed::Command {
                line: line.to_string(),
                why: Passed::Empty,
            };
        }

        for route in self.routes.iter().filter(|route| route.explicit) {
            if let Some(request) = recognised(route, text) {
                return Routed::Flow {
                    route: route.id.clone(),
                    flow: route.flow.clone(),
                    text: request,
                };
            }
        }

        if let Some(why) = looks_like_a_command(text, self.lookup.as_ref()) {
            return Routed::Command {
                line: line.to_string(),
                why,
            };
        }

        for route in self.routes.iter().filter(|route| !route.explicit) {
            if let Some(request) = recognised(route, text) {
                return Routed::Flow {
                    route: route.id.clone(),
                    flow: route.flow.clone(),
                    text: request,
                };
            }
        }

        Routed::Command {
            line: line.to_string(),
            why: Passed::NoRuleMatched,
        }
    }
}

/// Se `route` riconosce `text`, il testo da consegnare al flusso.
fn recognised(route: &Route, text: &str) -> Option<String> {
    if text.split_whitespace().count() < route.minimum_words {
        return None;
    }
    match &route.when {
        Match::StartsWith { text: marker } => {
            let lowered = text.to_lowercase();
            if !lowered.starts_with(&marker.to_lowercase()) {
                return None;
            }
            let request = if route.strip_match {
                // `get` e non un taglio diretto: il confronto è avvenuto sulla
                // versione minuscola, che per certe lettere non ha la stessa
                // lunghezza in byte dell'originale. Un taglio a metà carattere
                // sarebbe un panico dentro un terminale.
                match text.get(marker.len()..) {
                    Some(rest) => rest.trim().to_string(),
                    None => return None,
                }
            } else {
                text.to_string()
            };
            // Un marcatore senza niente dietro non è una richiesta: mandarlo a
            // un flusso vorrebbe dire pagare una corsa per una riga vuota.
            if request.is_empty() {
                None
            } else {
                Some(request)
            }
        }
        Match::ContainsAll { words } => {
            if words.is_empty() {
                // Una regola che non chiede niente riconoscerebbe ogni riga.
                return None;
            }
            let present: Vec<String> = text
                .split(|c: char| !c.is_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_lowercase)
                .collect();
            let all = words
                .iter()
                .all(|wanted| present.iter().any(|word| word == &wanted.to_lowercase()));
            if all {
                Some(text.to_string())
            } else {
                None
            }
        }
    }
}

/// La guardia. `Some(motivo)` vuol dire «questa è una riga di comando: passa».
///
/// **PENDE TUTTA DA UNA PARTE.** Ogni controllo qui dentro può solo aggiungere
/// un motivo per passare. Un controllo che sbaglia costa uno smistamento
/// mancato — che l'utente vede subito, perché la sua richiesta finisce nella
/// shell e la shell si lamenta — mentre l'errore opposto costa un comando non
/// eseguito, che l'utente scopre dopo, e che gli toglie la fiducia nel
/// terminale.
fn looks_like_a_command(text: &str, lookup: &dyn CommandLookup) -> Option<Passed> {
    for sign in SHELL_SIGNS {
        if text.contains(sign) {
            return Some(Passed::ShellSign((*sign).to_string()));
        }
    }
    let first = text.split_whitespace().next()?;
    if first.starts_with('/')
        || first.starts_with("./")
        || first.starts_with("../")
        || first.starts_with("~/")
        || first.contains('/')
    {
        return Some(Passed::PathLike(first.to_string()));
    }
    // `FOO=1 comando`: l'uguale prima di ogni spazio è un'assegnazione, e nessuna
    // frase in lingua ne contiene una nella prima parola.
    if let Some(at) = first.find('=') {
        if at > 0 {
            return Some(Passed::Assignment(first.to_string()));
        }
    }
    if SHELL_BUILTINS.contains(&first) {
        return Some(Passed::ShellWord(first.to_string()));
    }
    if lookup.is_command(first) {
        return Some(Passed::RunnableFirstWord(first.to_string()));
    }
    None
}

/// Le sorgenti da cui si prendono le regole su una macchina.
///
/// Nell'ordine in cui vincono: prima quelle spedite, poi quelle dell'utente. Con
/// `SAILOR_ROUTE_DESCRIPTORS` (percorsi separati da `:`, file o cartelle) si
/// aggiunge dove si vuole senza toccare la casa.
pub fn default_sources(machine: &Machine) -> Vec<Source> {
    let mut out = vec![Source::Builtin];
    out.push(Source::Dir(
        toolbox::sailor_home_for(machine).join("routes.d"),
    ));
    if let Some(extra) = machine.env.get("SAILOR_ROUTE_DESCRIPTORS") {
        for raw in extra.split(':').filter(|s| !s.is_empty()) {
            let path = PathBuf::from(machine.expand(raw));
            if path.is_dir() {
                out.push(Source::Dir(path));
            } else {
                out.push(Source::File(path));
            }
        }
    }
    out
}
