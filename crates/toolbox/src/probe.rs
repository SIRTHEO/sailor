//! I gesti che toccano il mondo: guardare nelle cartelle del percorso, guardare
//! se un file c'è, chiedere la versione a un binario, leggere le chiavi di un
//! file JSON.
//!
//! LA MACCHINA È UN PARAMETRO, NON L'AMBIENTE. Ogni gesto passa da `Machine`:
//! cartelle del percorso, casa, variabili. Non è un vezzo di collaudo — è
//! l'unico modo di provare che «assente» e «non ho potuto verificare» sono
//! davvero due risposte diverse, perché entrambe le situazioni si costruiscono
//! in una cartella temporanea e nessuna delle due dipende da cosa c'è installato
//! su chi esegue le prove.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

/// Il mondo in cui si cerca.
#[derive(Debug, Clone)]
pub struct Machine {
    /// Le cartelle in cui si cerca un eseguibile, nell'ordine in cui si
    /// guardano.
    pub path_dirs: Vec<PathBuf>,
    pub home: PathBuf,
    /// Le variabili che un percorso di descrittore può nominare.
    pub env: BTreeMap<String, String>,
    /// Se si può eseguire un binario per chiedergli la versione. Spento, ogni
    /// versione diventa «non chiesta»: serve a chi vuole l'elenco senza avviare
    /// nulla, ed è una scelta di chi chiama, non un ripiego.
    pub version_probes: bool,
}

impl Machine {
    /// La macchina su cui gira questo processo.
    pub fn current() -> Machine {
        let env: BTreeMap<String, String> = std::env::vars().collect();
        let home = env
            .get("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"));
        let path_dirs = env
            .get("PATH")
            .map(|p| {
                p.split(':')
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default();
        Machine {
            path_dirs,
            home,
            env,
            version_probes: true,
        }
    }

    /// `~/x`, `$VAR/x` e `${VAR}/x` diventano percorsi veri. Una variabile che
    /// non esiste resta scritta com'è: sostituirla col vuoto costruirebbe un
    /// percorso plausibile e sbagliato, e chi legge non saprebbe perché.
    pub fn expand(&self, raw: &str) -> String {
        let mut text = raw.to_string();
        if text == "~" {
            return self.home.to_string_lossy().into_owned();
        }
        if let Some(rest) = text.strip_prefix("~/") {
            text = self.home.join(rest).to_string_lossy().into_owned();
        }
        let mut out = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            let braced = chars.peek() == Some(&'{');
            if braced {
                chars.next();
            }
            let mut name = String::new();
            while let Some(&next) = chars.peek() {
                let ok = next.is_alphanumeric() || next == '_';
                if braced && next == '}' {
                    chars.next();
                    break;
                }
                if !ok {
                    break;
                }
                name.push(next);
                chars.next();
            }
            match self.env.get(&name) {
                Some(value) => out.push_str(value),
                None if braced => out.push_str(&format!("${{{name}}}")),
                None => {
                    out.push('$');
                    out.push_str(&name);
                }
            }
        }
        out
    }

    /// I percorsi che uno schema aggancia. Senza `*` è il percorso stesso, che
    /// esista o no: la differenza fra «non c'è» e «non l'ho potuto guardare» la
    /// decide chi lo interroga, non chi lo espande.
    pub fn resolve(&self, raw: &str) -> Vec<PathBuf> {
        let expanded = self.expand(raw);
        if !expanded.contains('*') {
            return vec![PathBuf::from(expanded)];
        }
        // La radice è la parte prima del primo componente con l'asterisco; il
        // resto è lo schema. `inventory::discovery::glob` aggancia esattamente
        // questa forma — componenti letterali e `*` — e riusarla evita una
        // seconda implementazione che diverge dalla prima.
        let parts: Vec<&str> = expanded.split('/').collect();
        let split = parts.iter().position(|p| p.contains('*')).unwrap_or(0);
        let root = PathBuf::from(parts[..split].join("/"));
        let pattern = parts[split..].join("/");
        let root = if root.as_os_str().is_empty() {
            PathBuf::from("/")
        } else {
            root
        };
        inventory::discovery::glob(&root, &pattern)
    }
}

/// C'è, non c'è, o non si è potuto guardare.
///
/// LA TERZA VOCE È IL PUNTO DI TUTTO IL CRATE. Un inventario che scrive «non
/// installato» dove in realtà non ha potuto guardare è peggio di nessun
/// inventario: chi legge installa una seconda copia di una cosa che c'era già,
/// oppure rinuncia a uno strumento che aveva. Ogni «non so» porta il motivo
/// misurato, così chi legge sa che cosa andrebbe guardato a mano.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Look {
    Found(PathBuf),
    Missing,
    Blocked(String),
}

/// Un percorso c'è? `symlink_metadata` e non `exists()`: `exists()` risponde
/// `false` anche quando il permesso è negato, ed è esattamente la bugia che
/// questo crate esiste per togliere.
pub fn look_at(path: &Path) -> Look {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Look::Found(path.to_path_buf()),
        Err(error) if error.kind() == ErrorKind::NotFound => Look::Missing,
        Err(error) => Look::Blocked(format!("{}: {error}", path.to_string_lossy())),
    }
}

/// Cerca un eseguibile nelle cartelle del percorso.
///
/// «NON C'È» SI DICE SOLO SE SI È GUARDATO OVUNQUE. Se anche una sola cartella
/// del percorso non si è potuta leggere, la risposta è «non so»: l'eseguibile
/// poteva stare lì. Con un percorso vuoto la risposta è «non so» a maggior
/// ragione — non si è guardato da nessuna parte.
pub fn look_up(name: &str, machine: &Machine) -> Look {
    if machine.path_dirs.is_empty() {
        return Look::Blocked("nessuna cartella in cui cercare: il percorso è vuoto".to_string());
    }
    let mut blocked: Vec<String> = Vec::new();
    for dir in &machine.path_dirs {
        let candidate = dir.join(name);
        match std::fs::metadata(&candidate) {
            Ok(meta) if is_runnable(&meta) => return Look::Found(candidate),
            // Un nome che c'è ma non è eseguibile non è il binario cercato: si
            // continua, come fa la shell.
            Ok(_) => continue,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => blocked.push(format!("{}: {error}", dir.to_string_lossy())),
        }
    }
    if blocked.is_empty() {
        Look::Missing
    } else {
        Look::Blocked(format!(
            "cercato `{name}`, ma {} cartell{} del percorso non si {} potut{} leggere: {}",
            blocked.len(),
            if blocked.len() == 1 { "a" } else { "e" },
            if blocked.len() == 1 { "è" } else { "sono" },
            if blocked.len() == 1 { "a" } else { "e" },
            blocked.join("; ")
        ))
    }
}

#[cfg(unix)]
fn is_runnable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_runnable(meta: &std::fs::Metadata) -> bool {
    meta.is_file()
}

/// Che cosa ha risposto un binario a cui si è chiesta la versione.
///
/// TRE VOCI PER LO STESSO MOTIVO DELLE TRE DI `Presence`: «non l'ho chiesta» e
/// «l'ho chiesta e non ha risposto» dicono a chi legge due cose diverse da fare.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", content = "detail", rename_all = "lowercase")]
pub enum VersionReading {
    Declared(String),
    /// Il descrittore non dice come chiederla, o chi chiama ha spento le
    /// esecuzioni. Non è un guasto: è una domanda che non è stata fatta.
    NotAsked(String),
    /// La domanda è stata fatta e non ha avuto una risposta utile, col perché.
    Unavailable(String),
}

/// Chiede la versione eseguendo il binario trovato.
///
/// L'INGRESSO SI CHIUDE SUBITO. Un motore che legge il proprio ingresso resta
/// appeso a un EOF che non arriva: su questa macchina è già costato un lavoro
/// rimasto «in corso» per ore. Qui il tetto di tempo lo salverebbe comunque, ma
/// aspettare dieci secondi per ogni strumento dell'elenco è un rilevamento che
/// nessuno lancia due volte.
pub fn read_version(
    bin: &Path,
    args: &[String],
    must_contain: &str,
    limit: Duration,
) -> VersionReading {
    let mut cmd = Command::new(bin);
    cmd.args(args).stdin(Stdio::null());
    let printed = format!("`{} {}`", bin.to_string_lossy(), args.join(" "));
    match actions::run_with_timeout(cmd, limit) {
        actions::RunOutcome::Finished {
            status,
            stdout,
            stderr,
        } => {
            let out = String::from_utf8_lossy(&stdout).into_owned();
            let err = String::from_utf8_lossy(&stderr).into_owned();
            if !status.success() {
                return VersionReading::Unavailable(format!(
                    "{printed} è uscito con {}: {}",
                    status.code().map(|c| c.to_string()).unwrap_or_else(|| "un segnale".to_string()),
                    pick(&err, must_contain).or_else(|| pick(&out, must_contain)).unwrap_or_else(|| "nessun messaggio".to_string())
                ));
            }
            // UN BINARIO CHE ESCE ZERO SENZA DIRE NIENTE non ha dichiarato una
            // versione, e scrivere «» al posto suo la farebbe sembrare
            // dichiarata: la differenza conta quando due macchine si confrontano.
            match pick(&out, must_contain).or_else(|| pick(&err, must_contain)) {
                Some(line) => VersionReading::Declared(line),
                None if must_contain.is_empty() => {
                    VersionReading::Unavailable(format!("{printed} non ha stampato niente"))
                }
                None => VersionReading::Unavailable(format!(
                    "nessuna riga di {printed} contiene «{must_contain}»"
                )),
            }
        }
        actions::RunOutcome::TimedOut => VersionReading::Unavailable(format!(
            "{printed} non è tornato entro {} secondi",
            limit.as_secs()
        )),
        actions::RunOutcome::SpawnFailed(reason) => {
            VersionReading::Unavailable(format!("{printed} non è partito: {reason}"))
        }
    }
}

/// La riga da tenere: la prima che contiene il testo chiesto, o la prima non
/// vuota se il descrittore non chiede niente.
fn pick(text: &str, must_contain: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && (must_contain.is_empty() || l.contains(must_contain)))
        .map(|l| l.to_string())
}

/// Le chiavi trovate seguendo un cammino dentro un JSON, in ordine.
///
/// Un `*` nel cammino sta per «tutte le chiavi di questo livello»: senza,
/// i server dichiarati progetto per progetto resterebbero invisibili, e
/// l'elenco direbbe zero dove ce ne sono.
pub fn json_keys(value: &serde_json::Value, pointer: &[String]) -> Vec<String> {
    let Some((head, tail)) = pointer.split_first() else {
        return match value.as_object() {
            Some(map) => map.keys().cloned().collect(),
            None => Vec::new(),
        };
    };
    let Some(map) = value.as_object() else {
        return Vec::new();
    };
    if head == "*" {
        let mut out = Vec::new();
        for child in map.values() {
            out.extend(json_keys(child, tail));
        }
        out.sort();
        out.dedup();
        return out;
    }
    match map.get(head) {
        Some(child) => json_keys(child, tail),
        None => Vec::new(),
    }
}
