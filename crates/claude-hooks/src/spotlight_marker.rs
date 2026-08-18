//! Il gancio PostToolUse che rimette l'esclusione da Spotlight appena sparisce.
//!
//! Un `pnpm install` che ricrea una `node_modules` cancellata ricrea anche il
//! buco nell'esclusione, e Spotlight riparte a indicizzare decine di migliaia di
//! file che nessuno cercherà mai. Il giudizio — cosa è un'installazione, quali
//! cartelle contano — sta in `guards::spotlight_marker`; qui c'è ciò che tocca
//! il mondo: stdin, l'albero su disco, i file scritti, il messaggio.
//!
//! NON REGISTRA IN `state/ganci.jsonl`, ed è una differenza voluta rispetto agli
//! altri porti: l'originale non importa nemmeno `hook_log`. Aggiungere qui una
//! riga di registro sarebbe un cambio di comportamento travestito da traduzione.
//!
//! ESCE 0 QUANDO DECIDE, MA NON SEMPRE. L'intestazione dell'originale promette
//! «esce sempre 0»; il codice no. Quattro ingressi malformati lo fanno morire di
//! `AttributeError`, cioè uscita 1 con un traceback su stderr, e uno di
//! `TypeError`. Sono difetti dell'originale, e qui si riproducono: l'equivalenza
//! si misura sul comportamento vero. Dettaglio e percorsi sui rami che li
//! generano, più sotto.
//!
//! Valvola: `MARCATORE_SPOTLIGHT=off`.

use guards::spotlight_marker::{is_an_install, is_dependency_dir, MARKER, MAX_DEPTH};
use serde_json::Value;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Il nome che Python darebbe al tipo di questo valore.
///
/// Serve solo per riprodurre i messaggi d'errore dell'originale: senza, i casi
/// malformati divergerebbero proprio sul canale che li rende visibili.
fn python_type(v: &Value) -> &'static str {
    match v {
        Value::Null => "NoneType",
        Value::Bool(_) => "bool",
        Value::Number(n) => {
            if n.is_f64() {
                "float"
            } else {
                "int"
            }
        }
        Value::String(_) => "str",
        Value::Array(_) => "list",
        Value::Object(_) => "dict",
    }
}

/// DIFETTO RIPRODOTTO: `spotlight-marker.py` righe 85, 87 e 42.
///
/// `json.load` sta dentro un `try`, ma `payload.get(...)` no. Un payload che non
/// è un oggetto, un `tool_input` che è una stringa o una lista, un `command` che
/// non è testo: tutti e tre escono di lì con un `AttributeError` e nessuno li
/// prende. Il processo termina con **uscita 1** e un traceback su stderr.
///
/// Qui si stampa la sola riga finale del traceback, che è l'unica parte
/// riproducibile: le righe con i percorsi del file e i numeri di riga cambiano
/// con la versione di Python. Il confronto normalizza il traceback del Python
/// alla sua ultima riga proprio per questo, e lo dichiara.
fn attribute_error(v: &Value, attr: &str) -> i32 {
    eprintln!(
        "AttributeError: '{}' object has no attribute '{}'",
        python_type(v),
        attr
    );
    1
}

/// La verità di Python: vuoto, zero, `false` e `null` sono falsi.
///
/// Serve al `payload.get('tool_input') or {}` dell'originale, che su un
/// `tool_input` falso ripiega su un dizionario vuoto invece di esplodere.
fn is_falsy(v: &Value) -> bool {
    match v {
        Value::Null | Value::Bool(false) => true,
        Value::Bool(true) => false,
        Value::Number(n) => n.as_f64().map(|f| f == 0.0).unwrap_or(false),
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
    }
}

/// Cosa diventa il campo `cwd` una volta passato a `os.path.isdir`.
enum Root {
    /// Una stringa: si usa come percorso e si stampa nel messaggio.
    Path(String),
    /// Un intero o `true`: `os.path.isdir` lo legge come **descrittore di file**
    /// e risponde no. Nessun errore, nessun marcatore, uscita 0.
    Opaque,
    /// `float`, `list`, `dict`: `os.stat` si rifiuta e nessuno prende il
    /// `TypeError`. Altro difetto dell'originale, riga 90.
    Refused(&'static str),
}

pub fn run() -> i32 {
    if std::env::var("MARCATORE_SPOTLIGHT").as_deref() == Ok("off") {
        return 0;
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    let Some(obj) = payload.as_object() else {
        return attribute_error(&payload, "get");
    };
    // `payload.get('tool_name') != 'Bash'`: un valore non testuale è
    // semplicemente diverso da «Bash», non un errore.
    if obj.get("tool_name") != Some(&Value::String("Bash".to_string())) {
        return 0;
    }

    let vuoto = Value::String(String::new());
    let command = match obj.get("tool_input") {
        None => vuoto,
        Some(v) if is_falsy(v) => vuoto,
        Some(Value::Object(m)) => m.get("command").cloned().unwrap_or(vuoto),
        Some(other) => return attribute_error(other, "get"),
    };
    let Some(text) = command.as_str() else {
        return attribute_error(&command, "split");
    };
    if !is_an_install(text) {
        return 0;
    }

    let root = match obj.get("cwd") {
        None => Root::Path(current_dir()),
        Some(v) if is_falsy(v) => Root::Path(current_dir()),
        Some(Value::String(s)) => Root::Path(s.clone()),
        Some(Value::Bool(true)) => Root::Opaque,
        Some(Value::Number(n)) if !n.is_f64() => Root::Opaque,
        Some(other) => Root::Refused(python_type(other)),
    };
    let root = match root {
        Root::Path(p) => p,
        Root::Opaque => return 0,
        Root::Refused(t) => {
            eprintln!(
                "TypeError: stat: path should be string, bytes, os.PathLike or integer, not {t}"
            );
            return 1;
        }
    };

    let touched = put_back(&root);
    if !touched.is_empty() {
        eprintln!(
            "spotlight: put back {} exclusion marker(s) under {root}",
            touched.len()
        );
    }
    0
}

/// `os.getcwd()`, che come `getcwd(3)` restituisce il percorso fisico.
fn current_dir() -> String {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Rimette il marcatore dove manca. Torna i percorsi toccati.
fn put_back(root: &str) -> Vec<String> {
    let mut touched = Vec::new();
    for d in dirs_to_mark(root) {
        let path = PathBuf::from(&d).join(MARKER);
        // `os.path.exists`: segue i link, e un link rotto vale «non c'è».
        if path.exists() {
            continue;
        }
        // `open(path, 'a').close()`: se la cartella non c'è, se il nome è già
        // una cartella, se manca il permesso — l'originale ingoia l'OSError e
        // non conta il percorso fra i toccati.
        if fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .is_ok()
        {
            touched.push(d);
        }
    }
    touched
}

/// Le cartelle di dipendenze sotto la radice, senza scendere dentro di loro.
///
/// DIFETTO RIPRODOTTO, riga 60: l'originale non riceve una lista di percorsi ma
/// **un testo**, e lo rispezza con `splitlines()`. Una cartella intermedia con
/// un a capo nel nome produce due righe finte — `…/we` e `ird/node_modules` — e
/// se la prima esiste davvero come cartella il gancio ci scrive dentro un
/// marcatore che nessuno gli ha chiesto. Qui il testo si ricompone e si rispezza
/// uguale, perché è il comportamento vero.
fn dirs_to_mark(root: &str) -> Vec<String> {
    let Some(found) = run_find(root) else {
        return Vec::new();
    };
    python_splitlines(&found.join("\n"))
        .into_iter()
        .filter(|d| !d.trim_matches(is_python_space).is_empty())
        .collect()
}

/// Ciò che stamperebbe
/// `find <root> -maxdepth 4 -type d ( -name node_modules -o -name target ) -prune`.
///
/// `None` sta per «il testo non si decodifica»: `subprocess.run(..., text=True)`
/// decodifica **tutto** l'output in una volta, quindi un solo nome di file non
/// UTF-8 fa perdere l'intera lista, non la sua riga. Su APFS è irraggiungibile —
/// il file system rifiuta i nomi non UTF-8 — ma è la differenza fra «uguale» e
/// «quasi», e costa una riga.
fn run_find(root: &str) -> Option<Vec<String>> {
    let p = Path::new(root);
    // `os.path.isdir` segue i link simbolici…
    if !p.is_dir() {
        return Some(Vec::new());
    }
    // …ma `find` senza `-L` non attraversa una radice che è un link: la radice
    // stessa non è `-type d`, e l'uscita è vuota.
    if fs::symlink_metadata(p)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false)
    {
        return Some(Vec::new());
    }

    let mut found: Vec<PathBuf> = Vec::new();
    // Profondità 0: la radice può combaciare con sé stessa, e allora `find` la
    // stampa **come è stata scritta** — barra finale compresa — e si ferma lì.
    if is_dependency_dir(&find_name(root)) {
        found.push(PathBuf::from(root));
    } else {
        walk(Path::new(root), 0, &mut found);
    }

    let mut out = Vec::with_capacity(found.len());
    for path in found {
        out.push(path.into_os_string().into_string().ok()?);
    }
    Some(out)
}

/// Il nome che `-name` confronta per la radice: l'ultimo segmento, senza le
/// barre finali. `find /x/node_modules/` combacia, e stampa la barra.
fn find_name(root: &str) -> String {
    let trimmed = root.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some((_, name)) => name.to_string(),
        None => trimmed.to_string(),
    }
}

/// Scende di un livello, senza seguire i link e senza entrare in ciò che ha
/// già combaciato (`-prune`).
///
/// `depth` è la profondità di `dir` con la radice a zero: i figli stanno a
/// `depth + 1`, e `-maxdepth 4` li ammette solo fino a quattro.
fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth >= MAX_DEPTH {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return; // `find` scrive l'errore su stderr, che l'originale butta via
    };
    for entry in entries.flatten() {
        // `-type d` guarda il tipo del file, non del bersaglio: un link a una
        // cartella non combacia.
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_dir() {
            continue;
        }
        let name: OsString = entry.file_name();
        let child = dir.join(&name);
        if dependency_dir_name(&name) {
            out.push(child);
            continue;
        }
        walk(&child, depth + 1, out);
    }
}

/// Come `is_dependency_dir`, ma su un nome che potrebbe non essere UTF-8: un
/// nome del genere non può essere uguale a `node_modules`, quindi risponde no
/// senza dover decodificare.
fn dependency_dir_name(name: &OsStr) -> bool {
    name.to_str().map(is_dependency_dir).unwrap_or(false)
}

/// `str.splitlines()` di Python.
///
/// Non è `str::lines()`: Python spezza anche su `\x0b`, `\x0c`, `\x1c`–`\x1e`,
/// `\u{85}`, `\u{2028}` e `\u{2029}`. Un nome di cartella può contenerli tutti —
/// il file system vieta solo `/` e il byte zero — e da lì passa il difetto
/// descritto in `dirs_to_mark`.
fn python_splitlines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let breaks = matches!(
            c,
            '\n' | '\r'
                | '\u{b}'
                | '\u{c}'
                | '\u{1c}'
                | '\u{1d}'
                | '\u{1e}'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !breaks {
            current.push(c);
            continue;
        }
        if c == '\r' && chars.peek() == Some(&'\n') {
            chars.next();
        }
        out.push(std::mem::take(&mut current));
    }
    // `"a\n".splitlines()` è `["a"]`: dopo l'ultimo a capo non c'è una riga vuota.
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Lo spazio secondo `str.strip()` di Python, gemello di quello in `guards`.
fn is_python_space(c: char) -> bool {
    c.is_whitespace() || matches!(c, '\u{1c}'..='\u{1f}')
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Un albero usa-e-getta, cancellato alla fine del caso.
    struct Albero(PathBuf);

    impl Albero {
        fn nuovo(nome: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("spotlight-marker-prove-{nome}"));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn dir(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            fs::create_dir_all(&p).unwrap();
            p
        }
        fn radice(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for Albero {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Il caso che presidia il `-prune`: senza, la `node_modules` annidata
    /// riceverebbe un marcatore dentro un albero che ha già il suo.
    #[test]
    fn trova_le_cartelle_senza_scendere_dentro() {
        let a = Albero::nuovo("prune");
        a.dir("app/node_modules/pkg/node_modules");
        a.dir("target");
        let trovate = dirs_to_mark(&a.radice());
        assert_eq!(trovate.len(), 2, "{trovate:?}");
        assert!(!trovate.iter().any(|d| d.matches("node_modules").count() > 1));
    }

    /// `-maxdepth 4` conta la radice come zero: al quinto livello non si guarda.
    #[test]
    fn la_profondita_massima_e_quattro() {
        let a = Albero::nuovo("profondita");
        a.dir("b/c/d/node_modules"); // profondità 4: dentro
        a.dir("b/c/d/e/node_modules"); // profondità 5: fuori
        let trovate = dirs_to_mark(&a.radice());
        assert_eq!(trovate.len(), 1, "{trovate:?}");
        assert!(trovate[0].ends_with("b/c/d/node_modules"));
    }

    /// La radice che combacia con sé stessa si stampa e non si attraversa.
    #[test]
    fn una_radice_che_e_gia_node_modules_combacia_da_sola() {
        let a = Albero::nuovo("radice-nm");
        let nm = a.dir("node_modules/pkg/node_modules");
        let radice = nm.parent().unwrap().parent().unwrap().to_string_lossy().into_owned();
        let trovate = dirs_to_mark(&radice);
        assert_eq!(trovate, vec![radice.clone()]);
        // La barra finale si conserva, come fa `find`.
        let con_barra = format!("{radice}/");
        assert_eq!(dirs_to_mark(&con_barra), vec![con_barra]);
    }

    /// Scrive una volta sola: la seconda passata non tocca niente.
    #[test]
    fn la_seconda_passata_non_tocca_niente() {
        let a = Albero::nuovo("seconda-passata");
        a.dir("app/node_modules");
        a.dir("target");
        assert_eq!(put_back(&a.radice()).len(), 2);
        assert!(a.0.join("target").join(MARKER).exists());
        assert_eq!(put_back(&a.radice()).len(), 0);
        // Tolto uno solo, ne rimette uno solo.
        fs::remove_file(a.0.join("target").join(MARKER)).unwrap();
        assert_eq!(put_back(&a.radice()).len(), 1);
    }

    /// Una radice che non esiste, o che è un file, non fa esplodere niente.
    #[test]
    fn una_radice_impossibile_non_fa_danni() {
        assert_eq!(put_back("/no/such/path").len(), 0);
        let a = Albero::nuovo("radice-file");
        let f = a.0.join("file");
        fs::write(&f, "x").unwrap();
        assert_eq!(put_back(&f.to_string_lossy()).len(), 0);
    }

    /// `splitlines` non è `lines`: il difetto della lista che torna testo si
    /// vede solo con i separatori che Python riconosce e Rust no.
    #[test]
    fn splitlines_spezza_come_python() {
        assert_eq!(python_splitlines("a\nb"), vec!["a", "b"]);
        assert_eq!(python_splitlines("a\r\nb"), vec!["a", "b"]);
        assert_eq!(python_splitlines("a\u{2028}b"), vec!["a", "b"]);
        assert_eq!(python_splitlines("a\u{b}b"), vec!["a", "b"]);
        assert_eq!(python_splitlines("a\n"), vec!["a"]);
        assert_eq!(python_splitlines(""), Vec::<String>::new());
    }

    /// I nomi dei tipi servono a riprodurre i messaggi dei difetti: sbagliarne
    /// uno farebbe divergere proprio i casi malformati.
    #[test]
    fn i_nomi_dei_tipi_sono_quelli_di_python() {
        assert_eq!(python_type(&serde_json::json!(null)), "NoneType");
        assert_eq!(python_type(&serde_json::json!(true)), "bool");
        assert_eq!(python_type(&serde_json::json!(42)), "int");
        assert_eq!(python_type(&serde_json::json!(4.2)), "float");
        assert_eq!(python_type(&serde_json::json!([])), "list");
        assert_eq!(python_type(&serde_json::json!({})), "dict");
    }
}
