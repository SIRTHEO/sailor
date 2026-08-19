//! Quali controlli hanno ancora qualcuno che li lancia, e quali no.
//!
//! Porta di `other-repo/work/.claude/scripts/reachability.py`, scritto il 18/08/2026
//! e migrato lo stesso giorno: il §14 pretende che non si introduca nuova logica
//! applicativa in Python, e quello strumento la introduceva — misurando, per
//! giunta, proprio il criterio che smentiva.
//!
//! LA DOMANDA. «Un controllo che nessuno lancia» aveva solo risposte parziali:
//! quanti gate esistono, quanti lasciano traccia nel registro, quanti hanno un
//! caso di prova. Nessuna dice la cosa che conta — **partendo da ciò che si
//! accende oggi, dove si arriva?** L'analisi del 14/08 quella domanda l'aveva
//! posta e risolta **a mano** («i punti d'ingresso veri sono cinque», con una
//! tabella di undici script vivi solo sulla carta): un censimento fatto a mano
//! invecchia il giorno dopo, ed è il motivo per cui qui diventa un comando.
//!
//! Contare gli invocatori con un `grep` sbaglia in modo prevedibile:
//! `collaudo-carta.sh` nomina nove script, quindi quei nove «hanno un
//! invocatore», ma quello script non lo lancia nessuno e un invocatore morto non
//! accende niente. La raggiungibilità è transitiva: si parte dalle radici vive e
//! si cammina.
//!
//! COSA CONTA COME ARCO. Il testo di A nomina il file B: allora A può lanciare
//! B. È un'approssimazione generosa di proposito — sovrastima gli archi, quindi
//! **sottostima** gli irraggiungibili. Un file che risulta irraggiungibile qui lo
//! è davvero; uno raggiungibile può esserlo solo sulla carta.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

// ─── Dove si guarda ──────────────────────────────────────────────────────────

fn env_path(key: &str, fallback: PathBuf) -> PathBuf {
    std::env::var_os(key).map(PathBuf::from).unwrap_or(fallback)
}

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

fn workspace() -> PathBuf {
    env_path("REACH_WORKSPACE", PathBuf::from("/home/someone/other-repo/work"))
}

fn home_claude() -> PathBuf {
    env_path("REACH_HOME_CLAUDE", home().join(".claude"))
}

fn launch_agents() -> PathBuf {
    env_path("REACH_LAUNCH_AGENTS", home().join("Library/LaunchAgents"))
}

/// La cartella dei git hook di un repo: `core.hooksPath` quando c'è, `.git/hooks`
/// altrimenti.
///
/// Serve perché un clone di monte tiene i propri hook FUORI dal repo — dentro
/// `.git/hooks` i file non sono versionati né scrivibili dagli agenti — e senza
/// questa lettura `sblocca-skill-mattpocock.sh` risultava un orfano mentre git lo
/// esegue a ogni merge. È la classificazione peggiore possibile: dice «nessuno
/// lo lancia» proprio di uno script armato, e invita a cancellarlo.
///
/// Il valore si legge da `.git/config` invece che da `git config` perché la
/// misura non deve dipendere da un processo esterno; un percorso relativo si
/// risolve rispetto al repo, come fa git.
fn hooks_dir(repo: &Path) -> PathBuf {
    for line in safe_read(&repo.join(".git/config")).lines() {
        if let Some(value) = line.trim().strip_prefix("hooksPath") {
            let value = value.trim_start().strip_prefix('=').unwrap_or("").trim();
            if !value.is_empty() {
                let p = PathBuf::from(value);
                return if p.is_absolute() { p } else { repo.join(p) };
            }
        }
    }
    repo.join(".git/hooks")
}

/// Le estensioni di ciò che può essere lanciato. Un `.md` entra solo se è un
/// mandato: la documentazione cita gli script per spiegarli, non per eseguirli, e
/// contarla come arco farebbe risultare vivo tutto ciò che è ben documentato.
const EXECUTABLE_SUFFIXES: &[&str] = &["py", "sh", "mjs", "js"];

/// Cartelle che non contengono né radici né nodi. Un file di lock porta il nome
/// dello script che l'ha creato, e senza questo filtro ogni script con un lock
/// risulterebbe invocato da sé stesso.
const SKIP_DIRS: &[&str] = &[
    "__pycache__", "node_modules", ".git", "state", "archivio", "docs",
    "memoria", ".venv",
];

pub fn is_skipped(relative: &Path) -> bool {
    relative
        .components()
        .any(|c| SKIP_DIRS.contains(&c.as_os_str().to_string_lossy().as_ref()))
}

fn safe_read(path: &Path) -> String {
    std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default()
}

/// I percorsi sotto una cartella, ordinati come `sorted(Path.rglob())` in
/// Python: per la stringa del percorso, non per componenti. L'ordine conta
/// perché a parità di nome vince il primo trovato, e i due porti devono
/// scegliere lo stesso.
fn walk_sorted(base: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![base.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p.clone());
            }
            out.push(p);
        }
    }
    out.sort_by(|a, b| a.to_string_lossy().cmp(&b.to_string_lossy()));
    out
}

// ─── Le radici: cosa si accende davvero ──────────────────────────────────────

/// Le etichette di `launchctl list`, saltando l'intestazione.
///
/// Tre colonne separate da tabulazione: PID, ultimo esito, etichetta. Il PID
/// vale `-` per ciò che è caricato ma non in esecuzione adesso, ed è esattamente
/// il caso normale di un lavoro periodico: filtrare per PID presente
/// cancellerebbe quasi tutte le radici vere.
pub fn parse_launchctl_list(out: &str) -> BTreeSet<String> {
    let mut labels = BTreeSet::new();
    for line in out.lines().skip(1) {
        let parts: Vec<&str> = line.split('\t').collect();
        if parts.len() >= 3 && !parts[2].trim().is_empty() {
            labels.insert(parts[2].trim().to_string());
        }
    }
    labels
}

/// Le etichette spente secondo `launchctl print-disabled`: righe `"nome" => stato`.
pub fn disabled_labels(out: &str) -> BTreeSet<String> {
    let mut off = BTreeSet::new();
    for line in out.lines() {
        let Some(open) = line.find('"') else { continue };
        let rest = &line[open + 1..];
        let Some(close) = rest.find('"') else { continue };
        let label = &rest[..close];
        let after = &rest[close + 1..];
        let Some(arrow) = after.find("=>") else { continue };
        if after[arrow + 2..].trim().starts_with("disabled") {
            off.insert(label.to_string());
        }
    }
    off
}

/// Le etichette che launchd ha caricato adesso.
///
/// `disable` è più forte di `load`: una scritta in entrambi gli elenchi non gira.
/// Se `launchctl` non risponde si torna l'insieme vuoto e il rapporto lo
/// dichiara — meglio nessuna radice che radici inventate, perché una radice in
/// più nasconde proprio gli irraggiungibili che stiamo cercando.
/// Le due valvole del banco: fissano ciò che launchd risponderebbe.
///
/// Senza, il confronto d'equivalenza col Python non può toccare la regione più
/// importante — «un plist caricato è una radice, uno spento no» — perché nessuna
/// etichetta inventata risulta mai caricata sulla macchina vera. È la trappola
/// già pagata altrove: mille casi che non toccano la regione non provano niente.
/// E sta in **entrambi** i porti, perché una valvola letta da un lato solo li
/// farebbe divergere proprio dove il banco crede di confrontarli.
const FAKE_LIST: &str = "REACH_LAUNCHCTL_LIST";
const FAKE_DISABLED: &str = "REACH_LAUNCHCTL_DISABLED";

fn loaded_launchd_labels() -> BTreeSet<String> {
    let listing = match std::env::var(FAKE_LIST) {
        Ok(v) => v,
        Err(_) => match Command::new("launchctl").arg("list").output() {
            Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
            Err(_) => return BTreeSet::new(),
        },
    };
    let mut loaded = parse_launchctl_list(&listing);
    let disabled = match std::env::var(FAKE_DISABLED) {
        Ok(v) => v,
        Err(_) => match Command::new("launchctl")
            .arg("print-disabled")
            .arg(format!("gui/{}", current_uid()))
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
            Err(_) => return loaded,
        },
    };
    for label in disabled_labels(&disabled) {
        loaded.remove(&label);
    }
    loaded
}

/// `getuid` senza portarsi dietro la crate `libc` per una chiamata sola.
fn current_uid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

/// L'etichetta dichiarata dentro il plist, non dedotta dal nome del file.
///
/// Quasi sempre coincidono, e quando non coincidono launchd guarda il campo:
/// dedurre dal nome darebbe una radice viva a un plist che nessuno ha caricato.
/// Si scorre coppia per coppia perché `Label` non è quasi mai la prima chiave, e
/// prendere il primo `<string>` del file restituirebbe il percorso del programma.
pub fn plist_label(text: &str) -> String {
    let mut rest = text;
    loop {
        let Some(start) = rest.find("<key>") else { return String::new() };
        let after_key = &rest[start + 5..];
        let Some(end_key) = after_key.find("</key>") else { return String::new() };
        if after_key[..end_key].trim() == "Label" {
            let tail = &after_key[end_key + 6..];
            let Some(s) = tail.find("<string>") else { return String::new() };
            let value = &tail[s + 8..];
            let Some(e) = value.find("</string>") else { return String::new() };
            return value[..e].trim().to_string();
        }
        rest = &after_key[end_key..];
    }
}

/// Vero se una scheda Orca è accesa. La scheda porta `attiva`, l'API di Orca
/// risponde `enabled`: si accettano entrambi, e l'assenza vale «spenta», perché
/// un default vero renderebbe viva ogni scheda malformata.
fn card_enabled(text: &str) -> bool {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return false;
    };
    v.get("attiva") == Some(&serde_json::Value::Bool(true))
        || v.get("enabled") == Some(&serde_json::Value::Bool(true))
}

pub struct Root {
    pub path: PathBuf,
    pub why: String,
}

fn label_of(plist: &Path) -> String {
    let l = plist_label(&safe_read(plist));
    if l.is_empty() {
        plist.file_stem().unwrap_or_default().to_string_lossy().into_owned()
    } else {
        l
    }
}

fn sorted_children(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .collect();
    out.sort();
    out
}

/// I file da cui parte il cammino, con la ragione per cui sono vivi.
fn live_roots(loaded: &BTreeSet<String>) -> Vec<Root> {
    let (ws, hc, la) = (workspace(), home_claude(), launch_agents());
    let mut roots: Vec<Root> = Vec::new();

    for name in ["settings.json", "settings.local.json"] {
        for base in [&hc, &ws.join(".claude")] {
            let p = base.join(name);
            if p.is_file() {
                roots.push(Root { path: p, why: "Claude Code hooks".into() });
            }
        }
    }

    for p in sorted_children(&la) {
        if p.extension().map(|e| e == "plist").unwrap_or(false) {
            let label = label_of(&p);
            if loaded.contains(&label) {
                roots.push(Root { path: p, why: format!("launchd: {label}") });
            }
        }
    }

    let aut = ws.join(".claude/automazioni");
    for p in sorted_children(&aut) {
        if p.to_string_lossy().ends_with(".scheda.json") && card_enabled(&safe_read(&p)) {
            roots.push(Root { path: p, why: "enabled Orca automation".into() });
        }
    }

    let mut repos = sorted_children(&ws);
    repos.extend(sorted_children(&hc.join("skills")));
    repos.extend(sorted_children(&hc.join("plugins/marketplaces")));
    for repo in repos {
        if !repo.join(".git").exists() {
            continue;
        }
        let hooks = hooks_dir(&repo);
        if !hooks.is_dir() {
            continue;
        }
        let name = repo.file_name().unwrap_or_default().to_string_lossy().into_owned();
        for p in sorted_children(&hooks) {
            if p.is_file() && !p.to_string_lossy().ends_with(".sample") {
                roots.push(Root { path: p, why: format!("git hook: {name}") });
            }
        }
    }

    let mut justfiles = vec![ws.join("justfile")];
    justfiles.extend(sorted_children(&ws).into_iter().map(|d| d.join("justfile")));
    for p in justfiles {
        if p.is_file() {
            roots.push(Root { path: p, why: "justfile".into() });
        }
    }

    roots
}

/// I sorgenti del binario, che NON sono radici e servono a un'altra domanda.
///
/// La prima versione li metteva fra le radici, ragionando che una radice esegue
/// `claude-hooks` e quindi quel codice gira. Il ragionamento è giusto e la
/// conclusione sbagliata, perché confonde due relazioni diverse:
///
///   «A lancia B»       — è il cammino, ed è la domanda di questo strumento
///   «A ha sostituito B» — è il porto, ed è una classificazione
///
/// Mescolarle costa caro, e il conto è arrivato subito: i commenti di **questo
/// file**, che nomina `capienza.py`, `oracolo.py` e `collaudo-carta.sh` per
/// spiegare i propri casi, li facevano risultare raggiunti. Cinque orfani veri
/// spariti dall'elenco perché qualcuno li aveva citati in una spiegazione — cioè
/// il rapporto migliorava scrivendoci sopra della prosa.
///
/// Qui i sorgenti servono soltanto a riconoscere i ripieghi: `linear_readonly.rs`
/// nomina `linear-sola-lettura.py`, e quel `.py` non è dimenticato, è sostituito.
///
/// E VALE SOLO IL MODULO CHE PORTA QUEL GANCIO. Un file citato da un modulo
/// qualsiasi non è un ripiego: sposterebbe soltanto il difetto da «raggiunto» a
/// «non è un difetto», che è lo stesso errore con un'altra etichetta. Il legame
/// è il nome: `linear_readonly.rs` porta lo slug `linear-readonly`, che una
/// radice viva esegue. Questo modulo si chiama `reachability.rs` e nessuna
/// radice esegue `claude-hooks reachability`, quindi i suoi commenti — che
/// nominano mezzo parco per spiegarsi — non salvano nessuno.
fn ported_fallback_names(
    node_paths: &BTreeMap<String, PathBuf>,
    ported: &BTreeSet<String>,
) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for p in walk_sorted(&home_claude().join("rust")) {
        if !(p.extension().map(|e| e == "rs").unwrap_or(false)
            && p.parent().map(|d| d.ends_with("src")).unwrap_or(false))
        {
            continue;
        }
        let slug = p
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .replace('_', "-");
        if !ported.contains(&slug) {
            continue;
        }
        let text = safe_read(&p);
        for name in node_paths.keys() {
            if cites(&text, name) {
                out.insert(name.clone());
            }
        }
    }
    out
}

/// Le radici che ci sono e non si accendono: spente per decisione.
///
/// Un plist su disco che launchd non ha caricato, e una scheda con
/// `"attiva": false`, sono la stessa cosa: un punto d'ingresso in pausa. Ciò che
/// si raggiunge **solo** di lì non è dimenticato — è fermo insieme a loro, e il
/// giorno che si riaccendono torna vivo senza toccare una riga. Serve perché il
/// riconoscimento per nome copre un caso solo: la catena del Loop engineer
/// (`motore.py` → `oracolo.py` → `capienza.py` → `repo.py`) è ferma perché
/// `work.other-repo.motore` è `disabled` dal 16/08, e nessuno di quei nomi somiglia a
/// quello del lavoro launchd che li accendeva.
///
/// L'elenco dei caricati arriva da fuori e non si rilegge qui: due
/// interrogazioni a launchd in momenti diversi possono rispondere diverso, e uno
/// stesso lavoro finirebbe vivo per un elenco e dormiente per l'altro.
fn dormant_roots(loaded: &BTreeSet<String>) -> Vec<Root> {
    let (ws, la) = (workspace(), launch_agents());
    let mut out = Vec::new();

    for p in sorted_children(&la) {
        if p.extension().map(|e| e == "plist").unwrap_or(false) {
            let label = label_of(&p);
            if !loaded.contains(&label) {
                out.push(Root { path: p, why: format!("launchd job not loaded: {label}") });
            }
        }
    }

    let aut = ws.join(".claude/automazioni");
    let children = sorted_children(&aut);
    for p in &children {
        if !p.to_string_lossy().ends_with(".scheda.json") || card_enabled(&safe_read(p)) {
            continue;
        }
        let stem = p
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .replace(".scheda.json", "");
        out.push(Root { path: p.clone(), why: "switched-off Orca automation".into() });
        // Il mandato e il precheck sono il corpo della scheda: senza di loro una
        // scheda spenta non porta a niente, e la catena che teneva in vita
        // l'automazione resterebbe fra i difetti.
        for s in &children {
            let is_sibling = s
                .file_name()
                .map(|n| n.to_string_lossy().starts_with(&format!("{stem}.")))
                .unwrap_or(false);
            if is_sibling && s != p {
                out.push(Root {
                    path: s.clone(),
                    why: "body of a switched-off automation".into(),
                });
            }
        }
    }
    out
}

// ─── I nodi: ciò che può essere lanciato ─────────────────────────────────────

/// Mappa nome-del-file → percorso, per tutto ciò che si può lanciare.
///
/// A parità di nome vince il primo trovato: due script omonimi in due cartelle
/// sono un caso che qui non si è mai presentato, e risolverlo per contenuto
/// costerebbe più di quanto renda.
fn nodes(active_mandates: &BTreeSet<String>) -> BTreeMap<String, PathBuf> {
    let (ws, hc) = (workspace(), home_claude());
    let mut found: BTreeMap<String, PathBuf> = BTreeMap::new();
    for base in [ws.join(".claude"), hc.join("scripts"), hc.join("skills/hooks")] {
        if !base.is_dir() {
            continue;
        }
        for p in walk_sorted(&base) {
            if !p.is_file() {
                continue;
            }
            let Ok(rel) = p.strip_prefix(&base) else { continue };
            if is_skipped(rel) {
                continue;
            }
            let name = p.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let ext_ok = p
                .extension()
                .map(|e| EXECUTABLE_SUFFIXES.contains(&e.to_string_lossy().as_ref()))
                .unwrap_or(false);
            if ext_ok || (name.ends_with(".mandato.md") && active_mandates.contains(&name)) {
                found.entry(name).or_insert(p);
            }
        }
    }
    found
}

/// I nomi dei mandati di automazioni accese. Oggi, di norma, nessuno.
fn active_mandate_names() -> BTreeSet<String> {
    let aut = workspace().join(".claude/automazioni");
    let mut names = BTreeSet::new();
    for p in sorted_children(&aut) {
        if p.to_string_lossy().ends_with(".scheda.json") && card_enabled(&safe_read(&p)) {
            let n = p.file_name().unwrap_or_default().to_string_lossy();
            names.insert(n.replace(".scheda.json", ".mandato.md"));
        }
    }
    names
}

/// Le automazioni spente, per nome: il loro `.sh` e il loro precheck sono fermi
/// per decisione dell'11/08, non per dimenticanza.
fn off_automation_names() -> BTreeSet<String> {
    let aut = workspace().join(".claude/automazioni");
    let mut names = BTreeSet::new();
    for p in sorted_children(&aut) {
        if p.to_string_lossy().ends_with(".scheda.json") && !card_enabled(&safe_read(&p)) {
            let n = p.file_name().unwrap_or_default().to_string_lossy();
            names.insert(n.replace(".scheda.json", ""));
        }
    }
    names
}

/// Vero se il testo nomina quel file.
///
/// Il confine a sinistra e a destra evita l'inclusione fra nomi: senza,
/// `rami.py` risulterebbe citato da ogni riga che nomina `chiudi-rami.py`, e
/// l'arco falso terrebbe in vita uno script morto.
///
/// LA BARRA NON È UN CONFINE, ed è l'errore che questa funzione ha fatto al
/// primo giro nel Python: `scripts/rami.py` è il modo normale di citare uno
/// script, e mettendo `/` fra i caratteri vietati a sinistra sparivano quasi
/// tutti gli archi veri — cioè lo strumento avrebbe dichiarato morto mezzo parco.
///
/// Scritto a mano e non con una regex perché `regex` non ha i lookaround, e
/// perché qui si cerca una sottostringa letterale su decine di migliaia di
/// coppie: confrontare i due caratteri adiacenti costa meno di costruire un automa.
pub fn cites(text: &str, name: &str) -> bool {
    let bytes = text.as_bytes();
    let n = name.len();
    let mut from = 0usize;
    while let Some(rel) = text[from..].find(name) {
        let start = from + rel;
        let end = start + n;
        let before_ok = start == 0 || !forbids_on_the_left(bytes[start - 1]);
        let after_ok = end >= bytes.len() || !forbids_on_the_right(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

/// A sinistra vietano l'arco: lettere, cifre, `_`, `.` e `-`. La barra no.
fn forbids_on_the_left(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'.' || c == b'-'
}

/// A destra vietano l'arco: lettere, cifre, `_` e `-` — non il punto, o
/// `rami.py.bak` non conterebbe come citazione di `rami.py`.
fn forbids_on_the_right(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_' || c == b'-'
}

// ─── Il cammino ──────────────────────────────────────────────────────────────

type Graph = BTreeMap<String, BTreeSet<String>>;

fn edges(sources: &BTreeMap<String, PathBuf>, targets: &BTreeMap<String, PathBuf>) -> Graph {
    let mut out: Graph = BTreeMap::new();
    for (key, path) in sources {
        let text = safe_read(path);
        let own = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        let mut named = BTreeSet::new();
        for n in targets.keys() {
            if *n != own && cites(&text, n) {
                named.insert(n.clone());
            }
        }
        out.insert(key.clone(), named);
    }
    out
}

/// Chi si raggiunge dalle radici, e gli archi di tutti i nodi.
///
/// Gli archi servono anche dopo: il rapporto sugli irraggiungibili deve poter
/// dire **chi li cita**, perché «nessuno li nomina» e «li nomina solo un morto»
/// sono due diagnosi diverse, e la cura pure.
fn walk(roots: &[Root], node_paths: &BTreeMap<String, PathBuf>) -> (BTreeSet<String>, Graph) {
    let mut sources: BTreeMap<String, PathBuf> = BTreeMap::new();
    for r in roots {
        sources.insert(r.path.to_string_lossy().into_owned(), r.path.clone());
    }
    for p in node_paths.values() {
        sources.insert(p.to_string_lossy().into_owned(), p.clone());
    }
    let graph = edges(&sources, node_paths);

    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut frontier: Vec<PathBuf> = roots.iter().map(|r| r.path.clone()).collect();
    while let Some(path) = frontier.pop() {
        let Some(named) = graph.get(path.to_string_lossy().as_ref()) else {
            continue;
        };
        let next: Vec<String> = named.iter().cloned().collect();
        for name in next {
            if reached.insert(name.clone()) {
                frontier.push(node_paths[&name].clone());
            }
        }
    }
    (reached, graph)
}

fn citers(name: &str, graph: &Graph, node_paths: &BTreeMap<String, PathBuf>) -> Vec<String> {
    let by_path: BTreeMap<String, String> = node_paths
        .iter()
        .map(|(n, p)| (p.to_string_lossy().into_owned(), n.clone()))
        .collect();
    let mut out: Vec<String> = graph
        .iter()
        .filter(|(_, named)| named.contains(name))
        .map(|(src, _)| {
            by_path.get(src).cloned().unwrap_or_else(|| {
                Path::new(src).file_name().unwrap_or_default().to_string_lossy().into_owned()
            })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

// ─── Perché è irraggiungibile: cinque ragioni, e una sola è un difetto ───────
//
// Un elenco di decine di file senza distinzioni è rumore, e un controllo saturo
// di rumore è cieco: dei gruppi qui sotto, uno solo va letto.

pub const PORTED: &str = "ported to Rust";
pub const PORTED_FALLBACK: &str = "part of a ported fallback";
pub const DISABLED_AUTOMATION: &str = "automation switched off";
pub const DORMANT: &str = "reachable only from a switched-off root";
pub const ONE_SHOT: &str = "one-shot tool";
pub const ORPHAN: &str = "orphan";

/// Gli slug che una radice viva esegue attraverso il binario Rust.
///
/// Un gancio portato lascia il suo `.py` sul disco come ripiego, e da quel
/// momento nessuno lo nomina più: risulta irraggiungibile ed è **giusto** che lo
/// sia. Si guardano tutte le radici, non il solo `settings.json`: la staffetta è
/// l'unico loop vivo e il suo slug (`relay`) sta in un `.plist`. Leggendo i soli
/// settings, `relay.py` risultava un orfano — cioè il rapporto avrebbe segnalato
/// come dimenticato il ripiego del meccanismo più attivo che c'è.
pub fn ported_stems(texts: &[String]) -> BTreeSet<String> {
    let mut stems = BTreeSet::new();
    for text in texts {
        let mut from = 0usize;
        while let Some(rel) = text[from..].find("claude-hooks") {
            let start = from + rel + "claude-hooks".len();
            let rest = &text[start..];
            let trimmed = rest.trim_start_matches([' ', '\t']);
            if rest.len() != trimmed.len() {
                let slug: String = trimmed
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                    .collect();
                if !slug.is_empty() {
                    stems.insert(slug);
                }
            }
            from = start;
        }
    }
    stems
}

/// Uno strumento nato per un giorno porta la data nel nome: è la convenzione in
/// uso qui («muore dopo l'uso»), e senza questo riconoscimento ogni pulizia
/// passata resterebbe per sempre in cima all'elenco dei difetti.
fn is_dated(name: &str) -> bool {
    let b = name.as_bytes();
    if b.len() < 11 {
        return false;
    }
    for start in 0..=b.len() - 11 {
        // La finestra si taglia su indici di byte, e un accento occupa due byte:
        // senza questa guardia `però-2026-08-19` andava in panico invece di
        // rispondere, e un gancio che va in errore rifiuta ogni strumento. Il
        // modello cercato è tutto ASCII, quindi le finestre scartate qui non
        // possono contenerlo.
        if !name.is_char_boundary(start) || !name.is_char_boundary(start + 11) {
            continue;
        }
        let w = &name[start..start + 11];
        let digits_ok = w.as_bytes()[3].is_ascii_digit()
            && w.as_bytes()[4].is_ascii_digit()
            && w.as_bytes()[6].is_ascii_digit()
            && w.as_bytes()[7].is_ascii_digit()
            && w.as_bytes()[9].is_ascii_digit()
            && w.as_bytes()[10].is_ascii_digit();
        if !(w.starts_with("-20") && w.as_bytes()[5] == b'-' && w.as_bytes()[8] == b'-' && digits_ok)
        {
            continue;
        }
        let tail = &name[start + 11..];
        if tail.is_empty() {
            return true;
        }
        if let Some(rest) = tail.strip_prefix('.') {
            if !rest.is_empty() && rest.chars().all(|c| c.is_ascii_alphanumeric()) {
                return true;
            }
        }
    }
    false
}

pub fn why_unreachable(
    name: &str,
    stem: &str,
    ported: &BTreeSet<String>,
    off: &BTreeSet<String>,
) -> String {
    if ported.contains(stem) {
        return PORTED.into();
    }
    let base = name.split('.').next().unwrap_or(name);
    if off.contains(base) {
        return DISABLED_AUTOMATION.into();
    }
    if is_dated(stem) || is_dated(name) {
        return ONE_SHOT.into();
    }
    ORPHAN.into()
}

/// La ragione finale, in un posto solo e senza toccare il disco.
///
/// Il declassamento da difetto a «in pausa» viveva dentro la misura, dove
/// nessuna prova arriva: un mutante che lo toglieva restava verde, e il rapporto
/// avrebbe rimesso fra i difetti l'intera catena del Loop engineer senza che
/// niente lo segnalasse.
pub fn classify(
    name: &str,
    stem: &str,
    ported: &BTreeSet<String>,
    off: &BTreeSet<String>,
    dormant_reached: &BTreeSet<String>,
) -> String {
    let why = why_unreachable(name, stem, ported, off);
    if why == ORPHAN && dormant_reached.contains(name) {
        return DORMANT.into();
    }
    why
}

#[derive(Clone)]
pub struct Orphan {
    pub file: String,
    pub path: String,
    pub cited_by: Vec<String>,
    pub reason: String,
}

/// Chi è nominato soltanto da ripieghi, è ripiego a sua volta.
///
/// `handoff_common.py` non è dimenticato: lo importano tre ganci che oggi girano
/// dal binario. Senza questo passaggio i moduli condivisi di un porto compaiono
/// uno per uno fra i difetti, e ogni porto riuscito peggiora il rapporto invece
/// di migliorarlo.
///
/// Basta un citante portato, non tutti: la prima versione pretendeva che lo
/// fossero tutti e lasciava fuori proprio `handoff_common.py`, cui bastava
/// essere nominato anche da uno strumento qualsiasi per tornare fra i difetti.
pub fn spread_ported(orphans: &mut [Orphan]) {
    let mut ported: BTreeSet<String> = orphans
        .iter()
        .filter(|o| o.reason == PORTED || o.reason == PORTED_FALLBACK)
        .map(|o| o.file.clone())
        .collect();
    let mut changed = true;
    while changed {
        changed = false;
        for o in orphans.iter_mut() {
            if o.reason != ORPHAN || o.cited_by.is_empty() {
                continue;
            }
            if o.cited_by.iter().any(|c| ported.contains(c)) {
                o.reason = PORTED_FALLBACK.into();
                ported.insert(o.file.clone());
                changed = true;
            }
        }
    }
}

pub struct Measure {
    pub roots: Vec<(String, String)>,
    pub launchd_loaded: Vec<String>,
    pub nodes: usize,
    pub reached: usize,
    pub orphans: Vec<Orphan>,
}

pub fn measure() -> Measure {
    let loaded = loaded_launchd_labels();
    let roots = live_roots(&loaded);
    let node_paths = nodes(&active_mandate_names());
    let (reached, graph) = walk(&roots, &node_paths);

    let root_texts: Vec<String> = roots.iter().map(|r| safe_read(&r.path)).collect();
    let ported = ported_stems(&root_texts);
    let off = off_automation_names();
    // Le radici spente si camminano a parte: ciò che si raggiunge solo di lì è
    // in pausa insieme a loro, non dimenticato.
    let (dormant_reached, _) = walk(&dormant_roots(&loaded), &node_paths);

    let fallbacks = ported_fallback_names(&node_paths, &ported);

    let mut orphans = Vec::new();
    for (name, path) in &node_paths {
        if reached.contains(name) {
            continue;
        }
        let who: Vec<String> = citers(name, &graph, &node_paths)
            .into_iter()
            .filter(|c| c != name)
            .collect();
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let mut reason = classify(name, &stem, &ported, &off, &dormant_reached);
        if reason == ORPHAN && fallbacks.contains(name) {
            reason = PORTED_FALLBACK.into();
        }
        orphans.push(Orphan {
            file: name.clone(),
            path: path.to_string_lossy().into_owned(),
            cited_by: who,
            reason,
        });
    }
    spread_ported(&mut orphans);

    Measure {
        roots: roots
            .iter()
            .map(|r| (r.path.to_string_lossy().into_owned(), r.why.clone()))
            .collect(),
        launchd_loaded: loaded.into_iter().collect(),
        nodes: node_paths.len(),
        reached: reached.len(),
        orphans,
    }
}

fn to_json(m: &Measure) -> serde_json::Value {
    serde_json::json!({
        "roots": m.roots.iter().map(|(p, w)| serde_json::json!({"path": p, "why": w}))
            .collect::<Vec<_>>(),
        "launchd_loaded": m.launchd_loaded,
        "nodes": m.nodes,
        "reached": m.reached,
        "orphans": m.orphans.iter().map(|o| serde_json::json!({
            "file": o.file,
            "path": o.path,
            "cited_by": o.cited_by,
            "reason": o.reason,
        })).collect::<Vec<_>>(),
    })
}

fn report(m: &Measure) -> i32 {
    println!("live roots: {}", m.roots.len());
    for (p, w) in &m.roots {
        let name = Path::new(p).file_name().unwrap_or_default().to_string_lossy();
        println!("  · {name}  ({w})");
    }
    if m.launchd_loaded.is_empty() {
        println!(
            "  WARNING: launchctl did not answer — no launchd root in this run, \
             the orphans below are overcounted"
        );
    }
    println!();
    println!(
        "launchable files: {} · reached: {} · unreachable: {}",
        m.nodes,
        m.reached,
        m.orphans.len()
    );

    for reason in [PORTED, PORTED_FALLBACK, DISABLED_AUTOMATION, DORMANT, ONE_SHOT] {
        let group: Vec<&Orphan> = m.orphans.iter().filter(|o| o.reason == reason).collect();
        if group.is_empty() {
            continue;
        }
        let head: Vec<&str> = group.iter().take(4).map(|o| o.file.as_str()).collect();
        let more = if group.len() > 4 { " …" } else { "" };
        println!(
            "  · {} {reason} (expected, not a defect): {}{more}",
            group.len(),
            head.join(", ")
        );
    }

    let real: Vec<&Orphan> = m.orphans.iter().filter(|o| o.reason == ORPHAN).collect();
    if real.is_empty() {
        println!("\nno orphaned check");
        return 0;
    }
    println!("\nORPHANS ({}) — nothing live leads here:", real.len());
    for o in &real {
        let who = if o.cited_by.is_empty() {
            String::new()
        } else {
            format!("  ← named by {}", o.cited_by.join(", "))
        };
        println!("  · {}{who}", o.file);
    }
    1
}

pub fn run() -> i32 {
    let json = std::env::args().any(|a| a == "--json");
    let m = measure();
    if json {
        println!("{}", serde_json::to_string_pretty(&to_json(&m)).unwrap_or_default());
        return i32::from(!m.orphans.is_empty());
    }
    report(&m)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    /// Le prove che fissano le cartelle passano da qui, una alla volta.
    ///
    /// `std::env` e' globale al PROCESSO e cargo esegue i test in parallelo su
    /// piu' thread: due prove che scrivono `REACH_HOME_CLAUDE` nello stesso
    /// istante si guastano a vicenda, e il guasto e' intermittente — la
    /// batteria dava 136, 137 o rosso a giri diversi senza che niente fosse
    /// cambiato. Il commento che diceva «girano in un thread solo» era
    /// semplicemente falso, ed e' il tipo di affermazione che sopravvive
    /// proprio perche' nessuno la verifica.
    fn ambiente() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
            .lock()
            .unwrap_or_else(|e| e.into_inner())
    }

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn names_a_file_only_on_a_real_boundary() {
        assert!(cites("bash scripts/rami.py --check", "rami.py"));
        assert!(cites("rami.py", "rami.py"));
        assert!(cites("\"rami.py\"", "rami.py"));
        // Dopo una cartella l'arco è vero, dopo un trattino no: il primo giro
        // del Python li sbagliava entrambi allo stesso modo.
        assert!(cites(".claude/scripts/rami.py", "rami.py"));
        assert!(!cites(".claude/scripts/chiudi-rami.py", "rami.py"));
        assert!(!cites("python chiudi-rami.py", "rami.py"));
        assert!(!cites("rami.python", "rami.py"));
        // Il punto non è un jolly: in una regex lo sarebbe.
        assert!(!cites("ramixpy", "rami.py"));
    }

    #[test]
    fn reads_a_loaded_label_with_no_pid() {
        // `launchctl list` mette `-` nel PID di ciò che è caricato e non sta
        // girando adesso: è la forma di ogni lavoro periodico.
        let listing = "PID\tStatus\tLabel\n662\t0\tcom.a.running\n-\t0\twork.other-repo.idle\n";
        assert_eq!(
            parse_launchctl_list(listing),
            set(&["com.a.running", "work.other-repo.idle"])
        );
        assert!(!parse_launchctl_list(listing).contains("Label"));
    }

    #[test]
    fn reads_the_disabled_ones() {
        assert_eq!(
            disabled_labels("\"work.other-repo.motore\" => disabled\n\"other\" => enabled"),
            set(&["work.other-repo.motore"])
        );
        assert!(disabled_labels("\"other\" => enabled").is_empty());
    }

    #[test]
    fn plist_label_comes_from_the_field() {
        assert_eq!(
            plist_label("<key>Label</key><string>work.other-repo.x</string>"),
            "work.other-repo.x"
        );
        // `Label` non è quasi mai la prima chiave: prendere il primo `<string>`
        // del file restituirebbe il percorso del programma.
        assert_eq!(
            plist_label(
                "<key>Program</key><string>/bin/x</string>\
                 <key>Label</key><string>vero</string>"
            ),
            "vero"
        );
        assert_eq!(plist_label("<dict></dict>"), "");
    }

    #[test]
    fn skips_state_and_cache() {
        assert!(is_skipped(Path::new("state/catene/x.json")));
        assert!(is_skipped(Path::new("__pycache__/x.py")));
        assert!(!is_skipped(Path::new("scripts/x.py")));
    }

    #[test]
    fn a_card_is_off_unless_it_says_otherwise() {
        assert!(card_enabled("{\"attiva\": true}"));
        assert!(card_enabled("{\"enabled\": true}"));
        assert!(!card_enabled("{\"attiva\": false}"));
        assert!(!card_enabled("{}"));
        assert!(!card_enabled("non e json"));
    }

    #[test]
    fn ported_slugs_come_from_the_binary_invocation() {
        let texts = vec![
            "nohup claude-hooks work-status --riconcilia".to_string(),
            "/path/claude-hooks relay".to_string(),
            "claude-hooksnospace".to_string(),
        ];
        assert_eq!(ported_stems(&texts), set(&["work-status", "relay"]));
    }

    #[test]
    fn a_dated_name_is_a_one_shot_tool() {
        assert!(is_dated("clean-orphans-2026-08-16"));
        assert!(is_dated("test-leftovers-2026-08-18.py"));
        assert!(!is_dated("cross-repo"));
        assert!(!is_dated("rami"));
    }

    /// La finestra scorre sui byte, e prima di questa prova un accento la faceva
    /// cadere dentro un carattere: il gancio andava in panico invece di
    /// rispondere. La data si riconosce ancora, perché il modello è tutto ASCII.
    #[test]
    fn an_accent_in_the_name_does_not_break_the_window() {
        assert!(is_dated("però-2026-08-19"));
        assert!(is_dated("misura-perché-2026-08-19.md"));
        assert!(!is_dated("però-non-datato"));
    }

    #[test]
    fn the_reasons_do_not_overlap() {
        let ported = set(&["cd-guard"]);
        let off = set(&["maggiordomo-pr"]);
        assert_eq!(why_unreachable("cd-guard.py", "cd-guard", &ported, &off), PORTED);
        assert_eq!(
            why_unreachable("maggiordomo-pr.precheck.sh", "maggiordomo-pr.precheck", &ported, &off),
            DISABLED_AUTOMATION
        );
        assert_eq!(
            why_unreachable("clean-orphans-2026-08-16.sh", "clean-orphans-2026-08-16", &ported, &off),
            ONE_SHOT
        );
        assert_eq!(why_unreachable("cross-repo.py", "cross-repo", &ported, &off), ORPHAN);
    }

    #[test]
    fn a_dormant_root_puts_an_orphan_on_hold() {
        let none = BTreeSet::new();
        assert_eq!(
            classify("ciclo.py", "ciclo", &none, &none, &set(&["ciclo.py"])),
            DORMANT
        );
        assert_eq!(classify("ciclo.py", "ciclo", &none, &none, &none), ORPHAN);
        // Ciò che una decisione ha già spiegato non viene riscritto in pausa.
        assert_eq!(
            classify("cd-guard.py", "cd-guard", &set(&["cd-guard"]), &none, &set(&["cd-guard.py"])),
            PORTED
        );
    }

    #[test]
    fn a_module_a_ported_hook_imports_is_not_a_defect() {
        let mut orphans = vec![
            Orphan { file: "gancio.py".into(), path: String::new(), cited_by: vec![],
                     reason: PORTED.into() },
            Orphan { file: "comune.py".into(), path: String::new(),
                     cited_by: vec!["gancio.py".into(), "strumento.py".into()],
                     reason: ORPHAN.into() },
            Orphan { file: "nipote.py".into(), path: String::new(),
                     cited_by: vec!["comune.py".into()], reason: ORPHAN.into() },
            Orphan { file: "strumento.py".into(), path: String::new(), cited_by: vec![],
                     reason: ORPHAN.into() },
        ];
        spread_ported(&mut orphans);
        let by: BTreeMap<&str, &str> =
            orphans.iter().map(|o| (o.file.as_str(), o.reason.as_str())).collect();
        assert_eq!(by["comune.py"], PORTED_FALLBACK);
        assert_eq!(by["nipote.py"], PORTED_FALLBACK);
        assert_eq!(by["strumento.py"], ORPHAN);
    }

    #[test]
    fn only_the_module_that_ports_a_hook_marks_a_fallback() {
        let dir = crate::test_home::test_root().join("reach-fb");
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("casa/rust/crates/x/src");
        std::fs::create_dir_all(&src).unwrap();
        // Il modulo che porta un gancio vivo: cio che nomina e ripiego.
        std::fs::write(src.join("linear_readonly.rs"),
                       "//! Porta di linear-sola-lettura.py\n").unwrap();
        // Un modulo che nessuna radice esegue: i suoi commenti nominano mezzo
        // parco per spiegarsi, e non devono salvare niente. E il caso vero:
        // scrivendo la documentazione di questo file, cinque orfani erano
        // spariti dall'elenco.
        std::fs::write(src.join("reachability.rs"),
                       "//! Cita capienza.py e oracolo.py per spiegarsi\n").unwrap();

        // SAFETY: serializzata da `ambiente()`, e rimette la variabile com'era.
        let _guardia = ambiente();
        unsafe { std::env::set_var("REACH_HOME_CLAUDE", dir.join("casa")); }

        let node_paths: BTreeMap<String, PathBuf> =
            ["linear-sola-lettura.py", "capienza.py", "oracolo.py"]
                .iter()
                .map(|n| (n.to_string(), PathBuf::from(n)))
                .collect();
        let got = ported_fallback_names(&node_paths, &set(&["linear-readonly"]));
        assert!(got.contains("linear-sola-lettura.py"));
        assert!(!got.contains("capienza.py"), "un modulo che nessuno esegue non salva nessuno");
        assert!(!got.contains("oracolo.py"));

        unsafe { std::env::remove_var("REACH_HOME_CLAUDE"); }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_walk_is_transitive_and_stops_at_orphans() {
        let dir = crate::test_home::test_root().join("reach-walk");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("root.sh"), "bash a.sh\n").unwrap();
        std::fs::write(dir.join("a.sh"), "python b.py\n").unwrap();
        std::fs::write(dir.join("b.py"), "print(1)\n").unwrap();
        std::fs::write(dir.join("citato-da-orfano.py"), "print(3)\n").unwrap();
        std::fs::write(dir.join("orfano.py"), "# citato-da-orfano.py\n").unwrap();

        let node_paths: BTreeMap<String, PathBuf> =
            ["a.sh", "b.py", "citato-da-orfano.py", "orfano.py"]
                .iter()
                .map(|n| (n.to_string(), dir.join(n)))
                .collect();
        let roots = vec![Root { path: dir.join("root.sh"), why: "test".into() }];
        let (reached, graph) = walk(&roots, &node_paths);

        assert!(reached.contains("a.sh"));
        assert!(reached.contains("b.py"), "il cammino deve essere transitivo");
        assert!(!reached.contains("orfano.py"));
        assert!(!reached.contains("citato-da-orfano.py"));
        assert_eq!(citers("citato-da-orfano.py", &graph, &node_paths), vec!["orfano.py"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_root_is_what_fires_not_what_exists() {
        let dir = crate::test_home::test_root().join("reach-roots");
        let _ = std::fs::remove_dir_all(&dir);
        let agents = dir.join("agents");
        let aut = dir.join("ws/.claude/automazioni");
        std::fs::create_dir_all(&agents).unwrap();
        std::fs::create_dir_all(&aut).unwrap();
        std::fs::write(agents.join("vivo.plist"),
                       "<key>Label</key><string>work.other-repo.vivo</string>").unwrap();
        std::fs::write(agents.join("fermo.plist"),
                       "<key>Label</key><string>work.other-repo.fermo</string>").unwrap();
        std::fs::write(aut.join("accesa.scheda.json"), "{\"attiva\": true}").unwrap();
        std::fs::write(aut.join("spenta.scheda.json"), "{\"attiva\": false}").unwrap();
        std::fs::write(aut.join("spenta.mandato.md"), "esegui qualcosa").unwrap();

        // SAFETY: `ambiente()` serializza le prove che scrivono `std::env`, e
        // ognuna rimette le variabili com'erano prima di uscire.
        let _guardia = ambiente();
        unsafe {
            std::env::set_var("REACH_LAUNCH_AGENTS", &agents);
            std::env::set_var("REACH_WORKSPACE", dir.join("ws"));
            std::env::set_var("REACH_HOME_CLAUDE", dir.join("casa"));
        }

        let loaded = set(&["work.other-repo.vivo"]);
        let live: BTreeSet<String> = live_roots(&loaded)
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(live.contains("vivo.plist"));
        assert!(!live.contains("fermo.plist"), "un plist non caricato non è una radice");
        assert!(live.contains("accesa.scheda.json"));
        assert!(!live.contains("spenta.scheda.json"));

        let dormant: BTreeSet<String> = dormant_roots(&loaded)
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(dormant.contains("fermo.plist"));
        assert!(!dormant.contains("vivo.plist"));
        assert!(dormant.contains("spenta.scheda.json"));
        assert!(dormant.contains("spenta.mandato.md"), "il corpo segue la scheda");

        assert_eq!(active_mandate_names(), set(&["accesa.mandato.md"]));
        assert_eq!(off_automation_names(), set(&["spenta"]));

        unsafe {
            std::env::remove_var("REACH_LAUNCH_AGENTS");
            std::env::remove_var("REACH_WORKSPACE");
            std::env::remove_var("REACH_HOME_CLAUDE");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_hook_outside_the_repo_is_still_a_root() {
        let dir = crate::test_home::test_root().join("reach-hookspath");
        let _ = std::fs::remove_dir_all(&dir);
        let clone = dir.join("casa/skills/skills-di-monte");
        let altrove = dir.join("ganci-fuori");
        let dentro = dir.join("ws/repo-normale/.git/hooks");
        std::fs::create_dir_all(clone.join(".git")).unwrap();
        std::fs::create_dir_all(&altrove).unwrap();
        std::fs::create_dir_all(&dentro).unwrap();
        std::fs::create_dir_all(dir.join("ws/repo-normale/.git")).unwrap();
        std::fs::write(clone.join(".git/config"),
                       format!("[core]\n\thooksPath = {}\n", altrove.display())).unwrap();
        std::fs::write(altrove.join("post-merge"), "#!/bin/sh\nsblocca.sh\n").unwrap();
        // Il clone ha anche la cartella di default, vuota: la scelta deve cadere
        // sul percorso dichiarato, non su quella.
        std::fs::create_dir_all(clone.join(".git/hooks")).unwrap();
        std::fs::write(dentro.join("pre-commit"), "#!/bin/sh\n").unwrap();
        std::fs::write(dentro.join("pre-push.sample"), "#!/bin/sh\n").unwrap();

        // SAFETY: `ambiente()` serializza le prove che scrivono `std::env`, e
        // ognuna rimette le variabili com'erano prima di uscire.
        let _guardia = ambiente();
        unsafe {
            std::env::set_var("REACH_LAUNCH_AGENTS", dir.join("agents-vuoti"));
            std::env::set_var("REACH_WORKSPACE", dir.join("ws"));
            std::env::set_var("REACH_HOME_CLAUDE", dir.join("casa"));
        }

        let live: BTreeSet<String> = live_roots(&BTreeSet::new())
            .iter()
            .map(|r| r.path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert!(live.contains("post-merge"),
                "un gancio dichiarato da core.hooksPath è una radice viva");
        assert!(live.contains("pre-commit"), "e i git hook normali restano radici");
        assert!(!live.contains("pre-push.sample"), "un esempio non spara");

        unsafe {
            std::env::remove_var("REACH_LAUNCH_AGENTS");
            std::env::remove_var("REACH_WORKSPACE");
            std::env::remove_var("REACH_HOME_CLAUDE");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
