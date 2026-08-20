//! `UserPromptSubmit`: dice quale competenza già esiste per il lavoro che arriva.
//!
//! Il giudizio sta in `guards::skill_nudge`; qui c'è ciò che tocca il mondo: il
//! payload su stdin, la stazza della trascrizione, il catalogo delle competenze
//! installate, il marcatore del freno, il registro e la riga su stdout.
//!
//! PERCHÉ ESISTE. Misura dell'11/08/2026 su 1.079 sessioni in trenta giorni:
//! `Skill` è stata invocata 81 volte contro 96.513 chiamate a strumenti — lo
//! 0,08% — e metà di quelle 81 riguarda una competenza che si attiva da sola.
//! Non manca niente: quello che è stato scritto e collaudato non arriva nel
//! momento in cui servirebbe, e il lavoro viene rifatto a mano.
//!
//! IL CATALOGO STA QUI E NON IN `guards`, ed è la parte più grossa del file.
//! L'originale lo importa da `scripts/catalogo_skill.py`, che scandisce il disco
//! e tiene una cache oraria. Non è un di più: è l'unica cosa che impedisce di
//! nominare una competenza che non c'è, e chi la nomina manda a cercare
//! l'inesistente. Portarlo era obbligatorio anche per un secondo motivo
//! misurabile — **solo questo gancio** usa quella cache, quindi un porto che si
//! limitasse a leggerla la lascerebbe scadere per sempre, e da lì in poi ogni
//! nome risulterebbe raggiungibile.
//!
//! IL REGISTRO È QUELLO DI `hook_log.py`, cioè `json.dumps(ensure_ascii=False)`:
//! separatori `", "` e `": "`, con lo spazio. Non quello senza spazi di
//! `journal::record`, che riproduce il JavaScript. Le due forme convivono già in
//! `ganci.jsonl` e chi porta un gancio deve guardare quale usa il suo originale.
//!
//! NON BLOCCA MAI: esce sempre 0, e la riga va su stdout, che qui è il canale
//! che finisce dritto nel contesto del modello.

use guards::skill_nudge::{lines, threshold_mb, CHAR_CUTOFF, PASSARE};
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const CEILING_BYTES: u64 = 5 * 1024 * 1024;
/// La validità della cache del catalogo, come in `catalogo_skill.py`.
const CACHE_VALID_SEC: u64 = 3600;

/// Copie per altri runtime: stessi nomi, non le legge Claude Code. Misurate 44
/// l'11/08/2026, ed erano la metà del gonfiore del conto «485».
const OTHER_RUNTIMES: &[&str] = &[
    "skills-codex",
    "opencode",
    ".codex",
    "node_modules",
    "backups",
    "tests",
    "fixtures",
    "skills-archivio-2026-08-11",
];

fn home() -> PathBuf {
    // Come `Path.home()` dell'originale: dall'ambiente, così il confronto di
    // equivalenza può spostarla senza toccare nessuna delle due implementazioni.
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into()))
}

fn state_dir() -> PathBuf {
    home().join(".claude").join("state")
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── il catalogo delle competenze raggiungibili ──────────────────────────────

/// Quello che `catalogo()` restituisce, con i modi storti compresi.
///
/// Non è un semplice dizionario perché in Python il valore torna da
/// `json.loads` e può essere qualunque cosa: `exists` fa `name in c`, che su una
/// stringa è una sottostringa, su una lista un confronto elemento per elemento,
/// e su un numero un `TypeError` che spegne l'intero gancio. Schiacciare i
/// quattro casi in uno farebbe divergere il porto su una cache scritta a metà.
enum Catalog {
    Map(Vec<String>),
    List(Vec<Value>),
    Text(String),
    /// `name in 5` è un `TypeError`: l'eccezione risale fino a `main` e il
    /// gancio esce muto, senza registrare e senza marcatore.
    NotIterable,
}

impl Catalog {
    fn from(value: Value) -> Catalog {
        match value {
            Value::Object(map) => Catalog::Map(map.keys().cloned().collect()),
            Value::Array(items) => Catalog::List(items),
            Value::String(s) => Catalog::Text(s),
            // `if not c: return True` — zero, falso e nullo sono falsi in
            // Python, quindi non arrivano mai al `in`.
            Value::Null | Value::Bool(false) => Catalog::Map(Vec::new()),
            Value::Number(n) if n.as_f64() == Some(0.0) => Catalog::Map(Vec::new()),
            _ => Catalog::NotIterable,
        }
    }

    fn contains(&self, name: &str) -> Option<bool> {
        match self {
            Catalog::Map(keys) if keys.is_empty() => Some(true),
            Catalog::Map(keys) => Some(keys.iter().any(|k| k == name)),
            Catalog::List(items) if items.is_empty() => Some(true),
            Catalog::List(items) => Some(items.iter().any(|v| v.as_str() == Some(name))),
            Catalog::Text(s) if s.is_empty() => Some(true),
            Catalog::Text(s) => Some(s.contains(name)),
            Catalog::NotIterable => None,
        }
    }
}

fn cache_path() -> PathBuf {
    state_dir().join("catalogo-skill.json")
}

/// Nome invocabile → descrizione. Cache su disco, validità un'ora.
fn catalog() -> Catalog {
    let path = cache_path();
    if let Ok(meta) = fs::metadata(&path) {
        let fresh = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            // `time.time() - mtime < 3600`: una data nel futuro è «fresca» anche
            // per l'originale, e la sottrazione satura invece di andare sotto zero.
            .map(|d| now().saturating_sub(d.as_secs()) < CACHE_VALID_SEC)
            .unwrap_or(false);
        if fresh {
            if let Ok(text) = fs::read_to_string(&path) {
                if let Ok(value) = serde_json::from_str::<Value>(&text) {
                    return Catalog::from(value);
                }
            }
        }
    }
    let scanned = scan();
    // La cache si riscrive anche quando è vuota: è il caso di una HOME senza
    // competenze, ed è osservabile — il file compare.
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(&path, dumps_indent1(&scanned));
    Catalog::Map(scanned.into_iter().map(|(k, _)| k).collect())
}

/// `json.dumps(c, ensure_ascii=False, indent=1)`: un solo spazio di rientro, e
/// gli accenti restano sé stessi. Il dizionario vuoto è `{}` su una riga sola.
fn dumps_indent1(entries: &[(String, String)]) -> String {
    if entries.is_empty() {
        return "{}".to_string();
    }
    let mut out = String::from("{\n");
    for (i, (key, value)) in entries.iter().enumerate() {
        if i > 0 {
            out.push_str(",\n");
        }
        out.push(' ');
        out.push_str(&hook_io::python_json::dumps_unicode(&Value::String(key.clone())));
        out.push_str(": ");
        out.push_str(&hook_io::python_json::dumps_unicode(&Value::String(value.clone())));
    }
    out.push_str("\n}");
    out
}

/// I posti da cui Claude Code carica davvero le competenze, nell'ordine in cui
/// le carica. ELENCO CHIUSO, non elenco di esclusioni: un elenco di posti da
/// saltare è sempre in ritardo sulla cartella nuova.
fn skill_sources(h: &Path) -> Vec<(PathBuf, &'static str)> {
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

fn agent_sources(h: &Path) -> Vec<(PathBuf, &'static str)> {
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
fn glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
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
fn prefix(path: &Path) -> String {
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
fn enabled_plugins(h: &Path) -> BTreeSet<String> {
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
fn truthy(v: &Value) -> bool {
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
fn manifest(from: &Path) -> Option<BTreeSet<String>> {
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
fn frontmatter(path: &Path) -> Option<(String, String)> {
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
fn command(path: &Path) -> Option<(String, String)> {
    let matter = matter_of(path)?;
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    Some((stem, description(&matter, true)))
}

/// Il frontmatter grezzo, con gli stessi tagli dell'originale.
fn matter_of(path: &Path) -> Option<String> {
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
fn description(matter: &str, hyphen_in_field: bool) -> String {
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
fn field_start(matter: &str, field: &str) -> Option<usize> {
    let mut offset = 0;
    for line in matter.split_inclusive('\n') {
        if let Some(rest) = line.strip_prefix(field) {
            return Some(offset + line.len() - rest.len());
        }
        offset += line.len();
    }
    None
}

fn skipped_runtime(path: &Path) -> bool {
    path.components()
        .any(|c| OTHER_RUNTIMES.contains(&c.as_os_str().to_string_lossy().as_ref()))
}

/// La scansione vera. Le voci si inseriscono con `setdefault`: la prima vince.
fn scan() -> Vec<(String, String)> {
    let h = home();
    let on = enabled_plugins(&h);
    let mut out: Vec<(String, String)> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let add = |out: &mut Vec<(String, String)>, seen: &mut BTreeSet<String>, k: String, v: String| {
        if seen.insert(k.clone()) {
            out.push((k, v));
        }
    };

    let mut manifests: std::collections::HashMap<PathBuf, Option<BTreeSet<String>>> =
        std::collections::HashMap::new();
    for (root, pattern) in skill_sources(&h) {
        for p in glob(&root, pattern) {
            if skipped_runtime(&p) {
                continue;
            }
            let pre = prefix(&p);
            if !pre.is_empty()
                && !on.contains(&pre[..pre.len() - 1])
                && !pre.contains("mattpocock")
            {
                continue; // plugin spento: le sue competenze non sono raggiungibili
            }
            let Some(parent) = p.parent() else { continue };
            let Some(key) = parent.parent() else { continue };
            let declared = manifests
                .entry(key.to_path_buf())
                .or_insert_with(|| manifest(&p));
            if let Some(names) = declared {
                let own = parent
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                if !names.contains(&own) {
                    continue;
                }
            }
            if let Some((name, desc)) = frontmatter(&p) {
                add(&mut out, &mut seen, format!("{pre}{name}"), desc);
            }
        }
    }

    // I comandi di `~/.claude/commands/`: si invocano per nome esattamente come
    // una competenza (`/handoff`, o `Skill(handoff)`), ma non hanno un
    // `SKILL.md`. `sorted()` dell'originale, che qui è l'ordine dei percorsi.
    let mut commands = glob(&h.join(".claude/commands"), "*.md");
    commands.sort();
    for p in commands {
        if skipped_runtime(&p) {
            continue;
        }
        if let Some((name, desc)) = command(&p) {
            add(&mut out, &mut seen, name, desc);
        }
    }

    // Gli agenti: stesso frontmatter, altra cartella, e si nominano allo stesso
    // modo. Il manifesto qui non si guarda: elenca le competenze, non gli
    // agenti, quindi filtrarci gli agenti li scarterebbe tutti.
    for (root, pattern) in agent_sources(&h) {
        for p in glob(&root, pattern) {
            if skipped_runtime(&p) {
                continue;
            }
            let pre = prefix(&p);
            if !pre.is_empty() && !on.contains(&pre[..pre.len() - 1]) {
                continue;
            }
            if let Some((name, desc)) = frontmatter(&p) {
                add(&mut out, &mut seen, format!("{pre}{name}"), desc);
            }
        }
    }
    out
}

// ── il resto del mondo ──────────────────────────────────────────────────────

/// `os.path.getsize(path) // 1_000_000`, con lo zero su tutto ciò che non è un
/// file leggibile.
///
/// RESIDUO DICHIARATO: in Python un `transcript_path` **numerico** sarebbe letto
/// come descrittore di file (`os.stat(1)` è stdout), non come percorso. Qui vale
/// zero. Nessun evento di Claude Code manda un numero lì, e su un descrittore di
/// pipa entrambe le implementazioni darebbero comunque zero.
fn size_mb(value: Option<&Value>) -> u64 {
    let Some(Value::String(path)) = value else {
        return 0;
    };
    fs::metadata(path).map(|m| m.len() / 1_000_000).unwrap_or(0)
}

/// `str(x)` di Python, quel tanto che serve prima della ripulitura.
///
/// Del risultato sopravvivono solo `[A-Za-z0-9_-]`, quindi le virgolette e gli
/// spazi del `repr` non contano: conta che una lista `["a"]` diventi `a` e non
/// stringa vuota, come nel caso storto già previsto dalle prove dell'originale.
fn python_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => {
            if n.as_f64() == Some(0.0) {
                String::new() // `0 or ''` è `''`
            } else {
                n.to_string()
            }
        }
        Some(Value::Array(items)) => {
            if items.is_empty() {
                String::new() // lista vuota: falsa, quindi `''`
            } else {
                let inner: Vec<String> = items.iter().map(|v| python_repr(v)).collect();
                format!("[{}]", inner.join(", "))
            }
        }
        Some(Value::Object(map)) => {
            if map.is_empty() {
                String::new()
            } else {
                let inner: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("'{}': {}", k, python_repr(v)))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
        }
    }
}

fn python_repr(value: &Value) -> String {
    match value {
        Value::String(s) => format!("'{s}'"),
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Number(n) => n.to_string(),
        other => python_str(Some(other)),
    }
}

/// La riga del registro, nella forma che scrive `hook_log.registra`.
///
/// Registrare COSA è stato suggerito è l'unico modo di sapere, fra un mese, se
/// il suggerimento è stato raccolto: il confronto fra questo registro e le
/// invocazioni di `Skill` nelle trascrizioni è l'esperimento che oggi non si può
/// fare, perché 83 usi in 45 giorni non bastano a dire se una competenza
/// migliora il risultato.
fn log_suggestion(session: &str, reason: &str, skills: &[String], chars: usize) {
    let line = hook_io::python_json::dumps_unicode(&serde_json::json!({
        "t": hook_io::journal::now_iso8601_python(),
        "gancio": "innesco",
        "decisione": "suggerisce",
        "motivo": reason,
        "session": session,
        "competenze": skills,
        "caratteri": chars,
    }));
    let dir = state_dir();
    let path = dir.join("ganci.jsonl");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > CEILING_BYTES {
            let _ = fs::rename(&path, path.with_extension("jsonl.1"));
        }
    }
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// I nomi fra apici rovesci, che è come il registro sa di cosa si è parlato.
fn quoted_names(text: &str) -> Vec<String> {
    static PATTERN: std::sync::OnceLock<Regex> = std::sync::OnceLock::new();
    let re = PATTERN.get_or_init(|| Regex::new(r"`([a-zA-Z0-9:_/-]+)`").expect("nomi citati"));
    re.captures_iter(text)
        .filter_map(|c| c.get(1).map(|m| m.as_str().to_string()))
        .filter(|n| !n.starts_with("handoff"))
        .take(2)
        .collect()
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(data) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    // Chi legge stdin da sé deve dichiararlo, o ogni sua riga di registro esce
    // marcata come prova.
    hook_io::mark_live_from_payload(&data);
    if !data.is_object() {
        return 0;
    }
    // `data.get('prompt') or ''`: un valore falso diventa la stringa vuota, e
    // solo quello che resta e non è testo fa uscire subito.
    let prompt = match data.get("prompt") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(a)) if a.is_empty() => String::new(),
        Some(Value::Object(o)) if o.is_empty() => String::new(),
        Some(Value::Number(n)) if n.as_f64() == Some(0.0) => String::new(),
        _ => return 0, // non è una stringa: l'originale esce senza dire niente
    };
    let chars = prompt.chars().count();
    // Una richiesta lunga è già circostanziata: l'aiuto serve alle corte.
    if chars > CHAR_CUTOFF {
        return 0;
    }

    // Il segno che l'invito a passare il lavoro è già stato dato in questa
    // sessione. Un file vuoto in `/tmp` — il percorso è quello dell'originale,
    // non `TMPDIR`: se sparisce, al massimo l'avviso ricompare una volta.
    // Il nome si ripulisce: con un `session_id` contenente una barra, `open()`
    // falliva in silenzio e il freno non si attivava mai.
    let sid: String = python_str(data.get("session_id"))
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(36)
        .collect();
    let mark = if sid.is_empty() {
        None
    } else {
        Some(PathBuf::from("/tmp").join(format!(".innesco-{sid}")))
    };
    let already = mark.as_ref().map(|m| m.exists()).unwrap_or(false);

    // Il catalogo si costruisce solo se qualcuno lo interroga: quando nessuno
    // schema aggancia, l'originale non lo tocca e la cache non compare.
    let mut cached: Option<Catalog> = None;
    let mut exists = |name: &str| -> Option<bool> {
        // Un nome con la barra passa comunque, perché lì serve la CLI.
        if name.starts_with('/') {
            return Some(true);
        }
        if cached.is_none() {
            cached = Some(catalog());
        }
        cached.as_ref().unwrap().contains(name)
    };

    let threshold = threshold_mb(std::env::var("INNESCO_SOGLIA_MB").ok().as_deref());
    let Some(rows) = lines(
        &prompt,
        size_mb(data.get("transcript_path")),
        already,
        threshold,
        &mut exists,
    ) else {
        return 0;
    };
    if rows.is_empty() {
        return 0;
    }

    for (reason, text) in &rows {
        log_suggestion(
            if sid.is_empty() { "ignota" } else { &sid },
            reason,
            &quoted_names(text),
            chars,
        );
    }
    if let Some(path) = &mark {
        if rows.iter().any(|(reason, _)| *reason == PASSARE) {
            // `O_EXCL` fa fallire se esiste già — che è il caso normale quando il
            // freno è attivo, e infatti l'errore si ignora. `O_NOFOLLOW` non ha
            // un equivalente portabile in `std`, e `create_new` è già `O_EXCL`:
            // un collegamento simbolico piazzato in `/tmp` col nome giusto fa
            // fallire la creazione invece di far scrivere altrove, che è lo
            // stesso verso in cui sbaglia l'originale.
            use std::os::unix::fs::OpenOptionsExt;
            let _ = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path);
        }
    }
    let body: Vec<&str> = rows.iter().map(|(_, t)| t.as_str()).collect();
    // Anche la stampa passa da una prova: il testo emesso è italiano accentato, e
    // su una pipa rotta lo svuotamento fallisce. Nell'originale l'errore arrivava
    // fuori da ogni `except` e usciva 120.
    let mut out = std::io::stdout();
    let _ = out.write_all(format!("{}\n", body.join("\n")).as_bytes());
    let _ = out.flush();
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    #[test]
    fn the_indented_dump_is_the_one_python_writes() {
        assert_eq!(dumps_indent1(&[]), "{}");
        assert_eq!(
            dumps_indent1(&[
                ("a".to_string(), "prima".to_string()),
                ("b".to_string(), "perché".to_string()),
            ]),
            "{\n \"a\": \"prima\",\n \"b\": \"perché\"\n}"
        );
    }

    /// La descrizione si ferma al campo successivo, e il campo col trattino vale
    /// solo per i comandi: è la differenza fra `\w+` e `\w[\w-]*`.
    #[test]
    fn the_description_stops_at_the_next_field() {
        let matter = "\nname: x\ndescription: fa una cosa\nargument-hint: [x]\n";
        assert_eq!(description(matter, true), "fa una cosa");
        // Con `\w+` il campo `argument-hint` non chiude niente, e la descrizione
        // si porta dietro la riga dopo — è così per le competenze.
        assert_eq!(description(matter, false), "fa una cosa argument-hint: [x]");
        assert_eq!(description("\ndescription: >- una cosa\nname: y\n", false), "una cosa");
        assert_eq!(description("\nname: y\n", false), "");
    }

    /// Un catalogo storto non deve mentire in nessuna delle due direzioni.
    #[test]
    fn a_broken_catalogue_never_invents_a_skill() {
        assert_eq!(Catalog::from(serde_json::json!({})).contains("x"), Some(true));
        assert_eq!(
            Catalog::from(serde_json::json!({"x": "d"})).contains("x"),
            Some(true)
        );
        assert_eq!(
            Catalog::from(serde_json::json!({"y": "d"})).contains("x"),
            Some(false)
        );
        // `name in ["x"]` è un confronto elemento per elemento.
        assert_eq!(Catalog::from(serde_json::json!(["x"])).contains("x"), Some(true));
        // `name in "abcx"` è una sottostringa: il quirk si conserva.
        assert_eq!(Catalog::from(serde_json::json!("abcx")).contains("x"), Some(true));
        // `name in 5` è un TypeError, e lì il gancio si ferma del tutto.
        assert_eq!(Catalog::from(serde_json::json!(5)).contains("x"), None);
        // Zero e nullo sono falsi: `if not c: return True`.
        assert_eq!(Catalog::from(serde_json::json!(0)).contains("x"), Some(true));
        assert_eq!(Catalog::from(Value::Null).contains("x"), Some(true));
    }

    /// La scansione su una casa vuota scrive comunque la cache: è il file da cui
    /// si vede che il catalogo è stato interrogato.
    #[test]
    fn an_empty_home_still_leaves_the_cache_behind() {
        let casa = HomeIsolata::nuova("skill-nudge-catalogo");
        let _ = catalog();
        let text = fs::read_to_string(casa.stato().join("catalogo-skill.json")).unwrap();
        assert_eq!(text, "{}");
    }

    /// Una competenza vera, un comando e un plugin spento, nella stessa casa.
    #[test]
    fn the_scan_reads_skills_commands_and_honours_the_manifest() {
        let casa = HomeIsolata::nuova("skill-nudge-scansione");
        let h = casa.dir.join(".claude");
        fs::create_dir_all(h.join("skills/mia")).unwrap();
        fs::write(
            h.join("skills/mia/SKILL.md"),
            "---\nname: mia\ndescription: fa una cosa\n---\ncorpo\n",
        )
        .unwrap();
        fs::create_dir_all(h.join("commands")).unwrap();
        fs::write(
            h.join("commands/handoff.md"),
            "---\ndescription: scrive la consegna\nargument-hint: \"[x]\"\n---\n",
        )
        .unwrap();
        // Un plugin spento non offre niente.
        fs::create_dir_all(h.join("plugins/cache/mercato/spento/skills/x")).unwrap();
        fs::write(
            h.join("plugins/cache/mercato/spento/skills/x/SKILL.md"),
            "---\nname: x\ndescription: d\n---\n",
        )
        .unwrap();
        let found = scan();
        let names: Vec<&str> = found.iter().map(|(k, _)| k.as_str()).collect();
        assert!(names.contains(&"mia"), "{names:?}");
        assert!(names.contains(&"handoff"), "{names:?}");
        assert!(!names.contains(&"spento:x"), "{names:?}");
        assert_eq!(found.iter().find(|(k, _)| k == "handoff").unwrap().1, "scrive la consegna");
    }

    /// I nomi citati finiscono nel registro, tranne la consegna.
    #[test]
    fn the_quoted_names_drop_the_handoff() {
        assert_eq!(quoted_names("Esiste `mastra`: copre"), vec!["mastra"]);
        assert_eq!(quoted_names("Esiste `handoff`: copre"), Vec::<String>::new());
        assert_eq!(
            quoted_names("`Agent` per un contesto, `SendMessage` viva, `handoff` chiusa"),
            vec!["Agent", "SendMessage"]
        );
        assert_eq!(quoted_names("Esiste `/code-review`:"), vec!["/code-review"]);
    }

    /// `str()` di Python su un `session_id` storto, che è come si scrive il nome
    /// del marcatore.
    #[test]
    fn a_crooked_session_id_still_names_the_marker() {
        assert_eq!(python_str(Some(&serde_json::json!(["a"]))), "['a']");
        assert_eq!(python_str(Some(&serde_json::json!(4242))), "4242");
        assert_eq!(python_str(Some(&serde_json::json!(0))), "");
        assert_eq!(python_str(None), "");
    }
}
