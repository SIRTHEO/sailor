//! SocratiCode-first: sposta la scelta dello strumento al momento in cui si sceglie.
//!
//! Porta di `scripts/socraticode-gate-v2.js`, il secondo dei due ganci in Node e
//! — dopo il porting di `block-pr-merge-admin` — quello che detta il muro della
//! catena `PreToolUse` (48 ms misurati).
//!
//! Tre casi, una sola idea: affidare alla disciplina la scelta fra grep e
//! ricerca semantica non ha funzionato, e le misure lo dicono una per caso.
//!
//! 1. **Ricerca** (`Grep`, e `rg`/`grep -r` via `Bash`) dentro un repo
//!    indicizzato: si blocca una ricerca ogni 30. Il rilancio consapevole passa
//!    sempre. Un `… | grep x` filtra output, non cerca nel codice: passa.
//! 2. **Edit che rinomina o rimuove un simbolo esportato**: 119 commit di luglio
//!    che rinominano nei tre repo, contro 3 usi di `codebase_impact`.
//! 3. **File di codice nuovo senza aver cercato riuso**: la regola dichiarata
//!    «vincolante» era disattesa nel 42% delle scritture principali e nel 99% di
//!    quelle dei subagent, dove il vincolo non si eredita.
//!
//! **Fail-open ovunque**: qualunque errore lascia passare.
//!
//! Lo stato (contatori, tracce) vive su file in `TMPDIR`. Qui è tutto dietro a
//! `Workspace`, che nei test punta a una cartella temporanea: provare un gancio
//! contro lo stato vero lo renderebbe non deterministico, e in un caso
//! distruttivo.

use hook_io::{journal, Decision};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Dove il gancio tiene lo stato. Iniettabile perché i test non devono poter
/// toccare i contatori delle sessioni vive.
pub struct Workspace {
    pub home: PathBuf,
    pub tmp: PathBuf,
}

impl Workspace {
    pub fn from_env() -> Workspace {
        Workspace {
            home: std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("/")),
            tmp: std::env::temp_dir(),
        }
    }

    fn declared_projects(&self) -> PathBuf {
        self.home
            .join(".claude")
            .join("state")
            .join("socraticode-progetti.txt")
    }
}

/// Cosa il gate ha deciso, con il motivo che finisce nel registro: senza il
/// denominatore dei passaggi valutati non esce nessun tasso.
pub struct Verdict {
    pub decision: Decision,
    pub reason: &'static str,
    pub path: Option<String>,
    pub count: Option<i64>,
}

impl Verdict {
    fn pass(reason: &'static str) -> Verdict {
        Verdict {
            decision: Decision::Pass,
            reason,
            path: None,
            count: None,
        }
    }
    fn with_path(mut self, p: &str) -> Verdict {
        self.path = Some(p.to_string());
        self
    }
    fn with_count(mut self, n: i64) -> Verdict {
        self.count = Some(n);
        self
    }
    /// Il caso fuori perimetro non si registra affatto: il registro conta i casi
    /// **di competenza**, e sporcarlo con ogni chiamata a strumento renderebbe
    /// il denominatore inutile.
    fn out_of_scope() -> Verdict {
        Verdict {
            decision: Decision::Pass,
            reason: "",
            path: None,
            count: None,
        }
    }
    fn is_recorded(&self) -> bool {
        !self.reason.is_empty()
    }
}

// ── Indicizzazione ──────────────────────────────────────────────────────────

/// In un worktree `.git` è un **file** che contiene «gitdir: /…/.git/worktrees/x»:
/// il canonico è quel percorso troncato prima di `/.git/`.
///
/// Serve perché `.socraticodeignore` vive solo nel canonico, mentre gli agenti
/// lavorano tutti in `~/orca/workspaces/*` — lì il walk-up arrivava alla radice
/// del filesystem e il gate non scattava mai.
fn canonical_of_worktree(dir: &Path) -> Option<PathBuf> {
    let dotgit = dir.join(".git");
    if !std::fs::metadata(&dotgit).ok()?.is_file() {
        return None;
    }
    let text = std::fs::read_to_string(&dotgit).ok()?;
    let line = text.lines().find_map(|l| l.strip_prefix("gitdir:"))?;
    let gitdir = line.trim();
    let marker = format!(
        "{}.git{}",
        std::path::MAIN_SEPARATOR,
        std::path::MAIN_SEPARATOR
    );
    let i = gitdir.find(&marker)?;
    if i == 0 {
        return None;
    }
    Some(PathBuf::from(&gitdir[..i]))
}

/// L'elenco dichiarato batte il marcatore, perché **il marcatore mente**: l'11/08
/// `codebase_list_projects` dichiarava due progetti indicizzati e
/// `.socraticodeignore` esisteva solo nel primo.
fn declared(ws: &Workspace, dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(ws.declared_projects()) else {
        return false;
    };
    text.lines()
        .map(str::trim)
        .filter(|r| !r.is_empty() && !r.starts_with('#'))
        .any(|root| dir == Path::new(root) || dir.starts_with(root))
}

pub fn is_indexed(ws: &Workspace, dir: &Path) -> bool {
    is_indexed_within(ws, dir, 2)
}

fn is_indexed_within(ws: &Workspace, dir: &Path, hops_left: u8) -> bool {
    if declared(ws, dir) {
        return true;
    }
    let mut cur = dir.to_path_buf();
    loop {
        if cur.join(".socraticodeignore").exists() {
            return true;
        }
        if hops_left > 0 {
            if let Some(canonical) = canonical_of_worktree(&cur) {
                return is_indexed_within(ws, &canonical, hops_left - 1);
            }
        }
        match cur.parent() {
            Some(parent) if parent != cur => cur = parent.to_path_buf(),
            _ => return false,
        }
    }
}

// ── Contatore condiviso ─────────────────────────────────────────────────────

/// Il contatore parte a **metà quota**, non da zero.
///
/// Prima, senza marcatore, la funzione scriveva `0` e cadeva nel blocco: la
/// prima ricerca di *ogni* sessione veniva bloccata comunque, anche quella che
/// la regola consente per iscritto. Misurati 147 blocchi in 123 sessioni in una
/// settimana. Nelle sessioni corte si pagava quasi solo quel pedaggio, perché le
/// trenta ricerche successive non arrivavano mai.
fn throttle(ws: &Workspace, session: &str, reason: &str, quota: i64) -> Option<i64> {
    let marker = ws
        .tmp
        .join(format!("claude-socraticode-gate-{reason}-{session}"));
    match std::fs::read_to_string(&marker) {
        Ok(text) => {
            let n: i64 = text.trim().parse().unwrap_or(0);
            if n < quota {
                let _ = std::fs::write(&marker, (n + 1).to_string());
                return Some(n + 1);
            }
        }
        Err(_) => {
            let start = quota / 2 + 1;
            let _ = std::fs::write(&marker, start.to_string());
            return Some(start);
        }
    }
    let _ = std::fs::write(&marker, "0"); // a quota: si blocca e si riarma
    None
}

// ── Parti pure, quelle su cui si può ragionare senza filesystem ─────────────

/// `rg`/`grep` sono una ricerca nel codice solo in **posizione di comando**:
/// inizio riga, dopo `;`/`&&`/`||`, o dopo assegnazioni d'ambiente. Mai dopo una
/// pipe — lì filtrano l'output di qualcun altro.
pub fn is_code_search(command: &str) -> bool {
    static RG: OnceLock<Regex> = OnceLock::new();
    static GREP: OnceLock<Regex> = OnceLock::new();
    static RECURSIVE: OnceLock<Regex> = OnceLock::new();
    const ENV_PREFIX: &str = r"(?:[A-Za-z_][A-Za-z0-9_]*=\S*\s+)*";

    let rg = RG.get_or_init(|| Regex::new(&format!(r"(^|;|&&|\|\|)\s*{ENV_PREFIX}rg\s")).unwrap());
    if rg.is_match(command) {
        return true;
    }
    let grep = GREP.get_or_init(|| {
        Regex::new(&format!(r"(^|;|&&|\|\|)\s*{ENV_PREFIX}grep\s+([^|;]*)")).unwrap()
    });
    let recursive = RECURSIVE
        .get_or_init(|| Regex::new(r"\s(-[a-zA-Z]*[rR][a-zA-Z]*\b|--recursive\b)").unwrap());
    match grep.captures(command) {
        Some(c) => recursive.is_match(&format!(" {}", c.get(2).map_or("", |m| m.as_str()))),
        None => false,
    }
}

/// I nomi esportati dichiarati in un pezzo di sorgente.
pub fn exported_names(src: &str) -> BTreeSet<String> {
    static DECL: OnceLock<Regex> = OnceLock::new();
    static BRACED: OnceLock<Regex> = OnceLock::new();
    let decl = DECL.get_or_init(|| {
        Regex::new(
            r"export\s+(?:default\s+)?(?:async\s+)?(?:function\*?|const|let|var|class|interface|type|enum)\s+([A-Za-z_$][\w$]*)",
        )
        .unwrap()
    });
    let braced = BRACED.get_or_init(|| Regex::new(r"export\s*(?:type\s*)?\{([^}]*)\}").unwrap());
    static IDENT: OnceLock<Regex> = OnceLock::new();
    let ident = IDENT.get_or_init(|| Regex::new(r"^[A-Za-z_$][\w$]*$").unwrap());

    let mut names = BTreeSet::new();
    for c in decl.captures_iter(src) {
        names.insert(c[1].to_string());
    }
    // `\s+as\s+`, non un letterale: `foo   as   bar` è la stessa cosa, e
    // l'originale lo trattava così.
    static ALIAS: OnceLock<Regex> = OnceLock::new();
    let alias = ALIAS.get_or_init(|| Regex::new(r"\s+as\s+").unwrap());
    for c in braced.captures_iter(src) {
        for part in c[1].split(',') {
            // `foo as bar` → conta il nome pubblico, cioè bar
            let name = alias.split(part).last().unwrap_or("").trim();
            if ident.is_match(name) {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// I nomi che spariscono dal testo nuovo: rinominati o rimossi. Un export che
/// resta tale e quale — cambia solo il corpo — non muove niente per chi importa.
pub fn lost_exports(before: &str, after: &str) -> Vec<String> {
    exported_names(before)
        .into_iter()
        .filter(|n| {
            Regex::new(&format!(r"\b{}\b", regex::escape(n)))
                .map(|re| !re.is_match(after))
                .unwrap_or(false)
        })
        .collect()
}

const CODE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "js", "jsx", "mjs", "cjs", "py", "go", "rs", "java", "rb", "php", "swift", "kt",
];

pub fn is_code_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| CODE_EXTENSIONS.contains(&e))
        .unwrap_or(false)
}

pub fn is_out_of_perimeter(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^|/)(node_modules|dist|build|\.next|coverage|\.git)/").unwrap())
        .is_match(path)
}

pub fn is_throwaway(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^/tmp/|^/private/tmp/|/scratchpad/|\.claude/state/)").unwrap())
        .is_match(path)
}

/// Il primo percorso assoluto del comando — copre `S=/repo/… grep "$S/src"`.
pub fn first_absolute_path(command: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"/(?:Users|home)/[^\s\x22'`;|&)]+").unwrap())
        .find(command)
        .map(|m| m.as_str().to_string())
}

// ── I messaggi ──────────────────────────────────────────────────────────────

fn message_search() -> String {
    // Gli strumenti SocratiCode sono DIFFERITI: la sessione ne conosce il nome,
    // non lo schema. Un blocco che ordina di usare `codebase_search` senza dire
    // come si carica manda l'agente contro uno strumento che per lui non
    // esiste, e lo rimanda al grep — misurato il 12/08 sul banco di prova: due
    // giri, 45 comandi di ricerca, ZERO chiamate a `ToolSearch`.
    [
        "SocratiCode-first: questo repo e' indicizzato (index green).",
        "Se gli strumenti codebase_* non ti risultano disponibili sono DIFFERITI:",
        "  ToolSearch query \"select:codebase_search,codebase_symbol,codebase_flow\"",
        "Poi usa la navigazione semantica al posto del grep:",
        "  - codebase_search \"<dominio>\"  -> trovare riuso / dove vive una feature",
        "  - codebase_impact / codebase_graph_query  -> blast radius prima di refactor/rename/delete",
        "  - codebase_flow / codebase_symbol  -> bug-hunt (entry point, def/caller/callee)",
        "Se invece cerchi un identificatore ESATTO gia' noto, una error string letterale",
        "o una regex su pattern preciso: rilancia lo STESSO comando, ora passa",
        "(il gate si riarma ogni 30 ricerche). Regola: ~/.claude/rules/socraticode-first.md",
    ]
    .join("\n")
}

fn message_impact(lost: &[String]) -> String {
    format!(
        "SocratiCode-first: questo Edit rinomina o rimuove un simbolo esportato ({}).\n\
         Chi lo importa non e' visibile da qui. Prima misura il raggio:\n\
         \x20 - codebase_impact  -> chi rompe questo cambio\n\
         \x20 - codebase_graph_query  -> dipendenze entranti, anche cross-repo\n\
         Poi rilancia lo STESSO Edit: passa (il gate si riarma ogni 10).\n\
         Se il simbolo e' interno malgrado l'export (test, barrel locale), rilancia e basta.",
        lost.join(", ")
    )
}

fn message_reuse(file: &str) -> String {
    format!(
        "SocratiCode-first: stai creando un file di codice NUOVO senza aver cercato riuso.\n\
         \x20 {file}\n\
         La regola del workspace lo chiama «vincolante, non saltabile»: prima cerca se\n\
         la cosa esiste gia, poi decidi se estendere invece di affiancare.\n\
         \x20 codebase_search \"<il dominio a parole>\"   — non l'identificatore: «dove si\n\
         \x20 normalizza un numero di telefono», non normalizePhone\n\
         Trovato qualcosa di simile: estendilo. Un secondo formatDate a tre cartelle di\n\
         distanza e il modo in cui nasce il debito.\n\
         Poi rilancia lo STESSO Write: passa (la ricerca vale 20 minuti).\n\
         Se stai delegando a un subagent, il vincolo va ripetuto nel suo prompt: li si\n\
         perde nel 99% dei casi. Regola: other-repo/work/.claude/rules/moduli-profondi-e-riuso.md"
    )
}

// ── Il giudizio ─────────────────────────────────────────────────────────────

const SEARCH_WINDOW_MS: u128 = 20 * 60 * 1000;

fn search_trace(ws: &Workspace, session: &str) -> PathBuf {
    ws.tmp.join(format!("claude-reuse-cercato-{session}"))
}

pub fn judge(ws: &Workspace, input: &hook_io::HookInput) -> Verdict {
    let tool = input.tool_name.as_deref().unwrap_or("");
    let session = input.session_id.as_deref().unwrap_or("nosession");

    // Una chiamata alla ricerca semantica lascia la traccia che il caso 3 legge.
    // Il nome cambia col prefisso del plugin: si riconosce dal suffisso.
    if tool.ends_with("codebase_search")
        || tool.ends_with("codebase_context_search")
        || tool.ends_with("codebase_symbol")
        || tool.ends_with("codebase_flow")
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = std::fs::write(search_trace(ws, session), now.to_string());
        return Verdict::out_of_scope();
    }

    match tool {
        "Edit" => judge_edit(ws, input, session),
        "Write" => judge_write(ws, input, session),
        "Grep" | "Bash" => judge_search(ws, input, session),
        _ => Verdict::out_of_scope(),
    }
}

fn field<'a>(input: &'a hook_io::HookInput, name: &str) -> &'a str {
    input
        .tool_input
        .as_ref()
        .and_then(|v| v.get(name))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn judge_edit(ws: &Workspace, input: &hook_io::HookInput, session: &str) -> Verdict {
    let file = field(input, "file_path");
    static TS: OnceLock<Regex> = OnceLock::new();
    let ts = TS.get_or_init(|| Regex::new(r"\.(ts|tsx|js|jsx|mjs|cjs)$").unwrap());
    if !ts.is_match(file) {
        return Verdict::out_of_scope();
    }
    let dir = Path::new(file).parent().unwrap_or(Path::new("."));
    if !is_indexed(ws, dir) {
        return Verdict::out_of_scope();
    }
    let lost = lost_exports(field(input, "old_string"), field(input, "new_string"));
    if lost.is_empty() {
        return Verdict::out_of_scope();
    }
    match throttle(ws, session, "impact", 10) {
        Some(n) => Verdict::pass("sotto-quota-impact").with_count(n),
        None => Verdict {
            decision: Decision::Block(message_impact(&lost)),
            reason: "export-toccato-senza-impatto",
            path: Some(file.to_string()),
            count: None,
        },
    }
}

fn judge_write(ws: &Workspace, input: &hook_io::HookInput, session: &str) -> Verdict {
    let file = field(input, "file_path");
    if file.is_empty() || !is_code_file(file) {
        return Verdict::out_of_scope(); // documenti, config, dati: non è riuso di codice
    }
    if is_out_of_perimeter(file) || is_throwaway(file) {
        return Verdict::out_of_scope();
    }
    if Path::new(file).exists() {
        return Verdict::out_of_scope(); // riscrittura, non file nuovo
    }
    if !is_indexed(ws, Path::new(file).parent().unwrap_or(Path::new("."))) {
        return Verdict::out_of_scope(); // senza indice la ricerca semantica non c'è
    }

    if let Ok(text) = std::fs::read_to_string(search_trace(ws, session)) {
        if let Ok(t) = text.trim().parse::<u128>() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0);
            if now.saturating_sub(t) < SEARCH_WINDOW_MS {
                return Verdict::pass("ricerca-recente").with_path(file);
            }
        }
    }

    // Non il contatore a quota — parte a metà, giusto per centinaia di grep e
    // sbagliato qui: creare un file è raro e ogni volta merita la domanda. Vale
    // invece la regola del caso 2, il rilancio consapevole dello STESSO file
    // passa. Così il gate parla una volta per file e non manda in stallo chi ha
    // deciso di scrivere lo stesso.
    let stamp = base64url_tail(file, 40);
    let already = ws.tmp.join(format!("claude-reuse-detto-{session}-{stamp}"));
    if already.exists() {
        return Verdict::pass("rilancio-consapevole").with_path(file);
    }
    if std::fs::write(&already, "1").is_err() {
        return Verdict::out_of_scope();
    }
    Verdict {
        decision: Decision::Block(message_reuse(file)),
        reason: "file-nuovo-senza-ricerca",
        path: Some(file.to_string()),
        count: None,
    }
}

fn judge_search(ws: &Workspace, input: &hook_io::HookInput, session: &str) -> Verdict {
    let tool = input.tool_name.as_deref().unwrap_or("");
    let cwd = input.cwd.clone().unwrap_or_default();
    let mut target = if tool == "Grep" {
        let p = field(input, "path");
        if p.is_empty() {
            cwd.clone()
        } else {
            p.to_string()
        }
    } else {
        let command = field(input, "command");
        if !is_code_search(command) {
            return Verdict::out_of_scope();
        }
        first_absolute_path(command).unwrap_or_else(|| cwd.clone())
    };

    let mut path = PathBuf::from(&target);
    if !path.is_absolute() {
        path = Path::new(&cwd).join(&target);
    }
    match std::fs::metadata(&path) {
        Ok(meta) if meta.is_dir() => {}
        _ => path = path.parent().map(Path::to_path_buf).unwrap_or(path),
    }
    target = path.to_string_lossy().into_owned();

    if !is_indexed(ws, &path) {
        return Verdict::out_of_scope(); // non indicizzato → il grep è il ripiego giusto
    }
    match throttle(ws, session, "search", 30) {
        Some(n) => Verdict::pass("sotto-quota-search").with_count(n),
        None => Verdict {
            decision: Decision::Block(message_search()),
            reason: "grep-in-repo-indicizzato",
            path: Some(target),
            count: None,
        },
    }
}

/// Gli ultimi `n` caratteri del percorso in base64url — lo stesso nome di
/// marcatore che scriveva il JavaScript, così una sessione a cavallo della
/// migrazione non si ritrova il gate che parla due volte per lo stesso file.
fn base64url_tail(s: &str, n: usize) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        let take = chunk.len() + 1;
        for i in 0..take {
            out.push(ALPHABET[((n >> (18 - 6 * i)) & 0x3F) as usize] as char);
        }
    }
    let start = out.len().saturating_sub(n);
    out[start..].to_string()
}

/// Scrive la riga di registro, con gli stessi campi del JavaScript.
pub fn record(verdict: &Verdict, tool: &str, session: &str) {
    if !verdict.is_recorded() {
        return;
    }
    let decision = match verdict.decision {
        Decision::Block(_) => "blocca",
        _ => "passa",
    };
    let mut extra: Vec<(&str, journal::Field)> =
        vec![("strumento", tool.into()), ("sessione", session.into())];
    if let Some(n) = verdict.count {
        extra.push(("conteggio", n.into()));
    }
    if let Some(p) = &verdict.path {
        extra.push(("percorso", p.clone().into()));
    }
    journal::record("socraticode-gate", decision, verdict.reason, &extra);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_grep_after_a_pipe_filters_output_and_is_not_a_code_search() {
        assert!(!is_code_search("ls -la | grep -r foo"));
        assert!(!is_code_search("cat file | grep pattern"));
    }

    #[test]
    fn a_recursive_grep_in_command_position_is_a_code_search() {
        assert!(is_code_search("grep -r foo src/"));
        assert!(is_code_search("grep -rn foo src/"));
        assert!(is_code_search("S=/repo grep -r foo \"$S\""));
        assert!(is_code_search("echo x; grep -r foo src/"));
        assert!(is_code_search("rg foo"));
        assert!(is_code_search("cd /x && rg foo"));
    }

    #[test]
    fn a_non_recursive_grep_is_left_alone() {
        assert!(!is_code_search("grep foo file.txt"));
    }

    #[test]
    fn it_finds_the_exported_names_including_renamed_ones() {
        let src = "export const foo = 1; export function bar() {} export { baz as qux }";
        let names = exported_names(src);
        assert!(names.contains("foo"));
        assert!(names.contains("bar"));
        // `baz as qux` esporta il nome pubblico: qux
        assert!(names.contains("qux"));
        assert!(!names.contains("baz"));
    }

    #[test]
    fn only_the_exports_that_disappear_count_as_lost() {
        let before = "export const foo = 1;\nexport const bar = 2;";
        let after = "export const foo = 42;";
        assert_eq!(lost_exports(before, after), vec!["bar".to_string()]);
    }

    #[test]
    fn a_body_only_change_loses_nothing() {
        let before = "export function foo() { return 1 }";
        let after = "export function foo() { return 2 }";
        assert!(lost_exports(before, after).is_empty());
    }

    #[test]
    fn it_recognises_throwaway_and_out_of_perimeter_paths() {
        assert!(is_throwaway("/private/tmp/x/scratchpad/a.py"));
        assert!(is_throwaway("/tmp/a.ts"));
        assert!(is_out_of_perimeter("/repo/node_modules/x/index.js"));
        assert!(!is_out_of_perimeter("/repo/src/index.js"));
    }

    #[test]
    fn it_encodes_the_marker_name_like_the_javascript_did() {
        // node: Buffer.from('/home/someone/a.ts').toString('base64url').slice(-40)
        assert_eq!(
            base64url_tail("/home/someone/a.ts", 40),
            "L1VzZXJzL3RoZW8vYS50cw"
        );
    }

    fn workspace() -> (Workspace, tempdir::TempDir) {
        let dir = tempdir::TempDir::new();
        let ws = Workspace {
            home: dir.path().join("home"),
            tmp: dir.path().join("tmp"),
        };
        std::fs::create_dir_all(&ws.tmp).unwrap();
        std::fs::create_dir_all(ws.home.join(".claude").join("state")).unwrap();
        (ws, dir)
    }

    #[test]
    fn the_counter_starts_at_half_quota_so_short_sessions_never_pay_the_toll() {
        let (ws, _keep) = workspace();
        // quota 30 → si parte da 16, quindi restano 14 passaggi prima del blocco
        let first = throttle(&ws, "s1", "search", 30).unwrap();
        assert_eq!(first, 16);
        let mut last = first;
        for _ in 0..14 {
            last = throttle(&ws, "s1", "search", 30).expect("ancora sotto quota");
        }
        assert_eq!(last, 30);
        assert!(
            throttle(&ws, "s1", "search", 30).is_none(),
            "a quota deve bloccare"
        );
        // e subito dopo si riarma: il rilancio consapevole passa sempre
        assert_eq!(throttle(&ws, "s1", "search", 30), Some(1));
    }

    #[test]
    fn an_unindexed_directory_is_left_to_grep() {
        let (ws, keep) = workspace();
        assert!(!is_indexed(&ws, keep.path()));
    }

    #[test]
    fn a_marker_makes_the_directory_and_its_children_indexed() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        assert!(is_indexed(&ws, &repo.join("src")));
    }

    #[test]
    fn a_worktree_follows_its_gitdir_back_to_the_canonical_checkout() {
        let (ws, keep) = workspace();
        let canonical = keep.path().join("canonico");
        std::fs::create_dir_all(canonical.join(".git")).unwrap();
        std::fs::write(canonical.join(".socraticodeignore"), "").unwrap();
        let worktree = keep.path().join("copia");
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}/.git/worktrees/copia\n", canonical.display()),
        )
        .unwrap();
        assert!(
            is_indexed(&ws, &worktree),
            "il marcatore vive solo nel canonico"
        );
    }

    #[test]
    fn the_declared_list_wins_where_the_marker_is_absent() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("dichiarato");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(
            ws.declared_projects(),
            format!("# commento\n{}\n", repo.display()),
        )
        .unwrap();
        assert!(is_indexed(&ws, &repo.join("src")));
    }

    /// Una cartella temporanea che si cancella da sé: i test non devono poter
    /// toccare i contatori delle sessioni vive.
    mod tempdir {
        use std::path::{Path, PathBuf};
        use std::sync::atomic::{AtomicU32, Ordering};

        static COUNTER: AtomicU32 = AtomicU32::new(0);

        pub struct TempDir(PathBuf);

        impl TempDir {
            pub fn new() -> TempDir {
                let n = COUNTER.fetch_add(1, Ordering::SeqCst);
                let p = std::env::temp_dir()
                    .join(format!("socraticode-gate-prova-{}-{n}", std::process::id()));
                std::fs::create_dir_all(&p).unwrap();
                TempDir(p)
            }
            pub fn path(&self) -> &Path {
                &self.0
            }
        }

        impl Drop for TempDir {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
    }
}
