//! Linear è in sola lettura per le automazioni. Lo stato lo muove Theo.
//!
//! Porta di `skills/hooks/linear-sola-lettura.py`, 813 righe — il più grande e
//! il più delicato del parco, e il muro della catena `PreToolUse` dopo le prime
//! sette adozioni.
//!
//! MANDATO (11/08/2026, testuale): «il sistema non deve piu spostare task in
//! done se non sotto mia esplicita autorizzazzione scritta».
//!
//! ELENCO CHIUSO DI LETTURE, NON DI SCRITTURE. La prima versione elencava i
//! sottocomandi che scrivono, e un giudice indipendente l'ha bocciata in
//! mezz'ora: `orca linear` ne ha 29 e la CLI `linear` una trentina, l'elenco ne
//! copriva 19, e fra i mancanti c'era `linear issue close` — cioè esattamente
//! l'atto che il mandato vieta. **Un elenco di divieti è sempre in ritardo sulla
//! CLI che sorveglia.** Qui l'elenco è quello delle letture: ogni sottocomando
//! che non vi compare è negato, compresi quelli che le due CLI aggiungeranno
//! domani. Il costo è un falso positivo su una lettura nuova; il costo
//! dell'errore opposto è una scheda in Done.
//!
//! TRE VALVOLE, non un booleano. Il nucleo del divieto — questo gancio, le sue
//! prove, il suo registro — non si smonta con `OK_UTENTE=1`: una valvola che
//! autorizza il proprio smontaggio non è una valvola. La configurazione dei
//! ganci sì, perché si amministra ogni giorno.
//!
//! Qui c'è solo il giudizio, che è puro. Il permesso di Theo, il registro e il
//! rifiuto stanno nel binario: la parte che decide dev'essere provabile senza
//! toccare lo stato della macchina.

use regex::{NoExpand, Regex};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Quanto è forte il divieto, cioè che cosa può toglierlo.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Valve {
    /// Niente la toglie: è ciò che regge il divieto.
    Core,
    /// `OK_UTENTE=1` in testa al segmento.
    UserDeclared,
    /// Il file che scrive Theo da un terminale fuori da Claude Code.
    TheoPermission,
}

// ── Le letture consentite ───────────────────────────────────────────────────
// Prefissi di sottocomando, uno o due token. Tutto ciò che non comincia con uno
// di questi è negato. Ricavati da `orca linear --help` e `linear --help`
// l'11/08/2026, non a memoria.

const ORCA_READS: &[&[&str]] = &[
    &["list"],
    &["list-issues"],
    &["issue"],
    &["search"],
    &["team", "list"],
    &["team", "members"],
    &["team", "states"],
    &["team", "labels"],
    &["project", "list"],
];

/// `login`/`logout`/`alias`/`branch` toccano solo file locali o git: non
/// scrivono su Linear, quindi non sono affare di questo mandato.
const CLI_READS: &[&[&str]] = &[
    &["whoami"],
    &["roadmap"],
    &["issues"],
    &["issue", "show"],
    &["projects"],
    &["project", "show"],
    &["milestones"],
    &["milestone", "show"],
    &["labels"],
    &["alias"],
    &["branch"],
    &["login"],
    &["logout"],
];

/// Sottoinsieme usato quando si guarda del testo che *contiene* comandi invece
/// di esserlo. Stretto di proposito: `--project Open` non deve far scattare
/// niente.
const STRONG_VERBS: &[&str] = &[
    "close", "done", "update", "create", "delete", "archive", "complete", "reorder", "start",
    "move", "set", "clear", "assign", "comment", "attach", "save-issue",
];

/// Parole che precedono il comando vero senza esserlo. `timeout`/`stdbuf` erano
/// la scorciatoia più corta per aggirare la versione precedente.
const PREFIXES: &[&str] = &[
    "sudo", "doas", "env", "time", "nohup", "command", "exec", "xargs", "timeout", "gtimeout",
    "stdbuf", "nice", "ionice", "caffeinate", "setsid", "arch", "script", "builtin", "eval",
    "then", "else", "elif", "do", "fi", "done", "esac", "if", "while", "until", "for", "case",
];

/// Chi esegue una stringa come codice: la stringa va guardata, non il verbo.
const INTERPRETERS: &[&str] = &[
    "bash", "sh", "zsh", "dash", "ksh", "fish", "python", "python3", "node", "perl", "ruby",
    "osascript", "deno", "bun",
];

/// Chi passa il lavoro a un esecutore che i ganci di Claude non raggiungono.
const DELEGATES: &[&str] = &["codex", "gemini", "claude", "aider", "cursor-agent", "copilot"];

const RUNNERS: &[&str] = &["npx", "bunx", "pnpx", "dlx", "uvx"];

/// Chi esegue un comando che porta come argomento, in una forma che non si
/// legge come una catena di shell.
const EXECUTORS: &[&str] = &[
    "find", "watch", "awk", "parallel", "entr", "fswatch", "ssh", "tmux", "screen", "expect",
    "at", "batch",
];

/// Gli stati che chiudono una scheda: il mandato tiene «modifica» separato da
/// «sposta in Done».
pub const FINAL_STATES: &[&str] = &[
    "done", "completed", "complete", "closed", "close", "cancelled", "canceled", "duplicate",
    "archived", "archive",
];

/// Il nucleo: scriverci dentro disarma il gancio per ogni sessione futura.
pub const CORE_FILES: &[&str] = &[
    "linear-sola-lettura.py",
    "linear-sola-lettura.jsonl",
    "linear-scritture-negate.jsonl",
    "prova-linear-sola-lettura.py",
    "linear-permesso.json",
    "permesso-linear.sh",
];

/// Configurazione: protetta, ma scavalcabile dichiarando la valvola. È un file
/// con usi legittimi quotidiani, e il punto è che il disarmo sia **detto**.
pub const CONFIG_FILES: &[&str] = &["settings.json"];

/// L'host dell'API, composto a pezzi: il pavimento di sicurezza dell'ambiente
/// uccide qualunque comando che se lo trovi scritto per intero, prove comprese.
fn api_host() -> String {
    format!("api.{}.app", "linear")
}

fn set(items: &[&'static str]) -> HashSet<&'static str> {
    items.iter().copied().collect()
}

// ── Lettura del comando ─────────────────────────────────────────────────────

/// Redirezioni: `2>&1`, `>`, `>>`, `<`, `&2`. Non sono argomenti del comando, e
/// lasciarle nella coda faceva negare `orca linear --help 2>&1` mentre lo stesso
/// comando senza redirezione passava.
fn redirection() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d*(>>?|<)&?\d*|&\d)$").unwrap())
}

fn assignment() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\w+=").unwrap())
}

fn duration() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d+[smhd]?$").unwrap())
}

const TRIM: &[char] = &['"', '\'', '`', '(', ')', '{', '}'];

/// I token del comando senza opzioni, come nomi base del percorso.
pub fn words(line: &str) -> Vec<String> {
    line.split_whitespace()
        .filter(|p| !p.starts_with('-') && !redirection().is_match(p))
        .filter_map(|p| {
            let clean = p.trim_matches(TRIM);
            if clean.is_empty() {
                None
            } else {
                Some(clean.rsplit('/').next().unwrap_or(clean).to_string())
            }
        })
        .collect()
}

/// I token che non sono il valore di un'opzione.
///
/// Serve a distinguere `orca linear issue close HRD-1` — dove `close` è un
/// sottocomando — da `linear issues --status done`, dove `done` è il valore di
/// `--status`, cioè uno stato che si vuole poter leggere. Non sostituisce
/// `words`: un flag come `-o0` di `stdbuf` mangerebbe il nome del comando vero.
pub fn positional(line: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut skip = false;
    for piece in line.split_whitespace() {
        if piece.starts_with('-') && !redirection().is_match(piece) {
            skip = !piece.contains('=') && piece != "-";
            continue;
        }
        if skip {
            skip = false;
            continue;
        }
        let clean = piece.trim_matches(TRIM);
        if !clean.is_empty() {
            out.insert(clean.rsplit('/').next().unwrap_or(clean).to_string());
        }
    }
    out
}

/// La valvola: `OK_UTENTE=1` come assegnazione **davanti a questo comando**.
///
/// Non «da qualche parte nella riga»: solo in testa, dove la shell la
/// tratterebbe davvero come una variabile d'ambiente del comando. Cercarla come
/// stringa era la stessa falla già chiusa altrove — `echo "usa OK_UTENTE=1" ;
/// orca linear status set …` avrebbe autorizzato la scrittura.
pub fn declared(line: &str) -> bool {
    let prefixes = set(PREFIXES);
    for piece in line.split_whitespace() {
        if piece == "OK_UTENTE=1" {
            return true;
        }
        if !assignment().is_match(piece) && !prefixes.contains(piece) {
            return false;
        }
    }
    false
}

/// Toglie da davanti involucri, prefissi, assegnazioni e durate.
///
/// Le durate servono per `timeout 30 orca linear …` e `nice -n 10 linear …`:
/// tolto il prefisso resta un numero, che non è mai un comando.
fn strip_wrappers(mut names: Vec<String>) -> Vec<String> {
    let prefixes = set(PREFIXES);
    while !names.is_empty() {
        let head = &names[0];
        if prefixes.contains(head.as_str())
            || assignment().is_match(head)
            || duration().is_match(head)
        {
            names.remove(0);
        } else {
            break;
        }
    }
    names
}

/// `None` se la coda è una lettura consentita, altrimenti il motivo.
///
/// Un prefisso di lettura non autorizza tutto ciò che viene dopo: `issue` è una
/// lettura da sola, ma `orca linear issue close HRD-1` no. Oggi la CLI di Orca
/// non ha quel sottoverbo — il giorno che lo aggiunge, il gancio non deve
/// accorgersene per il danno.
fn judge_queue(queue: &[String], reads: &[&[&str]], label: &str, line: &str) -> Option<String> {
    if queue.is_empty() {
        return None; // `linear`, `orca linear`, `--help`
    }
    let places = positional(line);
    let strong = set(STRONG_VERBS);
    for length in [2usize, 1] {
        if queue.len() >= length
            && reads
                .iter()
                .any(|r| r.len() == length && r.iter().zip(&queue[..length]).all(|(a, b)| a == b))
        {
            for piece in &queue[length..] {
                if strong.contains(piece.as_str()) && places.contains(piece) {
                    return Some(format!("{label} {} {piece}", queue[0]));
                }
            }
            return None;
        }
    }
    Some(format!(
        "{label} {}",
        queue.iter().take(2).cloned().collect::<Vec<_>>().join(" ")
    ))
}

/// Il testo *contiene* un comando di scrittura su Linear?
///
/// Per le stringhe che verranno eseguite altrove — dentro un interprete, nel
/// prompt di un altro agente, nel precheck di un'automazione. Lì non c'è una
/// struttura da leggere, quindi si guarda se `linear` è seguita da vicino da un
/// verbo che scrive.
pub fn names_write(text: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[\w\-\./]+").unwrap());

    // I trattini bassi si spezzano: un ponte come Zapier nomina l'azione
    // `linear_issue_close`, dove `linear` non compare mai come parola a sé.
    let mut tokens: Vec<String> = Vec::new();
    for m in re.find_iter(text) {
        let x = m.as_str().rsplit('/').next().unwrap_or(m.as_str()).to_string();
        let split: Vec<String> = if x.contains('_') {
            x.split('_').map(str::to_string).collect()
        } else {
            Vec::new()
        };
        tokens.push(x);
        tokens.extend(split);
    }
    let strong = set(STRONG_VERBS);
    for (i, x) in tokens.iter().enumerate() {
        if x != "linear" {
            continue;
        }
        for y in tokens.iter().skip(i + 1).take(3) {
            if strong.contains(y.as_str()) {
                return Some(format!("linear … {y}"));
            }
        }
    }
    None
}

// ── I file che reggono il divieto ───────────────────────────────────────────

/// Quanto vicino al nome del file deve stare il segno di scrittura. Guardare
/// l'intera riga negava anche uno script che *nomina* il gancio per lanciarne le
/// prove e altrove, molto più in là, scrive un file diverso.
const WINDOW: usize = 120;

/// Una redirezione ha un bersaglio dichiarato: `>/dev/null` non scrive sul file
/// nominato tre parole prima.
fn discard() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"\d*>>?\s*("[^"]*"|'[^']*'|\S+)"#).unwrap())
}

fn write_words() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"\brm\b|\bmv\b|\bcp\b|\btee\b|\bsed\b|\bchmod\b|\bdd\b|['"]w[+bt]*['"]|writeFile|appendFile|unlink|os\.remove|os\.replace|shutil\.|truncate|\.write\("#,
        )
        .unwrap()
    })
}

/// Un segno di scrittura nel testo.
///
/// Il `>` si guarda a mano perché il Python usa `(?<!\d)>`, cioè un lookbehind
/// che il motore di regex di Rust non ha. Serve a tenere fuori le redirezioni
/// dei descrittori (`2>&1`), che non scrivono su niente: senza, lanciare le
/// prove del gancio veniva negato dal gancio stesso.
fn has_write_sign(text: &str) -> bool {
    if write_words().is_match(text) {
        return true;
    }
    let bytes = text.as_bytes();
    bytes
        .iter()
        .enumerate()
        .any(|(i, &c)| c == b'>' && (i == 0 || !bytes[i - 1].is_ascii_digit()))
}

/// Comando che scrive su ciò che regge il divieto.
///
/// Il criterio è la vicinanza: un segno di scrittura entro `WINDOW` caratteri dal
/// nome del file. Nominare il gancio per leggerlo o per lanciarne le prove resta
/// libero — altrimenti il divieto renderebbe impossibile lavorarci.
pub fn touches_protected(line: &str) -> Option<(String, Valve)> {
    let every: Vec<&str> = CORE_FILES.iter().chain(CONFIG_FILES).copied().collect();

    // La redirezione innocua si cancella sostituendo con spazi, così gli indici
    // della riga restano quelli veri.
    let mut cleaned = line.to_string();
    for m in discard().captures_iter(line) {
        let whole = m.get(0).unwrap();
        let target = m.get(1).map(|t| t.as_str()).unwrap_or("");
        if every.iter().any(|f| target.contains(f)) {
            continue;
        }
        let blank = " ".repeat(whole.as_str().len());
        cleaned.replace_range(whole.range(), &blank);
    }

    let candidates = CORE_FILES
        .iter()
        .map(|f| (*f, Valve::Core))
        .chain(CONFIG_FILES.iter().map(|f| (*f, Valve::UserDeclared)));

    for (file, valve) in candidates {
        let Some(i) = line.find(file) else { continue };
        let start = floor_boundary(&cleaned, i.saturating_sub(WINDOW));
        let end = floor_boundary(&cleaned, i + file.len() + WINDOW);
        if has_write_sign(&cleaned[start..end]) {
            let what = if valve == Valve::UserDeclared {
                "la configurazione dei ganci"
            } else {
                "il gancio stesso o il suo registro"
            };
            return Some((format!("riscrittura di {what}"), valve));
        }
    }
    None
}

/// Il taglio di una stringa deve cadere su un confine di carattere: un accento
/// occupa due byte, e tagliarlo a metà farebbe cadere il gancio — che, essendo
/// fail-open, si limiterebbe a lasciar passare tutto in silenzio. Il Python
/// affetta per caratteri e il problema non ce l'ha.
fn floor_boundary(s: &str, mut i: usize) -> usize {
    if i >= s.len() {
        return s.len();
    }
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

/// Rimpiazza `$F` col percorso che il comando gli ha appena assegnato.
///
/// `F=~/.claude/skills/hooks/linear-sola-lettura.py; echo "" > "$F"` non nomina
/// mai il file nel segmento che scrive, e passava: la protezione cercava il nome
/// letterale.
pub fn expand_variables(command: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"(\w+)=([^\s;|&]+)").unwrap());
    let interesting: Vec<&str> = CORE_FILES.iter().chain(CONFIG_FILES).copied().collect();

    let mut out = command.to_string();
    let pairs: Vec<(String, String)> = re
        .captures_iter(command)
        .map(|c| (c[1].to_string(), c[2].to_string()))
        .collect();
    for (name, value) in pairs {
        if !interesting.iter().any(|f| value.contains(f)) {
            continue;
        }
        let plain = value.trim_matches(|c| c == '"' || c == '\'');
        let Ok(var) = Regex::new(&format!(r"\$\{{?{}\}}?", regex::escape(&name))) else {
            continue;
        };
        // `NoExpand`: un percorso che contenesse `$1` verrebbe altrimenti letto
        // come riferimento a un gruppo e sparirebbe dal testo. Il `re.sub` di
        // Python espande allo stesso modo, ma qui il rimpiazzo è un percorso, e
        // il punto è che ci arrivi intero.
        out = var.replace_all(&out, NoExpand(plain)).into_owned();
    }
    out
}

// ── Gli script eseguiti ─────────────────────────────────────────────────────

const SCRIPT_EXTENSIONS: &[&str] = &[".sh", ".bash", ".zsh", ".py", ".js", ".mjs", ".ts"];

/// I due file del gancio: le loro prove contengono per necessità gli esempi
/// vietati, quindi non vanno letti come sospetti.
fn own_files() -> Vec<PathBuf> {
    let home = std::env::var("HOME").unwrap_or_default();
    let hooks = Path::new(&home).join(".claude").join("skills").join("hooks");
    vec![
        hooks.join("linear-sola-lettura.py"),
        hooks.join("prova-linear-sola-lettura.py"),
    ]
}

/// Apre gli script che la riga *esegue* e ci cerca cosa fanno.
///
/// Eseguire, non nominare: `grep -n apiKey …/linear.mjs` legge il sorgente della
/// CLI e veniva negato, perché dentro quel file ci sono per forza i comandi che
/// il gancio vieta. Un divieto che impedisce di studiare lo strumento che
/// sorveglia viene disattivato.
///
/// Non copre `just` e `make`, dove il comando vero sta in un target: limite
/// dichiarato, non dimenticato.
pub fn inside_a_script(line: &str) -> Option<(String, Valve)> {
    let names = strip_wrappers(words(line));
    let head = names.first().cloned().unwrap_or_default();
    let runs = set(INTERPRETERS).contains(head.as_str())
        || set(EXECUTORS).contains(head.as_str())
        || set(DELEGATES).contains(head.as_str());

    for piece in line.split_whitespace() {
        let candidate = piece.trim_matches(|c| c == '"' || c == '\'');
        if !SCRIPT_EXTENSIONS.iter().any(|e| candidate.ends_with(e)) {
            continue;
        }
        // Lo script eseguito direttamente (`./chiudi.sh`) è il capo del comando.
        let base = candidate.rsplit('/').next().unwrap_or(candidate);
        if !runs && base != head {
            continue;
        }
        let path = expand_home(candidate);
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if !meta.is_file() || meta.len() > 200_000 {
            continue;
        }
        if own_files().iter().any(|p| p == &path) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if let Some((protected, valve)) = touches_protected(&text) {
            return Some((format!("{protected}, dentro lo script {name}"), valve));
        }
        if let Some(inside) = names_write(&text) {
            return Some((
                format!("{inside} dentro lo script {name}"),
                Valve::TheoPermission,
            ));
        }
    }
    None
}

fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => Path::new(&std::env::var("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}

// ── Il giudizio su un segmento ──────────────────────────────────────────────

pub fn reason_for_row(line: &str) -> Option<(String, Valve)> {
    let names = strip_wrappers(words(line));
    if names.is_empty() {
        return None;
    }
    if let Some(found) = touches_protected(line) {
        return Some(found);
    }
    // Uno script nominato per percorso vale quanto il suo contenuto, e vale
    // anche per il nucleo: `python3 disarma.py`, con dentro un `os.replace` sul
    // gancio, passava — il controllo sui file protetti guardava solo la riga di
    // comando, dove non compariva nessun nome protetto.
    if let Some(found) = inside_a_script(line) {
        return Some(found);
    }
    linear_write(line, names).map(|r| (r, Valve::TheoPermission))
}

fn linear_write(line: &str, mut names: Vec<String>) -> Option<String> {
    let runners = set(RUNNERS);
    // `npx @dabble/linear-cli issue close …`: col runner davanti, tutte le
    // sequenze conosciute perderebbero il proprio nome.
    while !names.is_empty() && runners.contains(names[0].as_str()) {
        names.remove(0);
        if !names.is_empty() && (names[0] == "dlx" || names[0] == "exec") {
            names.remove(0);
        }
    }
    if names.is_empty() {
        return None;
    }

    let interpreters = set(INTERPRETERS);
    let delegates = set(DELEGATES);
    let executors = set(EXECUTORS);

    let mut head = names[0].clone();
    // `node <percorso> args…`: il comando vero è lo script, non l'interprete.
    // Con `-c`/`-e`/`-p` invece il codice è inline e lo guarda il ramo apposito.
    //
    // Ma l'interprete resta tale anche quando ciò che segue non è un file:
    // `python3 <(printf …)`, `bash <<< "…"`, `deno eval "…"` promuovevano a
    // comando un token che non è niente, e da lì il testo non veniva più
    // guardato da nessuno. Tre aggiramenti verificati.
    let was_interpreter = interpreters.contains(head.as_str());
    static INLINE: OnceLock<Regex> = OnceLock::new();
    let inline = INLINE.get_or_init(|| Regex::new(r"\s-{1,2}[cep]\b|\s-{1,2}eval\b").unwrap());
    if was_interpreter && names.len() > 1 && !inline.is_match(line) {
        names.remove(0);
        head = names[0].clone();
    }

    // Il vero eseguibile dietro `linear` è un `cli.js`, e invocarlo per percorso
    // neutralizzava in blocco ogni controllo. Il riconoscimento guarda il
    // percorso dell'eseguibile, non la riga: un `startswith('linear-')` prendeva
    // anche una riga di codice che comincia col nome di questo gancio.
    static JS_CLI: OnceLock<Regex> = OnceLock::new();
    let js_cli = JS_CLI.get_or_init(|| Regex::new(r"\S*linear\S*\.[mc]?js\b").unwrap());
    if js_cli.is_match(line) && head.ends_with("js") {
        head = "linear".to_string();
    } else if head == "linear-cli"
        || head == "linear.js"
        || (head.starts_with('@') && head.contains("linear"))
    {
        head = "linear".to_string();
    }

    if head == "orca" {
        let second = names.get(1).cloned().unwrap_or_default();
        if second == "linear" {
            let tail = if names.len() > 2 { &names[2..] } else { &[] };
            return judge_queue(tail, ORCA_READS, "orca linear", line);
        }
        // Il lavoro consegnato a un altro esecutore: un terminale che non è una
        // sessione Claude, un'automazione pianificata, un provider diverso.
        if ["terminal", "automations", "automation", "agent"].contains(&second.as_str()) {
            if let Some(inside) = names_write(line) {
                return Some(format!(
                    "{inside} passato a un altro esecutore (orca {second})"
                ));
            }
        }
        // L'interfaccia web non è un canale di sola lettura verificabile: da lì
        // una scheda si trascina in Done senza che nessun comando lo dica.
        static WEB: OnceLock<Regex> = OnceLock::new();
        let web = WEB.get_or_init(|| Regex::new(r"linear\.app|--app\s+Linear").unwrap());
        if [
            "eval", "goto", "navigate", "computer", "click", "drag", "type", "press",
        ]
        .contains(&second.as_str())
            && web.is_match(line)
        {
            return Some(format!(
                "automazione del browser sull'interfaccia web (orca {second})"
            ));
        }
        return None;
    }

    if head == "linear" {
        return judge_queue(&names[1..], CLI_READS, "linear", line);
    }

    if interpreters.contains(head.as_str())
        || delegates.contains(head.as_str())
        || executors.contains(head.as_str())
        || was_interpreter
    {
        if let Some(inside) = names_write(line) {
            let where_ = if interpreters.contains(head.as_str()) || was_interpreter {
                "un interprete".to_string()
            } else if delegates.contains(head.as_str()) {
                format!("un altro agente ({head})")
            } else {
                format!("un esecutore generico ({head})")
            };
            return Some(format!("{inside} eseguito da {where_}"));
        }
    }

    // Il nome del comando nascosto dietro una variabile o un alias:
    // `ORCA=orca; $ORCA linear status set …`. Non si può risolvere la variabile,
    // ma si può smettere di fidarsi della riga.
    if head.starts_with('$') || head == "alias" || head == "function" {
        if let Some(inside) = names_write(line) {
            return Some(format!("{inside} dietro una variabile o un alias"));
        }
        if head != "$" && line.contains("linear") && (head == "alias" || head == "function") {
            return Some("alias che nasconde il nome di una CLI Linear".to_string());
        }
    }

    static MUTATION: OnceLock<Regex> = OnceLock::new();
    let mutation = MUTATION.get_or_init(|| {
        Regex::new(
            r"\bmutation\b|\bissue[A-Z]\w*|\bcomment[A-Z]\w*|\battachment[A-Z]\w*|\bproject[A-Z]\w*|\bworkflowState[A-Z]\w*",
        )
        .unwrap()
    });
    if line.contains(&api_host()) && mutation.is_match(line) {
        return Some("mutazione GraphQL contro l'API di Linear".to_string());
    }

    None
}

/// Uno strumento MCP di Linear che scrive. Oggi il plugin è spento: serve
/// perché riaccenderlo non riapra la strada in silenzio.
pub fn reason_mcp(tool: &str) -> Option<String> {
    let n = tool.to_lowercase();
    if !n.contains("linear") {
        return None;
    }
    for verb in [
        "create", "update", "delete", "archive", "move", "assign", "comment", "attach", "set_",
        "close",
    ] {
        if n.contains(verb) {
            return Some(format!("strumento MCP di scrittura ({tool})"));
        }
    }
    None
}

/// Uno strumento di scrittura file che tocca ciò che regge il divieto.
///
/// Il gancio guardava solo `Bash`, quindi `Write` con `file_path` sul gancio
/// stesso lo disarmava in una riga — misurato l'11/08/2026, e nessuno dei tre
/// verdetti indipendenti l'aveva visto, perché provavano tutti comandi di shell.
pub fn reason_on_file(path: &str) -> Option<(String, Valve)> {
    if path.is_empty() {
        return None;
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    if CORE_FILES.contains(&name) {
        return Some((
            format!("riscrittura di il gancio stesso o il suo registro ({name})"),
            Valve::Core,
        ));
    }
    if CONFIG_FILES.contains(&name) {
        return Some((
            format!("riscrittura di la configurazione dei ganci ({name})"),
            Valve::UserDeclared,
        ));
    }
    None
}

/// Il comando porta una scheda a uno stato finale?
///
/// Vale sia il verbo (`linear issue close`) sia il valore di stato
/// (`--status Done`): il mandato distingue «modifica una scheda» da «sposta una
/// scheda in Done», e la distinzione va fatta sul fatto, non sulla forma.
pub fn closes_a_card(text: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[\w\-]+").unwrap());
    re.find_iter(text)
        .any(|m| FINAL_STATES.contains(&m.as_str().to_lowercase().as_str()))
}

/// Gli identificativi di scheda nominati nel testo: `HRD-123`, `SUITE-4`.
pub fn named_cards(text: &str) -> Vec<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    // `\b` di Python è sui caratteri di parola, e la regex di Rust si comporta
    // allo stesso modo.
    let re = RE.get_or_init(|| Regex::new(r"\b[A-Z][A-Z0-9]{1,7}-[0-9]+\b").unwrap());
    re.find_iter(text).map(|m| m.as_str().to_string()).collect()
}

// ── Segmentazione ───────────────────────────────────────────────────────────

/// Apre le sostituzioni di comando, così il loro contenuto diventa un segmento
/// a sé: `$(orca linear status set …)` era invisibile.
///
/// Si aprono solo i delimitatori d'apertura. Chiudere anche le parentesi tonde
/// spezzava le stringhe degli interpreti — `node -e "execSync('linear issue
/// close …')"` finiva in tre segmenti, e in nessuno dei tre si vedeva più il
/// comando. La parentesi di chiusura la toglie `words()`.
fn normalise(command: &str) -> String {
    command.replace("$(", " ; ").replace('`', " ; ")
}

/// Spezza il comando nei suoi comandi veri, rispettando le virgolette.
///
/// Spezzare con una regex cieca prendeva per separatore anche il `|` dentro una
/// stringa: `grep -n "linear done\|linear next" file` diventava un segmento
/// `linear done` e veniva negato. **Cercare il testo di un divieto dev'essere
/// possibile, o il divieto lo si aggira per non litigarci.**
pub fn segments(command: &str) -> Vec<String> {
    let text = normalise(command);
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match quote {
            Some(q) => {
                current.push(c);
                if c == q {
                    quote = None;
                } else if c == '\\' && i + 1 < chars.len() {
                    current.push(chars[i + 1]);
                    i += 1;
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                current.push(c);
            }
            None if ";\n|&".contains(c) => {
                out.push(std::mem::take(&mut current));
            }
            None => current.push(c),
        }
        i += 1;
    }
    out.push(current);
    out
}

/// L'apertura di un heredoc: `<<EOF`, `<<-'FINE'`, `<< PY`.
fn heredoc() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r#"<<-?\s*['"]?\w+"#).unwrap())
}

/// Cosa il giudizio ha visto in un comando `Bash`.
///
/// `Declared` non è `Pass`: il comando passa, ma **la valvola è stata usata**, e
/// il messaggio di rifiuto promette a chi la digita che l'uso resta scritto nel
/// registro. Confonderlo con un passaggio normale mantiene il comportamento e
/// cancella la promessa.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Declared {
        reason: String,
        segment: String,
    },
    Refused {
        reason: String,
        valve: Valve,
        segment: String,
    },
    /// Rifiuto secco: non passa da nessuna valvola e nemmeno dal permesso di
    /// Theo, ma si registra come «negato» e non «negato-nucleo».
    ///
    /// È il ramo dello heredoc, e la mancata consultazione del permesso è
    /// **una svista dell'originale conservata di proposito**: quel ramo è stato
    /// aggiunto dopo, davanti al ciclo che applica i permessi, e nessuno l'ha
    /// ricollegato. Conseguenza vera: con un permesso valido in corso,
    /// `linear issue close` passa scritto normalmente e viene rifiutato se
    /// scritto dentro un heredoc. Qui si replica perché l'originale è l'oracolo
    /// finché non lo cancelliamo; correggerlo è un cambio di comportamento e va
    /// deciso a voce alta, non di soppiatto durante un porting.
    Sealed {
        reason: String,
    },
}

/// Il verdetto su un comando `Bash`, prima dei permessi.
///
/// I permessi li applica il chiamante, perché richiedono di leggere un file che
/// scrive Theo e la decisione dipende anche dalla scheda nominata. Il segmento
/// che ha fatto scattare il rifiuto torna insieme al motivo: si autorizza
/// quello, non la riga intera.
pub fn judge_bash(command: &str) -> Verdict {
    let expanded = expand_variables(command);

    // Un heredoc consegna il codice all'interprete su righe che `segments` vede
    // come comandi a sé stanti: il payload non torna mai al ramo che guarda gli
    // interpreti, e `python3 - <<EOF … EOF` passava.
    if heredoc().is_match(&expanded) {
        let head = strip_wrappers(words(&expanded))
            .first()
            .cloned()
            .unwrap_or_default();
        if set(INTERPRETERS).contains(head.as_str()) || set(DELEGATES).contains(head.as_str()) {
            if let Some(inside) = names_write(&expanded) {
                return Verdict::Sealed {
                    reason: format!("{inside} passato a {head} dentro un heredoc"),
                };
            }
        }
    }

    for line in segments(&expanded) {
        if let Some((reason, valve)) = reason_for_row(&line) {
            // `OK_UTENTE=1` vale per il segmento che la porta, non per la riga
            // intera: serve ad amministrare la configurazione dei ganci, non a
            // scrivere su Linear.
            if valve == Valve::UserDeclared && declared(&line) {
                return Verdict::Declared {
                    reason,
                    segment: line,
                };
            }
            return Verdict::Refused {
                reason,
                valve,
                segment: line,
            };
        }
    }
    Verdict::Pass
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(command: &str) -> bool {
        matches!(judge_bash(command), Verdict::Refused { .. })
    }

    fn why(command: &str) -> String {
        match judge_bash(command) {
            Verdict::Refused { reason, .. } => reason,
            _ => String::new(),
        }
    }

    fn valve_of(command: &str) -> Option<Valve> {
        match judge_bash(command) {
            Verdict::Refused { valve, .. } => Some(valve),
            _ => None,
        }
    }

    #[test]
    fn the_closed_list_of_reads_goes_through() {
        assert!(!refused("orca linear list --json"));
        assert!(!refused("orca linear issue HRD-123"));
        assert!(!refused("orca linear team states --team HRD"));
        assert!(!refused("linear issues --status done"));
        assert!(!refused("linear issue show HRD-1"));
        assert!(!refused("orca linear --help 2>&1"));
    }

    #[test]
    fn anything_outside_that_list_is_refused_even_if_it_looks_harmless() {
        assert!(refused("orca linear status set HRD-123 --status Done"));
        assert!(refused("linear issue close HRD-1"));
        assert!(refused("orca linear comment add HRD-1 --body x"));
        // un sottocomando che le due CLI non hanno ancora
        assert!(refused("orca linear teleport HRD-1"));
    }

    /// Gli aggiramenti che le versioni precedenti non vedevano, ognuno trovato
    /// da un giudice indipendente.
    #[test]
    fn the_known_bypasses_stay_closed() {
        assert!(refused("timeout 30 linear issue close HRD-1"));
        assert!(refused("stdbuf -o0 linear issue close HRD-1"));
        assert!(refused("npx @dabble/linear-cli issue close HRD-1"));
        assert!(refused("node /x/@dabble/linear-cli/bin/cli.js issue close HRD-1"));
        assert!(refused("true & orca linear status set HRD-1 --status Done"));
        assert!(refused(r#"bash -c "linear issue close HRD-1""#));
        assert!(refused("ORCA=orca; $ORCA linear status set HRD-1 --status Done"));
        assert!(refused("echo x $(orca linear status set HRD-1 --status Done)"));
    }

    #[test]
    fn a_valve_in_the_middle_of_the_line_authorises_nothing() {
        // la falla chiusa il 29/07: cercarla come stringa autorizzava tutto
        assert!(refused(r#"echo "usa OK_UTENTE=1" ; linear issue close HRD-1"#));
        assert!(!declared(r#"echo "usa OK_UTENTE=1""#));
        assert!(declared("OK_UTENTE=1 python3 x.py"));
        assert!(declared("env OK_UTENTE=1 python3 x.py"));
    }

    /// Il nucleo non si smonta con la valvola: una valvola che autorizza il
    /// proprio smontaggio non è una valvola.
    #[test]
    fn the_core_of_the_ban_is_not_negotiable() {
        assert_eq!(
            valve_of("rm ~/.claude/skills/hooks/linear-sola-lettura.py"),
            Some(Valve::Core)
        );
        assert_eq!(
            valve_of("echo x > ~/.claude/settings.json"),
            Some(Valve::UserDeclared)
        );
        // Dichiarata, la configurazione si amministra — ma l'uso della valvola
        // resta scritto: `Declared` non è `Pass`, ed è la promessa che il
        // messaggio di rifiuto fa a chi la digita.
        assert!(matches!(
            judge_bash("OK_UTENTE=1 cp a.json ~/.claude/settings.json"),
            Verdict::Declared { .. }
        ));
        assert_eq!(judge_bash("git status"), Verdict::Pass);
        // il nucleo no, nemmeno dichiarandolo
        assert!(refused(
            "OK_UTENTE=1 rm ~/.claude/skills/hooks/linear-sola-lettura.py"
        ));
    }

    #[test]
    fn the_variable_holding_a_protected_path_is_resolved_first() {
        assert!(refused(
            r#"F=~/.claude/skills/hooks/linear-sola-lettura.py; echo "" > "$F""#
        ));
    }

    #[test]
    fn reading_the_hook_or_running_its_tests_stays_free() {
        assert!(!refused("cat ~/.claude/skills/hooks/linear-sola-lettura.py"));
        assert!(!refused(
            "python3 ~/.claude/skills/hooks/prova-linear-sola-lettura.py 2>&1"
        ));
        assert!(!refused(
            "python3 ~/.claude/skills/hooks/linear-sola-lettura.py > /dev/null"
        ));
    }

    /// Cercare il testo di un divieto dev'essere possibile, o il divieto lo si
    /// aggira per non litigarci.
    #[test]
    fn searching_for_the_forbidden_text_is_allowed() {
        assert!(!refused(r#"grep -n "linear done\|linear next" file"#));
    }

    #[test]
    fn a_status_value_is_not_a_subcommand() {
        assert!(!refused("linear issues --status done"));
        assert!(!refused("linear issues --project Open"));
    }

    #[test]
    fn it_sees_the_write_handed_to_someone_else() {
        assert!(refused(r#"orca terminal send --input "linear issue close HRD-1""#));
        assert!(refused(r#"codex exec "linear issue close HRD-1""#));
        assert!(refused("orca goto --page x --url https://linear.app/team"));
        assert!(refused("python3 - <<EOF\nrun('linear issue close HRD-1')\nEOF"));
    }

    #[test]
    fn a_file_tool_pointed_at_the_core_is_refused_too() {
        assert_eq!(
            reason_on_file("/x/linear-sola-lettura.py").map(|(_, v)| v),
            Some(Valve::Core)
        );
        assert_eq!(
            reason_on_file("/x/settings.json").map(|(_, v)| v),
            Some(Valve::UserDeclared)
        );
        assert!(reason_on_file("/x/altro.py").is_none());
    }

    #[test]
    fn the_mcp_names_are_judged_on_the_verb() {
        assert!(reason_mcp("mcp__linear__create_issue").is_some());
        assert!(reason_mcp("mcp__linear__list_issues").is_none());
        assert!(reason_mcp("mcp__github__create_issue").is_none());
    }

    #[test]
    fn a_final_state_is_recognised_by_word_not_by_form() {
        assert!(closes_a_card("linear issue close HRD-1"));
        assert!(closes_a_card("--status Done"));
        assert!(!closes_a_card("linear issue show HRD-1"));
        assert_eq!(named_cards("chiudi HRD-123 e SUITE-4"), ["HRD-123", "SUITE-4"]);
    }

    #[test]
    fn the_reason_names_what_was_seen() {
        assert_eq!(why("linear issue close HRD-1"), "linear issue close");
        assert_eq!(
            why(r#"bash -c "linear issue close HRD-1""#),
            "linear … close eseguito da un interprete"
        );
    }
}
