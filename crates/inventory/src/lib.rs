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
    /// Una cartella di competenze che **Claude Code non carica**: quello che ci
    /// sta esiste sul disco e da qui non è invocabile.
    ///
    /// «NON LO CARICA NESSUNO» SAREBBE FALSO, ed è stato scritto qui per mezza
    /// giornata prima che una misura lo smentisse. `~/.agents/skills` è
    /// referenziato da due altri harness — `~/.factory` e `~/.commandcode` —
    /// con **767 collegamenti ciascuno**, di cui **1.508 rotti**: il magazzino
    /// si è svuotato da oltre 767 voci alle 33 di oggi, e nessuno dei due lati
    /// se n'è accorto. Quindi il verdetto giusto è ristretto a ciò che questo
    /// programma può sapere: da **qui** non si invocano, e chi le vuole le
    /// collega. Che le carichi qualcun altro è una cosa che l'inventario, per
    /// ora, non guarda — e dirlo è più onesto che affermare il contrario.
    pub is_warehouse: bool,
}

impl Root {
    pub fn home(path: &Path) -> Root {
        Root {
            label: "casa".to_string(),
            path: path.to_path_buf(),
            is_home: true,
            is_warehouse: false,
        }
    }

    pub fn repo(path: &Path) -> Root {
        Root {
            label: named_after(path),
            path: path.to_path_buf(),
            is_home: false,
            is_warehouse: false,
        }
    }

    /// Una cartella di competenze che nessuna configurazione carica. `path` è
    /// la cartella che le contiene direttamente, non la radice di un repo.
    pub fn warehouse(label: &str, path: &Path) -> Root {
        Root {
            label: label.to_string(),
            path: path.to_path_buf(),
            is_home: false,
            is_warehouse: true,
        }
    }
}

fn named_after(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
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
    /// **DOVE NON HA POTUTO GUARDARE**, col motivo. È il completamento di
    /// `roots`: dire dove si è cercato rende l'elenco smentibile, dire dove non
    /// si è riusciti a cercare lo rende onesto.
    #[serde(default)]
    pub unseen: Vec<String>,
    /// Se qualcuno ha dichiarato dove cercare. Quando è `false`, un inventario
    /// magro non dice che la macchina è vuota: dice che nessuno ha detto dove
    /// guardare.
    #[serde(default)]
    pub bases_declared: bool,
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
/// L'inventario di una ricognizione, **con dentro anche ciò che non si è visto**.
///
/// Esiste accanto a `collect` e non al suo posto perché chi costruisce le radici
/// a mano — le prove — non ha nessun rendiconto da passare, e obbligarlo a
/// fabbricarne uno vuoto sposterebbe la bugia di un gradino invece di toglierla.
pub fn collect_survey(survey: &Survey) -> Inventory {
    let mut out = collect(&survey.roots);
    out.unseen = survey
        .unreadable
        .iter()
        .map(|u| format!("{}: {}", u.path.display(), u.reason))
        .collect();
    out.bases_declared = survey.bases_declared;
    out
}

pub fn collect(roots: &[Root]) -> Inventory {
    let mut entries = Vec::new();
    let mut stale = 0usize;
    let home = roots.iter().find(|r| r.is_home).map(|r| r.path.as_path());
    for root in roots {
        if root.is_home {
            let (found, dropped) = home_skills(&root.path);
            entries.extend(found);
            stale += dropped;
            let (found, dropped) = home_agents(&root.path);
            entries.extend(found);
            stale += dropped;
        } else if root.is_warehouse {
            entries.extend(warehouse_skills(root, home));
            continue;
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
        // VUOTI PERCHÉ CHI CHIAMA `collect` NON HA UN RENDICONTO: ha costruito
        // le radici da sé e sa cosa gli ha dato. Li riempie `collect_survey`,
        // che invece parte da una ricognizione e sa anche cosa le è mancato.
        unseen: Vec::new(),
        bases_declared: true,
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
            let origin = discovery::origin(&path);
            let prefix = origin.prefix();
            // **CHI PUÒ SPEGNERLA DIPENDE DA DOVE VIENE**, e il `match` lo dice
            // in tre righe. Prima erano tre rami di cui i primi due tornavano
            // la stessa cosa, e dentro il secondo stava nascosto il nome di una
            // persona: una condizione il cui esito non cambia niente non si
            // legge, si scorre — ed è per questo che quella riga è sopravvissuta.
            let reach = match &origin {
                discovery::Origin::Home | discovery::Origin::Collection(_) => Reach::Active,
                discovery::Origin::Plugin(name) if on.contains(name) => Reach::Active,
                discovery::Origin::Plugin(name) => {
                    Reach::Inactive(format!("il plugin {name} non è abilitato"))
                }
            };
            let declared = discovery::manifest(&path);
            let reach = match (&declared, path.parent().and_then(|p| p.file_name())) {
                (Some(names), Some(own)) if !names.contains(&own.to_string_lossy().into_owned()) => {
                    Reach::Inactive(format!(
                        "sta sul disco ma il manifesto di {} non la dichiara",
                        prefix.trim_end_matches(':')
                    ))
                }
                _ => reach,
            };
            out.push(Entry {
                kind: Kind::Skill,
                name: format!("{prefix}{name}"),
                description,
                // **DA DOVE VIENE SI DICE CON LA PAROLA GIUSTA**: una raccolta
                // installata come cartella non è un plugin, e chiamarla così in
                // un elenco che una persona legge la manda a cercarla fra i
                // plugin, dove non c'è.
                origin: match &origin {
                    discovery::Origin::Home => "casa".to_string(),
                    discovery::Origin::Plugin(name) => format!("plugin {name}"),
                    discovery::Origin::Collection(name) => format!("raccolta {name}"),
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

/// Le competenze di una cartella che nessuna configurazione carica.
///
/// Ce n'è una sola oggi, e vale la pena dire perché non è un caso limite: 95
/// competenze scritte, cinque raggiungibili. Non è un difetto da riparare qui —
/// collegarle è una decisione, non una manutenzione — ma finché l'elenco tace,
/// quella decisione non arriva mai perché nessuno sa che c'è da prenderla.
fn warehouse_skills(root: &Root, home: Option<&Path>) -> Vec<Entry> {
    discovery::glob(&root.path, "*/SKILL.md")
        .into_iter()
        .filter_map(|path| {
            let (name, description, by_model) = named(&path)?;
            let folder = path.parent()?.file_name()?.to_string_lossy().into_owned();
            // UNA COMPETENZA COLLEGATA È RAGGIUNGIBILE, e va detto: cinque di
            // queste lo sono. Dichiararle spente perché stanno nel magazzino
            // sarebbe lo stesso errore al contrario — l'inventario perderebbe
            // credito proprio sulle voci che sa giudicare.
            let linked = home
                .map(|h| h.join(".claude").join("skills").join(&folder).exists())
                .unwrap_or(false);
            Some(Entry {
                kind: Kind::Skill,
                name,
                description,
                origin: format!("magazzino {}", root.label),
                path: path.to_string_lossy().into_owned(),
                reach: if linked {
                    Reach::Active
                } else {
                    Reach::Inactive(
                        "sta in una cartella che nessuna configurazione carica: per invocarla va \
                         collegata fra le competenze di casa"
                            .to_string(),
                    )
                },
                by_model,
            })
        })
        .collect()
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
                    name: format!("{event} · {matcher} · {}", hook_label(command)),
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

/// Come si chiama un gancio, per chi lo legge: non il comando intero, ma il
/// pezzo che lo distingue dagli altri.
///
/// SENZA QUESTO DUE GANCI DIVENTANO UNO. Evento e matcher non bastano a
/// identificarli: su `PreToolUse · Bash` ne vivono **otto**, e depositarli con
/// la stessa chiave ne faceva sparire sette in silenzio — misurato il
/// 28/08/2026, 30 voci perse su 358. Un elenco che perde per collisione è
/// peggio di un elenco che non c'è, perché sembra completo.
fn hook_label(command: &str) -> String {
    // LE OPZIONI DISTINGUONO, e la prima versione le buttava: due ganci che
    // lanciano lo stesso sottocomando con opzioni diverse — `orca-cleanup
    // --close` e `orca-cleanup --names --rename` — sono due ganci, e scartare
    // le opzioni li faceva collassare in uno.
    const SHELL_NOISE: &[&str] = &["cd", "||", "&&", ";", "&", "true", "exec", "nohup", "sh", "-c"];
    let words: Vec<&str> = command.split_whitespace().collect();
    // L'eseguibile è l'ultima parola che è un percorso: prima di lei ci sono
    // solo preamboli di shell, dopo ci sono gli argomenti che contano.
    let executable = words.iter().rposition(|word| {
        word.contains('/') && !word.starts_with('>') && !word.contains(">/") && !word.contains("</")
    });
    let mut label: Vec<&str> = words
        .iter()
        .skip(executable.map_or(0, |index| index + 1))
        .copied()
        .filter(|word| {
            !word.contains('/')
                && !word.starts_with('>')
                && !SHELL_NOISE.contains(word)
                && !word.is_empty()
        })
        .collect();
    if label.is_empty() {
        // Nessun argomento: allora il gancio è lo script che lancia.
        if let Some(index) = executable {
            label.push(words[index].rsplit('/').next().unwrap_or(words[index]));
        }
    }
    if label.is_empty() {
        return command.to_string();
    }
    let joined = label
        .join(" ")
        .trim_matches(|c: char| c == '"' || c == '\'' || c == ';')
        .to_string();
    // Un gancio scritto come programma in linea non ha un nome: ha un corpo.
    // Si tronca, perché serve a distinguerlo, non a raccontarlo — e il comando
    // per intero resta nella descrizione, dove chi vuole leggerlo lo trova.
    const NAME_CEILING: usize = 48;
    if joined.chars().count() <= NAME_CEILING {
        return joined;
    }
    let short: String = joined.chars().take(NAME_CEILING).collect();
    format!("{short}…")
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

/// Il rendiconto di una ricognizione: cosa si è trovato, **e cosa non si è
/// potuto guardare**.
///
/// I DUE ELENCHI ESISTONO PERCHÉ UNO SOLO MENTE. Fino al 01/09/2026
/// `repos_under` incontrava una base illeggibile, faceva `continue` e
/// restituiva un elenco più corto: «non ce ne sono» e «non ho potuto guardare»
/// arrivavano a chi legge nella stessa forma, e portano a decisioni opposte. È
/// la forma del guasto 12, dove dentro un perimetro `launchctl` risponde vuoto
/// invece di negare — e la stessa che il 01/09 ha fatto dichiarare «nessuna CLI
/// in esecuzione» mentre ne giravano cinque, perché `ps` incanalato non dice
/// che gli è stato negato.
#[derive(Debug, Default)]
pub struct Survey {
    /// Le radici trovate davvero.
    pub roots: Vec<Root>,
    /// Le basi che non si sono potute leggere, col motivo.
    pub unreadable: Vec<Unreadable>,
    /// Se qualcuno ha dichiarato dove guardare. `false` significa che un elenco
    /// vuoto non dice niente sul mondo: dice che nessuno ha detto dove cercare.
    pub bases_declared: bool,
}

/// Una base che non si è potuta leggere, e perché.
#[derive(Debug)]
pub struct Unreadable {
    pub path: PathBuf,
    pub reason: String,
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
pub fn default_roots(config_dir: Option<&Path>) -> Survey {
    let home = std::env::var("HOME").map(PathBuf::from).unwrap_or_default();
    default_roots_from(&home, &declared_bases(config_dir))
}

/// La stessa regola applicata a una casa e a basi dichiarate, invece che a
/// quelle di questo processo.
///
/// **SEPARATA PERCHÉ ALTRIMENTI NON SI PROVA.** Una funzione che legge
/// l'ambiente si può provare solo cambiando l'ambiente del processo, e le prove
/// girano in parallelo: la prima che tocca una variabile falsa le altre.
pub fn default_roots_from(home: &Path, bases: &[PathBuf]) -> Survey {
    let mut survey = repos_under(bases);
    survey.roots.insert(0, Root::home(home));
    // I MAGAZZINI SI CERCANO DOVE SI GUARDA, non dove guardava una persona. Il
    // primo è quello di casa; gli altri stanno dentro le basi dichiarate, e se
    // non ne è dichiarata nessuna non ce ne sono — che è la verità, non un
    // ripiego.
    let mut warehouses = vec![(".agents".to_string(), home.join(".agents").join("skills"))];
    for base in bases {
        let label = base
            .file_name()
            .map(|n| format!("{}/.agents", n.to_string_lossy()))
            .unwrap_or_else(|| ".agents".to_string());
        warehouses.push((label, base.join(".agents").join("skills")));
    }
    // DUE MAGAZZINI SONO DAVVERO DUE: i collegamenti dentro le competenze di
    // casa puntano al primo, mai al secondo. Trattarli come uno solo faceva
    // dire «nessuna è collegata» anche di quelle che lo sono.
    for (label, path) in warehouses {
        if path.is_dir() {
            survey.roots.push(Root::warehouse(&label, &path));
        }
    }
    survey
}

/// Le basi di lavoro **dichiarate**: `SAILOR_WORK_ROOTS` se c'è, altrimenti il
/// file `work-roots` nella casa di Sailor, una riga per base.
///
/// FINO AL 01/09/2026 QUI C'ERANO `~/gyver/work` E `~/personal`, compilate. Su
/// questa macchina esistono, quindi il difetto non si vedeva; su qualunque
/// altra l'inventario avrebbe risposto «zero repo» con uscita 0, che è
/// indistinguibile da una macchina davvero vuota. Le cartelle di una persona
/// sola non sono un fatto della macchina: sono configurazione, e adesso
/// vivono lì. Chi non dichiara niente ottiene `bases_declared` a `false`, così
/// chi legge può dire *non me l'hai detto* invece di *non c'è niente*.
pub fn declared_bases(config_dir: Option<&Path>) -> Vec<PathBuf> {
    if let Ok(declared) = std::env::var("SAILOR_WORK_ROOTS") {
        let bases: Vec<PathBuf> = declared
            .split(':')
            .filter(|piece| !piece.trim().is_empty())
            .map(PathBuf::from)
            .collect();
        if !bases.is_empty() {
            return bases;
        }
    }
    let Some(dir) = config_dir else {
        return Vec::new();
    };
    let Ok(text) = fs::read_to_string(dir.join("work-roots")) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(PathBuf::from)
        .collect()
}

/// I repo che portano una `.claude/`, cercati sotto le cartelle di lavoro.
///
/// Profondità due e non di più: `~/gyver/work/suite` è un repo, ma scendere
/// oltre significherebbe entrare nelle copie di lavoro, dove le stesse regole
/// ricompaiono collegate — e l'inventario direbbe di avere venti volte le cose
/// che ha.
pub fn repos_under(bases: &[PathBuf]) -> Survey {
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    let mut unreadable = Vec::new();
    for base in bases {
        if base.join(".claude").is_dir() {
            found.insert(base.clone());
        }
        // IL `continue` CHE C'ERA QUI SI MANGIAVA IL MOTIVO. Una base che non si
        // apre e una base vuota producevano lo stesso elenco più corto, e chi
        // leggeva concludeva «non ce ne sono» in tutti e due i casi.
        let entries = match fs::read_dir(base) {
            Ok(entries) => entries,
            Err(why) => {
                unreadable.push(Unreadable {
                    path: base.clone(),
                    reason: why.to_string(),
                });
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join(".claude").is_dir() {
                found.insert(path);
            }
        }
    }
    Survey {
        roots: found.iter().map(|p| Root::repo(p)).collect(),
        unreadable,
        bases_declared: !bases.is_empty(),
    }
}
