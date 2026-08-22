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
//!    indicizzato: il giudizio guarda la DOMANDA, non un contatore. Un
//!    identificatore esatto, una regex o una stringa d'errore passano
//!    sempre, senza dire niente — anche dentro un file solo. Una domanda
//!    concettuale parla sempre, non a turno, che il bersaglio sia una
//!    cartella o un file già individuato. Un `… | grep x` filtra l'output di
//!    un altro comando, non cerca nel codice: passa.
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

/// Le radici dichiarate nel file di stato, una per riga: la stessa lista che
/// `declared()` consulta per rispondere sì/no a una cartella, esposta qui
/// perché anche `index-freshness` (in `claude-hooks`) deve sapere QUALI repo
/// sono indicizzati — non solo se lo è una cartella data.
pub fn declared_projects_list(home: &Path) -> Vec<PathBuf> {
    let path = home
        .join(".claude")
        .join("state")
        .join("socraticode-progetti.txt");
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|r| !r.is_empty() && !r.starts_with('#'))
        .map(PathBuf::from)
        .collect()
}

/// L'elenco dichiarato batte il marcatore, perché **il marcatore mente**: l'11/08
/// `codebase_list_projects` dichiarava due progetti indicizzati e
/// `.socraticodeignore` esisteva solo nel primo.
fn declared(ws: &Workspace, dir: &Path) -> bool {
    declared_projects_list(&ws.home)
        .iter()
        .any(|root| dir == root.as_path() || dir.starts_with(root))
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
    if let Some(c) = grep.captures(command) {
        if recursive.is_match(&format!(" {}", c.get(2).map_or("", |m| m.as_str()))) {
            return true;
        }
    }
    scans_many_files(command)
}

/// Le ricerche che non si scrivono `grep -r`, e che per questo passavano tutte.
///
/// La regola «mai dopo una pipe» è giusta per `… | grep x`, che filtra l'output
/// di qualcun altro. Ma `find … | xargs grep` non filtra niente: apre i file che
/// `find` gli passa, cioè è esattamente un grep ricorsivo scritto in due pezzi.
/// Stesso discorso per `find … -exec grep`.
///
/// E c'è la strada che il gate non vedeva affatto: uno script che cammina
/// l'albero e applica espressioni regolari. Misurato il 18/08/2026 su un
/// censimento delle rotte del matching-engine — due regex su `method:` e
/// `path:` — le sue **214 operazioni diventavano 168, il 21% perso**, e il
/// numero sbagliato era finito in un piano prima di essere verificato. Lo
/// strumento giusto stava già nel repo (`scripts/openapi-dump.ts`), e il gate,
/// che serve proprio a farlo cercare, in tutta la sessione non ha detto niente:
/// nessuno di quei comandi era un `grep`.
///
/// Si riconosce solo chi **cammina** l'albero (`glob`, `os.walk`, `rglob`,
/// `iterdir`) **e** applica regex: leggere un file solo non è cercare.
fn scans_many_files(command: &str) -> bool {
    static XARGS: OnceLock<Regex> = OnceLock::new();
    static WALK: OnceLock<Regex> = OnceLock::new();
    static REGEX_USE: OnceLock<Regex> = OnceLock::new();

    let xargs = XARGS.get_or_init(|| {
        Regex::new(r"(\|\s*xargs\s+(-\S+\s+)*(grep|rg)\b|-exec\s+(grep|rg)\b)").unwrap()
    });
    if xargs.is_match(command) {
        return true;
    }
    let walk = WALK
        .get_or_init(|| Regex::new(r"\b(glob\.glob|glob\(|os\.walk|rglob|iterdir)").unwrap());
    let regex_use = REGEX_USE.get_or_init(|| Regex::new(r"\bre\.(findall|search|match|compile|finditer)").unwrap());
    walk.is_match(command) && regex_use.is_match(command)
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

/// `plugins/cache` accanto agli altri: un plugin installato dal marketplace è
/// vendorizzato quanto `node_modules`, a versione fissata. Cercare una
/// stringa letterale lì dentro non è una domanda sul dominio di questo
/// workspace, ed è il motivo per cui `judge_search` lo esclude allo stesso
/// modo (21/08/2026, «touched by other sessions» bloccato dentro
/// `claude-code-harness-marketplace`).
pub fn is_out_of_perimeter(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(^|/)(node_modules|dist|build|\.next|coverage|\.git|plugins/cache)/").unwrap()
    })
    .is_match(path)
}

pub fn is_throwaway(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(^/tmp/|^/private/tmp/|/scratchpad/|\.claude/state/)").unwrap())
        .is_match(path)
}

/// Un file di prova: qui la domanda sul riuso non ottiene niente.
///
/// FUORI PERIMETRO DAL 21/08/2026, per misura e non per gusto. Sul registro dei
/// ganci, 11-20/08, 207 rifiuti veri di questo gate: chi stava creando un file
/// di prova è andato a cercare 15 volte su 81 (18%) e ha rilanciato lo stesso
/// file senza cercare 55 volte (67%). Chi stava scrivendo codice cerca nel 42%
/// dei casi, chi scrive script nel 64%: lì il conto regge, e lì il gate resta.
///
/// Un suggeritore che sbaglia due volte su tre insegna a ignorarsi, e si porta
/// dietro anche i casi in cui aveva ragione.
///
/// Il motivo per cui non ottiene niente sta nel mestiere, non nell'abitudine: un
/// file di prova nasce accanto a ciò che prova, e la domanda «esiste già
/// qualcosa del genere» ha per risposta il file che si sta per provare.
///
/// Decisione del capitano, 21/08/2026.
pub fn is_test_file(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // `\.(test|spec)\.` prende `x.test.ts` e `x.spec.tsx` e non `x.ts`; le
        // cartelle prendono l'altra convenzione, quella per posizione.
        Regex::new(r"(\.(test|spec)\.|/__tests__/|/tests?/)").unwrap()
    })
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

/// Il consiglio dato quando una ricerca è giudicata concettuale.
///
/// Porta la distinzione che nel resto della configurazione manca: per una
/// domanda STRUTTURALE («dove vive», «esiste gia'», «cosa esporta», «chi
/// dipende») l'indice vale anche se e' vecchio, perche' la forma del codice
/// cambia lentamente; il testo remoto (grep) serve al VALORE DI OGGI — quante
/// occorrenze adesso, quale versione dichiara il manifesto — non a dire dove
/// vive qualcosa.
fn concept_advice() -> &'static str {
    "  ToolSearch query \"select:codebase_search,codebase_symbol,codebase_flow\"\n  codebase_search \"<il dominio a parole>\"   -> dove vive, esiste gia'\n  codebase_graph_query          -> chi importa questo file (projectPath del REPO)\nDomanda STRUTTURALE (dove vive, cosa esporta, chi dipende): l'indice vale\nanche se e' vecchio, perche' la forma del codice cambia lentamente. Il grep\nresta lo strumento giusto solo per il VALORE DI OGGI: quante occorrenze\nadesso, quale versione dichiara il manifesto.\nLa domanda si fa nella lingua in cui e' scritto cio' che cerchi.\nSe invece cerchi un identificatore ESATTO, una stringa d'errore letterale o\nuna regex, riscrivi il pattern in quella forma: passa senza pedaggio."
}

/// Il messaggio della ricerca concettuale.
///
/// NON e' piu' identico byte per byte al gemello in
/// `scripts/socraticode-gate-v2.js`: qui la sostanza e' cambiata (giudizio
/// sulla domanda, non sul conteggio; la distinzione strutturale/valore-di-
/// oggi) e il JS resta fuori dal perimetro di questa modifica — il ripiego,
/// quando il binario manca, parla ancora col testo vecchio finche' qualcuno
/// non lo riporta in pari.
///
/// Porta **cosa** cercavi, e non e' cosmesi: senza il pattern, chi legge il
/// blocco non sa quale delle sue ricerche e' stata giudicata concettuale.
fn message_concept(sought: &str) -> String {
    format!(
        "SocratiCode-first: questa e' una ricerca CONCETTUALE in un repo indicizzato.\nCercavi: {sought}\nIl grep conta le occorrenze; non dice se una cosa e' viva, chi la importa,\nne' dove vive il dominio. Per questa domanda la risposta sta altrove:\n{}",
        concept_advice()
    )
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
         perde nel 99% dei casi. Regola: gyver/work/.claude/rules/moduli-profondi-e-riuso.md"
    )
}

// ── Il giudizio ─────────────────────────────────────────────────────────────

const SEARCH_WINDOW_MS: u128 = 20 * 60 * 1000;

fn search_trace(ws: &Workspace, session: &str) -> PathBuf {
    ws.tmp.join(format!("claude-reuse-cercato-{session}"))
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
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
        let _ = std::fs::write(search_trace(ws, session), now_ms().to_string());
        return Verdict::out_of_scope();
    }

    match tool {
        "Edit" => judge_edit(ws, input, session),
        "Write" => judge_write(ws, input, session),
        "Grep" | "Bash" => judge_search(ws, input),
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
    if is_out_of_perimeter(file) || is_throwaway(file) || is_test_file(file) {
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

fn judge_search(ws: &Workspace, input: &hook_io::HookInput) -> Verdict {
    let tool = input.tool_name.as_deref().unwrap_or("");
    let cwd = input.cwd.clone().unwrap_or_default();
    // Vuoto per il tool `Grep`, che non passa da una riga di comando: le
    // funzioni che lo leggono (`explicit_target_count`) restituiscono 0 e non
    // contano nulla, il che è corretto per quel caso.
    let command = field(input, "command");
    let (raw_target, pattern) = if tool == "Grep" {
        let p = field(input, "path");
        (
            if p.is_empty() { cwd.clone() } else { p.to_string() },
            field(input, "pattern").to_string(),
        )
    } else {
        if !is_code_search(command) {
            return Verdict::out_of_scope();
        }
        (
            first_absolute_path(command).unwrap_or_else(|| cwd.clone()),
            grep_pattern(command).unwrap_or_default(),
        )
    };

    let mut path = PathBuf::from(&raw_target);
    if !path.is_absolute() {
        path = Path::new(&cwd).join(&path);
    }

    // Il percorso riportato resta quello vero, file compreso: prima veniva
    // collassato alla cartella padre anche quando il bersaglio era un file
    // gia' individuato, e il registro perdeva quale file si stava cercando.
    // Si collassa solo un percorso che non esiste affatto — ne' `is_indexed`
    // ne' il giudizio sul pattern dipendono dall'essere file o cartella.
    if !path.exists() {
        path = path.parent().map(Path::to_path_buf).unwrap_or(path);
    }
    let target = path.to_string_lossy().into_owned();

    // Codice vendorizzato (un plugin installato, non scritto qui) non è mai
    // la domanda «dove vive nel nostro dominio»: lo stesso motivo per cui
    // `judge_write` lo tiene fuori dal riuso.
    if is_out_of_perimeter(&target) {
        return Verdict::out_of_scope();
    }

    if !is_indexed(ws, &path) {
        return Verdict::out_of_scope(); // non indicizzato → il grep è il ripiego giusto
    }

    // Due o più bersagli scritti a mano nella stessa chiamata: chi cerca ha
    // già scelto dove guardare, sta contando le occorrenze di qualcosa di
    // preciso. Solo su `Bash`: il tool `Grep` porta un `path` solo per
    // costruzione, e un bersaglio solo resta ambiguo apposta (vedi test).
    if tool != "Grep" && explicit_target_count(command) >= 2 {
        return Verdict::out_of_scope();
    }

    // IL GIUDIZIO GUARDA LA DOMANDA, NON UN CONTATORE. Un identificatore
    // esatto, una regex o una stringa d'errore sono lo strumento giusto e
    // passano sempre, senza pedaggio: se il criterio e' buono non serve un
    // contatore. Una domanda concettuale parla sempre, non a turno — anche
    // dentro un file solo: e' lo stesso censimento spezzato in tanti grep.
    if !allowed_search(&pattern) {
        return Verdict {
            decision: Decision::Block(message_concept(&pattern)),
            reason: "ricerca-concettuale",
            path: Some(target),
            count: None,
        };
    }

    Verdict::out_of_scope()
}

/// Il pattern dentro un `grep`/`rg` scritto a riga di comando.
///
/// Serve perché metà delle ricerche non passa dallo strumento `Grep` ma da
/// `Bash`, e lì il pattern è un argomento fra gli altri. Si prende il primo
/// pezzo non-opzione dopo il comando; se è fra virgolette si tolgono. `None`
/// quando non si riesce a isolarlo, e chi chiama tratta `None` come «ammesso»:
/// non aver capito la ricerca non è una ragione per negarla.
/// Divide una riga di comando in parole, ma tiene insieme cio' che sta fra
/// virgolette.
///
/// Serve perche' `split_whitespace()` spezzava
/// `grep -rn "dove si gestisce il pagamento fallito" src` alla prima parola, e
/// `grep_pattern` restituiva `dove`: una domanda concettuale diventava un
/// identificatore e passava. Il difetto non mordeva finche' il gate contava le
/// ricerche invece di leggerle; da quando il giudizio guarda il pattern, meta'
/// delle ricerche — quelle che passano da `Bash` — sfuggivano tutte.
fn split_respecting_quotes(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match quote {
            Some(q) => {
                cur.push(c);
                if c == q {
                    quote = None;
                }
            }
            None if c == '"' || c == '\'' => {
                quote = Some(c);
                cur.push(c);
            }
            None if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            None => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Tutto ciò che segue `grep`/`rg` sulla riga di comando dove inizia
/// l'invocazione. `.*` non attraversa un `\n`: se il comando incolla più
/// righe con `grep` su una e `echo`/un altro comando sulla successiva, la
/// coda si ferma alla fine della riga giusta.
fn grep_invocation_tail(command: &str) -> Option<&str> {
    static CALL: OnceLock<Regex> = OnceLock::new();
    let call = CALL.get_or_init(|| Regex::new(r"(?:^|;|&&|\|\|)\s*(?:rg|grep)\s+(.*)").unwrap());
    Some(call.captures(command)?.get(1)?.as_str())
}

pub fn grep_pattern(command: &str) -> Option<String> {
    let tail = grep_invocation_tail(command)?;
    for word in split_respecting_quotes(tail) {
        let word = word.as_str();
        if word.starts_with('-') {
            continue;
        }
        let cleaned = word.trim_matches(|c| c == '"' || c == '\'');
        if cleaned.is_empty() {
            continue;
        }
        return Some(cleaned.to_string());
    }
    None
}

/// Quanti bersagli scritti a mano seguono il pattern, nella STESSA
/// invocazione — non nell'intero comando: fra loro può starci un `;`, un
/// `|`, una redirezione, ed è già un comando diverso.
///
/// Due o più dicono «so già dove guardare, sto contando le occorrenze», non
/// «dove vive questa cosa»: è la restrizione che il messaggio del gate
/// promette («un file solo, poche righe») generalizzata a una manciata di
/// percorsi nominati invece che all'intero repo implicito. Un solo bersaglio
/// resta ambiguo — anche una domanda concettuale si scrive spesso contro una
/// cartella sola — e non basta da solo (vedi il caso della cartella singola
/// nei test).
fn explicit_target_count(command: &str) -> usize {
    let Some(tail) = grep_invocation_tail(command) else {
        return 0;
    };
    let mut seen_pattern = false;
    let mut count = 0;
    for word in split_respecting_quotes(tail) {
        let word = word.as_str();
        if word.starts_with('-') {
            continue; // un'opzione, non il pattern né un bersaglio
        }
        if !seen_pattern {
            seen_pattern = true; // questo pezzo è il pattern
            continue;
        }
        // Pipe, `;`, redirezione: da qui in poi è un altro comando, non un
        // altro bersaglio della stessa ricerca.
        if word.chars().any(|c| "|;&<>".contains(c)) {
            break;
        }
        count += 1;
    }
    count
}

/// Metacaratteri di regex, o la sagoma di un identificatore di codice
/// (`snake_case`, `SCREAMING_CASE`, `camelCase`, `dotted.path`, `a::b`).
/// Nessuna di queste forme è una parola di dominio.
fn looks_like_a_shape(p: &str) -> bool {
    let has_metachar = p.contains('\\')
        || p.contains('[')
        || p.contains('(')
        || p.contains('|')
        || p.contains('^')
        || p.contains('$')
        || p.contains(".*")
        || p.contains('+')
        // `<`, `>`, `=`, `#`: nessuna frase in lingua li usa. Aggiunti il
        // 22/08/2026 perché `<<<<<<< HEAD` — un marcatore di conflitto, non
        // una domanda — restava fuori da ogni condizione qui sotto: tutto
        // maiuscolo sì, ma con `<` di mezzo, che non è né maiuscola né
        // trattino, e la frase falliva anche il conteggio delle parole.
        || p.contains('<')
        || p.contains('>')
        || p.contains('=')
        || p.contains('#');
    if has_metachar {
        return true;
    }
    if p.contains('_')
        || p.contains("::")
        || p.contains('.')
        || (p.chars().any(|c| c.is_ascii_uppercase()) && p.chars().any(|c| c.is_ascii_lowercase()))
        || p.chars().all(|c| c.is_ascii_uppercase() || c == '-' || c.is_ascii_digit())
    {
        return true;
    }

    // LE DUE FORME CHE IL MESSAGGIO DEL GATE PROMETTE E CHE NEGAVA LO STESSO.
    // Aggiunte il 21/08/2026 dopo averci sbattuto due volte in una notte: il
    // gate dice «una stringa d'errore letterale passa senza pedaggio» e poi
    // negava `CONTESTO AL`, che è il principio di un messaggio suo. Un gate che
    // contraddice il criterio che stampa insegna a non leggerlo.
    //
    // 1. Ogni parola tutta maiuscola: una stringa d'errore o una costante sono
    //    quasi sempre più parole, e la condizione qui sopra le perdeva tutte
    //    perché lo spazio non è né maiuscola né trattino. Una domanda di
    //    dominio, che è ciò che il gate vuole fermare, si scrive in minuscolo.
    let all_shouting = p
        .split_whitespace()
        .all(|w| w.chars().all(|c| c.is_ascii_uppercase() || c == '-' || c.is_ascii_digit()));
    if all_shouting {
        return true;
    }
    // 2. Frammento di codice ricopiato: virgolette e punteggiatura che nel
    //    linguaggio naturale non compaiono. `"code-language" =>` era il secondo
    //    caso negato, e non è una domanda: è una riga presa da un sorgente.
    p.contains('"') || p.contains('\'') || p.contains("=>") || p.contains('{') || p.contains(';')
}

// `git` in coda: non è una parola chiave di un linguaggio, ma di un
// programma — `grep -rn "git commit"` cerca un'invocazione, non chiede dove
// vive un concetto, esattamente come `def journal`.
const SEARCH_KEYWORDS: [&str; 15] = [
    "def", "fn", "class", "function", "const", "let", "var", "import", "export", "pub", "struct",
    "enum", "interface", "type", "git",
];

/// Una frase è la firma più netta di una ricerca concettuale — ma «frase» non
/// è «più di una parola»: `def journal` e `fn record` sono due parole e sono
/// il modo normale di cercare una definizione. La differenza è che una delle
/// due è una parola chiave di linguaggio, non di dominio. Provato: senza
/// questo elenco il caso preso da una ricerca vera falliva.
fn is_keyword_phrase(p: &str) -> bool {
    let words: Vec<&str> = p.split_whitespace().collect();
    words.len() > 1 && words.iter().any(|w| SEARCH_KEYWORDS.contains(&w.to_lowercase().as_str()))
}

/// Questa ricerca è di quelle che la regola **ammette per iscritto**?
///
/// PERCHÉ SERVE UN GIUDIZIO, E NON UNA QUOTA. Un contatore che blocca una
/// ricerca ogni trenta non guarda cosa si cerca: `grep -c "ORCA_TAB_ID"` e una
/// ricerca di dominio avevano la stessa probabilità di cadere nel blocco.
/// Misurato il 17/08/2026 in una sessione sola: **215 passaggi
/// `sotto-quota-search` contro 5 blocchi**, e nel frattempo Theo ha dovuto
/// correggere a mano l'uso dello strumento «almeno la decima volta oggi». Se
/// il criterio è buono non serve un contatore; se serve un contatore, il
/// criterio non è buono — qui non ce n'è più uno: una ricerca ammessa passa
/// **sempre**, una concettuale parla **sempre**.
///
/// LA REGOLA CHE QUESTA FUNZIONE RENDE ESEGUIBILE, dal `CLAUDE.md`: «Il grep
/// resta giusto per identificatori esatti, stringhe d'errore, espressioni
/// regolari e per filtrare l'output di un altro comando».
///
/// NEL DUBBIO SI AMMETTE. Le violazioni chiare erano il 2,3% delle ricerche
/// (misura del 10/08/2026, `docs/perche-queste-regole.md`): un giudizio troppo
/// severo produrrebbe molti più falsi blocchi che veri, e un gate che dà
/// fastidio a torto viene spento — che è il modo in cui un presidio smette di
/// esistere.
pub fn allowed_search(pattern: &str) -> bool {
    let p = pattern.trim();
    if p.is_empty() {
        return true;
    }
    if looks_like_a_shape(p) {
        return true;
    }
    if is_keyword_phrase(p) {
        return true;
    }
    // Resta la parola singola ambigua (ammessa, sotto osservazione altrove) o
    // la frase senza parola chiave, che è un concetto e basta.
    p.split_whitespace().count() <= 1
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
    fn a_grep_fed_by_find_is_still_a_code_search() {
        // Dopo una pipe, ma non filtra: apre i file che find gli passa.
        assert!(is_code_search("find src -name '*.ts' | xargs grep -l method:"));
        assert!(is_code_search("find src -name '*.ts' | xargs -0 rg method:"));
        assert!(is_code_search("find src -name '*.ts' -exec grep -l method: {} +"));
    }

    #[test]
    fn a_script_that_walks_the_tree_with_regexes_is_a_code_search() {
        assert!(is_code_search(
            "python3 -c \"import re,glob;[re.findall(r'method:',open(f).read()) for f in glob.glob('src/**')]\""
        ));
        assert!(is_code_search("python3 -c \"import re,os;[re.search(p,l) for r,d,fs in os.walk('src')]\""));
    }

    #[test]
    fn reading_or_walking_alone_is_not_a_code_search() {
        // Camminare senza cercare, o cercare in un file solo, non e' un censimento.
        assert!(!is_code_search("python3 -c \"import glob;print(glob.glob('src/**'))\""));
        assert!(!is_code_search("python3 -c \"import re;re.findall(r'x',open('a.ts').read())\""));
        assert!(!is_code_search("sed -n '1,80p' src/api/index.ts"));
        assert!(!is_code_search("find src -name '*.ts' | head -20"));
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
        // Un plugin installato dal marketplace, a versione fissata: preso
        // dal vivo il 21/08/2026 in `claude-code-harness-marketplace`.
        assert!(is_out_of_perimeter(
            "/Users/theo/.claude/plugins/cache/claude-code-harness-marketplace/claude-code-harness/5.2.0"
        ));
    }

    /// Il differenziale sul gate vero, non sulla funzione che lo alimenta.
    ///
    /// PERCHÉ ESISTE: il caso qui sotto dichiarava un mutante su `judge_write`
    /// e non lo poteva prendere, perché non chiama `judge_write` — controlla
    /// solo `is_test_file`. Mutare una funzione e provarne un'altra lascia
    /// verde qualunque cosa. Trovato il 21/08/2026 censendo le mutazioni
    /// dichiarate: una promessa che occupa il posto della prova è peggio di una
    /// prova assente, perché chi legge crede che quella riga sia protetta.
    ///
    /// MUTANTE: togliere `is_test_file` dalla condizione di `judge_write`. Il
    /// file di prova smette di essere fuori perimetro e il gate lo giudica —
    /// questo caso va rosso, e stavolta davvero.
    #[test]
    fn the_gate_itself_lets_a_test_file_through_and_judges_its_sibling() {
        // I PERCORSI SONO FINTI DI PROPOSITO, e non è una scorciatoia: il banco
        // di prova vive sotto la cartella temporanea, che il gate scarta come
        // usa-e-getta *prima* di guardare qualunque altra cosa. Un file vero
        // creato lì uscirebbe fuori perimetro per il motivo sbagliato, e la
        // prova direbbe di sì senza aver provato niente — è il motivo per cui
        // fino al 21/08/2026 nessun caso chiamava questa parte del gate.
        // L'indice si dichiara nello stato, che è l'altra via che il gate legge.
        let (ws, _keep) = workspace();
        std::fs::write(
            ws.home.join(".claude").join("state").join("socraticode-progetti.txt"),
            "/repo\n",
        )
        .unwrap();

        let write_input = |path: &str| hook_io::HookInput {
            session_id: Some("s-write".to_string()),
            cwd: Some("/repo".to_string()),
            tool_name: Some("Write".to_string()),
            tool_input: Some(serde_json::json!({ "file_path": path })),
            ..Default::default()
        };

        // Il file di prova esce dal perimetro: il verdetto non si registra
        // nemmeno, ed è il segno che il gate non se ne è occupato.
        let spared = judge(&ws, &write_input("/repo/src/x.test.ts"));
        assert!(!spared.is_recorded(), "un file di prova sta fuori dal riuso");

        // Il fratello, identico tranne il nome, il gate lo guarda e lo ferma.
        let judged = judge(&ws, &write_input("/repo/src/x.ts"));
        assert!(
            matches!(judged.decision, Decision::Block(_)),
            "un file nuovo in una cartella indicizzata e' di competenza del gate (reason: {:?})",
            judged.reason
        );
    }

    /// Differenziale a variabile unica: due percorsi identici tranne il nome.
    #[test]
    fn a_test_file_leaves_the_reuse_perimeter_and_its_sibling_does_not() {
        // Questo caso prova SOLO come si riconosce il nome di un file di prova.
        // Il mutante su `judge_write` lo prende il caso qui sopra, che il gate
        // lo chiama davvero: qui il commento prometteva una copertura che non
        // c'era.
        assert!(is_test_file("/repo/src/lib/x.test.ts"));
        assert!(is_test_file("/repo/src/lib/x.spec.tsx"));
        assert!(is_test_file("/repo/src/__tests__/x.ts"));
        assert!(is_test_file("/repo/tests/e2e/x.ts"));
        assert!(is_test_file("/repo/test/x.ts"));
        // E ciò che resta dentro: il file provato, e un nome che contiene
        // «test» senza esserlo.
        assert!(!is_test_file("/repo/src/lib/x.ts"));
        assert!(!is_test_file("/repo/src/lib/latest.ts"));
        assert!(!is_test_file("/repo/src/testing-utils.ts"));
    }

    #[test]
    fn it_encodes_the_marker_name_like_the_javascript_did() {
        // node: Buffer.from('/Users/theo/a.ts').toString('base64url').slice(-40)
        assert_eq!(
            base64url_tail("/Users/theo/a.ts", 40),
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

    /// I casi vengono dalle ricerche vere di una sessione del 17/08/2026, non
    /// inventati: e' l'unico modo perche' la soglia fra «forma» e «concetto»
    /// somigli a cio' che si scrive davvero invece che a cio' che si immagina.
    #[test]
    fn a_search_for_a_shape_is_always_allowed() {
        for p in [
            "ORCA_TERMINAL_HANDLE",
            "CONSEGNA_TETTO_SESSIONI",
            "def journal",
            "fn record",
            "socraticode-gate",
            "codebase_search",
            "workspaceStatus",
            "paneKey",
            "crate::shell",
            "orca-cleanup.py",
            r"\bgh\s+pr\s+create",
            "in-review",
            "MERGED",
        ] {
            assert!(allowed_search(p), "{p:?} e' una forma, deve passare");
        }
    }

    /// I due casi che il messaggio del gate prometteva di lasciar passare e che
    /// negava lo stesso. Presi dal vivo il 21/08/2026, in una notte sola.
    #[test]
    fn a_literal_error_string_and_a_code_fragment_pass_as_promised() {
        for p in [
            // Il principio di un messaggio scritto dal gancio della consegna:
            // due parole, tutte maiuscole, e prima si fermava sullo spazio.
            "CONTESTO AL",
            "BLOCCATO (cd-guard)",
            "SocratiCode-first: questa e' una ricerca CONCETTUALE",
            // Frammenti ricopiati da un sorgente: virgolette e freccia.
            "\"code-language\" =>",
            "match self { Some(x)",
            "return 0;",
        ] {
            assert!(allowed_search(p), "{p:?} e' letterale, deve passare");
        }
    }

    /// Quattro dei 108 blocchi della settimana 15-22/08/2026, ripescati dal
    /// «Cercavi:» che il gate stesso stampa nei transcript. Il messaggio
    /// prometteva che un marcatore o un'invocazione letterale passassero
    /// senza pedaggio, e non succedeva: `<<<<<<< HEAD` falliva sia il test
    /// dei metacaratteri (nessuno dei simboli controllati) sia quello «tutto
    /// maiuscolo» (`<` non è né maiuscola né trattino); `git commit` non ha
    /// nessuna parola chiave di linguaggio nell'elenco.
    #[test]
    fn a_conflict_marker_and_a_cli_invocation_pass_as_promised() {
        for p in ["<<<<<<< HEAD", "git commit"] {
            assert!(allowed_search(p), "{p:?} e' letterale, deve passare");
        }
    }

    #[test]
    fn a_search_for_a_concept_pays_the_toll() {
        for p in [
            "duplicazione del codice",
            "gestione degli errori",
            "chiusura sessione",
            "where the handoff is written",
        ] {
            assert!(!allowed_search(p), "{p:?} e' un concetto, deve fermarsi");
        }
    }

    #[test]
    fn a_single_lowercase_word_is_given_the_benefit_of_the_doubt() {
        // Ambiguo per costruzione: `handoff` e' un concetto e anche il nome di
        // un file. Fra i due errori, bloccare a torto costa di piu': un gate che
        // da' fastidio a sproposito viene spento.
        for p in ["handoff", "relay", "throttle", ""] {
            assert!(allowed_search(p), "{p:?} e' ambiguo, si ammette");
        }
    }

    #[test]
    fn the_pattern_is_pulled_out_of_a_shell_search() {
        assert_eq!(grep_pattern("grep -rn \"ORCA_TAB_ID\" src").as_deref(), Some("ORCA_TAB_ID"));
        assert_eq!(grep_pattern("rg --hidden paneKey").as_deref(), Some("paneKey"));
        // MUTANTE: le opzioni non sono il pattern, e prenderle vorrebbe dire
        // giudicare `-rn` invece di cio' che si cerca.
        assert_eq!(grep_pattern("grep -r -i -n handoff .").as_deref(), Some("handoff"));
        // Nessun pattern isolabile: chi chiama tratta None come ammesso.
        assert_eq!(grep_pattern("ls -la"), None);
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
        // Lo stesso percorso che legge `declared_projects_list`.
        let list_path = ws.home.join(".claude").join("state").join("socraticode-progetti.txt");
        std::fs::write(list_path, format!("# commento\n{}\n", repo.display())).unwrap();
        assert!(is_indexed(&ws, &repo.join("src")));
    }

    fn grep_input(cwd: &Path, path: &Path, pattern: &str) -> hook_io::HookInput {
        hook_io::HookInput {
            session_id: Some("s-file".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            tool_name: Some("Grep".to_string()),
            tool_input: Some(serde_json::json!({
                "path": path.to_string_lossy(),
                "pattern": pattern,
            })),
            ..Default::default()
        }
    }

    #[test]
    fn a_conceptual_pattern_against_a_single_file_is_judged() {
        // MUTANTE: rimettere il ramo che usciva con out_of_scope() prima di
        // guardare il pattern quando il bersaglio e' un file esistente — questa
        // prova torna verde solo se il giudizio guarda anche i bersagli-file.
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let file = repo.join("payment.ts");
        std::fs::write(&file, "").unwrap();

        let input = grep_input(&repo, &file, "dove si gestisce il pagamento fallito");
        let verdict = judge_search(&ws, &input);
        assert!(
            matches!(verdict.decision, Decision::Block(_)),
            "un pattern concettuale su un file gia' individuato deve fermarsi, non passare in silenzio"
        );
    }

    #[test]
    fn a_targeted_search_against_a_single_file_still_passes() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let file = repo.join("payment.ts");
        std::fs::write(&file, "").unwrap();

        let input = grep_input(&repo, &file, "handlePaymentFailed");
        let verdict = judge_search(&ws, &input);
        assert!(matches!(verdict.decision, Decision::Pass));
    }

    #[test]
    fn three_real_identifiers_of_this_same_file_all_pass_in_a_row() {
        // Il caso dimostrato dal revisore: handoff, relay, throttle sono tre
        // identificatori veri di QUESTO file, ognuno ammesso da solo. In fila
        // non diventano un concetto solo perche' diversi fra loro.
        // MUTANTE: rimettere la regola delle tre parole ambigue — il terzo
        // giro tornerebbe a bloccare.
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        for word in ["handoff", "relay", "throttle"] {
            let input = grep_input(&repo, &repo, word);
            let verdict = judge_search(&ws, &input);
            assert!(
                matches!(verdict.decision, Decision::Pass),
                "{word:?} e' un identificatore ammesso da solo, deve passare anche in fila"
            );
        }
    }

    fn bash_input(cwd: &Path, command: &str) -> hook_io::HookInput {
        hook_io::HookInput {
            session_id: Some("s-bash".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            tool_name: Some("Bash".to_string()),
            tool_input: Some(serde_json::json!({ "command": command })),
            ..Default::default()
        }
    }

    #[test]
    fn a_quoted_question_in_a_bash_grep_is_read_whole() {
        // Meta' delle ricerche non passa dallo strumento Grep ma da Bash. Li'
        // il pattern veniva spezzato sugli spazi, quindi la frase diventava la
        // sua prima parola — un identificatore, che passa sempre.
        // MUTANTE: rimettere `tail.split_whitespace()` in grep_pattern.
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let cmd = format!(
            "grep -rn \"dove si gestisce il pagamento fallito\" {}",
            repo.display()
        );
        let verdict = judge_search(&ws, &bash_input(&repo, &cmd));
        assert!(
            matches!(verdict.decision, Decision::Block(_)),
            "la domanda via Bash e' sfuggita al giudizio: {:?}",
            verdict.reason
        );
    }

    #[test]
    fn a_quoted_identifier_in_a_bash_grep_still_passes() {
        // Il differenziale della prova sopra, una variabile sola: stesso
        // strumento e stesse virgolette, ma dentro c'e' un identificatore.
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let cmd = format!(
            "grep -rn \"pinnedSections\\|pinned_sections\" {} --include=*.ts -l",
            repo.display()
        );
        let verdict = judge_search(&ws, &bash_input(&repo, &cmd));
        assert!(
            matches!(verdict.decision, Decision::Pass),
            "un identificatore esatto non deve pagare pedaggio: {:?}",
            verdict.reason
        );
    }

    /// `destructive delete`, bloccato il 21/08/2026 mentre cercava dentro
    /// `plugins`, `scripts`, `rust/crates` e `*.sh` — quattro bersagli scritti
    /// a mano. Due o più dicono «so già dove guardare»: passa anche se la
    /// frase, presa da sola, avrebbe l'aria di un concetto.
    /// MUTANTE: abbassare la soglia di `explicit_target_count` a 1 — questo
    /// caso resterebbe verde ma diventerebbe indistinguibile dal prossimo,
    /// che deve restare rosso.
    #[test]
    fn two_or_more_named_targets_skip_the_concept_judgment() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let cmd = format!(
            "grep -rl \"destructive delete\" {}/plugins {}/scripts {}/rust {}/tools 2>/dev/null | head",
            repo.display(), repo.display(), repo.display(), repo.display()
        );
        let verdict = judge_search(&ws, &bash_input(&repo, &cmd));
        assert!(
            matches!(verdict.decision, Decision::Pass),
            "quattro bersagli nominati: chi cerca sa gia' dove, deve passare: {:?}",
            verdict.reason
        );
    }

    /// Il differenziale a soglia: stessa frase, un solo bersaglio. Una
    /// domanda concettuale vera si scrive quasi sempre contro una cartella
    /// sola — è il caso di `a_quoted_question_in_a_bash_grep_is_read_whole` —
    /// quindi un bersaglio solo non può bastare da sé.
    #[test]
    fn a_single_named_target_does_not_reach_the_threshold() {
        assert_eq!(
            explicit_target_count("grep -rl \"destructive delete\" /repo/plugins 2>/dev/null"),
            1
        );
        assert_eq!(
            explicit_target_count(
                "grep -rl \"destructive delete\" /repo/plugins /repo/scripts /repo/rust"
            ),
            3
        );
    }

    /// `touched by other sessions`, bloccato il 21/08/2026 mentre cercava
    /// dentro un plugin installato dal marketplace: il bersaglio non è mai il
    /// dominio di questo workspace, letterale o concettuale che sia il
    /// pattern. Passa perché la cartella è fuori perimetro, prima ancora di
    /// guardare la frase.
    /// MUTANTE: togliere `plugins/cache` da `is_out_of_perimeter` — questo
    /// caso tornerebbe a bloccarsi con `ricerca-concettuale`.
    #[test]
    fn a_search_inside_a_vendored_plugin_is_out_of_scope() {
        let (ws, keep) = workspace();
        let vendored = keep
            .path()
            .join("home")
            .join(".claude")
            .join("plugins")
            .join("cache")
            .join("claude-code-harness-marketplace")
            .join("claude-code-harness")
            .join("5.2.0");
        std::fs::create_dir_all(&vendored).unwrap();
        // Radice indicizzata: senza questa dichiarazione il gate uscirebbe
        // out-of-scope per un altro motivo (non indicizzato) e la prova non
        // proverebbe che e' il perimetro vendorizzato a farlo passare.
        std::fs::write(
            ws.home.join(".claude").join("state").join("socraticode-progetti.txt"),
            format!("{}\n", ws.home.join(".claude").display()),
        )
        .unwrap();
        let cmd = format!(
            "grep -rl \"touched by other sessions\" {} 2>/dev/null | head -3",
            vendored.display()
        );
        let verdict = judge_search(&ws, &bash_input(&vendored, &cmd));
        assert!(
            !verdict.is_recorded(),
            "un plugin vendorizzato e' fuori dal perimetro del gate, non di sua competenza: {:?}",
            verdict.reason
        );
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
                let p = hook_io::testing::test_root()
                    .join(format!("socraticode-gate-prova-{n}"));
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
