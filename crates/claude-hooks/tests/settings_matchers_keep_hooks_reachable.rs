//! L'evento e il `matcher` scritti nelle impostazioni devono raggiungere i rami
//! di codice che il gancio ha.
//!
//! IL GUASTO CHE QUESTA PROVA ESISTE PER PRENDERE, capitato il 21/08/2026: una
//! modifica al solo `matcher` ha reso **muto** un gancio il cui codice era
//! intatto e i cui casi di prova erano tutti verdi. Il pezzo che decide quali
//! strumenti arrivano al gancio non sta nel binario — sta in `settings.json`,
//! che non si compila e che nessuna batteria guardava.
//!
//! COSA SI CONFRONTA. Dal **codice**: una porta come `if !input.is_tool("X")` o
//! `if tool_name != Some("X")` dichiara che senza `X` quel ramo non fa niente.
//! Dalle **impostazioni**: l'evento e il `matcher` dei gruppi che invocano quel
//! sottocomando. Se nessun gruppo sta su un evento che porta uno strumento, o
//! nessun `matcher` lascia passare `X`, quel ramo non parte mai.
//!
//! E UN FRENO HA UNA SECONDA PRETESA, che il `matcher` non esaurisce: il suo
//! rifiuto vale solo dove l'esecuzione si può ancora impedire. Spostarlo da
//! `PreToolUse` a `PostToolUse` è una riga sola, lo lascia vivo e verde, e lo
//! riduce a un commento a cose fatte.
//!
//! I LIMITI, dichiarati perché chi vede verde sappia di cosa è difeso:
//! 1. si leggono solo le porte per **disuguaglianza**; un `if tool == "X"` che
//!    apre un ramo invece di chiuderli tutti non entra nel conto;
//! 2. si leggono i moduli che la mappa di `has_module_test` assegna a **un solo**
//!    sottocomando: una porta dentro un file condiviso (`successor.rs`) è
//!    invisibile, perché non si sa a quale dei due appartenga;
//! 3. una porta dentro una funzione chiamata altrove ha bisogno di una riga in
//!    `TOOL_GATES_BEHIND_A_CALL`, verificata ai due capi;
//! 4. lo strumento si attribuisce al **gancio**, non alla riga di comando che lo
//!    porta, e **scambiare fra loro due accensioni dello stesso gancio resta
//!    verde**. Le due coppie vive: `work-status` — la porta su `Bash` serve la
//!    riga senza argomenti, e mandarla su `SessionStart` con `--riconcilia` su
//!    `PostToolUse:Bash` non fa rossire niente — e `duplication pre`/`post`, che
//!    scambiate finiscono l'una dove il file esiste già e l'altra dove non
//!    esiste ancora: mute tutte e due;
//! 5. dove i due dialetti di espressioni regolari divergono si sceglie di non
//!    dare mai un falso allarme: un `matcher` che questa cassa rifiuta e
//!    JavaScript accetta (un lookahead) resta «non deciso», cioè coperto;
//! 6. due accensioni con la **stessa** riga di comando si giudicano insieme,
//!    perché spezzare un gruppo in due è identico a runtime — ma solo dentro lo
//!    stesso evento. La stessa riga su **due eventi diversi** si assolverebbe da
//!    sola, e per questo è vietata del tutto: la vieta
//!    `no_hook_is_wired_twice_with_the_same_command_on_two_events`. **Il prezzo
//!    del divieto**: un gancio legittimamente acceso due volte con lo stesso
//!    comando — `allow-worktree-deletes` su due eventi, per dire — diventa rosso
//!    pur essendo sano. Si paga apposta: qui il falso rosso ha un messaggio che
//!    dice cosa fare, il falso verde no;
//! 7. chi nega e dove il diniego morde si leggono dal **testo grezzo** dei
//!    sorgenti, commenti compresi: un commento che nomina `Decision::Deny`
//!    iscriverebbe un gancio fra i freni. Oggi le tredici iscrizioni sono tutte
//!    vere, verificate una per una;
//! 8. la riga di deroga di un freno senza denti (`DENIES_WITHOUT_STOPPING`) è
//!    stretta sul nome ma **non scade**: se quel gancio tornasse su un evento
//!    dove il diniego morde, la riga resterebbe lì senza che niente lo dica.
//!
//! IL FILE VERO NON SI TOCCA MAI: si apre in lettura con `std::fs` e i mutanti
//! si fanno su copie, indirizzate da `HOOK_SETTINGS_FILE`. (Il freno che
//! sorveglia quel percorso nega le **scritture**: leggerlo da shell riesce.)

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde_json::Value;

/// Il dispatch, letto a compilazione: un ramo aggiunto o tolto ricompila questa
/// prova invece di lasciarla ferma su una fotografia.
const DISPATCH: &str = include_str!("../src/main.rs");

/// Gli eventi che portano un `tool_name`, e sono i soli su cui un `matcher` di
/// strumento significhi qualcosa.
///
/// Misurati nel binario di Claude Code 2.1.238 e non dedotti: sono i cinque rami
/// che in `kPl` assegnano `a = n.tool_name`. Un gancio che pretende uno strumento
/// e vive su `SessionEnd` o su `Stop` non lo riceve mai, qualunque `matcher`
/// porti — ed è un modo di spegnerlo che non tocca una riga di codice.
const EVENTS_WITH_A_TOOL: &[&str] = &[
    "PreToolUse",
    "PostToolUse",
    "PostToolUseFailure",
    "PermissionRequest",
    "PermissionDenied",
];

/// I ganci che pretendono uno strumento e che **nessuna riga** delle impostazioni
/// invoca, con il motivo per cui il buco è tollerato.
///
/// NON è l'elenco dei sottocomandi che nessuno accende — quelli sono la metà del
/// catalogo, e quasi tutti sono strumenti che si lanciano a mano o da un
/// servizio, dove «nessuna riga» è la normalità. Qui stanno solo quelli il cui
/// codice giudica uno strumento: per loro «nessuna riga» vuol dire muto. Un nome
/// aggiunto qui nasconde proprio quel fatto, e per questo porta un motivo; il
/// caso `the_list_of_holes_stays_honest` pretende che sia ancora vero.
const NO_ENTRY_AT_ALL: &[(&str, &str)] = &[
    (
        "block-worktree-create",
        "giudica `git worktree add` fuori dai percorsi Orca; la riga che lo accende la scrive solo Theo",
    ),
    (
        "squash-orphans",
        "giudica le fusioni con schiacciamento; nato il 20/08, mai acceso",
    ),
    (
        "phase-router",
        "sceglie il modello di un `Agent` dal mestiere; scritto il 23/08 (gradino 5), la riga che lo accende è di Theo (`docs/2026-08-22-gesto-message-budget.md`)",
    ),
];

/// Le porte che decidono su un nome di strumento **dentro una funzione** che il
/// gancio chiama, invece che nel proprio modulo.
///
/// Il lettore per file non le vede: `is_handoff_call` vive in un modulo che la
/// mappa dei test assegna a un altro sottocomando. Ogni riga è verificata ai due
/// capi — il gancio deve chiamare davvero quella funzione, e la funzione deve
/// gateggiare davvero su quello strumento — così la tabella non può diventare
/// finzione.
///
/// COSA DICE e cosa non dice la riga di `handoff-required`: pretende che la
/// strada per cui una consegna viene **riconosciuta** resti raggiungibile — è
/// quella che arma il successore, e il 21/08 alle 12:55 spegnerla ha fermato il
/// ricambio delle figure. Non dice niente sui rami che quel gancio ha per gli
/// altri strumenti: sono irraggiungibili per la decisione del capitano delle
/// 15:42, che è uno stato scelto e non un guasto.
const TOOL_GATES_BEHIND_A_CALL: &[(&str, &str, &str)] =
    &[("handoff-required", "is_handoff_call", "Skill")];

/// Sotto questa soglia è rotto il lettore, non il file.
const FEWEST_CREDIBLE_ENTRIES: usize = 20;

/// Quante porte il censimento leggeva il giorno in cui è stato scritto. È un
/// canarino, non una soglia larga: una porta che sparisce perché `rustfmt` l'ha
/// spezzata su due righe non deve poter passare per «non c'era».
const GATES_READ_TODAY: usize = 15;

/// L'evento su cui il rifiuto di un gancio **impedisce** la chiamata, invece di
/// commentarla.
///
/// Misurato nel binario 2.1.238 e non dedotto: `permissionDecision` è
/// documentata «PreToolUse only» e il messaggio di scadenza di `PreToolUse` dice
/// *«The tool call was not executed»*. `PostToolUse` non c'è: lì il comando è
/// già stato eseguito.
const EVENT_THAT_CAN_STOP_A_TOOL: &str = "PreToolUse";

/// L'altro evento dove si decide prima dell'esecuzione — c'è il ramo che
/// confronta la risposta del gancio con le regole («PermissionRequest hook
/// allowed … but deny rule overrides») — **ma solo per chi sa parlarci**.
///
/// Chi risponde con `hook_io::Decision::Deny` non sa: quel canale scrive
/// `"hookEventName": "PreToolUse"` **fisso** (`hook-io/src/lib.rs`, `deny_payload`),
/// e il binario valida l'evento dichiarato e scarta. Un freno di quella famiglia
/// spostato qui muore in silenzio. Chi invece nomina l'evento nei propri
/// sorgenti si costruisce la risposta da sé, e qui vive.
const EVENT_FOR_WHOEVER_NAMES_IT: &str = "PermissionRequest";

/// I rifiuti che un gancio produce **dentro una funzione che chiama**, invisibili
/// al lettore per file. Stessa forma di `TOOL_GATES_BEHIND_A_CALL`, e ogni riga è
/// verificata ai due capi: il ramo deve chiamare davvero quella funzione, e il
/// file indicato deve contenere davvero il rifiuto.
///
/// `observe` è l'unico freno su `AskUserQuestion`, e il suo diniego nasce due
/// chiamate più in là.
const DENIALS_BEHIND_A_CALL: &[(&str, &str, &str)] = &[(
    "observe",
    "ask_routing_verdict",
    "../../guards/src/ask_routing.rs",
)];

/// I ganci che negano e che **stanno apposta** dove il diniego non impedisce.
///
/// Uno solo, e non è una svista: `handoff-required` su `PostToolUse` rimprovera
/// a cose fatte. Lo dice il suo stesso testo, è la decisione del capitano delle
/// 15:42, e se debba diventare un impedimento vero è una domanda di prodotto
/// aperta con Theo. Chi la risolve toglie questa riga.
const DENIES_WITHOUT_STOPPING: &[(&str, &str)] = &[(
    "handoff-required",
    "vive su PostToolUse per scelta: rimprovera, non impedisce",
)];

/// I ganci **accesi** che nessuna porta leggibile difende: il loro codice non
/// dice su quale strumento lavora, quindi cambiare il loro `matcher` non fa
/// rossire niente. Sta qui e non nel rapporto di chi l'ha scritto perché il
/// numero deve stare dove sta il verde: **sedici**. (Quanti siano in tutto
/// non lo scrivo: sarebbe un numero che nessun caso tiene onesto.)
///
/// `costs` e `long-session` sono gli ultimi due, accesi da Theo il 23/08:
/// vivono su eventi che non portano nessuno strumento — `Stop`, `SubagentStop`,
/// `UserPromptSubmit` — dove un `matcher` non significa niente.
///
/// Non è un permesso e non invecchia da solo: un nome che perde l'accensione o
/// che acquista una porta fa cadere `the_uncovered_hooks_are_named_here`.
const WIRED_WITHOUT_A_READABLE_GATE: &[&str] = &[
    "comment-refs",
    "costs",
    "handoff-arms-successor",
    "handoff-on-stop",
    "handoff-threshold",
    "hook-census",
    "linear-readonly",
    "live-rules",
    "long-session",
    "observe",
    "orca-cleanup",
    "register-session",
    "restart-notice",
    "ronda-trigger",
    "scope-drift",
    "skill-nudge",
    "socraticode-gate",
];

/// Il file delle impostazioni in servizio, o la copia che un mutante gli mette
/// davanti.
fn settings_path() -> PathBuf {
    if let Some(from_env) = std::env::var_os("HOOK_SETTINGS_FILE") {
        return PathBuf::from(from_env);
    }
    home().join(".claude").join("settings.json")
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

fn settings() -> Value {
    let path = settings_path();
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    serde_json::from_str(&raw)
        .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()))
}

/// I rami del `match` di `run()`: nome e corpo. Si taglia sui soli apici a otto
/// spazi d'indentazione, che dentro quella funzione sono i capi-ramo e nient'altro.
///
/// La funzione si chiude prima di leggerla, o tutto ciò che nel file viene dopo
/// finirebbe attribuito all'ultimo ramo: `json` risultava così con una porta che
/// non ha.
fn dispatch_arms() -> Vec<(String, String)> {
    let body = DISPATCH
        .split_once("fn run(which: &str)")
        .expect("the signature of run() changed: fix this reader")
        .1
        .split_once("\n}\n")
        .expect("the end of run() changed: fix this reader")
        .0;
    let mut arms = Vec::new();
    for chunk in body.split("\n        \"").skip(1) {
        let Some((name, rest)) = chunk.split_once('"') else {
            continue;
        };
        if rest.trim_start().starts_with("=>") {
            arms.push((name.to_string(), rest.to_string()));
        }
    }
    arms
}

/// Il blocco di `main.rs` fra due ancore, o il panico che dice cosa è cambiato.
fn source_block(after: &str, before: &str) -> &'static str {
    DISPATCH
        .split_once(after)
        .unwrap_or_else(|| panic!("`{after}` is gone from main.rs: fix this reader"))
        .1
        .split_once(before)
        .unwrap_or_else(|| panic!("`{before}` is gone from main.rs: fix this reader"))
        .0
}

/// Gli slug che non sono ganci: strumenti da riga di comando, non eventi. Per
/// loro non esiste nessun `matcher` con cui essere coerenti.
fn not_hooks() -> BTreeSet<String> {
    quoted_names(source_block("const NOT_HOOKS: &[&str] = &[", "\n];"))
}

fn quoted_names(block: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for line in block.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('"') {
            if let Some((name, _)) = rest.split_once('"') {
                names.insert(name.to_string());
            }
        }
    }
    names
}

/// La mappa gancio → sorgenti, riusata da `has_module_test`: è già scritta, è già
/// tenuta onesta da un caso di prova dentro `main.rs`, e dice qual è il codice di
/// ciascun gancio meglio di qualunque elenco nuovo.
fn hook_sources() -> BTreeMap<String, Vec<String>> {
    let block = source_block("fn has_module_test(name: &str) -> bool {", "\n    };\n");
    let mut map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut current = String::new();
    for line in block.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix('"') {
            if let Some((name, after)) = rest.split_once('"') {
                if after.trim_start().starts_with("=>") {
                    current = name.to_string();
                    map.entry(current.clone()).or_default();
                }
            }
        }
        for path in cited_paths(trimmed) {
            if !current.is_empty() {
                map.entry(current.clone()).or_default().push(path);
            }
        }
    }
    map
}

/// I percorsi dentro gli `include_str!` di una riga.
fn cited_paths(line: &str) -> Vec<String> {
    const OPEN: &str = "include_str!(\"";
    let mut out = Vec::new();
    for (at, _) in line.match_indices(OPEN) {
        let after = &line[at + OPEN.len()..];
        if let Some(end) = after.find('"') {
            out.push(after[..end].to_string());
        }
    }
    out
}

/// Il testo di un sorgente citato dalla mappa, **senza la sua batteria**: un caso
/// di prova può contenere la stessa forma di una porta vera e attribuirebbe al
/// gancio uno strumento che il suo codice non pretende.
fn source_text(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join(relative);
    let raw = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    match raw.split_once("#[cfg(test)]") {
        Some((before, _)) => before.to_string(),
        None => raw,
    }
}

/// Gli strumenti senza i quali un pezzo di codice esce subito.
///
/// Due forme, tutte e due per disuguaglianza: `!input.is_tool("X")` e il
/// confronto diretto sul campo `tool_name`, che i moduli portati dal Python
/// scrivono in quattro modi diversi.
/// Le righe di commento si tolgono e il resto si appiattisce in un blob: una
/// porta che `rustfmt` spezza su due righe non deve sparire dal censimento in
/// silenzio. Il salto fra il campo e il confronto resta limitato, o `tool_name`
/// finirebbe per legarsi a un `!=` che sta venti righe più in là.
fn tools_gated_in(text: &str) -> BTreeSet<String> {
    let by_helper = regex::Regex::new(r#"!\s*input\s*\.\s*is_tool\(\s*"([A-Za-z_][A-Za-z0-9_]*)""#)
        .expect("the helper shape must compile");
    let by_field = regex::Regex::new(
        r#"tool_name.{0,80}?!=\s*Some\(\s*(?:&Value::String\(\s*)?"([A-Za-z_][A-Za-z0-9_]*)""#,
    )
    .expect("the field shape must compile");
    let code: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with("//"))
        .collect();
    let blob = code.join(" ");
    let mut tools = BTreeSet::new();
    for shape in [&by_helper, &by_field] {
        for caught in shape.captures_iter(&blob) {
            tools.insert(caught[1].to_string());
        }
    }
    tools
}

/// Per ogni sottocomando, i sorgenti che la mappa assegna **a lui e a nessun
/// altro**: un file citato da due sottocomandi non dice a quale dei due
/// appartenga ciò che contiene, e si tace invece di accusare il primo.
fn owned_sources() -> BTreeMap<String, Vec<String>> {
    let sources = hook_sources();
    let mut owners: BTreeMap<&String, usize> = BTreeMap::new();
    for paths in sources.values() {
        for path in paths {
            *owners.entry(path).or_default() += 1;
        }
    }
    sources
        .iter()
        .map(|(hook, paths)| {
            let mine: Vec<String> = paths
                .iter()
                .filter(|path| owners.get(path).copied().unwrap_or(0) == 1)
                .cloned()
                .collect();
            (hook.clone(), mine)
        })
        .collect()
}

/// I ganci il cui codice sa produrre un rifiuto.
fn hooks_that_can_deny() -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for (hook, body) in dispatch_arms() {
        if denies(&body) {
            found.insert(hook);
        }
    }
    for (hook, paths) in owned_sources() {
        if paths.iter().any(|path| denies(&source_text(path))) {
            found.insert(hook);
        }
    }
    for (hook, function, file) in DENIALS_BEHIND_A_CALL {
        if calls_reach(hook, function) && denies(&source_text(file)) {
            found.insert(hook.to_string());
        }
    }
    found
}

fn denies(text: &str) -> bool {
    text.contains("Decision::Deny") || text.contains("Decision::Block")
}

/// Il ramo del gancio chiama quella funzione, e la funzione esiste.
fn calls_reach(hook: &str, function: &str) -> bool {
    let called = format!("{function}(");
    let declared = format!("fn {function}(");
    dispatch_arms()
        .iter()
        .any(|(name, body)| name == hook && body.contains(&called))
        && DISPATCH.contains(&declared)
}

/// Gli eventi su cui il rifiuto di **questo** gancio impedisce qualcosa.
fn stopping_events_for(hook: &str) -> Vec<&'static str> {
    let mut events = vec![EVENT_THAT_CAN_STOP_A_TOOL];
    let names_it = |text: &str| text.contains(EVENT_FOR_WHOEVER_NAMES_IT);
    let own = owned_sources().get(hook).cloned().unwrap_or_default();
    let in_arm = dispatch_arms()
        .iter()
        .any(|(name, body)| name == hook && names_it(body));
    if in_arm || own.iter().any(|path| names_it(&source_text(path))) {
        events.push(EVENT_FOR_WHOEVER_NAMES_IT);
    }
    events
}

/// Per ogni sottocomando, gli strumenti che il suo codice pretende — compresi
/// quelli che non sono ganci.
fn raw_gates() -> BTreeMap<String, BTreeSet<String>> {
    let sources = hook_sources();
    let owned = owned_sources();

    let mut required: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut add = |hook: &str, tools: BTreeSet<String>| {
        if !tools.is_empty() {
            required.entry(hook.to_string()).or_default().extend(tools);
        }
    };

    for (hook, body) in dispatch_arms() {
        add(&hook, tools_gated_in(&body));
    }
    for (hook, paths) in &owned {
        for path in paths {
            add(hook, tools_gated_in(&source_text(path)));
        }
    }
    for (hook, function, tool) in TOOL_GATES_BEHIND_A_CALL {
        if calls_the_gate(&sources, hook, function) && gate_demands(&sources, function, tool) {
            add(hook, BTreeSet::from([tool.to_string()]));
        }
    }

    required
}

/// Gli strumenti che ogni **gancio** pretende.
///
/// Gli strumenti da riga di comando escono di qui: non li accende nessun evento,
/// quindi non c'è nessun `matcher` con cui essere coerenti. È un secondo
/// silenziatore accanto a `NO_ENTRY_AT_ALL`, e oggi non toglie niente — nessuno
/// dei nomi dichiarati ha una porta. L'unico abuso possibile — spostare lì un
/// nome acceso per far tacere il controllo — lo chiude
/// `nothing_declared_a_command_line_tool_is_wired_as_a_hook`.
fn tools_required_by_code() -> BTreeMap<String, BTreeSet<String>> {
    let mut required = raw_gates();
    for name in not_hooks() {
        required.remove(&name);
    }
    required
}

/// Il gancio chiama davvero quella funzione?
fn calls_the_gate(sources: &BTreeMap<String, Vec<String>>, hook: &str, function: &str) -> bool {
    let call = format!("{function}(");
    sources
        .get(hook)
        .into_iter()
        .flatten()
        .any(|path| source_text(path).contains(&call))
}

/// E quella funzione decide davvero su quello strumento? Si guardano le prime
/// righe del corpo, dove sta la porta d'ingresso.
fn gate_demands(sources: &BTreeMap<String, Vec<String>>, function: &str, tool: &str) -> bool {
    let signature = format!("fn {function}(");
    let gate = format!("!= \"{tool}\"");
    for path in sources.values().flatten() {
        let text = source_text(path);
        if let Some(at) = text.find(&signature) {
            let head: Vec<&str> = text[at..].lines().take(12).collect();
            return head.join("\n").contains(&gate);
        }
    }
    false
}

/// Un'accensione: dove sta, che cosa lascia passare, e con quali argomenti
/// chiama il binario. Gli argomenti distinguono `duplication pre` da
/// `duplication post`, che sono due mestieri e non due copie.
#[derive(Debug, Clone)]
struct Wiring {
    event: String,
    matcher: String,
    args: String,
}

/// Nome del sottocomando e argomenti che lo seguono, fermandosi al primo pezzo
/// di shell — una redirezione o un separatore non sono argomenti del gancio.
fn invocation_of(command: &str) -> Option<(String, String)> {
    let mut tokens = command.split_whitespace();
    while let Some(token) = tokens.next() {
        if !token
            .trim_matches(|c| c == '"' || c == '\'')
            .ends_with("/claude-hooks")
        {
            continue;
        }
        let name = tokens.next().filter(|next| !next.starts_with('-'))?;
        let args: Vec<&str> = tokens
            .take_while(|t| !t.contains('>') && !matches!(*t, "&" | "|" | ";" | "&&" | "||"))
            .collect();
        return Some((name.to_string(), args.join(" ")));
    }
    None
}

/// Dove ogni gancio è acceso. Un gruppo senza `matcher` vale `*`, che è come lo
/// legge Claude Code.
fn registrations(settings: &Value) -> BTreeMap<String, Vec<Wiring>> {
    let mut found: BTreeMap<String, Vec<Wiring>> = BTreeMap::new();
    let Some(events) = settings.get("hooks").and_then(Value::as_object) else {
        return found;
    };
    for (event, groups) in events {
        for group in groups.as_array().into_iter().flatten() {
            let matcher = group
                .get("matcher")
                .and_then(Value::as_str)
                .unwrap_or("*")
                .to_string();
            let entries = group.get("hooks").and_then(Value::as_array);
            for entry in entries.into_iter().flatten() {
                let command = entry.get("command").and_then(Value::as_str).unwrap_or("");
                if let Some((name, args)) = invocation_of(command) {
                    found.entry(name).or_default().push(Wiring {
                        event: event.clone(),
                        matcher: matcher.clone(),
                        args,
                    });
                }
            }
        }
    }
    found
}

/// Questo gruppo consegna davvero quello strumento al gancio?
fn group_delivers(event: &str, matcher: &str, tool: &str) -> bool {
    EVENTS_WITH_A_TOOL.contains(&event) && matcher_covers(matcher, tool)
}

/// Il `matcher` lascia passare questo strumento?
///
/// Ricalca `J6T`, letta nel binario di Claude Code 2.1.238: vuoto o `*` passa
/// tutto; un `matcher` fatto di soli `[a-zA-Z0-9_|, -]` si spezza su barra e
/// virgola, si tocca con `trim` e si confronta **per uguaglianza esatta**; ogni
/// altro è un'espressione regolare **non ancorata**. Ancorare tutto darebbe
/// rosso su un `"Bash, Write"` che nella realtà raggiunge `Bash`, e un falso
/// allarme qui arrossa la batteria di un pacchetto intero.
///
/// (La forma stretta che `J6T` usa quando l'evento non porta uno strumento non
/// serve: quegli eventi sono già esclusi da `EVENTS_WITH_A_TOOL`.)
fn matcher_covers(matcher: &str, tool: &str) -> bool {
    if matcher.is_empty() || matcher == "*" {
        return true;
    }
    let plain = regex::Regex::new(r"^[a-zA-Z0-9_|, -]+$").expect("the plain shape must compile");
    if plain.is_match(matcher) {
        return matcher
            .split(['|', ','])
            .map(str::trim)
            .any(|piece| piece == tool);
    }
    // A decidere è `new RegExp(t)` di JavaScript, e i due dialetti non
    // combaciano. Dove divergono, si sceglie di non dare mai un falso allarme:
    // ciò che Rust rifiuta e JavaScript accetta resta «non deciso», cioè
    // coperto (limite 5 in testa al file).
    if uses_inline_flags(matcher) {
        return false; // JavaScript lancia: il gruppo intero è muto
    }
    match regex::Regex::new(matcher) {
        Ok(re) => re.is_match(tool),
        Err(_) => js_would_take_it(matcher),
    }
}

/// I gruppi con le opzioni in linea (`(?i)`, `(?s)`…): Rust li accetta,
/// `new RegExp` in JavaScript lancia — e un `matcher` che lancia fa cadere il
/// filtro, cioè rende muti tutti i ganci di quel gruppo.
fn uses_inline_flags(matcher: &str) -> bool {
    regex::Regex::new(r"\(\?[a-zA-Z]*[imsuxU][a-zA-Z]*[):]")
        .expect("the inline-flag shape must compile")
        .find_iter(matcher)
        .any(|hit| !hit.as_str().starts_with("(?:"))
}

/// Il contrario: costrutti che JavaScript ha e la cassa `regex` no. Qui il
/// controllo non sa decidere, e tace invece di accusare.
fn js_would_take_it(matcher: &str) -> bool {
    ["(?=", "(?!", "(?<=", "(?<!", "\\1", "\\2"]
        .iter()
        .any(|shape| matcher.contains(shape))
}

#[test]
fn every_hook_that_demands_a_tool_is_still_reached_by_event_and_matcher() {
    let registered = registrations(&settings());
    let entries: usize = registered.values().map(Vec::len).sum();
    assert!(
        entries >= FEWEST_CREDIBLE_ENTRIES,
        "only {entries} hook entries read from {}: the reader is broken, not the file",
        settings_path().display()
    );

    let required = tools_required_by_code();
    assert!(
        required.len() >= GATES_READ_TODAY,
        "only {} hooks declare a required tool, {GATES_READ_TODAY} were read when this line was \
         written: if a gate was removed on purpose lower the number and say why, otherwise the \
         source reader is broken",
        required.len()
    );

    let mut mute: Vec<String> = Vec::new();
    for (hook, tools) in &required {
        let Some(groups) = registered.get(hook) else {
            if !NO_ENTRY_AT_ALL.iter().any(|(name, _)| name == hook) {
                mute.push(format!(
                    "{hook} judges {tools:?} but no line of the settings invokes it"
                ));
            }
            continue;
        };
        for tool in tools {
            if groups
                .iter()
                .any(|w| group_delivers(&w.event, &w.matcher, tool))
            {
                continue;
            }
            let declared: Vec<String> = groups
                .iter()
                .map(|w| format!("{}:{}", w.event, w.matcher))
                .collect();
            mute.push(format!(
                "{hook} does nothing unless tool_name is {tool}, and no group delivers {tool}: {}",
                declared.join(", ")
            ));
        }

        // E ogni **riga di comando** dev'essere viva per conto suo, o un gancio
        // acceso due volte si copre da sé: spegnere `duplication pre` restava
        // verde grazie a `duplication post`, che però non vede nessuna nascita
        // di file. Si guarda la riga di comando e non il gruppo, perché due
        // gruppi che invocano lo **stesso** comando sono un'accensione sola
        // spezzata in due: accusarli separatamente darebbe rosso su una
        // configurazione che a runtime è identica.
        let mut by_command: BTreeMap<&str, Vec<&Wiring>> = BTreeMap::new();
        for wiring in groups {
            if EVENTS_WITH_A_TOOL.contains(&wiring.event.as_str()) {
                by_command
                    .entry(wiring.args.as_str())
                    .or_default()
                    .push(wiring);
            }
        }
        for (args, wirings) in by_command {
            if tools
                .iter()
                .any(|tool| wirings.iter().any(|w| matcher_covers(&w.matcher, tool)))
            {
                continue;
            }
            let declared: Vec<String> = wirings
                .iter()
                .map(|w| format!("{}:{}", w.event, w.matcher))
                .collect();
            let command = if args.is_empty() {
                hook.to_string()
            } else {
                format!("{hook} {args}")
            };
            mute.push(format!(
                "`{command}` is wired on {}, which delivers none of the tools it judges: {tools:?}",
                declared.join(", ")
            ));
        }
    }

    assert!(
        mute.is_empty(),
        "hooks made mute by the settings, with their code untouched:\n  {}",
        mute.join("\n  ")
    );
}

#[test]
fn every_hook_named_in_the_settings_exists_in_the_dispatch() {
    let registered = registrations(&settings());
    let known: BTreeSet<String> = dispatch_arms().into_iter().map(|(name, _)| name).collect();
    assert!(
        known.len() > 30,
        "only {} arms found in run(): the source reader is broken",
        known.len()
    );

    let unknown: Vec<&String> = registered.keys().filter(|n| !known.contains(*n)).collect();
    assert!(
        unknown.is_empty(),
        "the settings invoke subcommands the binary does not have: {unknown:?}"
    );
}

#[test]
fn the_list_of_holes_stays_honest() {
    let registered = registrations(&settings());
    let known: BTreeSet<String> = dispatch_arms().into_iter().map(|(name, _)| name).collect();
    let required = tools_required_by_code();
    for (name, why) in NO_ENTRY_AT_ALL {
        assert!(
            known.contains(*name),
            "{name} is listed as a hole but the dispatch does not know it"
        );
        assert!(
            required.contains_key(*name),
            "{name} no longer demands any tool: it is not a hole any more, drop the line"
        );
        assert!(
            !why.trim().is_empty(),
            "{name} is listed as a hole without a reason"
        );
        assert!(
            !registered.contains_key(*name),
            "{name} is registered now: drop it from the list, the check covers it"
        );
    }
}

#[test]
fn no_hook_is_wired_twice_with_the_same_command_on_two_events() {
    // Il prezzo della regola per riga di comando, tolto invece che tollerato.
    // Due gruppi che invocano lo stesso comando si giudicano insieme: dentro un
    // evento è giusto — spezzare un gruppo in due è identico a runtime — ma su
    // due eventi diversi il gruppo vivo assolverebbe quello morto, e nessuno
    // vedrebbe più che metà dell'accensione non parte mai.
    let registered = registrations(&settings());
    let mut doubled: Vec<String> = Vec::new();
    for (hook, wirings) in &registered {
        let mut events_by_command: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for wiring in wirings {
            if EVENTS_WITH_A_TOOL.contains(&wiring.event.as_str()) {
                events_by_command
                    .entry(wiring.args.as_str())
                    .or_default()
                    .insert(wiring.event.as_str());
            }
        }
        for (args, events) in events_by_command {
            if events.len() > 1 {
                doubled.push(format!(
                    "`{hook} {args}` is wired on {events:?}: one of the two would absolve the other"
                ));
            }
        }
    }
    assert!(
        doubled.is_empty(),
        "the same command wired on two tool events, where this check cannot tell which one is \
         alive:\n  {}",
        doubled.join("\n  ")
    );
}

#[test]
fn a_hook_that_denies_keeps_an_event_where_the_denial_bites() {
    // Il `matcher` non esaurisce le pretese di un freno: `PostToolUse` gli
    // consegna lo strumento giusto e lo lascia arrivare **dopo** l'esecuzione.
    // La domanda si fa solo ai ganci che lavorano sugli strumenti — un gancio
    // dello `Stop` nega un'altra cosa, e lì il diniego vale.
    let registered = registrations(&settings());
    let denying = hooks_that_can_deny();
    assert!(
        // Quindici sanno rifiutare, misurati il 24/08: dodici sono accesi su un
        // evento di strumento — undici passano di qui, il dodicesimo è la riga
        // scusata — e gli altri tre non li accende nessuno o vivono sullo `Stop`.
        denying.len() >= 15,
        "only {} hooks look able to deny, 15 were found when this line was written: if one lost \
         its refusal on purpose lower the number, otherwise the source reader is broken",
        denying.len()
    );

    let mut toothless: Vec<String> = Vec::new();
    for (hook, wirings) in &registered {
        if !denying.contains(hook) || DENIES_WITHOUT_STOPPING.iter().any(|(n, _)| n == hook) {
            continue;
        }
        if !wirings
            .iter()
            .any(|w| EVENTS_WITH_A_TOOL.contains(&w.event.as_str()))
        {
            continue;
        }
        if wirings
            .iter()
            .any(|w| stopping_events_for(hook).contains(&w.event.as_str()))
        {
            continue;
        }
        let declared: Vec<&str> = wirings.iter().map(|w| w.event.as_str()).collect();
        toothless.push(format!(
            "{hook} refuses tool calls but is wired only on {}, while its refusal bites on {:?}",
            declared.join(", "),
            stopping_events_for(hook)
        ));
    }
    assert!(
        toothless.is_empty(),
        "brakes that can no longer stop anything:\n  {}",
        toothless.join("\n  ")
    );

    for (name, why) in DENIES_WITHOUT_STOPPING {
        assert!(
            denying.contains(*name),
            "{name} is excused as a toothless brake but no longer denies: drop the line"
        );
        assert!(
            registered.contains_key(*name),
            "{name} is excused but nothing wires it: drop the line"
        );
        assert!(!why.trim().is_empty(), "{name} is excused without a reason");
    }
}

#[test]
fn the_uncovered_hooks_are_named_here() {
    // L'inventario di ciò che questa prova NON difende, tenuto onesto ai due
    // versi: nessun nome elencato per abitudine, nessun gancio acceso che
    // sparisce dal conto senza che qualcuno lo scriva.
    let registered = registrations(&settings());
    let required = tools_required_by_code();
    for name in WIRED_WITHOUT_A_READABLE_GATE {
        assert!(
            registered.contains_key(*name),
            "{name} is listed as uncovered but no line of the settings wires it: drop the line"
        );
        assert!(
            !required.contains_key(*name),
            "{name} has a readable gate now: it is covered, drop it from the list"
        );
    }
    let unlisted: Vec<&String> = registered
        .keys()
        .filter(|name| !required.contains_key(*name))
        .filter(|name| !WIRED_WITHOUT_A_READABLE_GATE.contains(&name.as_str()))
        .collect();
    assert!(
        unlisted.is_empty(),
        "wired hooks that no gate defends and that the list does not name: {unlisted:?}"
    );
}

#[test]
fn nothing_declared_a_command_line_tool_is_wired_as_a_hook() {
    // Il secondo silenziatore, e l'unico abuso che permette: spostare in
    // `NOT_HOOKS` un nome acceso toglie il controllo senza dire niente.
    //
    // La domanda si fa su TUTTI i nomi dichiarati, non sui soli che oggi
    // perdono una porta: oggi quel filtro non toglie niente — `json` sembrava
    // avere una porta, ed era la coda di `main.rs` letta come parte dell'ultimo
    // ramo — e un caso che gira sull'insieme vuoto passa a vuoto per sempre.
    let registered = registrations(&settings());
    let declared = not_hooks();
    assert!(
        declared.len() > 10,
        "only {} command-line tools declared: the source reader is broken",
        declared.len()
    );
    let wired: Vec<&String> = declared
        .iter()
        .filter(|name| registered.contains_key(*name))
        .collect();
    assert!(
        wired.is_empty(),
        "declared command-line tools that the settings wire as hooks, so their gates are \
         silenced: {wired:?}"
    );
}

#[test]
fn the_denial_behind_a_call_is_still_where_the_table_says() {
    for (hook, function, file) in DENIALS_BEHIND_A_CALL {
        assert!(
            calls_reach(hook, function),
            "{hook} no longer calls {function}(): the row is fiction"
        );
        assert!(
            denies(&source_text(file)),
            "{file} no longer refuses anything: the row is fiction"
        );
        assert!(
            hooks_that_can_deny().contains(*hook),
            "{hook} is not counted among the brakes: the row does not work"
        );
    }
}

#[test]
fn the_gate_behind_a_call_is_still_where_the_table_says() {
    // La tabella dei rinvii non può diventare finzione: si verifica ai due capi
    // contro i sorgenti, non contro se stessa.
    let sources = hook_sources();
    for (hook, function, tool) in TOOL_GATES_BEHIND_A_CALL {
        assert!(
            calls_the_gate(&sources, hook, function),
            "{hook} no longer calls {function}(): the row is fiction"
        );
        assert!(
            gate_demands(&sources, function, tool),
            "{function}() no longer gates on {tool}: the row is fiction"
        );
    }
}

#[test]
fn the_reader_finds_every_shape_of_tool_gate() {
    // Senza questi casi il lettore potrebbe non trovare più niente e i controlli
    // sopra sarebbero verdi per assenza di domande.
    let shapes = "
        if !input.is_tool(\"Bash\") { return 0; }
        if input.tool_name.as_deref() != Some(\"Write\") { return 0; }
        if obj.get(\"tool_name\") != Some(&Value::String(\"Skill\".to_string())) { return 0; }
        if p.get(\"tool_name\").and_then(|v| v.as_str()) != Some(\"SendMessage\") { return 0; }
        // if !input.is_tool(\"Commented\") { return 0; }
        if tool_name == \"Grep\" { measure(); }
    ";
    let found = tools_gated_in(shapes);
    assert_eq!(
        found,
        BTreeSet::from([
            "Bash".to_string(),
            "SendMessage".to_string(),
            "Skill".to_string(),
            "Write".to_string(),
        ]),
        "the gate reader changed shape"
    );
}

#[test]
fn a_group_that_stops_delivering_a_tool_is_seen() {
    // L'evento conta quanto il `matcher`: un gancio spostato su un evento che
    // non porta strumenti è muto anche con `*`.
    assert!(group_delivers("PreToolUse", "Bash", "Bash"));
    assert!(group_delivers(
        "PermissionRequest",
        "SendMessage",
        "SendMessage"
    ));
    assert!(!group_delivers("SessionEnd", "*", "Bash"));
    assert!(!group_delivers("Stop", "Bash", "Bash"));

    assert!(matcher_covers("Write|Edit|MultiEdit|Bash", "Bash"));
    assert!(matcher_covers("Bash, Write", "Write"));
    assert!(matcher_covers("", "Bash"));
    assert!(!matcher_covers("Skill", "Bash"));
    assert!(!matcher_covers("Bash", "BashOutput"));
    // Fuori dall'alfabeto semplice è un'espressione regolare, e non è ancorata.
    assert!(matcher_covers("Bash|mcp__.*", "mcp__linear__issue"));
    assert!(matcher_covers(
        "Grep|Bash|codebase_(search|symbol)",
        "codebase_symbol"
    ));
    // Le opzioni in linea: qui compilano, in JavaScript lanciano — e un
    // `matcher` che lancia rende muto tutto il gruppo.
    assert!(!matcher_covers("(?i)bash", "Bash"));
    assert!(!matcher_covers("(?i)bash", "bash"));
    assert!(matcher_covers("(?:Bash|Write)", "Bash"));
    // E ciò che questa cassa non sa leggere ma JavaScript sì resta non deciso,
    // cioè coperto: meglio tacere che dare un allarme falso.
    assert!(matcher_covers("Bash(?=Output)", "BashOutput"));
    assert!(!matcher_covers("Bash[a-", "Bash"));
}

#[test]
fn the_command_line_of_a_hook_gives_up_its_name_and_arguments() {
    let named = |command| invocation_of(command).map(|(n, a)| (n, a));
    assert_eq!(
        named("/x/claude-hooks cd-guard"),
        Some(("cd-guard".into(), String::new()))
    );
    // Gli argomenti si fermano prima della shell: senza, `duplication pre` e
    // `duplication pre 2>/dev/null` sembrerebbero due accensioni diverse.
    assert_eq!(
        named("nohup /x/claude-hooks work-status --riconcilia > /dev/null 2>&1 &"),
        Some(("work-status".into(), "--riconcilia".into()))
    );
    assert_eq!(
        named("cd \"$CLAUDE_PROJECT_DIR\" 2>/dev/null; /x/claude-hooks orca-cleanup --close --quiet 2>/dev/null || true"),
        Some(("orca-cleanup".into(), "--close --quiet".into()))
    );
    assert_eq!(named("/x/other-tool cd-guard"), None);
}
