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

/// Una competenza o uno script di shell: dal 22/08/2026 la domanda «esiste
/// già?» vale anche qui, perché è qui che si affianca di più. Il censimento del
/// 20/08 contava 45 competenze su 67 mai invocate, e 35 dei 52 file di
/// `scripts/` sono `.sh`, che `is_code_file` non vede. Il manifesto
/// `SKILL.md` è l'unico `.md` giudicato: un documento qualsiasi resta fuori.
pub fn is_skill_or_script(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(/skills/[^/]+/SKILL\.md$|\.(sh|bash|zsh)$)").unwrap())
        .is_match(path)
}

/// `plugins/cache` accanto agli altri: un plugin installato dal marketplace è
/// vendorizzato quanto `node_modules`, a versione fissata. Cercare una
/// stringa letterale lì dentro non è una domanda sul dominio di questo
/// workspace, ed è il motivo per cui `judge_search` lo esclude allo stesso
/// modo (21/08/2026, «touched by other sessions» bloccato dentro
/// `claude-code-harness-marketplace`).
/// I TRANSCRIT E I REGISTRI SONO DATI, NON DOMINIO — dal 24/08/2026. Sotto
/// `~/.claude` l'indice esiste, quindi ogni ricerca a parole dentro
/// `projects/` (le trascrizioni) e `state/` (i registri delle automazioni)
/// veniva negata e mandata a `codebase_search`, che lì non ha niente da
/// rispondere: nessun indice semantico contiene le trascrizioni. Riprodotto
/// sul binario in servizio. Un diniego che rimanda a uno strumento incapace di
/// rispondere non sposta il lavoro nel posto giusto: insegna ad aggirare.
pub fn is_out_of_perimeter(path: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            // La cartella stessa vale quanto ciò che contiene, e per questo la
            // coda ammette la fine della stringa: il bersaglio di un `grep -r`
            // è quasi sempre la cartella nuda, senza barra. `target` è la
            // cartella di build di Cargo (qui sotto ne vive una da 7,3 GB):
            // vendorizzata quanto `node_modules`, e senza di lei
            // `quoted_verbatim_in_target` ci spenderebbe dentro tutto il tetto.
            r"(^|/)(node_modules|dist|build|target|\.next|coverage|\.git|plugins/cache)/|\.claude/(projects|state)(/|$)",
        )
        .unwrap()
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
/// domanda STRUTTURALE («dove vive», «esiste già», «cosa esporta», «chi
/// dipende») l'indice vale anche se è vecchio, perché la forma del codice
/// cambia lentamente; il testo remoto (grep) serve al VALORE DI OGGI — quante
/// occorrenze adesso, quale versione dichiara il manifesto — non a dire dove
/// vive qualcosa.
fn concept_advice() -> &'static str {
    "  ToolSearch query \"select:codebase_search,codebase_symbol,codebase_flow\"\n  codebase_search \"<il dominio a parole>\"   -> dove vive, esiste già\n  codebase_graph_query          -> chi importa questo file (projectPath del REPO)\nDomanda STRUTTURALE (dove vive, cosa esporta, chi dipende): l'indice vale\nanche se è vecchio, perché la forma del codice cambia lentamente. Il grep\nresta lo strumento giusto solo per il VALORE DI OGGI: quante occorrenze\nadesso, quale versione dichiara il manifesto.\nLa domanda si fa nella lingua in cui è scritto ciò che cerchi.\nSe invece cerchi un identificatore ESATTO, una stringa d'errore letterale o\nuna regex, riscrivi il pattern in quella forma: passa senza pedaggio."
}

/// Il messaggio della ricerca concettuale.
///
/// NON è più identico byte per byte al gemello in
/// `scripts/socraticode-gate-v2.js`: qui la sostanza è cambiata (giudizio
/// sulla domanda, non sul conteggio; la distinzione strutturale/valore-di-
/// oggi) e il JS resta fuori dal perimetro di questa modifica — il ripiego,
/// quando il binario manca, parla ancora col testo vecchio finché qualcuno
/// non lo riporta in pari.
///
/// Porta **cosa** cercavi, e non è cosmesi: senza il pattern, chi legge il
/// blocco non sa quale delle sue ricerche è stata giudicata concettuale.
fn message_concept(sought: &str) -> String {
    format!(
        "SocratiCode-first: questa è una ricerca CONCETTUALE in un repo indicizzato.\nCercavi: {sought}\nIl grep conta le occorrenze; non dice se una cosa è viva, chi la importa,\nné dove vive il dominio. Per questa domanda la risposta sta altrove:\n{}",
        concept_advice()
    )
}

fn message_impact(lost: &[String]) -> String {
    format!(
        "SocratiCode-first: questo Edit rinomina o rimuove un simbolo esportato ({}).\n\
         Chi lo importa non è visibile da qui. Prima misura il raggio:\n\
         \x20 - codebase_impact  -> chi rompe questo cambio\n\
         \x20 - codebase_graph_query  -> dipendenze entranti, anche cross-repo\n\
         Poi rilancia lo STESSO Edit: passa (il gate si riarma ogni 10).\n\
         Se il simbolo è interno malgrado l'export (test, barrel locale), rilancia e basta.",
        lost.join(", ")
    )
}

/// L'albero bersaglio è un progetto Rust: c'è un `Cargo.toml` in questa
/// cartella o in una che la contiene.
///
/// SERVE PERCHÉ IL CONSIGLIO CAMBIA. Misurato il 26/08/2026 nel codice del
/// pacchetto: su Rust il grafo delle dipendenze risolve **solo** i `mod x;`
/// nudi e scarta ogni `use …::…`, quindi «chi lo usa» l'indice non lo sa — 36
/// chiamate risolte fra file su 11.003. Un freno che manda all'indice
/// promettendo una risposta che non arriva insegna a ignorare il freno.
///
/// Si risale invece di guardare la sola cartella perché il bersaglio tipico è
/// un sottoalbero (`crates/guards/src`), non la radice del progetto. Otto
/// livelli bastano e chiudono il caso di una radice senza `Cargo.toml`.
fn is_rust_tree(dir: &std::path::Path) -> bool {
    dir.ancestors().take(8).any(|d| d.join("Cargo.toml").is_file())
}

/// Il consiglio dato quando si scandisce un albero cercando il nome di una
/// cosa: la domanda è «dove vive», e l'indice risponde a quella.
fn message_name_lookup(pattern: &str, rust: bool) -> String {
    let ident = pattern.trim().trim_matches(|c| c == '"' || c == '\'');
    // Su Rust la promessa si accorcia a ciò che l'indice mantiene davvero, e la
    // parte che non mantiene si nomina invece di tacerla: chi cerca «chi lo
    // usa» deve sapere che qui il grep è la via giusta, non un ripiego.
    let promise = if rust {
        "dove è definito e cosa fa — in una chiamata. NON chi lo usa: su Rust\n\
         il grafo vede solo i `mod` e per «chi importa» va usato il grep."
    } else {
        "dove è definito, cosa fa e chi lo usa — in una chiamata."
    };
    format!(
        "SocratiCode-first: stai scandendo un albero indicizzato per trovare dove\n\
         vive \"{ident}\". Il grep ti dà le righe che lo nominano; l'indice ti dà\n\
         {promise}\n\
         \x20 ToolSearch \"select:codebase_symbol,codebase_search\"\n\
         \x20 codebase_symbol \"{ident}\"   (projectPath del repo)\n\
         \x20 oppure, dalla shell: claude-hooks indice simbolo {ident}\n\
         Se invece stai CONTANDO le occorrenze di oggi, aggiungi -c: passa.\n\
         Se sai già in quale file guardare, nominalo: un bersaglio preciso passa."
    )
}

/// Il consiglio dato quando il pattern èla sagoma di una definizione:
/// l'indice la risolve in una chiamata, non un pedaggio sul singolo grep.
fn message_definition(pattern: &str) -> String {
    let ident = definition_identifier(pattern).unwrap_or_else(|| pattern.trim().to_string());
    format!(
        "SocratiCode-first: \"{pattern}\" è una ricerca di DEFINIZIONE in un repo\n\
         indicizzato: l'indice la risolve in una chiamata, il grep in tre.\n\
         \x20 ToolSearch \"select:codebase_symbol,codebase_search\"\n\
         \x20 codebase_symbol \"{ident}\"   (projectPath del repo)\n\
         Se vuoi contare le occorrenze di oggi, aggiungi -c o cerca l'identificatore\n\
         nudo: passa."
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
    if file.is_empty() || !(is_code_file(file) || is_skill_or_script(file)) {
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
            // Lo stesso `now_ms` che ha timbrato la traccia: due letture
            // diverse dell'orologio si sarebbero potute scostare fra loro.
            if now_ms().saturating_sub(t) < SEARCH_WINDOW_MS {
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
    // già individuato, e il registro perdeva quale file si stava cercando.
    // Si collassa solo un percorso che non esiste affatto — né `is_indexed`
    // né il giudizio sul pattern dipendono dall'essere file o cartella.
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

    // Ricerca di DEFINIZIONE («dov'è fn judge») in un repo di codice: qui
    // l'identificatore esatto passerebbe sempre sotto `allowed_search`, ma
    // l'indice la risolve in una chiamata contro tre grep. `-c`/`output_mode:
    // count` restano il VALORE DI OGGI (quante occorrenze) e non pagano
    // pedaggio, né un bersaglio fuori da codice (`docs/`, `.md`).
    if is_code_target(&target)
        && !wants_count(tool, input, command)
        && looks_like_a_definition_lookup(&pattern)
    {
        return Verdict {
            decision: Decision::Block(message_definition(&pattern)),
            reason: "ricerca-di-definizione",
            path: Some(target),
            count: None,
        };
    }

    // Scandire un ALBERO indicizzato per il nome di una cosa è la domanda
    // «dove vive», non «quante volte compare».
    //
    // `path.is_dir()` non è un dettaglio, è il confine: chi ha già scelto il
    // file sa dove guardare, e cercare un identificatore lì dentro è leggere,
    // non orientarsi. Senza questa condizione il criterio fermava anche
    // `grep handlePaymentFailed payment.ts` — lo ha preso una prova che
    // esisteva già, ed è il genere di falso blocco per cui un presidio viene
    // spento.
    //
    // Le altre esclusioni sono quelle del caso sopra: un conteggio passa, due
    // bersagli espliciti passano, fuori dal codice passa. In più serve la
    // sagoma di un identificatore vero, perché una parola comune non è il
    // nome di niente.
    if is_code_target(&target)
        && path.is_dir()
        && !wants_count(tool, input, command)
        && looks_like_a_name_lookup(&pattern)
    {
        return Verdict {
            decision: Decision::Block(message_name_lookup(&pattern, is_rust_tree(&path))),
            reason: "ricerca-di-nome",
            path: Some(target),
            count: None,
        };
    }

    // IL GIUDIZIO GUARDA LA DOMANDA, NON UN CONTATORE. Un identificatore
    // esatto, una regex o una stringa d'errore sono lo strumento giusto e
    // passano sempre, senza pedaggio: se il criterio è buono non serve un
    // contatore. Una domanda concettuale parla sempre, non a turno — anche
    // dentro un file solo: èlo stesso censimento spezzato in tanti grep.
    if !allowed_search(&pattern) {
        // Una frase per forma è indistinguibile da un concetto vero —
        // "regge il divieto" e "duplicazione del codice" hanno la stessa
        // sagoma — e solo il contenuto le separa: se il testo esiste già,
        // letterale, in ciò che si sta per cercare, non è una domanda, è
        // una citazione presa da un messaggio già letto altrove.
        if quoted_verbatim_in_target(&pattern, &path) {
            return Verdict::pass("citazione-letterale").with_path(&target);
        }
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
/// Divide una riga di comando in parole, ma tiene insieme ciò che sta fra
/// virgolette.
///
/// Serve perché `split_whitespace()` spezzava
/// `grep -rn "dove si gestisce il pagamento fallito" src` alla prima parola, e
/// `grep_pattern` restituiva `dove`: una domanda concettuale diventava un
/// identificatore e passava. Il difetto non mordeva finché il gate contava le
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

/// Un conteggio (`grep -c`, o `output_mode: "count"` per lo strumento
/// `Grep`) è domanda sul VALORE DI OGGI — quante occorrenze adesso — anche
/// quando il pattern ha la sagoma di una definizione: lì il grep resta lo
/// strumento giusto, l'indice non conta occorrenze.
fn wants_count(tool: &str, input: &hook_io::HookInput, command: &str) -> bool {
    if tool == "Grep" {
        return field(input, "output_mode") == "count";
    }
    let Some(tail) = grep_invocation_tail(command) else {
        return false;
    };
    // Le opzioni possono seguire il pattern (`grep "fn x" -c .`): si
    // scandisce tutta l'invocazione, forma lunga compresa.
    split_respecting_quotes(tail).into_iter().any(|word| {
        word == "--count" || (word.starts_with('-') && !word.starts_with("--") && word[1..].contains('c'))
    })
}

/// Il bersaglio è codice: un file con estensione da codice, o una cartella
/// che contiene uno dei segmenti tipici di un repo (`rust`, `src`, `crates`,
/// `lib`, `app`). Un `grep "fn "` dentro `docs/` o un `.md` resta un grep
/// qualsiasi: li' non c'è una definizione da risolvere nell'indice.
fn is_code_target(target: &str) -> bool {
    if is_code_file(target) {
        return true;
    }
    Path::new(target).components().any(|c| {
        matches!(
            c.as_os_str().to_str(),
            Some("rust" | "src" | "crates" | "lib" | "app")
        )
    })
}

/// La sagoma di una ricerca di DEFINIZIONE: parola chiave del linguaggio,
/// poi — se c'è — l'identificatore. Il nome resta opzionale apposta:
/// `^fn ` da solo enumera ogni funzione di un file, la stessa domanda
/// strutturale con o senza un nome pinnato. Condivisa dal giudizio booleano
/// e dall'estrazione del nome per il messaggio, così non divergono.
fn definition_lookup_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            // Il `\b` dopo la parola chiave: `constructor` e `typescript` sono
            // identificatori nudi, non `const`/`type` con un nome.
            r"^(?:(?:pub(?:\(crate\))?\s+)?(?:fn|struct|enum|trait|type|const|static|mod|impl)\b(?:\s+(?P<a>\w+))?|(?:export\s+)?(?:async\s+)?(?:function|class|interface|def|const|let)\b(?:\s+(?P<b>\w+))?|fn\s+(?P<c>\w+)\s*\()",
        )
        .unwrap()
    })
}

/// Vero se il pattern è una ricerca di DEFINIZIONE: comincia con una parola
/// chiave, non con un identificatore nudo — quello resta un grep legittimo,
/// cerca il valore di oggi. Un'alternativa (`|`) conta solo se OGNI ramo lo
/// è: nel dubbio, falso.
fn looks_like_a_definition_lookup(pattern: &str) -> bool {
    let unquoted = pattern.trim().trim_matches(|c| c == '"' || c == '\'');
    if unquoted.is_empty() {
        return false;
    }
    unquoted.split('|').all(|branch| {
        let b = branch.trim().trim_start_matches('^');
        !b.is_empty() && definition_lookup_regex().is_match(b)
    })
}

/// Il pattern è il nome di una cosa che vive nel codice — e cercarlo **a
/// tappeto** è la domanda «dove vive», non «quante volte compare».
///
/// PERCHÉ SI AGGIUNGE, IL 26/08/2026. Il criterio precedente diceva per
/// iscritto che «un identificatore nudo resta un grep legittimo, cerca il
/// valore di oggi». È vero quando si conta — e infatti i conteggi restano
/// fuori — ma non quando si scandisce un intero albero per trovare dove una
/// funzione è definita e chi la chiama: quella è esattamente la domanda a cui
/// l'indice risponde in una chiamata invece che in tre grep. Detto da Theo lo
/// stesso giorno, dopo aver visto una sessione cercare così per un'intera
/// mattina: «grep crea solo casino quando si tratta di fare codice».
///
/// COSA NON DEVE CADERCI DENTRO, ed è il motivo della forma richiesta. Una
/// parola comune — `TODO`, `error`, `test`, `debug` — non è il nome di
/// niente: si cerca per contare, o per leggere. Si chiede quindi la sagoma di
/// un identificatore vero: `snake_case`, `camelCase`, `PascalCase`, un
/// percorso qualificato. Una parola sola tutta minuscola e senza separatori
/// **non** basta, per quanto sia un nome valido: il costo di sbagliare qui è
/// un blocco a torto, e un presidio che dà fastidio a torto viene spento.
fn looks_like_a_name_lookup(pattern: &str) -> bool {
    let p = pattern.trim().trim_matches(|c| c == '"' || c == '\'');
    if p.is_empty() || p.split_whitespace().count() > 1 {
        return false;
    }
    // Una regex non è un nome: se porta i metacaratteri, chi cerca sa già
    // cosa sta facendo.
    if p.contains(['*', '+', '[', ']', '(', ')', '\\', '^', '$', '?', '{', '}', '|']) {
        return false;
    }
    let ident = |s: &str| {
        !s.is_empty()
            && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
    };
    // Un nome qualificato — `guards::judge`, `notte.decide` — è un nome
    // anche se i suoi pezzi sono parole semplici.
    for sep in ["::", "."] {
        if p.contains(sep) {
            return p.split(sep).all(ident);
        }
    }
    if !ident(p) {
        return false;
    }
    let has_underscore = p.contains('_');
    // TUTTO MAIUSCOLO SENZA SEPARATORI NON È UN NOME: `TODO`, `FIXME`, `XXX`,
    // `HACK` sono marcatori che si cercano per leggerli o contarli. Con un
    // separatore invece è una costante vera — `MAX_TASK_ATTEMPTS` — e quella
    // vive nel codice come qualunque altro simbolo.
    let all_upper = p.chars().all(|c| !c.is_ascii_lowercase());
    if all_upper && !has_underscore {
        return false;
    }
    let has_inner_upper = p.chars().skip(1).any(|c| c.is_ascii_uppercase());
    let starts_upper = p.chars().next().is_some_and(|c| c.is_ascii_uppercase());
    has_underscore || has_inner_upper || starts_upper
}

/// Il nome che segue la parola chiave, per il messaggio del gate. `None`
/// quando il pattern non ne porta uno (`^fn ` da solo): il messaggio ripiega
/// sul pattern intero.
fn definition_identifier(pattern: &str) -> Option<String> {
    let unquoted = pattern.trim().trim_matches(|c| c == '"' || c == '\'');
    let first_branch = unquoted.split('|').next()?;
    let b = first_branch.trim().trim_start_matches('^');
    let caps = definition_lookup_regex().captures(b)?;
    caps.name("a")
        .or_else(|| caps.name("b"))
        .or_else(|| caps.name("c"))
        .map(|m| m.as_str().to_string())
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

// ── La citazione, distinta dal concetto per contenuto e non per forma ──────

/// Quanti file e byte questo controllo apre al massimo prima di arrendersi.
/// Un tetto, non una promessa di completezza: un riscontro mancato PER IL
/// TETTO resta bloccato, non passa — l'esenzione non si allarga mai per un
/// bersaglio che non si è riusciti a leggere tutto.
const CITATION_SCAN_MAX_FILES: usize = 4_000;
const CITATION_SCAN_MAX_BYTES: usize = 16 * 1024 * 1024;
const CITATION_SCAN_MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;

/// Un pattern bloccato da [`allowed_search`] per la sua forma può ancora
/// essere una citazione, non un concetto: "regge il divieto" e
/// "duplicazione del codice" hanno la stessa sagoma — nessuna parola chiave,
/// nessun metacarattere, più di una parola — e la lingua da sola non li
/// separa. Il precedente in casa è `code_language::written_text`, «il testo
/// ricopiato non è testo scritto»: là il confronto è con `old_string`, qui
/// non c'è un prima da diffare — fa da `before` il bersaglio stesso del
/// comando. Se la frase esiste già, letterale, in ciò che si sta per
/// cercare, chi l'ha scritta l'ha vista altrove (un diniego, un commento, un
/// messaggio), non l'ha composta come domanda.
///
/// Non un dizionario in cache come quello di `language.rs`: qui il contenuto
/// cambia file per file, quindi si legge il bersaglio vero a ogni chiamata,
/// col tetto sopra a tenerne basso il costo sul caso patologico (una cartella
/// di build da gigabyte, esclusa comunque da `is_out_of_perimeter`).
fn quoted_verbatim_in_target(pattern: &str, target: &Path) -> bool {
    let needle = pattern.trim();
    if needle.is_empty() {
        return false;
    }
    let mut files_left = CITATION_SCAN_MAX_FILES;
    let mut bytes_left = CITATION_SCAN_MAX_BYTES;
    scan_for_literal(target, needle, &mut files_left, &mut bytes_left)
}

/// Cammina `path` in cerca di `needle`, testo per testo. Un simlink non si
/// segue — eviterebbe un ciclo — e una cartella fuori perimetro
/// (`is_out_of_perimeter`, `target/` di Cargo compreso) non si apre: è lì che
/// vivono i gigabyte che il tetto da solo non basterebbe a rendere innocui in
/// tempo utile.
fn scan_for_literal(path: &Path, needle: &str, files_left: &mut usize, bytes_left: &mut usize) -> bool {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return false;
    };
    if meta.file_type().is_symlink() {
        return false;
    }
    if meta.is_file() {
        if *files_left == 0 || *bytes_left == 0 || meta.len() > CITATION_SCAN_MAX_FILE_BYTES {
            return false;
        }
        *files_left -= 1;
        let Ok(text) = std::fs::read_to_string(path) else {
            return false; // binario, o non UTF-8: non è dove vive una frase
        };
        *bytes_left = bytes_left.saturating_sub(text.len());
        return text.contains(needle);
    }
    if !meta.is_dir() {
        return false;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if is_out_of_perimeter(&p.to_string_lossy()) {
            continue;
        }
        if *files_left == 0 || *bytes_left == 0 {
            return false;
        }
        if scan_for_literal(&p, needle, files_left, bytes_left) {
            return true;
        }
    }
    false
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

// ── La regola, consegnata solo dove l'indice esiste ─────────────────────────

/// La regola prescrive strumenti che senza indice semantico non hanno niente da
/// rispondere, e il prologo la consegnava lo stesso: misurato il 24/08/2026 su
/// un repo non indicizzato, **+8,8% di token e zero usi** dello strumento in
/// tre corse su tre. Il frontmatter `paths:` non sa distinguere i due casi —
/// misurato lo stesso giorno, il confronto vede **solo** il percorso relativo
/// alla cartella di lavoro, quindi `src/x.rs` in un repo indicizzato e in uno
/// qualunque sono la stessa stringa. Da qui il gancio: guarda il percorso vero.
pub const RULE_PATH: &str = ".claude/rules/socraticode-first.md";

/// Il corpo della regola, senza il frontmatter: quei `paths:` parlano
/// all'harness, e in contesto sarebbero solo rumore.
pub fn rule_body(text: &str) -> &str {
    let Some(rest) = text.strip_prefix("---\n") else {
        return text.trim_start();
    };
    match rest.find("\n---\n") {
        Some(end) => rest[end + "\n---\n".len()..].trim_start(),
        None => text.trim_start(),
    }
}

/// La cartella su cui si chiede «qui l'indice c'è?».
///
/// Non è quella da cui è partita la sessione: si lavora di continuo su un repo
/// diverso da quello di partenza — l'84,5% dei tocchi, misurato il 19/08/2026 —
/// ed è il file toccato a dire dove siamo davvero. La cartella di partenza resta
/// il ripiego per gli strumenti che un percorso non ce l'hanno.
pub fn subject_dir(input: &hook_io::HookInput) -> Option<PathBuf> {
    let by_field = |name: &str| {
        let v = field(input, name);
        (!v.is_empty()).then(|| PathBuf::from(v))
    };
    let named = match input.tool_name.as_deref().unwrap_or("") {
        "Edit" | "Write" => by_field("file_path").and_then(|p| p.parent().map(Path::to_path_buf)),
        "Grep" => by_field("path"),
        _ => None,
    };
    named.or_else(|| input.cwd.as_deref().map(PathBuf::from))
}

/// Gli strumenti su cui la regola può ancora cambiare una decisione.
///
/// Un `Bash` qualunque non è fra questi: la regola parla di come si cerca nel
/// codice, e consegnarla al primo `ls` della sessione la brucerebbe prima del
/// momento in cui serve — che è esattamente il difetto per cui la si sposta.
fn rule_is_pertinent(input: &hook_io::HookInput) -> bool {
    match input.tool_name.as_deref().unwrap_or("") {
        "Grep" | "Edit" | "Write" => true,
        "Bash" => is_code_search(input.bash_command()),
        _ => false,
    }
}

/// La regola tocca a questa chiamata? Non tocca niente: chi decide di
/// consegnarla davvero prende il posto con [`claim_rule`].
pub fn rule_is_due(ws: &Workspace, input: &hook_io::HookInput) -> bool {
    rule_is_pertinent(input) && subject_dir(input).is_some_and(|d| is_indexed(ws, &d))
}

/// Prende il posto della sessione, una volta sola.
///
/// `create_new` fallisce se il marcatore esiste già: due chiamate a strumento
/// in parallelo consegnerebbero altrimenti la stessa regola due volte, e la
/// seconda copia costa quanto la prima. Separata da [`rule_is_due`] perché il
/// posto si prende **dopo** aver letto il file: se la regola non si legge, la
/// sessione deve poterci riprovare invece di restare a mani vuote per sempre.
pub fn claim_rule(ws: &Workspace, session: &str) -> bool {
    let marker = ws.tmp.join(format!("claude-socraticode-rule-{session}"));
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(marker)
        .is_ok()
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

    // ── «dove vive» non è «quante volte compare» ───────────────────────
    //
    // Il caso vero: una mattina intera passata a cercare a tappeto i nomi
    // delle funzioni in un albero indicizzato, dove l'indice risponde in una
    // chiamata. Detto da Theo il 26/08/2026.

    #[test]
    fn the_name_of_a_thing_in_the_code_is_a_where_does_it_live_question() {
        assert!(looks_like_a_name_lookup("reason_on_file"));
        assert!(looks_like_a_name_lookup("callGemini"));
        assert!(looks_like_a_name_lookup("WatchThresholds"));
        assert!(looks_like_a_name_lookup("guards::judge"));
        assert!(looks_like_a_name_lookup("notte.decide"));
        assert!(looks_like_a_name_lookup("\"resolve_bin\""));
    }

    /// Il confine che tiene in piedi il criterio: una parola comune non è il
    /// nome di niente, e chi la cerca sta contando o leggendo. Se questi
    /// cadessero, il presidio darebbe fastidio a torto — ed è così che un
    /// presidio smette di esistere.
    #[test]
    fn a_common_word_is_not_the_name_of_anything() {
        for word in [
            "TODO", "FIXME", "XXX", "HACK", "NOTE", "error", "test", "debug", "fixme", "warning",
        ] {
            assert!(
                !looks_like_a_name_lookup(word),
                "«{word}» non deve essere preso per un nome"
            );
        }
        // Il contro-caso, senza il quale la prova sopra si accontenterebbe di un
        // criterio che nega tutto: una costante vera porta il separatore, e resta
        // un nome che vive nel codice.
        assert!(looks_like_a_name_lookup("MAX_TASK_ATTEMPTS"));
        assert!(looks_like_a_name_lookup("WatchThresholds"));
    }

    /// Una regex non è un nome: chi porta i metacaratteri sa già cosa cerca.
    #[test]
    fn a_regular_expression_is_never_a_name() {
        assert!(!looks_like_a_name_lookup("fn .*_bin"));
        assert!(!looks_like_a_name_lookup("^pub fn"));
        assert!(!looks_like_a_name_lookup("errore|timeout"));
        assert!(!looks_like_a_name_lookup("call_[a-z]+"));
    }

    /// Più parole sono una frase, e una frase la giudica il criterio sulle
    /// ricerche concettuali, non questo.
    #[test]
    fn a_phrase_is_judged_elsewhere() {
        assert!(!looks_like_a_name_lookup("dove si decide la notte"));
        assert!(!looks_like_a_name_lookup("not found on PATH"));
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
        // Camminare senza cercare, o cercare in un file solo, non è un censimento.
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
            "un file nuovo in una cartella indicizzata è di competenza del gate (reason: {:?})",
            judged.reason
        );
    }

    /// MUTANTE: togliere `is_skill_or_script` dalla condizione di `judge_write`,
    /// o restringere la regex a `SKILL.md` solo. Il manifesto e lo script
    /// tornano fuori perimetro e i due blocchi sotto diventano silenzio.
    #[test]
    fn a_new_skill_manifest_and_a_shell_script_are_judged_a_plain_document_is_not() {
        let (ws, _keep) = workspace();
        std::fs::write(
            ws.home.join(".claude").join("state").join("socraticode-progetti.txt"),
            "/repo\n",
        )
        .unwrap();
        let write_input = |path: &str| hook_io::HookInput {
            session_id: Some("s-skill".to_string()),
            cwd: Some("/repo".to_string()),
            tool_name: Some("Write".to_string()),
            tool_input: Some(serde_json::json!({ "file_path": path })),
            ..Default::default()
        };

        for path in ["/repo/skills/nuova/SKILL.md", "/repo/scripts/nuovo.sh"] {
            let v = judge(&ws, &write_input(path));
            assert!(
                matches!(v.decision, Decision::Block(_)),
                "{path} è di competenza del gate (reason: {:?})",
                v.reason
            );
        }
        let doc = judge(&ws, &write_input("/repo/docs/nota.md"));
        assert!(!doc.is_recorded(), "un documento qualsiasi resta fuori");
        let nested = judge(&ws, &write_input("/repo/skills/nuova/README.md"));
        assert!(!nested.is_recorded(), "solo il manifesto conta, non ogni .md della skill");
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
    /// inventati: èl'unico modo perché la soglia fra «forma» e «concetto»
    /// somigli a ciò che si scrive davvero invece che a ciò che si immagina.
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
            assert!(allowed_search(p), "{p:?} è una forma, deve passare");
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
            "SocratiCode-first: questa è una ricerca CONCETTUALE",
            // Frammenti ricopiati da un sorgente: virgolette e freccia.
            "\"code-language\" =>",
            "match self { Some(x)",
            "return 0;",
        ] {
            assert!(allowed_search(p), "{p:?} è letterale, deve passare");
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
            assert!(allowed_search(p), "{p:?} è letterale, deve passare");
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
            assert!(!allowed_search(p), "{p:?} è un concetto, deve fermarsi");
        }
    }

    /// I due comandi negati due volte di fila il 25/08/2026: una frase presa
    /// **verbatim** dal diniego di un altro gancio (`linear_readonly`), non
    /// composta come domanda. `allowed_search` da sola le blocca ancora
    /// (prova sopra): è `judge_search`, con il bersaglio vero sotto mano, a
    /// dover distinguere la citazione dal concetto.
    /// MUTANTE: far tornare sempre `false` `quoted_verbatim_in_target` —
    /// questi due casi tornerebbero rossi.
    #[test]
    fn a_phrase_copied_verbatim_from_another_gates_denial_passes() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        std::fs::write(
            repo.join("linear_readonly.rs"),
            "// Questo file regge il divieto di scrittura su Linear, e non si \
             modifica dall'interno di una sessione: una valvola che autorizza \
             il proprio smontaggio non è ammessa.",
        )
        .unwrap();

        for pattern in ["una valvola che autorizza il proprio smontaggio", "regge il divieto"] {
            let cmd = format!("grep -rln \"{pattern}\" {}", repo.display());
            let verdict = judge_search(&ws, &bash_input(&repo, &cmd));
            assert!(
                matches!(verdict.decision, Decision::Pass),
                "{pattern:?} esiste già nel bersaglio, è una citazione: {:?}",
                verdict.reason
            );
        }
    }

    /// Il differenziale che tiene la porta stretta: stessa sagoma (nessuna
    /// parola chiave, nessun metacarattere, più di una parola), ma il testo
    /// NON esiste da nessuna parte nel bersaglio — è una domanda vera, non
    /// una citazione, e deve restare bloccata anche dopo l'esenzione.
    /// MUTANTE: far tornare sempre `true` `quoted_verbatim_in_target` —
    /// questo caso passerebbe, e il gate smetterebbe di fermare i concetti.
    #[test]
    fn a_genuine_concept_absent_from_the_target_still_pays_the_toll() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        std::fs::write(repo.join("x.rs"), "fn main() {}").unwrap();

        let cmd = format!("grep -rln \"duplicazione del codice\" {}", repo.display());
        let verdict = judge_search(&ws, &bash_input(&repo, &cmd));
        assert!(
            matches!(verdict.decision, Decision::Block(_)),
            "la frase non esiste nel bersaglio, è un concetto vero: {:?}",
            verdict.reason
        );
    }

    /// Il tetto non allarga mai l'esenzione: un file più grande del limite
    /// per-file resta illeggibile a questo controllo, e la citazione dentro
    /// non si trova — sbaglia dalla parte prudente, non passa in silenzio.
    /// MUTANTE: rimuovere il confronto con `CITATION_SCAN_MAX_FILE_BYTES` —
    /// questo caso tornerebbe verde e il tetto smetterebbe di valere.
    #[test]
    fn a_file_over_the_per_file_cap_is_not_read() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let filler = "x".repeat(CITATION_SCAN_MAX_FILE_BYTES as usize + 1);
        std::fs::write(repo.join("enorme.rs"), format!("{filler} regge il divieto")).unwrap();

        let cmd = format!("grep -rln \"regge il divieto\" {}", repo.display());
        let verdict = judge_search(&ws, &bash_input(&repo, &cmd));
        assert!(
            matches!(verdict.decision, Decision::Block(_)),
            "il file supera il tetto per-file, non va letto: {:?}",
            verdict.reason
        );
    }

    #[test]
    fn a_single_lowercase_word_is_given_the_benefit_of_the_doubt() {
        // Ambiguo per costruzione: `handoff` è un concetto e anche il nome di
        // un file. Fra i due errori, bloccare a torto costa di più: un gate che
        // da' fastidio a sproposito viene spento.
        for p in ["handoff", "relay", "throttle", ""] {
            assert!(allowed_search(p), "{p:?} è ambiguo, si ammette");
        }
    }

    #[test]
    fn the_pattern_is_pulled_out_of_a_shell_search() {
        assert_eq!(grep_pattern("grep -rn \"ORCA_TAB_ID\" src").as_deref(), Some("ORCA_TAB_ID"));
        assert_eq!(grep_pattern("rg --hidden paneKey").as_deref(), Some("paneKey"));
        // MUTANTE: le opzioni non sono il pattern, e prenderle vorrebbe dire
        // giudicare `-rn` invece di ciò che si cerca.
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
        // guardare il pattern quando il bersaglio è un file esistente — questa
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
            "un pattern concettuale su un file già individuato deve fermarsi, non passare in silenzio"
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
        // non diventano un concetto solo perché diversi fra loro.
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
                "{word:?} è un identificatore ammesso da solo, deve passare anche in fila"
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
            "la domanda via Bash è sfuggita al giudizio: {:?}",
            verdict.reason
        );
    }

    #[test]
    fn a_quoted_identifier_in_a_bash_grep_still_passes() {
        // Il differenziale della prova sopra, una variabile sola: stesso
        // strumento e stesse virgolette, ma dentro c'è un identificatore.
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
            "quattro bersagli nominati: chi cerca sa già dove, deve passare: {:?}",
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
        // proverebbe che èil perimetro vendorizzato a farlo passare.
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
            "un plugin vendorizzato è fuori dal perimetro del gate, non di sua competenza: {:?}",
            verdict.reason
        );
    }

    /// (1) definizione con nome esatto: l'indice la risolve in una chiamata.
    /// MUTANTE (rotto così → rosso): far tornare `false` sempre da
    /// `looks_like_a_definition_lookup` — il blocco sparisce e la ricerca passa.
    #[test]
    fn a_definition_lookup_with_an_exact_name_is_blocked() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("crates").join("guards").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        let input = grep_input(&src, &src, "pub fn judge");
        let verdict = judge_search(&ws, &input);
        match verdict.decision {
            Decision::Block(msg) => assert!(
                msg.contains("codebase_symbol \"judge\""),
                "il messaggio deve dare l'identificatore da cercare: {msg}"
            ),
            _ => panic!("una ricerca di definizione con nome esatto deve fermarsi"),
        }
        assert_eq!(verdict.reason, "ricerca-di-definizione");
    }

    /// (2) l'identificatore nudo resta un grep legittimo: cerca il valore di
    /// oggi, non dov'è la definizione.
    /// MUTANTE (rotto così → rosso): rendere `looks_like_a_definition_lookup`
    /// vera per qualunque parola — bloccherebbe anche `judge` da solo.
    #[test]
    fn a_bare_identifier_is_not_a_definition_lookup() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("crates").join("guards").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        let input = grep_input(&src, &src, "judge");
        let verdict = judge_search(&ws, &input);
        assert!(matches!(verdict.decision, Decision::Pass));
    }

    /// (3) `-c` conta le occorrenze di oggi: anche una sagoma di definizione
    /// passa, la domanda non è"dove vive" ma "quante volte c'è adesso".
    /// MUTANTE (rotto così → rosso): togliere `!wants_count(...)` dal
    /// giudizio — la stessa ricerca finirebbe bloccata.
    #[test]
    fn a_count_flag_bypasses_the_definition_block() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        let verdict = judge_search(&ws, &bash_input(&src, "grep -rc \"fn judge\" ."));
        assert!(
            matches!(verdict.decision, Decision::Pass),
            "-c conta il valore di oggi, non deve bloccarsi: {:?}",
            verdict.reason
        );
    }

    /// (3b) `-c` dopo il pattern e la forma lunga `--count` valgono quanto
    /// `-rc` davanti: il messaggio promette «aggiungi -c … passa».
    /// MUTANTE (rotto così → rosso): in `wants_count` fermarsi al primo
    /// token che non comincia per `-`, o escludere `--count`.
    #[test]
    fn a_count_flag_after_the_pattern_bypasses_the_definition_block() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        for cmd in ["grep -r \"fn judge\" -c .", "grep --count \"fn judge\" ."] {
            let verdict = judge_search(&ws, &bash_input(&src, cmd));
            assert!(
                matches!(verdict.decision, Decision::Pass),
                "{cmd}: conta il valore di oggi, non deve bloccarsi: {:?}",
                verdict.reason
            );
        }
    }

    /// (2b) un identificatore che COMINCIA per una parola chiave
    /// (`constructor`, `typescript`, `structured_logging`) resta nudo: la
    /// sagoma vuole la parola intera, non il prefisso.
    /// MUTANTE (rotto così → rosso): togliere i `\b` da
    /// `definition_lookup_regex`.
    #[test]
    fn a_keyword_prefixed_identifier_is_not_a_definition_lookup() {
        for id in ["constructor", "typescript", "structured_logging", "letters", "implementation"] {
            assert!(!looks_like_a_definition_lookup(id), "{id} è un identificatore nudo");
        }
        assert!(looks_like_a_definition_lookup("const FOO"));
        assert!(looks_like_a_definition_lookup("^pub fn"));
    }

    /// (4) fuori da un bersaglio di codice (`docs/`, `.md`) non c'è una
    /// definizione da risolvere nell'indice: il grep resta quello giusto.
    /// MUTANTE (rotto così → rosso): togliere `is_code_target(...)` dal
    /// giudizio — un `fn ` dentro `docs/` si bloccherebbe comunque.
    #[test]
    fn a_definition_shaped_pattern_outside_code_passes() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let docs = repo.join("docs");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let note = docs.join("note.md");
        std::fs::write(&note, "").unwrap();

        let input = grep_input(&docs, &note, "fn judge");
        let verdict = judge_search(&ws, &input);
        assert!(matches!(verdict.decision, Decision::Pass));
    }

    /// Una domanda a parole dentro le trascrizioni non ha un indice a cui
    /// rivolgersi: lì il `grep` è lo strumento, non il ripiego. Riprodotto il
    /// 24/08/2026 sul binario in servizio.
    /// MUTANTE (rotto così → rosso): togliere `projects|state` da
    /// `is_out_of_perimeter` — il diniego torna, e manda a `codebase_search`,
    /// che nelle trascrizioni non cerca.
    #[test]
    fn a_question_asked_inside_the_transcripts_is_not_sent_to_the_index() {
        for dove in [
            "/Users/theo/.claude/projects",
            "/Users/theo/.claude/projects/-Users-theo-orca-general/x.jsonl",
            "/Users/theo/.claude/state/staffetta.log",
        ] {
            assert!(is_out_of_perimeter(dove), "{dove}");
        }
        // Il controllo negativo, con la stessa radice: il codice della
        // configurazione resta dentro il perimetro, o la restrizione avrebbe
        // spento il gate proprio dove serve.
        for dove in [
            "/Users/theo/.claude/rust/crates/guards/src",
            "/Users/theo/.claude/scripts/queue-patrol.sh",
            "/Users/theo/.claude/docs",
        ] {
            assert!(!is_out_of_perimeter(dove), "{dove}");
        }
    }

    /// (5) due sagome di definizione in alternativa, senza nome pinnato:
    /// `^fn ` da solo enumera ogni funzione, la stessa domanda strutturale.
    /// MUTANTE (rotto così → rosso): richiedere sempre un identificatore
    /// dopo la parola chiave — questo pattern non ne porta e passerebbe.
    #[test]
    fn a_bare_keyword_alternation_is_still_a_definition_lookup() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        let input = grep_input(&src, &src, "^pub fn|^fn ");
        let verdict = judge_search(&ws, &input);
        assert!(
            matches!(verdict.decision, Decision::Block(_)),
            "due sagome di definizione in alternativa devono fermarsi: {:?}",
            verdict.reason
        );
    }

    /// (6) un ramo dell'alternativa non è una definizione (`TODO`): basta
    /// un ramo qualunque perché non sia più "ogni ramo è una definizione".
    /// MUTANTE (rotto così → rosso): bastare UN ramo invece di TUTTI (`.any`
    /// al posto di `.all`) — questo pattern si bloccherebbe.
    #[test]
    fn one_non_definition_branch_lets_the_alternation_pass() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        let input = grep_input(&src, &src, "fn judge\\|TODO");
        let verdict = judge_search(&ws, &input);
        assert!(
            matches!(verdict.decision, Decision::Pass),
            "un ramo non-definizione ammette l'intera alternativa: {:?}",
            verdict.reason
        );
    }

    // ── La porta pubblica ───────────────────────────────────────────────────
    //
    // I casi qui sopra entrano quasi tutti da `judge_search` o da `judge_edit`.
    // Chiamare la funzione dietro la porta lascia la porta stessa senza
    // sorveglianza: cancellando due rami del `match` di `judge`, il gate non
    // giudica più né una modifica né una ricerca — cadono tutte in
    // `out_of_scope()` — e la batteria resta verde. Da qui in giù si entra da
    // `judge`.

    /// MUTANTE: cancellare il ramo `"Edit"` di `judge`, o togliere un `!` dalle
    /// due condizioni d'uscita di `judge_edit`.
    #[test]
    fn an_edit_that_drops_an_export_is_judged_through_the_public_door() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        let edit = hook_io::HookInput {
            session_id: Some("s-porta-edit".to_string()),
            cwd: Some(repo.to_string_lossy().into_owned()),
            tool_name: Some("Edit".to_string()),
            tool_input: Some(serde_json::json!({
                "file_path": src.join("index.ts").to_string_lossy(),
                "old_string": "export const formatDate = 1;",
                "new_string": "export const formatDay = 1;",
            })),
            ..Default::default()
        };

        // Il contatore parte a metà quota: i primi passaggi si registrano come
        // «sotto quota», ed è già il segno che il gate se ne è occupato.
        for giro in 0..5 {
            let verdict = judge(&ws, &edit);
            assert!(
                verdict.is_recorded(),
                "giro {giro}: un Edit che perde un export è di competenza del gate"
            );
            assert_eq!(verdict.reason, "sotto-quota-impact");
        }
        match judge(&ws, &edit).decision {
            Decision::Block(m) => assert!(
                m.contains("formatDate"),
                "il messaggio deve dire quale simbolo sparisce: {m}"
            ),
            other => panic!("a quota il gate blocca, non {other:?}"),
        }
    }

    /// MUTANTE: cancellare il ramo `"Grep" | "Bash"` di `judge`, o svuotare i
    /// due messaggi che il blocco stampa.
    #[test]
    fn a_conceptual_search_is_judged_through_the_public_door_from_both_tools() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();
        let question = "dove si gestisce il pagamento fallito";

        for input in [
            grep_input(&repo, &repo, question),
            bash_input(&repo, &format!("grep -rn \"{question}\" {}", repo.display())),
        ] {
            let verdict = judge(&ws, &input);
            assert_eq!(verdict.reason, "ricerca-concettuale");
            match verdict.decision {
                Decision::Block(m) => {
                    assert!(
                        m.contains(&format!("Cercavi: {question}")),
                        "il blocco deve dire quale ricerca è stata giudicata: {m}"
                    );
                    assert!(
                        m.contains("codebase_search"),
                        "e quale strumento usare al posto del grep: {m}"
                    );
                }
                other => panic!("una domanda concettuale si ferma, non {other:?}"),
            }
        }
    }

    /// La traccia che il caso del riuso legge la lascia OGNI ricerca semantica,
    /// non solo la prima delle quattro, e nasce col timbro di adesso: una
    /// traccia che nascesse scaduta chiuderebbe per sempre la deroga «ho appena
    /// cercato», cioè bloccherebbe proprio chi ha fatto la cosa giusta.
    #[test]
    fn every_semantic_search_tool_leaves_a_fresh_trace() {
        let (ws, _keep) = workspace();
        for suffix in [
            "codebase_search",
            "codebase_context_search",
            "codebase_symbol",
            "codebase_flow",
        ] {
            let session = format!("s-traccia-{suffix}");
            let verdict = judge(
                &ws,
                &hook_io::HookInput {
                    session_id: Some(session.clone()),
                    // Il nome porta il prefisso del plugin: si riconosce dal suffisso.
                    tool_name: Some(format!("mcp__plugin_socraticode__{suffix}")),
                    ..Default::default()
                },
            );
            assert!(
                !verdict.is_recorded(),
                "{suffix}: la ricerca semantica non è un caso di competenza del gate"
            );
            let trace = search_trace(&ws, &session);
            let stamped: u128 = std::fs::read_to_string(&trace)
                .unwrap_or_else(|e| panic!("{suffix} deve lasciare la traccia: {e}"))
                .trim()
                .parse()
                .expect("la traccia porta un istante");
            assert!(
                stamped > 0 && now_ms().saturating_sub(stamped) < SEARCH_WINDOW_MS,
                "{suffix}: traccia nata già scaduta ({stamped})"
            );
        }
    }

    /// I due bordi della finestra: cinque minuti fa vale, venticinque no.
    /// Senza tutti e due, la durata non è sorvegliata da niente.
    #[test]
    fn the_search_trace_is_worth_twenty_minutes() {
        let (ws, _keep) = workspace();
        // Percorsi finti e radice dichiarata nello stato: sotto la cartella
        // temporanea il gate scarterebbe il file come usa-e-getta prima di
        // guardare la traccia, e la prova direbbe di sì senza aver provato.
        std::fs::write(
            ws.home.join(".claude").join("state").join("socraticode-progetti.txt"),
            "/Users/theo/repo-dichiarato-per-prova\n",
        )
        .unwrap();

        let write_input = |session: &str| hook_io::HookInput {
            session_id: Some(session.to_string()),
            cwd: Some("/Users/theo/repo-dichiarato-per-prova".to_string()),
            tool_name: Some("Write".to_string()),
            tool_input: Some(serde_json::json!({
                "file_path": "/Users/theo/repo-dichiarato-per-prova/src/nuovo.ts",
            })),
            ..Default::default()
        };
        let stamp = |session: &str, ago_ms: u128| {
            std::fs::write(
                search_trace(&ws, session),
                now_ms().saturating_sub(ago_ms).to_string(),
            )
            .expect("la traccia si scrive dove il gate la cerca");
        };

        stamp("s-fresca", 5 * 60 * 1000);
        assert_eq!(
            judge(&ws, &write_input("s-fresca")).reason,
            "ricerca-recente",
            "cinque minuti stanno dentro la finestra"
        );

        stamp("s-vecchia", 25 * 60 * 1000);
        let stale = judge(&ws, &write_input("s-vecchia"));
        assert!(
            matches!(stale.decision, Decision::Block(_)),
            "venticinque minuti sono fuori dalla finestra: {:?}",
            stale.reason
        );
        match stale.decision {
            Decision::Block(m) => assert!(
                m.contains("nuovo.ts") && m.contains("codebase_search"),
                "il blocco deve dire quale file e cosa fare: {m}"
            ),
            _ => unreachable!(),
        }
    }

    /// Una ricerca da `Bash` si giudica DOVE PUNTA, non dove gira.
    ///
    /// I percorsi sono finti di proposito, come nel caso del riuso: la radice
    /// indicizzata si dichiara nello stato, e `/Users/…` è l'unica forma che
    /// `first_absolute_path` riconosce — una cartella temporanea non lo è.
    #[test]
    fn a_bash_search_is_judged_where_it_points_not_where_it_runs() {
        let (ws, keep) = workspace();
        let altrove = keep.path().join("altrove");
        std::fs::create_dir_all(&altrove).unwrap();
        std::fs::write(
            ws.home.join(".claude").join("state").join("socraticode-progetti.txt"),
            "/Users/theo/repo-dichiarato-per-prova\n",
        )
        .unwrap();

        let verdict = judge(
            &ws,
            &bash_input(
                &altrove,
                "grep -rn \"dove si gestisce il pagamento fallito\" \
                 /Users/theo/repo-dichiarato-per-prova/src",
            ),
        );
        assert!(
            matches!(verdict.decision, Decision::Block(_)),
            "il bersaglio scritto nel comando è indicizzato, il cwd no: {:?}",
            verdict.reason
        );
    }

    /// Il `path` dello strumento `Grep` può essere relativo: si aggancia al
    /// `cwd`, o il bersaglio giudicato non è quello che l'agente ha in mano.
    #[test]
    fn a_relative_grep_target_is_anchored_to_the_working_directory() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        let input = grep_input(
            &repo,
            Path::new("src"),
            "dove si gestisce il pagamento fallito",
        );
        let verdict = judge(&ws, &input);
        assert!(
            matches!(verdict.decision, Decision::Block(_)),
            "`src` sotto un cwd indicizzato è indicizzato: {:?}",
            verdict.reason
        );
    }

    /// Un file usa-e-getta non è riuso di codice nemmeno dentro una radice
    /// dichiarata: lo stato e gli scratchpad restano fuori da soli, non perché
    /// per caso nessuno li indicizza.
    #[test]
    fn a_throwaway_file_inside_a_declared_root_stays_out_of_the_reuse_case() {
        let (ws, _keep) = workspace();
        std::fs::write(
            ws.home.join(".claude").join("state").join("socraticode-progetti.txt"),
            "/Users/theo/.claude\n",
        )
        .unwrap();
        let write_input = |path: &str| hook_io::HookInput {
            session_id: Some("s-usa-e-getta".to_string()),
            cwd: Some("/Users/theo/.claude".to_string()),
            tool_name: Some("Write".to_string()),
            tool_input: Some(serde_json::json!({ "file_path": path })),
            ..Default::default()
        };
        for path in [
            "/Users/theo/.claude/state/nuovo.ts",
            "/Users/theo/.claude/scratchpad/nuovo.ts",
        ] {
            assert!(
                !judge(&ws, &write_input(path)).is_recorded(),
                "{path} è usa-e-getta, il gate non se ne occupa"
            );
        }
        // Il fratello dentro la stessa radice, invece, il gate lo guarda.
        assert!(matches!(
            judge(&ws, &write_input("/Users/theo/.claude/rust/nuovo.ts")).decision,
            Decision::Block(_)
        ));
    }

    /// Una ricerca di definizione da `Bash` senza `-c` si ferma. La deroga è il
    /// conteggio, e si riconosce dalle OPZIONI: un bersaglio che per caso
    /// contiene una `c` — `src` — non è una richiesta di contare.
    #[test]
    fn a_bash_definition_lookup_without_a_count_flag_is_blocked() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        assert_eq!(
            judge(&ws, &bash_input(&src, "grep -rn \"fn judge\" src")).reason,
            "ricerca-di-definizione"
        );
    }

    /// Il pattern arriva spesso ancora fra virgolette: si leggono via prima di
    /// giudicare, e di nuovo prima di estrarre il nome per il messaggio.
    #[test]
    fn a_quoted_definition_pattern_is_read_without_its_quotes() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("repo");
        let src = repo.join("crates").join("guards").join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(repo.join(".socraticodeignore"), "").unwrap();

        match judge(&ws, &grep_input(&src, &src, "\"pub fn judge\"")).decision {
            Decision::Block(m) => assert!(
                m.contains("codebase_symbol \"judge\""),
                "il nome si estrae dal pattern senza le virgolette: {m}"
            ),
            other => panic!("una ricerca di definizione si ferma, non {other:?}"),
        }
    }

    /// L'elenco dichiarato: una riga vuota non è una radice.
    ///
    /// Letta come radice renderebbe indicizzato TUTTO — ogni percorso «comincia
    /// con» la stringa vuota — e il gate parlerebbe ovunque, anche dove la
    /// ricerca semantica non ha niente da rispondere.
    #[test]
    fn a_blank_line_is_not_a_root_and_what_is_outside_stays_outside() {
        let (ws, keep) = workspace();
        let repo = keep.path().join("dichiarato");
        let altrove = keep.path().join("altrove");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::create_dir_all(&altrove).unwrap();
        std::fs::write(
            ws.home.join(".claude").join("state").join("socraticode-progetti.txt"),
            format!("# commento\n\n   \n{}\n", repo.display()),
        )
        .unwrap();

        assert_eq!(declared_projects_list(&ws.home), vec![repo.clone()]);
        assert!(is_indexed(&ws, &repo.join("src")));
        assert!(
            !is_indexed(&ws, &altrove),
            "fuori dalle radici dichiarate il grep resta il ripiego giusto"
        );
    }

    /// Due salti di worktree, non tre: oltre il limite si torna al grep invece
    /// di risalire una catena che in questa casa non esiste.
    #[test]
    fn the_worktree_walk_up_stops_after_two_hops() {
        let (ws, keep) = workspace();
        let canonical = keep.path().join("canonico");
        std::fs::create_dir_all(canonical.join(".git")).unwrap();
        std::fs::write(canonical.join(".socraticodeignore"), "").unwrap();

        // copia3 → canonico, copia2 → copia3, copia1 → copia2
        let mut previous = canonical;
        let mut chain = Vec::new();
        for name in ["copia3", "copia2", "copia1"] {
            let dir = keep.path().join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(
                dir.join(".git"),
                format!("gitdir: {}/.git/worktrees/{name}\n", previous.display()),
            )
            .unwrap();
            previous = dir.clone();
            chain.push(dir);
        }
        assert!(is_indexed(&ws, &chain[0]), "un salto");
        assert!(is_indexed(&ws, &chain[1]), "due salti");
        assert!(!is_indexed(&ws, &chain[2]), "tre salti sono oltre il limite");
    }

    /// Una regex passa senza pedaggio, e OGNI metacarattere vale da solo.
    ///
    /// Ogni frase qui sotto ne porta uno, e per il resto è una domanda di
    /// dominio in minuscolo: se il riconoscimento di quel metacarattere smette
    /// di valere per conto suo, la frase torna a essere un concetto e il gate
    /// blocca chi stava scrivendo una regex.
    #[test]
    fn one_metacharacter_is_enough_to_make_a_pattern_a_shape() {
        for p in [
            "chiusura \\n rimandata",
            "sessione [aperta] ancora",
            "stato (aperto) ancora",
            "stato aperto|chiuso qui",
            "^stato aperto",
            "costo in $ al giorno",
            "riga.*seguente qui",
            "conto + spesa",
            "meno < di ieri",
            "piu > di ieri",
            "stato = aperto",
            "numero # della sessione",
        ] {
            assert!(allowed_search(p), "{p:?} porta un metacarattere, deve passare");
        }
    }

    /// Le forme che non sono metacaratteri, una per condizione: il trattino
    /// basso, il percorso Rust, il punto, il camelCase, e la stringa d'errore
    /// tutta maiuscola con un trattino o una cifra in mezzo.
    #[test]
    fn the_other_shapes_hold_on_their_own() {
        for p in [
            "dove sta il campo session_id",
            "dove sta crate::shell adesso",
            "dove sta orca.py adesso",
            "dove sta workspaceStatus adesso",
            "TOKEN-BUDGET SUPERATO",
            "ERRORE 500 SU LOGIN",
        ] {
            assert!(allowed_search(p), "{p:?} è una forma, deve passare");
        }
    }

    /// Un frammento ricopiato da un sorgente passa sulla sola punteggiatura,
    /// senza nessun metacarattere: virgolette, apici, graffa, punto e virgola.
    #[test]
    fn a_copied_fragment_passes_on_its_punctuation_alone() {
        for p in [
            "dove sta \"il conto\" delle sessioni",
            "dove sta 'il conto' delle sessioni",
            "dove sta { il conto",
            "dove sta il conto; poi",
        ] {
            assert!(
                allowed_search(p),
                "{p:?} è un frammento ricopiato, deve passare"
            );
        }
    }

    // ── La regola consegnata solo dove l'indice esiste ──────────────────────

    /// Dichiara una radice indicizzata e restituisce il percorso di un file di
    /// codice dentro. Passa dall'elenco su disco, che è la fonte vera.
    fn indexed_repo(ws: &Workspace, keep: &tempdir::TempDir, name: &str) -> PathBuf {
        let repo = keep.path().join(name);
        std::fs::create_dir_all(repo.join("src")).unwrap();
        let list = ws
            .home
            .join(".claude")
            .join("state")
            .join("socraticode-progetti.txt");
        std::fs::write(list, format!("{}\n", repo.display())).unwrap();
        repo
    }

    fn write_input(cwd: &Path, file: &Path) -> hook_io::HookInput {
        hook_io::HookInput {
            session_id: Some("s-regola".to_string()),
            cwd: Some(cwd.to_string_lossy().into_owned()),
            tool_name: Some("Write".to_string()),
            tool_input: Some(serde_json::json!({ "file_path": file.to_string_lossy() })),
            ..Default::default()
        }
    }

    #[test]
    fn the_rule_is_due_inside_an_indexed_repo_and_nowhere_else() {
        let (ws, keep) = workspace();
        let indexed = indexed_repo(&ws, &keep, "con-indice");
        assert!(rule_is_due(&ws, &write_input(&indexed, &indexed.join("src/x.ts"))));

        // MUTANTE (rotto così → rosso): togliere `is_indexed` da `rule_is_due`.
        // Senza, la regola tornerebbe a viaggiare ovunque, che è il difetto.
        let elsewhere = keep.path().join("senza-indice");
        std::fs::create_dir_all(elsewhere.join("src")).unwrap();
        assert!(
            !rule_is_due(&ws, &write_input(&elsewhere, &elsewhere.join("src/x.ts"))),
            "senza indice la ricerca semantica non ha niente da rispondere"
        );
    }

    #[test]
    fn the_file_being_touched_decides_not_the_folder_the_session_started_in() {
        // MUTANTE: far tornare a `subject_dir` sempre `input.cwd`. È il difetto
        // che la regola nel prologo ha davvero: si accende sull'albero di
        // partenza, e l'84,5% dei tocchi cade fuori.
        let (ws, keep) = workspace();
        let indexed = indexed_repo(&ws, &keep, "con-indice");
        let elsewhere = keep.path().join("altrove");
        std::fs::create_dir_all(elsewhere.join("src")).unwrap();

        assert!(
            rule_is_due(&ws, &write_input(&elsewhere, &indexed.join("src/x.ts"))),
            "sessione partita fuori, file toccato dentro: la regola serve"
        );
        assert!(
            !rule_is_due(&ws, &write_input(&indexed, &elsewhere.join("src/x.ts"))),
            "sessione partita dentro, file toccato fuori: la regola non serve"
        );
    }

    #[test]
    fn only_the_tools_that_can_still_change_the_choice_get_the_rule() {
        let (ws, keep) = workspace();
        let indexed = indexed_repo(&ws, &keep, "con-indice");
        let bash = |cmd: &str| hook_io::HookInput {
            session_id: Some("s-regola".to_string()),
            cwd: Some(indexed.to_string_lossy().into_owned()),
            tool_name: Some("Bash".to_string()),
            tool_input: Some(serde_json::json!({ "command": cmd })),
            ..Default::default()
        };
        assert!(rule_is_due(&ws, &bash("rg 'come si autentica'")));
        // MUTANTE: rendere `rule_is_pertinent` vera per ogni `Bash`. La regola
        // finirebbe sul primo comando qualunque, cioè prima del momento in cui
        // può cambiare una decisione — che è il difetto da cui si parte.
        assert!(
            !rule_is_due(&ws, &bash("ls -la")),
            "un comando che non cerca nel codice non merita la regola"
        );

        let mut read = write_input(&indexed, &indexed.join("src/x.ts"));
        read.tool_name = Some("Read".to_string());
        assert!(!rule_is_due(&ws, &read), "leggere un file non è cercare");
    }

    #[test]
    fn the_rule_is_handed_over_once_per_session() {
        let (ws, _keep) = workspace();
        assert!(claim_rule(&ws, "s1"), "la prima volta il posto è libero");
        // MUTANTE: sostituire `create_new` con `create`. Il file verrebbe
        // riaperto senza errore e la regola arriverebbe a ogni chiamata.
        assert!(!claim_rule(&ws, "s1"), "la seconda no");
        assert!(claim_rule(&ws, "s2"), "un'altra sessione ha il suo posto");
    }

    #[test]
    fn the_frontmatter_stays_out_of_what_the_model_reads() {
        assert_eq!(
            rule_body("---\npaths: [\"src/**\"]\n---\n\n# Titolo\n\ncorpo\n"),
            "# Titolo\n\ncorpo\n"
        );
        // MUTANTE: restituire sempre `text`. I `paths:` parlano all'harness e
        // in contesto sono rumore che il modello legge come prescrizione.
        assert!(!rule_body("---\npaths: [\"src/**\"]\n---\n\n# T\n").contains("paths:"));
        assert_eq!(rule_body("# senza frontmatter\n"), "# senza frontmatter\n");
        assert_eq!(rule_body("---\nmai chiuso\n"), "---\nmai chiuso\n");
    }

    /// Su Rust il consiglio non promette «chi lo usa», perché l'indice non lo
    /// sa: il suo grafo vede solo i `mod`. Un freno che promette una risposta
    /// che non arriva insegna a ignorare il freno.
    #[test]
    fn on_a_rust_tree_the_advice_does_not_promise_who_uses_it() {
        let dir = tempdir::TempDir::new();
        std::fs::write(dir.path().join("Cargo.toml"), "[package]\nname = \"x\"\n").unwrap();
        let sub = dir.path().join("crates").join("qualcosa").join("src");
        std::fs::create_dir_all(&sub).unwrap();

        assert!(is_rust_tree(&sub), "il Cargo.toml di un antenato conta");
        let advice = message_name_lookup("MAX_TASK_ATTEMPTS", is_rust_tree(&sub));
        assert!(!advice.contains("e chi lo usa"), "non lo promette:\n{advice}");
        assert!(advice.contains("grep"), "e dice dove sta la risposta:\n{advice}");
    }

    /// Fuori da un progetto Rust la promessa resta intera: lì è misurata vera
    /// (30 importatori esatti sulla suite), e accorciarla ovunque toglierebbe
    /// allo strumento la ragione per cui esiste.
    #[test]
    fn outside_a_rust_tree_the_advice_keeps_its_full_promise() {
        let dir = tempdir::TempDir::new();
        let sub = dir.path().join("src").join("lib");
        std::fs::create_dir_all(&sub).unwrap();

        assert!(!is_rust_tree(&sub), "nessun Cargo.toml, nessun albero Rust");
        let advice = message_name_lookup("candidateSchema", is_rust_tree(&sub));
        assert!(advice.contains("e chi lo usa"), "{advice}");
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
