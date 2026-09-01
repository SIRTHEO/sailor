//! Che cosa si può usare su questa macchina: righe di comando di intelligenza
//! artificiale, server MCP, e qualunque altro strumento che un flusso potrebbe
//! voler invocare.
//!
//! IL VINCOLO CHE DECIDE TUTTO IL PROGETTO: l'elenco di cosa cercare è un dato,
//! non codice. In questo crate non compare il nome di nessuno strumento — nessun
//! `if id == "docker"`, nessun percorso di questa macchina. Il codice sa
//! eseguire tre forme di verifica: cercare un eseguibile fra le cartelle del
//! percorso, guardare se un file c'è, e leggere le chiavi di un file di
//! configurazione. Quali eseguibili, quali file e quali chiavi lo dicono i
//! descrittori, che si aggiungono scrivendo una riga di JSON.
//!
//! I DESCRITTORI SPEDITI COL PRODOTTO SONO DATI COME GLI ALTRI. Stanno in
//! `descriptors/default.json`, incorporati nel binario perché non ci sia un
//! percorso di installazione da indovinare, e si riscrivono o si spengono per
//! `id` da un file dell'utente. Sostituirli non richiede di ricompilare.
//!
//! LE DUE RISPOSTE CHE NON VANNO CONFUSE. «Non installato» e «non ho potuto
//! verificare» sono cose diverse, e un inventario che le mescola è inutile: chi
//! legge installa una seconda copia di ciò che aveva, o rinuncia a uno strumento
//! che c'era. Ogni «non so» qui porta il motivo misurato.

pub mod action;
pub mod descriptor;
pub mod needs;
pub mod probe;
pub mod resolver;
pub mod session;

pub use action::{register_default, DetectToolsAction, DETECT_TOOLS_ACTION};
pub use needs::{register_needs, Need, ToolNeedsAction, TOOL_NEEDS_ACTION};
pub use descriptor::{
    builtin_catalog, Capability, CapabilityForm, CapabilityState, Catalog, Contradiction,
    Descriptor, Loaded, Problem, ResetContext, Source, ASK_WITHOUT_INTERACTION, BUILTIN_CATALOGS,
};
pub use probe::{Look, Machine, VersionReading};
pub use resolver::Tools;
pub use session::{SessionAbilities, SessionAbility};

// I TIPI DELL'ESITO SI LEGGONO ANCHE IN INGRESSO, dal 29/08/2026: un passo che
// riceve il rilevamento fatto dal passo prima lo deserializza come qualunque
// altro dato. Senza `Deserialize` quel passo dovrebbe rovistare in un
// `serde_json::Value` a puntatori, e il legame fra i due passi diventerebbe una
// convenzione fra stringhe invece di un tipo.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;

/// C'è, non c'è, o non si è potuto guardare — con il motivo, sempre.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "reason", rename_all = "lowercase")]
pub enum Presence {
    Present(String),
    Absent(String),
    Undetermined(String),
}

impl Presence {
    pub fn is_present(&self) -> bool {
        matches!(self, Presence::Present(_))
    }
}

/// Un posto dove vive la configurazione di uno strumento, e se c'è.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigPath {
    pub path: String,
    pub presence: Presence,
}

/// Una cosa trovata (o non trovata), con tutto quello che serve per risalire al
/// perché.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Finding {
    /// Come si chiama la cosa: l'`id` del descrittore, o — per un descrittore
    /// che scopre più voci da un file — il nome della voce scoperta.
    pub name: String,
    pub family: String,
    pub label: String,
    /// Da quale descrittore è stata riconosciuta, e da dove viene quel
    /// descrittore: senza queste due righe «perché è nell'elenco?» non ha
    /// risposta, e un elenco a cui non si può chiedere conto non si corregge.
    pub descriptor_id: String,
    pub descriptor_source: String,
    pub presence: Presence,
    /// Dove sta il suo eseguibile, quando ne ha uno.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub executable: Option<String>,
    pub version: VersionReading,
    pub config: Vec<ConfigPath>,
    /// La nota del descrittore, per chi legge.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// L'esito di un rilevamento.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub findings: Vec<Finding>,
    /// I descrittori che non si sono potuti leggere. Stanno qui e non fra le
    /// voci: una riga sbagliata nell'elenco di cosa cercare non è uno strumento
    /// assente, ed è un guasto di chi ha scritto l'elenco.
    pub problems: Vec<Problem>,
    /// Le cartelle in cui si è cercato un eseguibile, in chiaro: un elenco che
    /// non dice dove ha guardato non si può smentire.
    pub looked_in: Vec<String>,
}

impl Report {
    pub fn of_family<'a>(&'a self, family: &str) -> Vec<&'a Finding> {
        self.findings
            .iter()
            .filter(|f| f.family == family)
            .collect()
    }

    pub fn present(&self) -> Vec<&Finding> {
        self.findings
            .iter()
            .filter(|f| f.presence.is_present())
            .collect()
    }

    /// Le famiglie viste, in ordine: chi mostra l'elenco non deve conoscerle in
    /// anticipo, o una famiglia nuova richiederebbe di ricompilare chi la mostra.
    pub fn families(&self) -> Vec<String> {
        let mut out: Vec<String> = self.findings.iter().map(|f| f.family.clone()).collect();
        out.sort();
        out.dedup();
        out
    }
}

/// La casa di Sailor per una macchina descritta.
///
/// **Una riga sola, e chiama il deposito.** Fino al 30/08/2026 questa regola era
/// riscritta qui e in `trigger`, e la copia sbagliava: ignorava
/// `XDG_CONFIG_HOME` e cadeva su `~/.sailor`, mentre `ledger::sailor_home` — che
/// decide dove stanno deposito e listino dei prezzi — cade su `~/.config/sailor`.
/// Risultato: i descrittori dell'utente in una casa, i prezzi nell'altra, e
/// nessuno dei due si accorgeva dell'altro.
pub fn sailor_home_for(machine: &Machine) -> PathBuf {
    ledger::sailor_home_in(
        machine.env.get("SAILOR_HOME").map(PathBuf::from),
        machine.env.get("XDG_CONFIG_HOME").map(PathBuf::from),
        machine.home.clone(),
    )
}

/// Le sorgenti da cui si prendono i descrittori su questa macchina.
///
/// Nell'ordine in cui vincono: prima quelli spediti, poi quelli dell'utente. Con
/// `SAILOR_TOOL_DESCRIPTORS` (percorsi separati da `:`, file o cartelle) si
/// aggiunge dove si vuole senza toccare la casa.
pub fn default_sources(machine: &Machine) -> Vec<Source> {
    let mut out = vec![Source::Builtin];
    out.push(Source::Dir(sailor_home_for(machine).join("tools.d")));
    if let Some(extra) = machine.env.get("SAILOR_TOOL_DESCRIPTORS") {
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

/// Il rilevamento: esegue ogni descrittore vivo e raccoglie cosa ha risposto.
pub fn detect(catalog: &Catalog, machine: &Machine) -> Report {
    let mut findings = Vec::new();
    for loaded in catalog.live() {
        match &loaded.descriptor.enumerate {
            Some(enumerate) => findings.extend(discovered(loaded, enumerate, machine)),
            None => findings.push(probed(loaded, machine)),
        }
    }
    findings.sort_by(|a, b| (&a.family, &a.name).cmp(&(&b.family, &b.name)));
    Report {
        findings,
        problems: catalog.problems.clone(),
        looked_in: machine
            .path_dirs
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect(),
    }
}

/// Cerca **un solo** strumento, per identificativo.
///
/// Non è `detect` filtrata dopo, ed è una differenza che si paga: eseguire ogni
/// descrittore per sapere dove sta un binario vuol dire chiedere la versione a
/// ogni programma installato — decine di processi — ogni volta che un passo di
/// flusso parte. Qui si esegue una sola riga dell'elenco.
///
/// Vale solo per un descrittore con `detect`: uno che *scopre* voci leggendo un
/// file di configurazione non ha un eseguibile, e chi chiama distingue i due
/// casi guardando `detect`.
pub fn probe_one(loaded: &Loaded, machine: &Machine) -> Finding {
    probed(loaded, machine)
}

/// Un descrittore che dice «questa cosa o c'è o non c'è».
fn probed(loaded: &Loaded, machine: &Machine) -> Finding {
    let descriptor = &loaded.descriptor;
    let probes = descriptor
        .detect
        .as_ref()
        .map(|p| p.as_slice())
        .unwrap_or(&[]);
    let mut executable: Option<PathBuf> = None;
    let mut presence: Option<Presence> = None;
    let mut blocked: Vec<String> = Vec::new();
    let mut searched: Vec<String> = Vec::new();
    for single in probes {
        if let Some(command) = &single.command {
            searched.push(format!("l'eseguibile `{command}`"));
            match probe::look_up(command, machine) {
                Look::Found(path) => {
                    presence = Some(Presence::Present(format!(
                        "trovato `{command}` in {}",
                        path.to_string_lossy()
                    )));
                    executable = Some(path);
                    break;
                }
                Look::Missing => {}
                Look::Blocked(reason) => blocked.push(reason),
            }
        }
        if let Some(raw) = &single.path {
            searched.push(format!("il percorso `{raw}`"));
            for candidate in machine.resolve(raw) {
                match probe::look_at(&candidate) {
                    Look::Found(path) => {
                        presence = Some(Presence::Present(format!(
                            "trovato {}",
                            path.to_string_lossy()
                        )));
                        break;
                    }
                    Look::Missing => {}
                    Look::Blocked(reason) => blocked.push(reason),
                }
            }
            if presence.is_some() {
                break;
            }
        }
    }
    let presence = presence.unwrap_or_else(|| {
        if blocked.is_empty() {
            Presence::Absent(format!("cercato {}: niente", searched.join(", ")))
        } else {
            // NON SI DICE «ASSENTE» DOVE NON SI È POTUTO GUARDARE, ed è il
            // motivo per cui questo ramo esiste separato dall'altro.
            Presence::Undetermined(blocked.join("; "))
        }
    });
    let version = match (&presence, &executable, &descriptor.version) {
        (Presence::Present(_), Some(bin), Some(spec)) if machine.version_probes => {
            probe::read_version(
                bin,
                &spec.args,
                &spec.must_contain,
                Duration::from_secs(spec.timeout_secs),
            )
        }
        (Presence::Present(_), Some(_), Some(_)) => {
            VersionReading::NotAsked("le esecuzioni sono spente".to_string())
        }
        (Presence::Present(_), _, None) => {
            VersionReading::NotAsked("il descrittore non dice come chiederla".to_string())
        }
        (Presence::Present(_), None, Some(_)) => {
            VersionReading::NotAsked("non c'è un eseguibile a cui chiederla".to_string())
        }
        _ => VersionReading::NotAsked("non è qui".to_string()),
    };
    Finding {
        name: descriptor.id.clone(),
        family: descriptor.family.clone(),
        label: label_of(descriptor),
        descriptor_id: descriptor.id.clone(),
        descriptor_source: loaded.source.clone(),
        presence,
        executable: executable.map(|p| p.to_string_lossy().into_owned()),
        version,
        config: config_of(&descriptor.config, machine),
        note: descriptor.note.clone(),
    }
}

/// Un descrittore che scopre le voci leggendo un file di configurazione.
fn discovered(
    loaded: &Loaded,
    enumerate: &descriptor::Enumerate,
    machine: &Machine,
) -> Vec<Finding> {
    let descriptor = &loaded.descriptor;
    let mut out: Vec<Finding> = Vec::new();
    let mut blocked: Vec<String> = Vec::new();
    // Dove si è guardato, in chiaro: è quello che finisce nel motivo di un
    // «non c'è», e un elenco vuoto che non dice dove ha cercato non si può
    // smentire.
    let mut looked: Vec<String> = Vec::new();
    // Vero se almeno un posto è stato davvero letto. Senza questa distinzione
    // «nessuna voce» e «non ho guardato niente» si scrivono uguale.
    let mut read_any = false;

    if let Some(json_keys) = &enumerate.json_keys {
        looked.push(format!(
            "le chiavi sotto {} in {}",
            json_keys.pointer.join("/"),
            json_keys.files.join(", ")
        ));
        for raw in &json_keys.files {
            for path in machine.resolve(raw) {
                let text = match std::fs::read_to_string(&path) {
                    Ok(text) => text,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => {
                        blocked.push(format!("{}: {error}", path.to_string_lossy()));
                        continue;
                    }
                };
                let value: serde_json::Value = match serde_json::from_str(&text) {
                    Ok(value) => value,
                    Err(error) => {
                        // UN FILE ILLEGGIBILE NON È UN FILE SENZA VOCI. Contarlo
                        // come zero farebbe sparire in silenzio tutto ciò che
                        // dichiara.
                        blocked.push(format!(
                            "{} non è JSON valido: {error}",
                            path.to_string_lossy()
                        ));
                        continue;
                    }
                };
                read_any = true;
                for name in probe::json_keys(&value, &json_keys.pointer) {
                    let evidence = format!(
                        "dichiarato in {} sotto {}",
                        path.to_string_lossy(),
                        json_keys.pointer.join("/")
                    );
                    out.push(Finding {
                        name,
                        family: descriptor.family.clone(),
                        label: label_of(descriptor),
                        descriptor_id: descriptor.id.clone(),
                        descriptor_source: loaded.source.clone(),
                        presence: Presence::Present(evidence),
                        executable: None,
                        version: VersionReading::NotAsked(
                            "è una voce di configurazione, non un binario".to_string(),
                        ),
                        config: vec![ConfigPath {
                            path: path.to_string_lossy().into_owned(),
                            presence: Presence::Present("letto".to_string()),
                        }],
                        note: descriptor.note.clone(),
                    });
                }
            }
        }
    }

    if let Some(patterns) = &enumerate.paths {
        looked.push(format!("i file che agganciano {}", patterns.join(", ")));
        for raw in patterns {
            let found = machine.resolve(raw);
            if raw.contains('*') {
                // UNA CARTELLA CHE NON SI LEGGE NON È UNA CARTELLA VUOTA, e
                // `glob` non distingue le due: ingoia l'errore di `read_dir` e
                // restituisce zero percorsi in entrambi i casi. Qui si chiede
                // direttamente alla radice dello schema, perché «non hai niente
                // da migrare» detto a chi ha venti servizi è la bugia peggiore
                // che questo elenco possa dire.
                match read_dir_state(&machine.expand(raw)) {
                    DirState::Readable => read_any = true,
                    DirState::Missing(where_) => {
                        looked.push(format!("{where_} non esiste"));
                    }
                    DirState::Blocked(reason) => blocked.push(reason),
                }
            }
            for path in found {
                match probe::look_at(&path) {
                    Look::Found(path) => {
                        read_any = true;
                        let shown = path.to_string_lossy().into_owned();
                        out.push(Finding {
                            // IL NOME È IL PERCORSO INTERO. Due file con lo
                            // stesso nome in due cartelle diverse sono due
                            // automazioni diverse, e la fusione qui sotto li
                            // conterebbe per una sola.
                            name: shown.clone(),
                            family: descriptor.family.clone(),
                            label: label_of(descriptor),
                            descriptor_id: descriptor.id.clone(),
                            descriptor_source: loaded.source.clone(),
                            presence: Presence::Present(format!("il file c'è: {shown}")),
                            executable: None,
                            version: VersionReading::NotAsked(
                                "è un file, non un binario da interrogare".to_string(),
                            ),
                            config: vec![ConfigPath {
                                path: shown,
                                presence: Presence::Present("c'è".to_string()),
                            }],
                            note: descriptor.note.clone(),
                        });
                    }
                    Look::Missing => read_any = true,
                    Look::Blocked(reason) => {
                        blocked.push(format!("{}: {reason}", path.to_string_lossy()))
                    }
                }
            }
        }
    }

    // LO STESSO SERVER DICHIARATO IN DUE POSTI È UNO SOLO, ma i due posti vanno
    // detti entrambi: chi deve cambiarne la configurazione deve sapere quale
    // file toccare.
    out.sort_by(|a, b| a.name.cmp(&b.name));
    let mut merged: Vec<Finding> = Vec::new();
    for finding in out {
        match merged.iter_mut().find(|f| f.name == finding.name) {
            Some(existing) => existing.config.extend(finding.config),
            None => merged.push(finding),
        }
    }
    if merged.is_empty() {
        // Nessuna voce: e ora la differenza che conta. Se almeno un posto si è
        // letto, «non ce ne sono» è una misura; se non se n'è letto nessuno, non
        // si è guardato niente e dirlo sarebbe inventare.
        let presence = if !blocked.is_empty() {
            Presence::Undetermined(blocked.join("; "))
        } else if read_any {
            Presence::Absent(format!("nessuna voce in: {}", looked.join("; ")))
        } else {
            Presence::Absent(format!("non c'era niente da leggere in: {}", looked.join("; ")))
        };
        merged.push(Finding {
            name: descriptor.id.clone(),
            family: descriptor.family.clone(),
            label: label_of(descriptor),
            descriptor_id: descriptor.id.clone(),
            descriptor_source: loaded.source.clone(),
            presence,
            executable: None,
            version: VersionReading::NotAsked("non è qui".to_string()),
            config: config_of(&descriptor.config, machine),
            note: descriptor.note.clone(),
        });
    } else if !blocked.is_empty() {
        // Qualcosa si è trovato e qualcosa non si è potuto guardare: l'elenco è
        // parziale, e va detto invece di lasciarlo credere completo.
        merged.push(Finding {
            name: format!("{} (parziale)", descriptor.id),
            family: descriptor.family.clone(),
            label: label_of(descriptor),
            descriptor_id: descriptor.id.clone(),
            descriptor_source: loaded.source.clone(),
            presence: Presence::Undetermined(blocked.join("; ")),
            executable: None,
            version: VersionReading::NotAsked("non è un binario".to_string()),
            config: Vec::new(),
            note: descriptor.note.clone(),
        });
    }
    merged
}

/// Che cosa risponde la cartella da cui parte uno schema con `*`.
enum DirState {
    Readable,
    Missing(String),
    Blocked(String),
}

/// La radice di uno schema è la parte prima del primo componente con l'`*`; è
/// la stessa divisione che fa [`Machine::resolve`], e serve qui per poter
/// chiedere alla cartella cosa ha risposto invece di dedurlo da zero risultati.
fn read_dir_state(expanded: &str) -> DirState {
    let root: String = expanded
        .split('/')
        .take_while(|part| !part.contains('*'))
        .collect::<Vec<_>>()
        .join("/");
    let root = if root.is_empty() { "." } else { root.as_str() };
    match std::fs::read_dir(root) {
        Ok(_) => DirState::Readable,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            DirState::Missing(root.to_string())
        }
        Err(error) => DirState::Blocked(format!("{root}: {error}")),
    }
}

fn label_of(descriptor: &Descriptor) -> String {
    if descriptor.label.is_empty() {
        descriptor.id.clone()
    } else {
        descriptor.label.clone()
    }
}

fn config_of(raw: &[String], machine: &Machine) -> Vec<ConfigPath> {
    let mut out = Vec::new();
    for pattern in raw {
        let resolved = machine.resolve(pattern);
        if resolved.is_empty() {
            out.push(ConfigPath {
                path: machine.expand(pattern),
                presence: Presence::Absent("nessun percorso agganciato".to_string()),
            });
            continue;
        }
        for path in resolved {
            let presence = match probe::look_at(&path) {
                Look::Found(_) => Presence::Present("c'è".to_string()),
                Look::Missing => Presence::Absent("non c'è".to_string()),
                Look::Blocked(reason) => Presence::Undetermined(reason),
            };
            out.push(ConfigPath {
                path: path.to_string_lossy().into_owned(),
                presence,
            });
        }
    }
    out
}
