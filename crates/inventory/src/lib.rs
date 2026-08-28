//! Il punto comune: che cosa è installato su questa macchina, da dove viene, e
//! se è raggiungibile.
//!
//! PERCHÉ ESISTE, CON LA MISURA. Il 28/08/2026 per sapere che cosa aveva a
//! disposizione una persona doveva guardare in **diciannove cartelle diverse**:
//! sei per le competenze, sei per le regole, tre per i comandi, due per gli
//! agenti, due per i ganci. Nessuna di quelle categorie rispondeva a un comando:
//! si scopriva camminando nel filesystem, e quindi non si scopriva. Nello stesso
//! censimento è venuta fuori la conseguenza vera di non avere un elenco unico:
//! `codex` ha **due configurazioni divergenti** — quella che si legge a mano e
//! quella che Orca gli passa davvero — e nessuno lo segnalava.
//!
//! DUE RESPONSABILITÀ SEPARATE, come nel resto del sistema: `discovery` sa dove
//! Claude Code carica le cose, qui si costruisce l'elenco. Chi lo mostra — la
//! riga di comando o la pagina — non sa niente di percorsi.
//!
//! COSA NON FA. Non giudica se una competenza sia buona, non la cancella e non
//! la sposta: elenca. La decisione di togliere resta di chi legge.

pub mod discovery;

use serde::Serialize;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Le famiglie di cose che si possono avere installate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Skill,
    Agent,
    Command,
    Rule,
    Hook,
}

impl Kind {
    /// Il nome con cui la famiglia si presenta a chi legge.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Skill => "competenza",
            Kind::Agent => "agente",
            Kind::Command => "comando",
            Kind::Rule => "regola",
            Kind::Hook => "gancio",
        }
    }
}

/// È raggiungibile davvero?
///
/// LA TERZA VOCE NON È UN RIPIEGO. Una competenza dentro un plugin spento è
/// dimostrabilmente irraggiungibile; una regola in un repo dipende da chi apre
/// la sessione e da dove, e dire «attiva» sarebbe una bugia comoda. `Unknown`
/// porta il motivo, così chi legge sa che cosa andrebbe verificato.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "reason", rename_all = "lowercase")]
pub enum Reach {
    Active,
    Inactive(String),
    Unknown(String),
}

/// Una voce dell'inventario.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Entry {
    pub kind: Kind,
    /// Il nome con cui si invoca: `handoff`, `plugin:competenza`, `builder`.
    pub name: String,
    pub description: String,
    /// Da dove viene: `casa`, `plugin <nome>`, `repo <nome>`.
    pub origin: String,
    pub path: String,
    pub reach: Reach,
    /// Il modello può invocarla da sé, o solo la persona che digita.
    /// `disable-model-invocation: true` non vuol dire «non c'è».
    pub by_model: bool,
}

/// Una radice da cui si carica: la casa, o un repo con la sua `.claude/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Root {
    /// Come si chiama per chi legge: `casa`, o il nome del repo.
    pub label: String,
    /// La cartella che contiene `.claude/`, non la `.claude/` stessa.
    pub path: PathBuf,
    /// La casa carica sempre; un repo carica solo se ci si lavora dentro.
    pub is_home: bool,
}

impl Root {
    pub fn home(path: &Path) -> Root {
        Root {
            label: "casa".to_string(),
            path: path.to_path_buf(),
            is_home: true,
        }
    }

    pub fn repo(path: &Path) -> Root {
        Root {
            label: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_string_lossy().into_owned()),
            path: path.to_path_buf(),
            is_home: false,
        }
    }
}

/// L'inventario intero, in un ordine stabile: due letture di seguito danno la
/// stessa sequenza, o il confronto fra un giorno e l'altro non vale niente.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Inventory {
    pub entries: Vec<Entry>,
    /// Le radici davvero guardate, in chiaro: un elenco che non dice dove ha
    /// cercato non si può smentire.
    pub roots: Vec<String>,
    /// Quante copie di plugin restano in cache senza essere quella installata.
    /// Non sono voci dell'inventario — nessuno le carica — ma sono spazio, e
    /// finché nessuno le conta nessuno le toglie.
    pub stale_plugin_copies: usize,
}

impl Inventory {
    pub fn of(&self, kind: Kind) -> Vec<&Entry> {
        self.entries.iter().filter(|e| e.kind == kind).collect()
    }

    pub fn count(&self, kind: Kind) -> usize {
        self.entries.iter().filter(|e| e.kind == kind).count()
    }
}

/// Costruisce l'inventario camminando su ogni radice.
pub fn collect(roots: &[Root]) -> Inventory {
    let mut entries = Vec::new();
    let mut stale = 0usize;
    for root in roots {
        if root.is_home {
            let (found, dropped) = home_skills(&root.path);
            entries.extend(found);
            stale += dropped;
            let (found, dropped) = home_agents(&root.path);
            entries.extend(found);
            stale += dropped;
        } else {
            entries.extend(repo_dir(root, "skills", Kind::Skill));
            entries.extend(repo_dir(root, "agents", Kind::Agent));
        }
        entries.extend(commands_of(root));
        entries.extend(rules_of(root));
        entries.extend(hooks_of(root));
    }
    entries.sort_by(|a, b| {
        (a.kind, &a.name, &a.origin, &a.path).cmp(&(b.kind, &b.name, &b.origin, &b.path))
    });
    // LA DESCRIZIONE FA PARTE DELL'IDENTITÀ, e non è un dettaglio: su un evento
    // e un matcher condivisi vivono più ganci diversi, distinti solo dal comando
    // che lanciano. Senza questo campo il conto diceva 29 ganci dove ce n'erano
    // 57 — misurato il 28/08/2026 contro `settings.json` letto a mano.
    entries.dedup_by(|a, b| {
        a.kind == b.kind && a.name == b.name && a.path == b.path && a.description == b.description
    });
    Inventory {
        entries,
        roots: roots
            .iter()
            .map(|r| format!("{}: {}", r.label, r.path.to_string_lossy()))
            .collect(),
        stale_plugin_copies: stale,
    }
}

/// Il file sta dentro la versione di plugin che Claude Code carica davvero?
///
/// Fuori dalla cache dei plugin la domanda non si pone: vale sempre. Dentro,
/// vale solo se sta sotto uno degli `installPath` dichiarati — e se l'elenco è
/// vuoto (file assente o illeggibile) non si scarta niente: «non lo so» non
/// deve diventare «non c'è».
fn is_the_installed_copy(path: &Path, installed: &BTreeSet<PathBuf>) -> bool {
    if !path.to_string_lossy().contains("/plugins/cache/") || installed.is_empty() {
        return true;
    }
    installed.iter().any(|root| path.starts_with(root))
}

/// Le competenze della casa, plugin compresi — con il filtro che rende conto
/// dei plugin spenti invece di nasconderli.
///
/// LA DIFFERENZA COL SUGGERITORE È DELIBERATA: `skill_nudge` scarta in silenzio
/// ciò che non è raggiungibile, perché deve consigliare solo cose invocabili.
/// Qui una competenza spenta resta nell'elenco con scritto perché è spenta: è
/// esattamente il caso che oggi non si vede da nessuna parte, e senza il quale
/// «ho quattordici plugin» e «ne funzionano sei» sembrano la stessa frase.
fn home_skills(home: &Path) -> (Vec<Entry>, usize) {
    let on = discovery::enabled_plugins(home);
    let installed = discovery::installed_paths(home);
    let mut out = Vec::new();
    let mut stale = 0usize;
    for (root, pattern) in discovery::skill_sources(home) {
        for path in discovery::glob(&root, pattern) {
            if !is_the_installed_copy(&path, &installed) {
                stale += 1;
                continue;
            }
            let Some((name, description, by_model)) = named(&path) else {
                continue;
            };
            let prefix = discovery::prefix(&path);
            let plugin = prefix.strip_suffix(':').unwrap_or("").to_string();
            let reach = if plugin.is_empty() {
                Reach::Active
            } else if on.contains(&plugin) || plugin.contains("mattpocock") {
                Reach::Active
            } else {
                Reach::Inactive(format!("il plugin {plugin} non è abilitato"))
            };
            let declared = discovery::manifest(&path);
            let reach = match (&declared, path.parent().and_then(|p| p.file_name())) {
                (Some(names), Some(own)) if !names.contains(&own.to_string_lossy().into_owned()) => {
                    Reach::Inactive(format!(
                        "sta sul disco ma il manifesto di {plugin} non la dichiara"
                    ))
                }
                _ => reach,
            };
            out.push(Entry {
                kind: Kind::Skill,
                name: format!("{prefix}{name}"),
                description,
                origin: if plugin.is_empty() {
                    "casa".to_string()
                } else {
                    format!("plugin {plugin}")
                },
                path: path.to_string_lossy().into_owned(),
                reach,
                by_model,
            });
        }
    }
    (out, stale)
}

fn home_agents(home: &Path) -> (Vec<Entry>, usize) {
    let installed = discovery::installed_paths(home);
    let mut out = Vec::new();
    let mut stale = 0usize;
    for (root, pattern) in discovery::agent_sources(home) {
        for path in discovery::glob(&root, pattern) {
            if !is_the_installed_copy(&path, &installed) {
                stale += 1;
                continue;
            }
            let Some((name, description, by_model)) = named(&path) else {
                continue;
            };
            let plugin = discovery::prefix(&path);
            let plugin = plugin.strip_suffix(':').unwrap_or("").to_string();
            out.push(Entry {
                kind: Kind::Agent,
                name,
                description,
                origin: if plugin.is_empty() {
                    "casa".to_string()
                } else {
                    format!("plugin {plugin}")
                },
                path: path.to_string_lossy().into_owned(),
                reach: Reach::Active,
                by_model,
            });
        }
    }
    (out, stale)
}

/// (nome, descrizione, invocabile dal modello) di ciò che dichiara un `name:`.
///
/// Non riusa `discovery::frontmatter` perché quella scarta ciò che il modello
/// non può invocare, e qui va mostrato: la differenza sta scritta accanto a
/// `matter_and_invocability`.
fn named(path: &Path) -> Option<(String, String, bool)> {
    let (matter, by_model) = discovery::matter_and_invocability(path)?;
    let start = discovery::field_start(&matter, "name:")?;
    let name = matter[start..]
        .lines()
        .next()?
        .split_whitespace()
        .next()?
        .to_string();
    Some((name, discovery::description(&matter, false), by_model))
}

/// Competenze e agenti dichiarati dentro un repo.
///
/// Valgono solo per chi apre una sessione lì dentro: `Unknown`, col motivo.
fn repo_dir(root: &Root, folder: &str, kind: Kind) -> Vec<Entry> {
    let base = root.path.join(".claude").join(folder);
    let pattern = if kind == Kind::Skill {
        "*/SKILL.md"
    } else {
        "*.md"
    };
    discovery::glob(&base, pattern)
        .into_iter()
        .filter_map(|path| {
            let (name, description, by_model) = named(&path)?;
            Some(Entry {
                kind,
                name,
                description,
                origin: format!("repo {}", root.label),
                path: path.to_string_lossy().into_owned(),
                reach: Reach::Unknown(format!(
                    "solo per una sessione aperta dentro {}",
                    root.label
                )),
                by_model,
            })
        })
        .collect()
}

fn commands_of(root: &Root) -> Vec<Entry> {
    let base = root.path.join(".claude").join("commands");
    discovery::glob(&base, "*.md")
        .into_iter()
        .filter_map(|path| {
            // UN COMANDO SENZA FRONTMATTER È UN COMANDO. Claude Code prende il
            // file intero come prompt; pretendere il frontmatter faceva sparire
            // `/work-loop-headless` dall'elenco di ciò che esiste — e un elenco
            // che tace su ciò che c'è è peggio di nessun elenco.
            let (matter, by_model) = discovery::matter_and_invocability(&path)
                .unwrap_or_else(|| (String::new(), true));
            let name = path.file_stem()?.to_string_lossy().into_owned();
            let description = match discovery::description(&matter, true) {
                empty if empty.is_empty() => first_heading(&path),
                found => found,
            };
            Some(Entry {
                kind: Kind::Command,
                name: format!("/{name}"),
                description,
                origin: origin_of(root),
                path: path.to_string_lossy().into_owned(),
                reach: reach_of(root),
                by_model,
            })
        })
        .collect()
}

/// Le regole non hanno frontmatter: si chiamano come il file, e la descrizione
/// è il loro primo titolo. Leggere il testo intero per ricavarne una riga
/// costerebbe più di quanto rende su una cartella che si sfoglia.
fn rules_of(root: &Root) -> Vec<Entry> {
    let base = root.path.join(".claude").join("rules");
    let mut out = Vec::new();
    // Un livello di sottocartelle, perché le regole si raggruppano per materia
    // (`rules/common/`, `rules/typescript/`) e fermarsi al primo livello ne
    // perdeva un terzo senza dirlo.
    let found: Vec<PathBuf> = discovery::glob(&base, "*.md")
        .into_iter()
        .chain(discovery::glob(&base, "*/*.md"))
        .collect();
    for path in found {
        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        out.push(Entry {
            kind: Kind::Rule,
            name,
            description: first_heading(&path),
            origin: origin_of(root),
            path: path.to_string_lossy().into_owned(),
            reach: reach_of(root),
            // Una regola non si invoca: la si applica. Vale per tutti.
            by_model: true,
        });
    }
    out
}

/// Il titolo di un documento Markdown, o la prima riga di testo se non ne ha.
fn first_heading(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return String::new();
    };
    for line in text.lines().take(40) {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("# ") {
            return rest.trim().to_string();
        }
    }
    String::new()
}

/// I ganci dichiarati in `settings.json`, evento per evento.
///
/// IL COMANDO SI GUARDA, NON SI CREDE. Un gancio che punta a un file che non
/// esiste più non dà errore a nessuno: tace, e chi lo ha scritto continua a
/// credere che difenda qualcosa. Il 28/08/2026 quattro script puntavano ancora a
/// `~/.claude/rust/…`, cancellato quel giorno.
fn hooks_of(root: &Root) -> Vec<Entry> {
    let path = root.path.join(".claude").join("settings.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Vec::new();
    };
    let Some(events) = value.get("hooks").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (event, groups) in events {
        let Some(groups) = groups.as_array() else {
            continue;
        };
        for group in groups {
            let matcher = group
                .get("matcher")
                .and_then(|v| v.as_str())
                .unwrap_or("*")
                .to_string();
            let Some(hooks) = group.get("hooks").and_then(|v| v.as_array()) else {
                continue;
            };
            for hook in hooks {
                let Some(command) = hook.get("command").and_then(|v| v.as_str()) else {
                    continue;
                };
                out.push(Entry {
                    kind: Kind::Hook,
                    name: format!("{event} · {matcher}"),
                    description: command.to_string(),
                    origin: origin_of(root),
                    path: path.to_string_lossy().into_owned(),
                    reach: match missing_file(command, &root.path) {
                        Some(missing) => {
                            Reach::Inactive(format!("punta a {missing}, che non esiste"))
                        }
                        None => reach_of(root),
                    },
                    by_model: true,
                });
            }
        }
    }
    out
}

/// Il primo percorso nominato dal comando che non esiste sul disco.
///
/// Deliberatamente ingenua: guarda le parole che sembrano un file da eseguire,
/// non interpreta la shell.
///
/// LE DUE STRETTOIE VENGONO DA DUE FALSI ALLARMI VERI, presi alla prima
/// esecuzione sul disco del 28/08/2026. La punteggiatura della shell resta
/// attaccata alla parola — `…/claude-hook.sh';` — e va tolta, o un gancio vivo
/// risulta morto. E `/clear` comincia per `/` senza essere un file: è
/// l'argomento di un gancio `SessionStart`. Per questo serve anche
/// un'estensione nell'ultimo pezzo: chi punta a uno script lo scrive con la sua,
/// e chi non ce l'ha non è un file da cercare.
fn missing_file(command: &str, home: &Path) -> Option<String> {
    for word in command.split_whitespace() {
        let word = word.trim_matches(|c: char| {
            c == '"' || c == '\'' || c == '`' || c == ';' || c == ',' || c == ')' || c == '('
        });
        let expanded = match word.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None if word.starts_with('/') => PathBuf::from(word),
            None => continue,
        };
        let looks_like_a_file = expanded
            .file_name()
            .map(|n| n.to_string_lossy().contains('.'))
            .unwrap_or(false);
        if looks_like_a_file && !expanded.exists() {
            return Some(word.to_string());
        }
    }
    None
}

fn origin_of(root: &Root) -> String {
    if root.is_home {
        "casa".to_string()
    } else {
        format!("repo {}", root.label)
    }
}

fn reach_of(root: &Root) -> Reach {
    if root.is_home {
        Reach::Active
    } else {
        Reach::Unknown(format!(
            "solo per una sessione aperta dentro {}",
            root.label
        ))
    }
}

/// Le radici da guardare su questa macchina: la casa, e i repo di lavoro.
///
/// STA QUI E NON NEI DUE CHIAMANTI. La riga di comando e la pagina devono dire
/// lo stesso numero sulla stessa macchina: se ognuna sceglie le proprie radici,
/// la prima volta che una cambia tornano a divergere — che è il difetto che
/// questo crate esiste per togliere.
///
/// Le basi sono dichiarate, non cercate su tutto il disco: camminare da `/`
/// troverebbe anche le copie di lavoro, dove le stesse regole ricompaiono
/// collegate.
pub fn default_roots() -> Vec<Root> {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    let mut out = vec![Root::home(&home)];
    out.extend(repos_under(&[
        home.join("gyver").join("work"),
        home.join("personal"),
    ]));
    out
}

/// I repo che portano una `.claude/`, cercati sotto le cartelle di lavoro.
///
/// Profondità due e non di più: `~/gyver/work/suite` è un repo, ma scendere
/// oltre significherebbe entrare nelle copie di lavoro, dove le stesse regole
/// ricompaiono collegate — e l'inventario direbbe di avere venti volte le cose
/// che ha.
pub fn repos_under(bases: &[PathBuf]) -> Vec<Root> {
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    for base in bases {
        if base.join(".claude").is_dir() {
            found.insert(base.clone());
        }
        let Ok(entries) = fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(".claude").is_dir() {
                found.insert(path);
            }
        }
    }
    found.iter().map(|p| Root::repo(p)).collect()
}
