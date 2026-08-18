//! Lo stato di una lavorazione lo scrive chi lo cambia, nell'istante in cui cambia.
//!
//! Porto di `skills/hooks/work-status.py`. I tre gesti restano quelli:
//!
//! 1. **PostToolUse su Bash** (nessun argomento, payload da stdin): se il comando
//!    ha appena aperto una richiesta di modifica, la copia dove è nata passa a
//!    `in-review`; se ha appena creato una copia di lavoro, si chiude la tab di
//!    avvio che `--setup run` lascia indietro. Due gesti, un gancio solo.
//! 2. **`--riconcilia`** su `SessionStart`: le fusioni avvengono dopo e altrove,
//!    quindi nessuna sessione può presidiarle. Qui si rilegge lo stato delle
//!    richieste e si allineano tutte le copie.
//! 3. **`--riconcilia --secco`**: dice cosa farebbe e non scrive.
//!
//! È IL GANCIO CHE TOCCA IL MONDO PIÙ DI TUTTI GLI ALTRI PORTATI: scrive
//! metadato su Orca vero, e in modalità riconciliazione lo fa su ogni copia di
//! lavoro della macchina. Il confronto di equivalenza non può eseguire Orca —
//! riscriverebbe lo stato di copie che stanno lavorando — quindi il binario si
//! lascia deviare da `WORK_STATUS_ORCA`; vedi `orca_bin`.
//!
//! FAIL-OPEN E MUTO verso l'agente, rumoroso su
//! `state/work-status-failures.log`: un gancio rotto in silenzio è
//! indistinguibile da un gancio contento. VALVOLA: `STATO_LAVORAZIONE=off`.
//!
//! DOVE IL PYTHON SOLLEVA, QUI SI RESTITUISCE `Err`. L'originale è pieno di
//! `payload.get(...)` su valori che potrebbero non essere dizionari: ogni
//! `AttributeError`/`TypeError` che ne segue viene raccolto dal `try` del
//! chiamante, che annota il guasto e prosegue. Riprodurre quel confine è
//! l'unico modo perché un payload storto lasci la stessa traccia da entrambe le
//! parti — ed è la ragione del tipo `Py<T>` qui sotto.

use crate::handoff::state_dir;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

/// Il risultato di un pezzo di codice che nell'originale può sollevare.
/// `Err` = «qui il Python moriva», col testo che finisce nel registro dei guasti.
pub(crate) type Py<T> = Result<T, String>;

/// Dove il Python cerca Orca, e come lo si devia in prova.
///
/// L'originale ha `ORCA = "/usr/local/bin/orca"`, un percorso assoluto: il
/// `PATH` non lo tocca. Il valore predefinito qui è lo stesso, quindi in
/// produzione i due chiamano lo stesso binario. La variabile esiste per una
/// ragione sola: **nessun confronto di equivalenza deve poter parlare con Orca
/// vero**. Un giro di `--riconcilia` sul vivo riscrive lo stato di trenta copie,
/// alcune delle quali stanno lavorando. Il confronto la punta a uno script che
/// registra gli argomenti e risponde con un JSON preparato, e inietta lo stesso
/// finto nel `PATH` per il lato Python.
fn orca_bin() -> String {
    std::env::var("WORK_STATUS_ORCA").unwrap_or_else(|_| "/usr/local/bin/orca".to_string())
}

fn home() -> PathBuf {
    std::env::var("HOME").map(PathBuf::from).unwrap_or_default()
}

/// La carta dei repo: da lì escono i nomi da interrogare su GitHub.
fn carta() -> PathBuf {
    home()
        .join("other-repo")
        .join("work")
        .join(".claude")
        .join("repo-in-carico.json")
}

fn failures_log() -> PathBuf {
    state_dir().join("work-status-failures.log")
}

/// Il gesto che apre una richiesta, in qualunque punto della riga.
///
/// La coda `(?![\w-])` dell'originale non esiste nel motore di Rust, che non ha
/// il lookahead: si cerca la stessa forma e si guarda il carattere dopo. La
/// differenza che serve è quella fra `gh pr create` e `gh pr create-draft`.
fn opens_request(command: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\bgh\s+pr\s+create").unwrap());
    re.find_iter(command).any(|m| !word_continues(command, m.end()))
}

/// Il gesto gemello, che mancava. `--merge`, `--squash`, `--rebase`, con o
/// senza numero: la forma del comando è solo il primo filtro, la prova la dà
/// GitHub.
fn merges_request(command: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\bgh\s+pr\s+merge").unwrap());
    re.find_iter(command).any(|m| !word_continues(command, m.end()))
}

fn creates_worktree(command: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\borca\s+worktree\s+create").unwrap());
    re.find_iter(command).any(|m| !word_continues(command, m.end()))
}

/// `(?![\w-])`: `\w` di Python su testo è alfanumerico Unicode più `_`.
fn word_continues(s: &str, at: usize) -> bool {
    s[at..]
        .chars()
        .next()
        .map(|c| c.is_alphanumeric() || c == '_' || c == '-')
        .unwrap_or(false)
}

fn changes_dir() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(?:cd|git\s+-C)\s+([^\s;&|]+)").unwrap())
}

fn pull_url() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"github\.com[:/]([^/\s]+)/([^/\s]+)/pull/\d+").unwrap())
}

fn remote_slug() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `$` di Python accetta anche la posizione prima di un `\n` finale; qui la
    // lettura è già passata da `strip()`, quindi non c'è nessun a capo in coda.
    RE.get_or_init(|| Regex::new(r"github\.com[:/]+([^/\s]+)/([^/\s]+?)(?:\.git)?/?$").unwrap())
}

fn rango(state: &str) -> u8 {
    match state {
        "MERGED" => 3,
        "OPEN" => 2,
        "CLOSED" => 1,
        _ => 0,
    }
}

/// La verità di Python: vuoto, zero, `false` e `null` sono falsi.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().map(|x| x != 0.0).unwrap_or(true),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `str(x)` di Python per i valori che possono finire in un f-string.
///
/// `None` diventa «None» e `True` diventa «True»: sembra un dettaglio, ma è ciò
/// che il messaggio stampa quando un worktree non ha `displayName`, e il
/// confronto guarda quel testo byte per byte.
fn py_str(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "None".to_string(),
        Some(Value::Bool(true)) => "True".to_string(),
        Some(Value::Bool(false)) => "False".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
    }
}

fn spento() -> bool {
    let v = std::env::var("STATO_LAVORAZIONE")
        .unwrap_or_default()
        .to_lowercase();
    v == "off" || v == "0" || v == "false"
}

/// Un guasto non ferma il gancio, ma lascia una riga.
///
/// L'originale scrive `--- <dove>` seguito dal traceback di Python, che qui non
/// esiste: la prima riga è identica, il corpo dice l'errore in una riga. Il
/// confronto guarda le righe `--- `, che sono l'informazione — *quale* passo si
/// è rotto — e non il testo dell'interprete.
fn annota_guasto(where_: &str, detail: &str) {
    let path = failures_log();
    if let Some(dir) = path.parent() {
        if fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = write!(f, "--- {where_}\n{detail}\n");
    }
}

/// La riga del registro, nella forma di `scripts/hook_log.py`.
///
/// `json.dumps(..., ensure_ascii=False)`: separatori con lo spazio. Non è la
/// forma di `journal::record`, che ricopia il JavaScript e non ha spazi — le due
/// convivono in `ganci.jsonl` e chi porta un gancio deve guardare quale usa il
/// suo originale. Qui l'originale importa `registra`, e l'import è sano: le sue
/// righe nel registro ci sono davvero.
fn registra(decisione: &str, motivo: &str, extra: &[(&str, i64)]) {
    let mut line = serde_json::Map::new();
    line.insert(
        "t".into(),
        Value::String(hook_io::journal::now_iso8601_python()),
    );
    line.insert("gancio".into(), Value::String("work-status".into()));
    line.insert("decisione".into(), Value::String(decisione.into()));
    line.insert("motivo".into(), Value::String(motivo.into()));
    for (k, v) in extra {
        line.insert((*k).into(), json!(v));
    }
    let dir = state_dir();
    let path = dir.join("ganci.jsonl");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > 5 * 1024 * 1024 {
            let _ = fs::rename(&path, path.with_extension("jsonl.1"));
        }
    }
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}", hook_io::python_json::dumps_unicode(&Value::Object(line)));
    }
}

/// Una chiamata a Orca che risponde JSON. `None` = non ha risposto niente di
/// leggibile. Il codice d'uscita **non si guarda**, come nell'originale: conta
/// solo che su stdout ci sia del JSON.
fn orca_json(args: &[&str]) -> Option<Value> {
    let out = Command::new(orca_bin())
        .args(args)
        .arg("--json")
        .output()
        .ok()?;
    serde_json::from_str(&String::from_utf8_lossy(&out.stdout)).ok()
}

/// Il worktree più specifico che contiene questo percorso.
///
/// Il più lungo, perché i checkout canonici sono prefisso dei propri sotto-repo:
/// `~/other-repo/work` contiene `~/other-repo/work/suite`, e la lavorazione è la seconda.
fn worktree_del_percorso<'a>(path_: &str, listing: &'a [Value]) -> Py<Option<&'a Value>> {
    let mut best: Option<(&Value, usize)> = None;
    for w in listing {
        if !w.is_object() {
            return Err("una copia dell'elenco non è un dizionario".into());
        }
        let p = w.get("path");
        let dentro = match p {
            Some(Value::String(s)) => path_ == s || path_.starts_with(&format!("{s}/")),
            // `(w.get("path") or "\0") + "/"`: un percorso assente o falso
            // diventa `"\0/"`, che non è prefisso di niente.
            None | Some(Value::Null) => path_.starts_with("\0/"),
            Some(v) if !truthy(v) => path_.starts_with("\0/"),
            // Un valore vero che non è testo fa esplodere la concatenazione.
            Some(_) => return Err("startswith su un percorso non testuale".into()),
        };
        if !dentro {
            continue;
        }
        let len = match p {
            Some(Value::String(s)) => s.chars().count(),
            _ => 0,
        };
        // `max` di Python tiene il **primo** massimo: solo un `>` stretto sposta.
        if best.map(|(_, l)| len > l).unwrap_or(true) {
            best = Some((w, len));
        }
    }
    Ok(best.map(|(w, _)| w))
}

/// I worktree dell'elenco che Orca ha risposto.
///
/// Tre `.get` in fila, e l'originale li fa senza guardare su cosa: `listing`,
/// `result` e poi l'iterazione di `worktrees`. Su un valore che non è un
/// dizionario ognuno dei tre è un `AttributeError`, cioè il gancio **muore lì**
/// e annota il guasto. Un porto che proseguisse a mani vuote darebbe lo stesso
/// verdetto — non fare niente — ma perderebbe la traccia, e domani nessuno
/// saprebbe che quel giro si è rotto invece di non avere niente da fare.
fn worktrees_of(listing: &Value) -> Py<Vec<Value>> {
    if truthy(listing) && !listing.is_object() {
        return Err("la risposta di Orca non è un dizionario".into());
    }
    let result = match listing.get("result") {
        Some(v) if truthy(v) => v.clone(),
        _ => json!({}),
    };
    if truthy(&result) && !result.is_object() {
        return Err("il campo result non è un dizionario".into());
    }
    match result.get("worktrees") {
        Some(v) if truthy(v) => match v {
            Value::Array(items) => Ok(items.clone()),
            // Un dizionario o una stringa si iterano lo stesso in Python, e a
            // rompersi è il `.get` sull'elemento: l'esito osservabile è identico.
            _ => Err("l'elenco dei worktree non contiene copie".into()),
        },
        _ => Ok(Vec::new()),
    }
}

/// `owner/repo` dell'URL che la CLI ha appena stampato. Vuoto se non c'è.
fn slug_of_pull(answer: Option<&Value>) -> String {
    let raw = match answer {
        Some(Value::Object(o)) => {
            let pick = o
                .get("stdout")
                .filter(|v| truthy(v))
                .or_else(|| o.get("output").filter(|v| truthy(v)));
            match pick {
                Some(v) => py_str(Some(v)),
                None => String::new(),
            }
        }
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    match pull_url().captures(&raw) {
        Some(c) => format!("{}/{}", &c[1], &c[2]).to_lowercase(),
        None => String::new(),
    }
}

/// `owner/repo` del remoto `origin` di questa copia. Vuoto se git tace.
fn slug_of_remote(path_: &str) -> String {
    let url = read_git(path_, &["remote", "get-url", "origin"]).unwrap_or_default();
    match remote_slug().captures(&url) {
        Some(c) => format!("{}/{}", &c[1], &c[2]).to_lowercase(),
        None => String::new(),
    }
}

/// `os.path.abspath`: normalizzazione **lessicale**, senza toccare il disco e
/// senza sciogliere i collegamenti.
fn abspath(p: &str, cwd: &str) -> String {
    let joined = if p.starts_with('/') {
        p.to_string()
    } else if cwd.ends_with('/') {
        format!("{cwd}{p}")
    } else {
        format!("{cwd}/{p}")
    };
    // POSIX: esattamente due barre in testa si conservano, tre o più no.
    let doppia = joined.starts_with("//") && !joined.starts_with("///");
    let mut parti: Vec<&str> = Vec::new();
    for seg in joined.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parti.pop();
            }
            s => parti.push(s),
        }
    }
    let testa = if doppia { "//" } else { "/" };
    format!("{testa}{}", parti.join("/"))
}

/// La copia dove il comando ha davvero girato.
///
/// I cambi di cartella si provano dall'ultimo al primo perché è l'ultimo a
/// valere. Il `cwd` del gancio resta il ripiego, giusto quando il comando non
/// cambia cartella affatto.
fn worktree_of_command<'a>(
    command: &str,
    cwd: &str,
    listing: &'a [Value],
) -> Py<Option<&'a Value>> {
    let mut visti: Vec<&str> = changes_dir()
        .captures_iter(command)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    visti.reverse();
    for p in visti {
        if let Some(found) = worktree_del_percorso(&abspath(p, cwd), listing)? {
            return Ok(Some(found));
        }
    }
    worktree_del_percorso(cwd, listing)
}

/// Scrive lo stato su Orca. `false` = non c'era niente da cambiare, o Orca non
/// ha confermato.
fn write_state(worktree: &Value, state: &str) -> Py<bool> {
    if worktree.get("workspaceStatus") == Some(&Value::String(state.to_string())) {
        return Ok(false);
    }
    let id = match worktree.get("id") {
        Some(Value::String(s)) => s.clone(),
        None => return Err("worktree senza chiave id".into()),
        Some(_) => return Err("id del worktree non testuale".into()),
    };
    let r = orca_json(&[
        "worktree",
        "set",
        "--worktree",
        &format!("id:{id}"),
        "--workspace-status",
        state,
    ]);
    Ok(r
        .as_ref()
        .and_then(|v| v.get("ok"))
        .map(truthy)
        .unwrap_or(false))
}

// --- modo 1: la sessione ha appena aperto una richiesta --------------------

/// Il comando di un payload, con gli stessi punti in cui l'originale solleva.
fn command_of(payload: &Value) -> Py<String> {
    let ti = payload.get("tool_input");
    let command = match ti {
        Some(Value::Object(o)) => o.get("command").cloned().unwrap_or(Value::String(String::new())),
        None | Some(Value::Null) => Value::String(String::new()),
        Some(v) if !truthy(v) => Value::String(String::new()),
        Some(_) => return Err("tool_input non è un dizionario".into()),
    };
    match command {
        Value::String(s) => Ok(s),
        _ => Err("comando non testuale: la ricerca non lo accetta".into()),
    }
}

fn dopo_richiesta(payload: &Value) -> Py<Option<String>> {
    if !payload.is_object() {
        return Err("il payload non è un dizionario".into());
    }
    if payload.get("tool_name") != Some(&Value::String("Bash".into())) {
        return Ok(None);
    }
    let command = command_of(payload)?;
    if !opens_request(&command) {
        return Ok(None);
    }

    // Comando fallito o interrotto: nessuna richiesta è nata.
    let risposta = payload.get("tool_response");
    if let Some(Value::Object(o)) = risposta {
        let rotto = o.get("is_error").map(truthy).unwrap_or(false)
            || o.get("interrupted").map(truthy).unwrap_or(false);
        if rotto {
            return Ok(None);
        }
    }

    // LA PROVA È L'URL, non il comando: il testo `gh pr create` compare anche
    // dentro il messaggio di un commit che descrive questo gancio.
    let wanted = slug_of_pull(risposta);
    if wanted.is_empty() {
        return Ok(None);
    }

    let listing = orca_json(&["worktree", "list"]).unwrap_or(json!({}));
    let worktrees = worktrees_of(&listing)?;
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(worktree) = worktree_of_command(&command, &cwd, &worktrees)? else {
        return Ok(None);
    };

    // SI MARCA SOLO SU CONFERMA: la copia scelta deve parlare proprio col repo
    // dove la richiesta è nata, non basta che non lo smentisca. Un remoto muto è
    // la prova che lì nessuna richiesta può nascere, non un permesso.
    let path_ = match worktree.get("path") {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    if wanted != slug_of_remote(&path_) {
        return Ok(None);
    }

    if !write_state(worktree, "in-review")? {
        return Ok(None);
    }
    Ok(Some(format!(
        "Orca: '{}' marked in-review (pull request opened).",
        py_str(worktree.get("displayName"))
    )))
}

/// Lo stato che GitHub dà alla richiesta di questo ramo. Vuoto se non risponde.
///
/// LA PROVA È GITHUB, non l'uscita del comando. `gh pr merge` stampa un testo
/// che cambia forma fra versioni e fra modi di fusione, e un gancio che lo
/// interpreta si rompe in silenzio al primo aggiornamento della CLI. Qui si
/// chiede alla fonte: una chiamata di rete in più, ma solo nell'istante in cui
/// qualcuno fonde.
///
/// SI CHIEDE PER REPO, NON DALLA CARTELLA: da lì la domanda non parte affatto se
/// la cartella non c'è più — che dopo una fusione è proprio il caso che può
/// capitare, perché è il momento in cui una copia si smonta.
fn stato_richiesta_del_ramo(path_: &str, ramo: &str) -> String {
    let slug = slug_of_remote(path_);
    if slug.is_empty() {
        return String::new();
    }
    let out = Command::new("gh")
        .args([
            "pr", "list", "--repo", &slug, "--head", ramo, "--state", "all",
            "--limit", "5", "--json", "state,mergedAt",
        ])
        .output();
    let Ok(o) = out else { return String::new() };
    if !o.status.success() {
        return String::new();
    }
    let Ok(prs) = serde_json::from_slice::<Value>(&o.stdout) else {
        return String::new();
    };
    let mut migliore = String::new();
    for r in prs.as_array().map(|v| v.as_slice()).unwrap_or(&[]) {
        let merged = r
            .get("mergedAt")
            .map(|v| !v.is_null() && v != &Value::String(String::new()))
            .unwrap_or(false);
        let stato = if merged {
            "MERGED".to_string()
        } else {
            py_str(r.get("state"))
        };
        if rango(&stato) > rango(&migliore) {
            migliore = stato;
        }
    }
    migliore
}

/// La copia il cui lavoro è appena atterrato smette di essere «in revisione».
///
/// PERCHÉ NON BASTA IL RICONCILIATORE. `--riconcilia` gira a SessionStart, e fra
/// un avvio e l'altro passano ore: misurato il 18/08/2026, **7 copie su 13
/// dichiarate in-review avevano già la richiesta fusa**, e tutte e cinque le
/// fusioni verificabili erano avvenute **dopo** il suo ultimo giro (08:32 UTC;
/// fusioni fra le 08:46 e le 09:04). La barra di Orca — che è quello che si
/// guarda — raccontava fermo in revisione un lavoro già finito.
fn dopo_fusione(payload: &Value) -> Py<Option<String>> {
    if !payload.is_object() {
        return Err("il payload non è un dizionario".into());
    }
    if payload.get("tool_name") != Some(&Value::String("Bash".into())) {
        return Ok(None);
    }
    let command = command_of(payload)?;
    if !merges_request(&command) {
        return Ok(None);
    }
    if let Some(Value::Object(o)) = payload.get("tool_response") {
        let rotto = o.get("is_error").map(truthy).unwrap_or(false)
            || o.get("interrupted").map(truthy).unwrap_or(false);
        if rotto {
            return Ok(None);
        }
    }

    let listing = orca_json(&["worktree", "list"]).unwrap_or(json!({}));
    let worktrees = worktrees_of(&listing)?;
    let cwd = std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let Some(worktree) = worktree_of_command(&command, &cwd, &worktrees)? else {
        return Ok(None);
    };
    // Un checkout canonico non è una lavorazione: fondere stando lì dentro non
    // deve marcare niente. `state_giusto` lo sa già, ma uscire qui risparmia la
    // chiamata di rete.
    let git = worktree.get("git").cloned().unwrap_or(json!({}));
    if git.get("isMainWorktree").map(truthy).unwrap_or(false) {
        return Ok(None);
    }
    let ramo = py_str(git.get("branch")).replace("refs/heads/", "");
    if ramo.is_empty() {
        return Ok(None);
    }

    // SI MARCA SOLO SU CONFERMA, come per l'apertura: `gh pr merge 468` fonde la
    // richiesta di un ALTRO ramo, e marcare la copia da cui si è digitato il
    // comando scriverebbe lo stato sbagliato su una lavorazione viva.
    let path_ = py_str(worktree.get("path"));
    if stato_richiesta_del_ramo(&path_, &ramo) != "MERGED" {
        return Ok(None);
    }

    // Fusa non basta a dire finita: una copia con commit che non vivono da
    // nessun'altra parte resta in lavorazione.
    let leftover = unlanded_commits(&path_);
    let mut richieste = BTreeMap::new();
    richieste.insert(ramo, "MERGED".to_string());
    let giusto = state_giusto(worktree, &richieste, leftover)?;
    if !write_state(worktree, &giusto)? {
        return Ok(None);
    }
    Ok(Some(format!(
        "Orca: '{}' marked {giusto} (pull request merged).",
        py_str(worktree.get("displayName"))
    )))
}

// --- modo 2 dello stesso gancio: le tab lasciate indietro ------------------

/// Chi apre una copia di lavoro chiude le tab che non gli servono. Subito.
///
/// `orca worktree create --setup run` apre due tab per ogni copia nuova, e il
/// loro compito finisce in pochi secondi. L'istante deterministico è la
/// creazione: `--json` restituisce l'handle di ciò che ha aperto, quindi non
/// c'è niente da indovinare. Si chiude la tab di avvio **solo** se non è quella
/// dell'agente; senza handle non si chiude niente e si tace.
fn tabs_left_behind(payload: &Value) -> Py<Option<String>> {
    if !payload.is_object() {
        return Err("il payload non è un dizionario".into());
    }
    if payload.get("tool_name") != Some(&Value::String("Bash".into())) {
        return Ok(None);
    }
    let command = command_of(payload)?;
    if !creates_worktree(&command) {
        return Ok(None);
    }

    let answer = payload.get("tool_response");
    if let Some(Value::Object(o)) = answer {
        let rotto = o.get("is_error").map(truthy).unwrap_or(false)
            || o.get("interrupted").map(truthy).unwrap_or(false);
        if rotto {
            return Ok(None);
        }
    }

    let raw = match answer {
        Some(Value::Object(o)) => {
            let pick = o
                .get("stdout")
                .filter(|v| truthy(v))
                .or_else(|| o.get("output").filter(|v| truthy(v)));
            pick.map(|v| py_str(Some(v))).unwrap_or_default()
        }
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    };
    let Some(start) = raw.find('{') else {
        return Ok(None);
    };
    let Ok(parsed) = serde_json::from_str::<Value>(&raw[start..]) else {
        return Ok(None);
    };
    // `(json.loads(...) or {}).get("result")`: su un JSON che non è un
    // dizionario l'originale finisce nello stesso `except` del parse.
    let result = match &parsed {
        Value::Object(o) => o.get("result").cloned().unwrap_or(json!({})),
        v if !truthy(v) => json!({}),
        _ => return Ok(None),
    };
    let result = if truthy(&result) { result } else { json!({}) };
    // Da qui in poi si è fuori dal `try` dell'originale: un `result` che non è
    // un dizionario non fa più tacere il gancio, lo rompe.
    if truthy(&result) && !result.is_object() {
        return Err("il campo result della creazione non è un dizionario".into());
    }

    let startup = match result.get("startupTerminal") {
        Some(Value::Object(o)) => match o.get("handle") {
            Some(v) if truthy(v) => py_str(Some(v)),
            _ => String::new(),
        },
        Some(v) if truthy(v) => return Err("startupTerminal non è un dizionario".into()),
        _ => String::new(),
    };
    let agent = match result.get("agentTerminalHandle") {
        Some(v) if truthy(v) => py_str(Some(v)),
        _ => String::new(),
    };
    // Niente da chiudere, o la tab di avvio ospita l'agente.
    if startup.is_empty() || startup == agent {
        return Ok(None);
    }

    let r = orca_json(&["terminal", "close", "--terminal", &startup, "--tab"]);
    let ok = r
        .as_ref()
        .and_then(|v| v.get("ok"))
        .map(truthy)
        .unwrap_or(false);
    if !ok {
        return Ok(None);
    }
    Ok(Some(format!(
        "Orca: closed the startup tab of the new worktree ({}\u{2026}). It had finished its job.",
        startup.chars().take(18).collect::<String>()
    )))
}

// --- modo 3: le fusioni avvengono dopo e altrove ---------------------------

/// Per ogni ramo lo stato più avanzato, e **quali repo hanno davvero risposto**.
///
/// IL SECONDO VALORE È IL PUNTO. Un repo interrotto era indistinguibile da un
/// repo senza richieste, e ogni sua copia veniva riportata a «in-progress».
/// Misurato il 17/08/2026: `gh pr list` rispondeva 503 due volte su tre, e il
/// riconciliatore azzerava gli stati veri a ogni avvio di sessione.
fn state_richieste(repo_noti: &[String]) -> Py<(BTreeMap<String, String>, BTreeSet<String>)> {
    let mut fuori: BTreeMap<String, String> = BTreeMap::new();
    let mut answered: BTreeSet<String> = BTreeSet::new();
    for repo in repo_noti {
        for _ in 0..3 {
            let Ok(out) = Command::new("gh")
                .args([
                    "pr",
                    "list",
                    "--repo",
                    &format!("Other-repo-Work/{repo}"),
                    "--state",
                    "all",
                    "--limit",
                    "200",
                    "--json",
                    "headRefName,state,mergedAt",
                ])
                .output()
            else {
                continue;
            };
            if !out.status.success() {
                continue;
            }
            let testo = String::from_utf8_lossy(&out.stdout).into_owned();
            let testo = if testo.is_empty() { "[]".to_string() } else { testo };
            let Ok(listed) = serde_json::from_str::<Value>(&testo) else {
                continue;
            };
            answered.insert(repo.clone());
            let Value::Array(items) = listed else {
                return Err("la risposta di gh non è un elenco".into());
            };
            for r in items {
                let Value::Object(o) = r else {
                    return Err("una richiesta della lista non è un dizionario".into());
                };
                let state = if o.get("mergedAt").map(truthy).unwrap_or(false) {
                    "MERGED".to_string()
                } else {
                    match o.get("state") {
                        Some(Value::String(s)) => s.clone(),
                        Some(Value::Array(_)) | Some(Value::Object(_)) => {
                            return Err("stato non confrontabile".into())
                        }
                        // Rango zero: non sostituisce mai niente. Il Python lo
                        // userebbe come chiave, e nessuno lo rileggerebbe mai.
                        _ => continue,
                    }
                };
                // I rami con un nome non testuale finirebbero nel dizionario del
                // Python come chiavi che nessuna lettura può centrare: il
                // giudizio cerca sempre una stringa.
                let Some(Value::String(ramo)) = o.get("headRefName") else {
                    continue;
                };
                let prima = fuori.get(ramo).map(|s| rango(s)).unwrap_or(0);
                if rango(&state) > prima {
                    fuori.insert(ramo.clone(), state);
                }
            }
            break;
        }
    }
    Ok((fuori, answered))
}

/// Una lettura da git. `None` = non letto, `Some("")` = letto e vuoto.
fn read_git(path_: &str, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path_)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Quanto lavoro di questa copia non è atterrato da nessun'altra parte.
///
/// `None` = git non ha risposto, e su uno zero finto una copia verrebbe
/// dichiarata completata. **Non basta contare i commit non spinti**: con lo
/// squash-merge i commit originali non sono più antenati di niente, quindi
/// `rev-list --not --remotes` li conta tutti anche quando il contenuto è
/// atterrato da un pezzo. Prima si chiede al contenuto: se fondere nel transito
/// non cambierebbe un byte, non è rimasto niente.
fn unlanded_commits(path_: &str) -> Option<i64> {
    let quanti = read_git(path_, &["rev-list", "--count", "HEAD", "--not", "--remotes"])?;
    let quanti: i64 = if quanti.is_empty() {
        0
    } else {
        // `int(...)` di Python accetta gli spazi attorno e un segno.
        quanti.trim().parse().ok()?
    };
    if quanti == 0 {
        return Some(0);
    }
    for ramo in ["develop", "main", "master"] {
        let albero = read_git(path_, &["rev-parse", &format!("origin/{ramo}^{{tree}}")]);
        let albero = match albero {
            Some(a) if !a.is_empty() => a,
            _ => continue,
        };
        let fuso = read_git(
            path_,
            &["merge-tree", "--write-tree", &format!("origin/{ramo}"), "HEAD"],
        );
        if let Some(f) = fuso {
            if !f.is_empty() && f == albero {
                return Some(0);
            }
        }
        break;
    }
    Some(quanti)
}

/// Lo stato che questa copia dovrebbe avere.
///
/// «Richiesta unita» non basta per dire completata: `a-client/media-link-recovery`
/// era `completed` con la richiesta davvero unita e conteneva sette commit
/// scritti **dopo** la fusione. Una copia con commit che non vivono da
/// nessun'altra parte resta `in-progress`; `leftover = None` (git muto) vale
/// come residuo, perché non letto non è pulito.
pub(crate) fn state_giusto(
    worktree: &Value,
    richieste: &BTreeMap<String, String>,
    leftover: Option<i64>,
) -> Py<String> {
    let git = match worktree.get("git") {
        Some(v) if truthy(v) => {
            if !v.is_object() {
                return Err("il campo git non è un dizionario".into());
            }
            v.clone()
        }
        _ => json!({}),
    };
    if git.get("isMainWorktree").map(truthy).unwrap_or(false) {
        // Un checkout canonico non è una lavorazione: non finisce mai.
        return Ok("in-progress".into());
    }
    let ramo = match git.get("branch") {
        Some(Value::String(s)) => s.replace("refs/heads/", ""),
        Some(v) if truthy(v) => return Err("il ramo non è testo".into()),
        _ => String::new(),
    };
    let state = richieste.get(&ramo).map(String::as_str);
    match state {
        Some("MERGED") | Some("CLOSED") => Ok(match leftover {
            None => "in-progress".into(),
            Some(0) => "completed".into(),
            Some(_) => "in-progress".into(),
        }),
        Some("OPEN") => Ok("in-review".into()),
        _ => Ok("in-progress".into()),
    }
}

fn riconcilia(secco: bool) -> Py<i64> {
    // La carta illeggibile chiude il giro senza dire niente, come nell'originale.
    let repo_noti: Vec<String> = match fs::read_to_string(carta())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.get("repo").cloned())
    {
        Some(Value::Array(items)) => {
            let mut fuori = Vec::new();
            for r in items {
                match r.get("nome") {
                    Some(v) => fuori.push(py_str(Some(v))),
                    None => return Ok(0), // KeyError
                }
            }
            fuori
        }
        _ => return Ok(0),
    };

    let listing = orca_json(&["worktree", "list"]).unwrap_or(json!({}));
    let worktrees = worktrees_of(&listing)?;
    if worktrees.is_empty() {
        return Ok(0);
    }

    let (richieste, answered) = state_richieste(&repo_noti)?;
    if answered.is_empty() {
        // Nessun repo raggiunto: qui non si sa niente di niente, e riscrivere
        // trentatré stati su zero informazione è il guasto che questo giro deve
        // smettere di causare.
        println!("no repository answered: nothing to reconcile");
        registra(
            "tace",
            "nessun-repo-ha-risposto",
            &[
                ("interrogati", repo_noti.len() as i64),
                ("copie", worktrees.len() as i64),
            ],
        );
        return Ok(0);
    }

    let mut cambiati = 0i64;
    let mut skipped = 0i64;
    for w in &worktrees {
        // Il controllo sta DENTRO il ciclo, e non prima: l'originale si rompe
        // sull'elemento storto, dopo aver già scritto e stampato per quelli che
        // lo precedono. Anticiparlo cambierebbe cosa resta fatto a metà giro.
        if !w.is_object() {
            return Err("una copia dell'elenco non è un dizionario".into());
        }
        // Non sapere non è una smentita. Se il repo di questa copia è nella
        // carta ma non ha risposto, il suo stato resta com'è: senza, un 503
        // declassa a «in-progress» ogni lavorazione davvero in revisione.
        let path_ = match w.get("path") {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let slug = slug_of_remote(&path_);
        let mine = slug.rsplit('/').next().unwrap_or("").to_string();
        if repo_noti.contains(&mine) && !answered.contains(&mine) {
            skipped += 1;
            continue;
        }
        // Il conto si fa solo dove cambierebbe il verdetto — cioè dove senza di
        // esso la copia risulterebbe completata: quattro letture git invece di
        // venticinque.
        let mut leftover = Some(0);
        if state_giusto(w, &richieste, Some(0))? == "completed" {
            leftover = unlanded_commits(&path_);
        }
        let giusto = state_giusto(w, &richieste, leftover)?;
        if w.get("workspaceStatus") == Some(&Value::String(giusto.clone())) {
            continue;
        }
        cambiati += 1;
        let prima = py_str(w.get("workspaceStatus"));
        let nome = py_str(w.get("displayName"));
        if secco {
            println!("WOULD SET  {nome}: {prima} -> {giusto}");
        } else if write_state(w, &giusto)? {
            println!("set  {nome}: {prima} -> {giusto}");
        }
    }

    // UNA RIGA PER GIRO, e non è cosmesi: questo modo gira con l'uscita buttata
    // via, quindi ciò che stampa non lo legge nessuno. Con `saltate` nella riga,
    // «quanti repo non hanno risposto?» si chiede al registro invece che al caso.
    registra(
        if secco { "secco" } else { "riconcilia" },
        "giro-completato",
        &[
            ("risposti", answered.len() as i64),
            ("interrogati", repo_noti.len() as i64),
            ("copie", worktrees.len() as i64),
            ("cambiate", cambiati),
            ("saltate", skipped),
        ],
    );
    Ok(cambiati)
}

pub fn run() -> i32 {
    let args: Vec<String> = std::env::args().collect();
    // Le prove interne dell'originale non si portano: qui l'oracolo è il
    // confronto col Python (`tools/compare-work-status.py`) più la batteria di
    // questo modulo. Il ramo resta perché senza di lui `--test` finirebbe nel
    // modo gancio, cioè leggerebbe stdin e potrebbe scrivere su Orca.
    if args.iter().any(|a| a == "--test") {
        println!("le prove di questo porto: cargo test -p claude-hooks work_status");
        return 0;
    }
    if spento() {
        return 0;
    }
    if args.iter().any(|a| a == "--riconcilia") {
        let secco = args.iter().any(|a| a == "--secco");
        if let Err(e) = riconcilia(secco) {
            annota_guasto("riconcilia", &e);
        }
        return 0;
    }

    // Tre gesti, un gancio solo. Una sessione tocca il ciclo di vita di Orca
    // quando apre una richiesta, quando la fonde e quando crea una copia;
    // tenerli separati costerebbe due righe in più in `settings.json`, che il
    // 16/08/2026 si è rivelato il punto fragile del sistema.
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    for (name, step) in [
        ("dopo-richiesta", dopo_richiesta as fn(&Value) -> Py<Option<String>>),
        ("dopo-fusione", dopo_fusione),
        ("tab-di-avvio", tabs_left_behind),
    ] {
        match step(&payload) {
            Err(e) => annota_guasto(name, &e),
            Ok(None) => {}
            Ok(Some(said)) => println!(
                "{}",
                hook_io::python_json::dumps(&json!({
                    "hookSpecificOutput": {
                        "hookEventName": "PostToolUse",
                        "additionalContext": said,
                    }
                }))
            ),
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_is_recognised_like_opening() {
        // La forma del comando è il primo filtro: `(?![\w-])` esclude i vicini
        // che non sono lo stesso gesto — `gh pr merged`, `merge-queue`.
        for c in [
            "gh pr merge 468 --squash",
            "gh pr merge --auto",
            "git push && gh pr merge",
            "gh  pr  merge",
        ] {
            assert!(merges_request(c), "non riconosciuto: {c}");
        }
        for c in [
            "gh pr merged",
            "gh pr list --state merged",
            "gh pr create",
            "echo gh pr merge-queue",
        ] {
            assert!(!merges_request(c), "riconosciuto per sbaglio: {c}");
        }
        // MUTANTE: i due gesti non si confondono fra loro.
        assert!(!opens_request("gh pr merge"));
        assert!(!merges_request("gh pr create"));
    }

    fn lav(ramo: &str) -> Value {
        json!({"git": {"isMainWorktree": false, "branch": format!("refs/heads/{ramo}")}})
    }

    fn richieste(coppie: &[(&str, &str)]) -> BTreeMap<String, String> {
        coppie
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn riconosce_solo_il_gesto_che_apre_una_richiesta() {
        for c in [
            "gh pr create --title x",
            "git push && gh pr create --fill",
            "cd /tmp && gh pr create",
            "gh  pr  create",
        ] {
            assert!(opens_request(c), "{c}");
        }
        for c in [
            "gh pr list",
            "gh pr merge",
            "echo gh pr createx",
            "gh pr edit --title x",
            "gh pr create-draft",
        ] {
            assert!(!opens_request(c), "{c}");
        }
        // Il lookahead non deve fermare la ricerca al primo tentativo fallito.
        assert!(opens_request("gh pr create-draft && gh pr create"));
    }

    #[test]
    fn la_copia_piu_specifica_vince() {
        let listing = vec![json!({"path": "/a/work", "id": "1"}), json!({"path": "/a/work/suite", "id": "2"})];
        let scelto = |p: &str| {
            worktree_del_percorso(p, &listing)
                .unwrap()
                .map(|w| w["id"].as_str().unwrap().to_string())
        };
        assert_eq!(scelto("/a/work/suite/src"), Some("2".into()));
        assert_eq!(scelto("/a/work"), Some("1".into()));
        assert_eq!(scelto("/a/workshop"), None);
        assert_eq!(scelto("/b/other"), None);
    }

    #[test]
    fn l_ultimo_cambio_di_cartella_e_quello_che_conta() {
        let trees = vec![json!({"path": "/a/general", "id": "g"}), json!({"path": "/a/packages", "id": "p"})];
        let scelto = |c: &str| {
            worktree_of_command(c, "/a/general", &trees)
                .unwrap()
                .map(|w| w["id"].as_str().unwrap().to_string())
        };
        assert_eq!(scelto("cd /a/packages && gh pr create"), Some("p".into()));
        assert_eq!(
            scelto("cd /a/general && cd /a/packages && gh pr create"),
            Some("p".into())
        );
        assert_eq!(scelto("cd /tmp && cd /a/packages && gh pr create"), Some("p".into()));
        assert_eq!(scelto("git -C /a/packages push && gh pr create"), Some("p".into()));
        assert_eq!(scelto("gh pr create --fill"), Some("g".into()));
        assert_eq!(scelto("cd /nowhere && gh pr create"), Some("g".into()));
        // Un percorso relativo si risolve sul cwd, come `os.path.abspath`.
        assert_eq!(scelto("cd ../packages && gh pr create"), Some("p".into()));
    }

    #[test]
    fn il_giudizio_sullo_stato_giusto() {
        let principale = json!({"git": {"isMainWorktree": true, "branch": "refs/heads/main"}});
        let r = richieste(&[("main", "MERGED"), ("x", "MERGED")]);
        // Un checkout canonico non è mai in revisione né completato.
        assert_eq!(state_giusto(&principale, &r, Some(0)).unwrap(), "in-progress");
        assert_eq!(state_giusto(&principale, &r, Some(99)).unwrap(), "in-progress");
        assert_eq!(state_giusto(&lav("x"), &r, Some(0)).unwrap(), "completed");
        assert_eq!(
            state_giusto(&lav("x"), &richieste(&[("x", "OPEN")]), Some(0)).unwrap(),
            "in-review"
        );
        assert_eq!(
            state_giusto(&lav("x"), &richieste(&[("x", "CLOSED")]), Some(0)).unwrap(),
            "completed"
        );
        // Una richiesta unita non basta se la copia tiene commit che non vivono
        // da nessun'altra parte — il caso media-link-recovery.
        assert_eq!(state_giusto(&lav("x"), &r, Some(7)).unwrap(), "in-progress");
        // git muto non è un albero pulito.
        assert_eq!(state_giusto(&lav("x"), &r, None).unwrap(), "in-progress");
        // I residui non trasformano una richiesta aperta in lavorazione.
        assert_eq!(
            state_giusto(&lav("x"), &richieste(&[("x", "OPEN")]), Some(9)).unwrap(),
            "in-review"
        );
        assert_eq!(state_giusto(&lav("y"), &r, Some(0)).unwrap(), "in-progress");
        // Testa staccata: nessun ramo, nessuna richiesta.
        assert_eq!(
            state_giusto(&json!({"git": {"isMainWorktree": false}}), &r, Some(0)).unwrap(),
            "in-progress"
        );
    }

    #[test]
    fn lo_slug_esce_dall_url_stampato() {
        assert_eq!(
            slug_of_pull(Some(&json!({"stdout": "https://github.com/Other-repo-work/packages/pull/32"}))),
            "other-repo-work/packages"
        );
        assert_eq!(
            slug_of_pull(Some(&json!("git@github.com:Other-repo-work/packages/pull/9"))),
            "other-repo-work/packages"
        );
        assert_eq!(slug_of_pull(Some(&json!({"stdout": "done."}))), "");
        assert_eq!(slug_of_pull(None), "");
    }

    #[test]
    fn abspath_normalizza_come_python() {
        assert_eq!(abspath("/a/./b/../c", "/x"), "/a/c");
        assert_eq!(abspath("b/c", "/x"), "/x/b/c");
        assert_eq!(abspath("/a/", "/x"), "/a");
        assert_eq!(abspath("//a/b", "/x"), "//a/b");
        assert_eq!(abspath("///a/b", "/x"), "/a/b");
        assert_eq!(abspath("/..", "/x"), "/");
    }

    /// Un payload storto non deve produrre un verdetto: nell'originale solleva,
    /// e il chiamante annota il guasto invece di decidere.
    #[test]
    fn i_payload_storti_finiscono_dove_il_python_moriva() {
        assert!(dopo_richiesta(&json!([1, 2, 3])).is_err());
        assert!(dopo_richiesta(&json!({"tool_name": "Bash", "tool_input": "ciao"})).is_err());
        assert!(dopo_richiesta(&json!({"tool_name": "Bash", "tool_input": {"command": 42}})).is_err());
        // Questi invece tacciono e basta.
        assert_eq!(dopo_richiesta(&json!({"tool_name": "Write"})).unwrap(), None);
        assert_eq!(
            dopo_richiesta(&json!({"tool_name": "Bash", "tool_input": {}})).unwrap(),
            None
        );
        assert_eq!(
            dopo_richiesta(&json!({"tool_name": "Bash",
                                   "tool_input": {"command": "gh pr create"},
                                   "tool_response": {"is_error": true}}))
            .unwrap(),
            None
        );
    }

    /// La riga del registro esce con gli spazi di `json.dumps`, non senza.
    #[test]
    fn il_registro_usa_la_forma_del_python() {
        let casa = crate::test_home::HomeIsolata::nuova("work-status-registro");
        registra("riconcilia", "giro-completato", &[("risposti", 1), ("copie", 2)]);
        let riga = fs::read_to_string(casa.stato().join("ganci.jsonl")).unwrap();
        assert!(riga.contains(r#""gancio": "work-status""#), "{riga}");
        assert!(riga.contains(r#""decisione": "riconcilia""#), "{riga}");
        assert!(riga.contains(r#""risposti": 1, "copie": 2"#), "{riga}");
        // MUTANTE: con `journal::record` qui non ci sarebbe nessuno spazio.
        assert!(!riga.contains(r#""gancio":"work-status""#), "{riga}");
    }

    #[test]
    fn il_guasto_lascia_la_riga_col_nome_del_passo() {
        let casa = crate::test_home::HomeIsolata::nuova("work-status-guasti");
        annota_guasto("dopo-richiesta", "il payload non è un dizionario");
        let testo = fs::read_to_string(casa.stato().join("work-status-failures.log")).unwrap();
        assert!(testo.starts_with("--- dopo-richiesta\n"), "{testo}");
    }
}
