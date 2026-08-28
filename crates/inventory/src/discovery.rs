//! Come si trova ciò che è installato: le radici da cui Claude Code carica, il
//! filtro dei plugin spenti, e il frontmatter da cui escono nome e descrizione.
//!
//! DA DOVE VIENE QUESTO CODICE. Fino al 28/08/2026 viveva dentro
//! `claude-hooks::skill_nudge`, che è un porto di uno script Python con
//! un'equivalenza dimostrata contro `tools/oracle/skill-nudge.json`. Non è stato
//! riscritto: è stato spostato qui parola per parola, perché serve a due
//! chiamanti — il suggeritore di competenze e l'inventario — e la seconda copia
//! sarebbe divergita dalla prima al primo cambiamento di Claude Code.
//! L'equivalenza col Python resta la rete che prova lo spostamento: se questa
//! estrazione avesse cambiato un comportamento, l'oracolo lo direbbe.

use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// I posti da cui Claude Code carica davvero le competenze, nell'ordine in cui
/// le carica. ELENCO CHIUSO, non elenco di esclusioni: un elenco di posti da
/// saltare è sempre in ritardo sulla cartella nuova.
pub fn skill_sources(h: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (h.join(".claude/skills"), "*/SKILL.md"),
        (
            h.join(".claude/skills/mattpocock-skills/skills"),
            "*/*/SKILL.md",
        ),
        (h.join(".claude/plugins/cache"), "*/*/skills/*/SKILL.md"),
        (h.join(".claude/plugins/cache"), "*/*/*/skills/*/SKILL.md"),
    ]
}

pub fn agent_sources(h: &Path) -> Vec<(PathBuf, &'static str)> {
    vec![
        (h.join(".claude/agents"), "*.md"),
        (h.join(".claude/plugins/cache"), "*/*/agents/*.md"),
        (h.join(".claude/plugins/cache"), "*/*/*/agents/*.md"),
    ]
}

/// `Path.glob` per i soli schemi che servono qui: componenti letterali e `*`.
///
/// L'asterisco può portarsi dietro un suffisso (`*.md`), e allora aggancia per
/// prefisso e coda: sono le sole due forme che gli schemi qui usano.
///
/// `*` NON aggancia i nomi che cominciano con un punto, come in `pathlib` — ed è
/// la ragione per cui `.claude-plugin/` non compare mai fra i risultati e va
/// cercata a parte. L'ordine è quello di `readdir`, lo stesso da cui parte
/// `os.scandir` del Python: due letture della stessa cartella danno la stessa
/// sequenza, e da quella dipende quale descrizione vince fra due omonime.
pub fn glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let mut current = vec![root.to_path_buf()];
    let parts: Vec<&str> = pattern.split('/').collect();
    for (depth, part) in parts.iter().enumerate() {
        let last = depth + 1 == parts.len();
        let mut next = Vec::new();
        for dir in &current {
            if let Some((head, tail)) = part.split_once('*') {
                let Ok(entries) = fs::read_dir(dir) else {
                    continue;
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.')
                        || !name.starts_with(head)
                        || !name.ends_with(tail)
                        || name.len() < head.len() + tail.len()
                    {
                        continue;
                    }
                    let path = entry.path();
                    if last || path.is_dir() {
                        next.push(path);
                    }
                }
            } else {
                let path = dir.join(part);
                if path.exists() {
                    next.push(path);
                }
            }
        }
        current = next;
    }
    current
}

/// Prefisso con cui la competenza va invocata: `plugin:nome`.
pub fn prefix(path: &Path) -> String {
    let parts: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();
    if parts.iter().any(|p| p == "mattpocock-skills") {
        return "mattpocock-skills:".to_string();
    }
    if let Some(i) = parts.iter().position(|p| p == "cache") {
        // cache/<marketplace>/<plugin>/…
        return match parts.get(i + 2) {
            Some(name) => format!("{name}:"),
            None => String::new(), // IndexError di Python
        };
    }
    String::new()
}

/// Quali plugin sono accesi. Uno spento non offre competenze.
///
/// L'elenco vive in `settings.json`, non in `~/.claude.json`, dove la chiave
/// esiste ma vale `null`: leggere il posto sbagliato non dà errore, dà zero
/// plugin accesi e quindi un catalogo che tace sui plugin.
pub fn enabled_plugins(h: &Path) -> BTreeSet<String> {
    for path in [h.join(".claude/settings.json"), h.join(".claude.json")] {
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        let Some(map) = value.get("enabledPlugins").and_then(|v| v.as_object()) else {
            continue;
        };
        if map.is_empty() {
            continue;
        }
        return map
            .iter()
            .filter(|(_, v)| truthy(v))
            .map(|(k, _)| k.split('@').next().unwrap_or(k).to_string())
            .collect();
    }
    BTreeSet::new()
}

/// `bool(v)` di Python: vero anche per un numero diverso da zero o una stringa
/// non vuota, non solo per `true`.
pub fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().unwrap_or(0.0) != 0.0,
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// Nomi dichiarati nel `plugin.json` che governa questa competenza.
///
/// Serve perché sul disco restano competenze che il plugin non carica:
/// mattpocock ne dichiara 25 e ne tiene 35 nelle cartelle. `None` significa
/// «nessun filtro»: è il caso della cartella intera (`["./skills/"]`), che
/// letta come un nome farebbe sparire dal catalogo tutte le competenze del
/// plugin, in silenzio. Il manifesto si cerca risalendo, perché la sua distanza
/// dalla competenza cambia da plugin a plugin.
pub fn manifest(from: &Path) -> Option<BTreeSet<String>> {
    let mut cur = from.to_path_buf();
    for _ in 0..5 {
        cur = cur.parent()?.to_path_buf();
        let p = cur.join(".claude-plugin").join("plugin.json");
        if !p.exists() {
            continue;
        }
        let entries = fs::read_to_string(&p)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("skills").cloned());
        let Some(Value::Array(items)) = entries else {
            return None;
        };
        if items.is_empty() {
            return None;
        }
        // «Cartella intera» si riconosce dalla FORMA del percorso — una sola
        // tappa, tipo `./skills/` — non dalla parola finale: una competenza può
        // chiamarsi `setup-matt-pocock-skills`.
        let segments = |v: &str| {
            v.trim_matches(|c| c == '.' || c == '/')
                .split('/')
                .filter(|x| !x.is_empty())
                .count()
        };
        let declared: Vec<&str> = items.iter().filter_map(|v| v.as_str()).collect();
        if declared.len() != items.len() {
            // `Path(v)` su un non-testo solleva: come sopra, il gancio muore in
            // silenzio — ma qui l'eccezione è dentro `_scandisci`, che è già
            // avvolto dal `try` di `catalogo()` e vale «catalogo vuoto».
            return None;
        }
        if declared.iter().any(|v| segments(v) <= 1) {
            return None;
        }
        return Some(
            declared
                .iter()
                .map(|v| {
                    Path::new(v)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default()
                })
                .collect(),
        );
    }
    None
}

/// (nome, descrizione) dal frontmatter, o niente se non è invocabile.
pub fn frontmatter(path: &Path) -> Option<(String, String)> {
    let matter = matter_of(path)?;
    let name = Regex::new(r"(?m)^name:\s*(\S+)")
        .ok()?
        .captures(&matter)?
        .get(1)?
        .as_str()
        .trim()
        .to_string();
    Some((name, description(&matter, false)))
}

/// (nome, descrizione) di un comando di `commands/`.
///
/// Non riusa `frontmatter`: quella pretende un campo `name`, e un comando non ce
/// l'ha — si chiama come il suo file. Senza i comandi il catalogo diceva che
/// `handoff` non esiste, e questo gancio taceva proprio sul consiglio che
/// serviva di più (misurato il 14/08/2026).
pub fn command(path: &Path) -> Option<(String, String)> {
    let matter = matter_of(path)?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    Some((stem, description(&matter, true)))
}

/// Le cartelle di plugin che Claude Code carica davvero, una per plugin.
///
/// PERCHÉ NON BASTA CAMMINARE NELLA CACHE, con la misura del 28/08/2026. Sotto
/// `plugins/cache/` restano **tutte** le versioni mai scaricate: la prima
/// versione di questo inventario contava 756 agenti, di cui 7 veri e il resto
/// copie vecchie dello stesso plugin — `pr-review-toolkit` da solo ne teneva
/// decine. La versione in uso la dichiara `installed_plugins.json`, ed è l'unica
/// fonte che lo sa: sul disco le copie sono indistinguibili.
///
/// Un file assente o illeggibile dà un elenco vuoto, che il chiamante deve
/// trattare come «non lo so», non come «nessun plugin installato».
pub fn installed_paths(h: &Path) -> BTreeSet<PathBuf> {
    let path = h.join(".claude/plugins/installed_plugins.json");
    let Ok(text) = fs::read_to_string(&path) else {
        return BTreeSet::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return BTreeSet::new();
    };
    let Some(plugins) = value.get("plugins").and_then(|v| v.as_object()) else {
        return BTreeSet::new();
    };
    plugins
        .values()
        .filter_map(|v| v.as_array())
        .flatten()
        .filter_map(|entry| entry.get("installPath").and_then(|v| v.as_str()))
        .map(PathBuf::from)
        .collect()
}

/// Il frontmatter grezzo e se il modello può invocare ciò che descrive.
///
/// LA DIFFERENZA CON `matter_of` È IL PUNTO. Il suggeritore deve tacere su una
/// competenza che il modello non può invocare, quindi `matter_of` la scarta.
/// L'inventario invece deve mostrarla: `/learn` e `/work-loop-headless` portano
/// `disable-model-invocation: true`, esistono, e una persona li invoca a mano —
/// nasconderli dall'elenco di ciò che si ha sarebbe la stessa bugia che
/// l'inventario esiste per togliere.
pub fn matter_and_invocability(path: &Path) -> Option<(String, bool)> {
    let raw = fs::read(path).ok()?;
    let text: String = String::from_utf8_lossy(&raw).chars().take(4000).collect();
    if !text.starts_with("---") {
        return None;
    }
    let end = text[3..].find("\n---").map(|i| i + 3);
    let matter = match end {
        Some(i) if i > 0 => text[3..i].to_string(),
        _ => text[3..].to_string(),
    };
    let by_model = !matter.contains("disable-model-invocation: true");
    Some((matter, by_model))
}

/// Il frontmatter grezzo, con gli stessi tagli dell'originale.
pub fn matter_of(path: &Path) -> Option<String> {
    let raw = fs::read(path).ok()?;
    // `errors='replace'` e poi `[:4000]`: **caratteri**, non byte.
    let text: String = String::from_utf8_lossy(&raw).chars().take(4000).collect();
    if !text.starts_with("---") {
        return None;
    }
    let end = text[3..].find("\n---").map(|i| i + 3);
    // `text.find('\n---', 3)` torna -1 se non c'è, e `text[3:-1]` in Python
    // sarebbe tutto meno l'ultimo carattere; l'originale però confronta
    // `end_ > 0`, quindi il caso «non trovato» prende `text[3:4000]`.
    let matter = match end {
        Some(i) if i > 0 => text[3..i].to_string(),
        _ => text[3..].to_string(),
    };
    if matter.contains("disable-model-invocation: true") {
        return None;
    }
    Some(matter)
}

/// `^description:\s*(.+?)(?=\n\w+:|\Z)` con `re.M | re.S`, riscritta a mano.
///
/// LO SGUARDO AVANTI NON ESISTE nella crate `regex`, e qui non basta consumare
/// il carattere seguente come negli schemi delle competenze: il gruppo catturato
/// **è** il valore, quindi mangiare l'a-capo del campo successivo lo
/// sporcherebbe. Si riproduce la semantica direttamente: il gruppo è la
/// stringa non vuota più corta dopo la quale comincia o la fine del testo o un
/// a-capo seguito da un nome di campo e dai due punti. `commands/` usa
/// `\w[\w-]*` invece di `\w+`, e la differenza è vera: un campo col trattino
/// (`argument-hint:`) chiude la descrizione lì.
pub fn description(matter: &str, hyphen_in_field: bool) -> String {
    let Some(start) = field_start(matter, "description:") else {
        return String::new();
    };
    let rest = &matter[start..];
    let value = rest.trim_start_matches([' ', '\t', '\r', '\n', '\u{b}', '\u{c}']);
    // Il gruppo è `(.+?)`, quindi almeno un carattere: la ricerca del terminatore
    // comincia dopo il primo.
    let bytes: Vec<(usize, char)> = value.char_indices().collect();
    let mut cut = value.len();
    for (i, c) in bytes.iter().skip(1) {
        if *c != '\n' {
            continue;
        }
        let after = &value[i + 1..];
        let mut chars = after.chars();
        let first = chars.next();
        let is_word = |c: Option<char>| c.map(|c| c.is_alphanumeric() || c == '_') == Some(true);
        if !is_word(first) {
            continue;
        }
        let mut end = 1;
        for c in chars {
            if c.is_alphanumeric() || c == '_' || (hyphen_in_field && c == '-') {
                end += c.len_utf8();
            } else {
                break;
            }
        }
        if after[end..].starts_with(':') {
            cut = *i;
            break;
        }
    }
    // `' '.join(x.split())` e poi `.strip('>|- ')`.
    let collapsed = value[..cut].split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed
        .trim_matches(|c| c == '>' || c == '|' || c == '-' || c == ' ')
        .to_string()
}

/// L'inizio del valore di un campo a inizio riga (`(?m)^campo:`).
pub fn field_start(matter: &str, field: &str) -> Option<usize> {
    let mut offset = 0;
    for line in matter.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix(field) {
            return Some(offset + line.len() - rest.len());
        }
        offset += line.len();
    }
    None
}
