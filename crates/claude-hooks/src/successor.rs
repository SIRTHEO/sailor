//! La parte con disco, ambiente e `orca` del gancio che arma il successore.
//!
//! Il giudizio puro sta in `guards::successor`; qui c'è ciò che deve toccare la
//! macchina: leggere la testa di un file per il frontmatter, contare le sessioni
//! vive, chiedere a Orca i pannelli dell'albero.
//!
//! FAIL-OPEN OVUNQUE. Un gancio che rompe la scrittura di una consegna è peggio
//! del problema che risolve: ogni errore diventa «non lo so», e un «non lo so»
//! non frena.

use guards::successor::{count_agents, is_handoff_doc, mandate};
use hook_io::journal::{self, Field};
use std::fs;
use std::io::Read;

/// Quanto si legge di un file candidato.
///
/// Non più 400 byte: dal 17/08/2026 il riconoscimento guarda anche le sezioni
/// del corpo, che stanno oltre il frontmatter. Il tetto resta perché il gancio
/// scatta su ogni Write/Edit, e in `memory/` potrebbe un giorno finire qualcosa
/// di grosso: la consegna più lunga sul disco di quel giorno era 19 KB.
const MAX_READ: usize = 64 * 1024;

fn text_of(path: &str) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; MAX_READ];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// È un documento di consegna? Legge il file solo quando serve davvero.
///
/// La guardia sul percorso viene prima della lettura, e non è un'ottimizzazione
/// oziosa: questo gancio gira a ogni Write ed Edit della sessione, e senza di
/// essa aprirebbe ogni file toccato solo per scoprire che non sta in `memory/`.
pub fn is_doc(path: &str) -> bool {
    if guards::successor::name_says_handoff(path) {
        return true;
    }
    if !path.contains("/memory/") {
        return false;
    }
    is_handoff_doc(path, text_of(path).as_deref())
}

/// L'inventario di ciò che è rimasto acceso in `root`, già in forma di clausola.
///
/// Due chiamate a `lsof` e non una per processo: i pid in ascolto sono una
/// decina, e interrogarli uno a uno costerebbe una decina di processi in un
/// gancio che ne ha già tre. Fail-open: se `lsof` non risponde l'inventario è
/// vuoto e il mandato resta quello di prima — meglio senza terza clausola che
/// senza mandato.
fn inherited_clause(root: &str) -> String {
    let lsof = |args: &[&str]| -> String {
        std::process::Command::new("lsof")
            .args(args)
            .output()
            .ok()
            .filter(|o| !o.stdout.is_empty())
            .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
            .unwrap_or_default()
    };
    let listeners =
        guards::successor::parse_listeners(&lsof(&["-nP", "-iTCP", "-sTCP:LISTEN", "-Fpcn"]));
    if listeners.is_empty() {
        return String::new();
    }
    let pids = listeners
        .iter()
        .map(|l| l.pid.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let cwds = guards::successor::parse_cwds(&lsof(&["-a", "-p", &pids, "-d", "cwd", "-Fn"]));
    guards::successor::inherited_clause(&guards::successor::inherited_listeners(
        &listeners, &cwds, root,
    ))
}

/// Quanti pannelli con un agente ci sono in questo albero. `None` se ignoto.
pub fn panes_here(root: &str) -> Option<usize> {
    let out = std::process::Command::new("orca")
        .args(["terminal", "list", "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(count_agents(&v, root))
}

/// Le sessioni Claude vive adesso, contate dal FATTO. `None` se non si è potuto.
///
/// SI CONTANO I PROCESSI, e il commento che stava qui diceva il contrario: «un
/// `ps | grep claude` conta anche i subagent e i wrapper della shell, e
/// sovrastima di molto». È falso per questa forma. `comm` è il **nome
/// dell'eseguibile senza argomenti**, l'ancoraggio `^claude$` scarta ogni riga
/// che lo contenga soltanto, e i subagent non sono processi: girano dentro il
/// padre. Misurato il 17/08/2026 sullo stesso istante — `ps` dà 4 e
/// `claude agents --json` dà 4, come il 5 contro 5 già registrato in
/// `docs/2026-08-17-cron-e-soglie.md`.
///
/// Ed è la misura giusta per un tetto sulla saturazione: `claude agents` è il
/// registro che la CLI tiene di sé, quindi non vede una sessione headless né una
/// caduta male, mentre la RAM la occupano i processi. È anche la stessa
/// grandezza su cui il registro dello swap ha raccolto 5.176 campioni in una
/// settimana: il tetto e la serie storica parlano dello stesso numero, invece di
/// due numeri che un giorno divergono. E costa un `ps` invece di avviare la CLI.
pub fn live_sessions() -> Option<usize> {
    let out = std::process::Command::new("/bin/ps")
        .args(["-Ao", "comm"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.trim() == "claude")
            .count(),
    )
}

/// Il tetto globale alle sessioni, come gancio `PreToolUse` su `Bash`.
///
/// PERCHÉ ESISTE UN SECONDO POSTO. Il tetto di `arm()` vive dentro **un**
/// produttore: la consegna che apre il successore. Nessun'altra strada lo
/// consulta — non l'apertura a mano di una scheda, non un dispaccio di
/// orchestrazione. Il 16/08/2026 le sessioni contemporanee sono arrivate a
/// **64** contro gli 8 dichiarati, e la macchina si è riavviata per saturazione
/// alle 23:20: quel tetto non è stato scavalcato, è stato aggirato. Qui il
/// controllo sta sulla superficie che **tutti** i produttori attraversano — il
/// comando `Bash` — invece che dentro chi lo esegue.
///
/// Il giudizio è in `guards::handoff`, che non tocca niente: qui restano il
/// conteggio, la valvola e il registro.
pub fn session_cap(command: &str) -> i32 {
    let mode = hook_io::Mode::from_env("SESSION_CAP_GUARD");
    if mode == hook_io::Mode::Off {
        return 0;
    }
    let delta = guards::handoff::session_delta(command);
    if delta == guards::handoff::SessionDelta::None {
        return 0;
    }
    let facts = guards::handoff::CapFacts {
        delta,
        live: live_sessions(),
        cap: cap("SESSION_CAP_LIMIT", guards::handoff::SESSION_CAP_DEFAULT),
    };
    let verdict = guards::handoff::session_cap_verdict(&facts);
    // SI REGISTRA ANCHE CHI PASSA, e non è simmetria oziosa: oggi nessuno sa
    // quante sessioni vengano aperte in un giorno, e senza quel numero la soglia
    // non si potrà ritarare — è l'errore già pagato dalla regola della pressione
    // di memoria, buona all'11/08 e indistinguibile dal normale una settimana
    // dopo. Le aperture sono poche, quindi il registro non si riempie.
    journal::record(
        "session-cap",
        if verdict.is_some() { "ferma" } else { "apre" },
        match facts.delta {
            guards::handoff::SessionDelta::Replaces => "sostituzione",
            _ => "aggiunta",
        },
        &[
            ("vive", Field::Number(facts.live.unwrap_or(0) as i64)),
            ("tetto", Field::Number(facts.cap as i64)),
            (
                "comando",
                Field::Text(command.chars().take(200).collect::<String>()),
            ),
        ],
    );
    let Some(message) = verdict else {
        return 0;
    };
    let decision = match mode {
        hook_io::Mode::WarnOnly => hook_io::Decision::Warn(message),
        _ => hook_io::Decision::Deny(message),
    };
    hook_io::emit("session-cap", &decision)
}

use crate::handoff::state_dir;

/// Il marcatore «per questa sessione un successore c'è già», e se c'era.
///
/// Legge **e scrive** in una volta sola, come il Python: il freno si arma
/// consumandosi. Separare le due cose introdurrebbe una finestra in cui due
/// eventi ravvicinati passano entrambi, ed è esattamente il caso che questo
/// freno esiste per chiudere.
fn already_armed(path: &str, session: &str) -> bool {
    let marker = state_dir().join(format!(
        "successore-armato-{}",
        guards::successor::armed_fingerprint(path, session)
    ));
    if marker.exists() {
        return true;
    }
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(&marker, format!("{path}\n"));
    false
}

/// Apre il pannello col mandato dentro, e lo fa partire dopo un conto alla rovescia.
///
/// L'attesa non è una conferma da dare: è una via d'uscita per chi sta
/// guardando. Prima qui c'era un Invio da premere, e nessuno lo premeva — 21
/// schede armate e zero avviate. L'inerzia deve lavorare nel verso giusto.
///
/// PANNELLO, NON SCHEDA, dal 19/08/2026. La consegna raccolta prosegue lo stesso
/// lavoro, e il `CLAUDE.md` riserva `terminal create` a un lavoro che non
/// appartiene a questo: venti schede aperte così hanno saturato la colonna di
/// sinistra, dove Theo cerca le sessioni vive. Con l'handle del terminale che
/// consegna si divide quello; senza handle — fuori da Orca — resta la scheda,
/// che è meglio di nessuna ripresa.
fn open_tab(path: &str, inherited: &str) -> (bool, String) {
    let wait_s = std::env::var("CONSEGNA_ATTESA_S")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(30);
    // Il mandato entra in una stringa fra apici singoli della shell: l'unico
    // carattere da neutralizzare è l'apice stesso.
    let text = mandate(path, inherited).replace('\'', r"'\''");
    let command = format!(
        "printf '%s\\n\\n' '{text}'; \
         printf 'Starting in {wait_s}s. Ctrl-C to cancel.\\n'; \
         sleep {wait_s}; exec claude '{text}'"
    );
    // L'handle NON si prende dall'ambiente: `ORCA_TERMINAL_HANDLE` è catturato
    // all'avvio e non si aggiorna più, e proprio le sessioni lunghe — quelle in
    // cui questo gancio scatta — sono quelle che hanno riattaccato il terminale
    // almeno una volta. Chiedere adesso costa una chiamata e vale la differenza
    // fra un pannello e un errore; se non si sa, resta la scheda.
    let mut orca = |args: &[&str]| -> (i32, String) {
        match std::process::Command::new("orca").args(args).output() {
            Ok(o) => (
                o.status.code().unwrap_or(1),
                String::from_utf8_lossy(&o.stdout).into_owned(),
            ),
            Err(_) => (1, String::new()),
        }
    };
    let handle = match crate::relay::read_terminals(&mut orca) {
        Some(list) => guards::handoff::resolve_terminal_handle(
            &std::env::var("ORCA_TAB_ID").unwrap_or_default(),
            &std::env::var("ORCA_WORKTREE_ID").unwrap_or_default(),
            &std::env::var("ORCA_TERMINAL_HANDLE").unwrap_or_default(),
            &list,
        ),
        None => String::new(),
    };
    let args: Vec<String> = if handle.is_empty() {
        ["terminal", "create", "--command", &command, "--title",
         "consegna raccolta (parte da sola)", "--json"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    } else {
        ["terminal", "split", "--terminal", &handle, "--command", &command, "--json"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    };
    let out = std::process::Command::new("orca")
        .args(&args)
        // La figlia eredita il marchio di generazione: è il freno che le impedisce
        // di armarne un'altra, e l'unico che sopravvive al cambio di processo.
        .env(guards::successor::GENERATION_ENV, "1")
        .output();
    match out {
        Ok(o) => {
            let text = if o.stdout.is_empty() {
                String::from_utf8_lossy(&o.stderr).into_owned()
            } else {
                String::from_utf8_lossy(&o.stdout).into_owned()
            };
            // 600 e non 200: la `tabId` arriva dopo l'handle nella risposta di
            // Orca, e a 200 caratteri veniva tagliata a metà — l'handle da solo
            // scade al primo riattacco e il marcatore diventa inservibile.
            (o.status.success(), text.chars().take(600).collect())
        }
        Err(e) => (false, e.to_string().chars().take(600).collect()),
    }
}

/// Lascia detto QUALE scheda prosegue, perché il congedo sia verificabile.
///
/// Si registra la `tabId` accanto all'handle: l'handle invecchia al primo
/// riattacco del pannello e chi legge il marcatore conclude «morto» aprendone un
/// altro. Misurato il 17/08/2026 — il marcatore citava un handle assente dagli
/// undici vivi mentre la sua tab lavorava.
fn note_successor(session: &str, detail: &str) {
    // Senza sessione il marcatore è illeggibile per costruzione: chi congeda lo
    // cerca per `successore-di-<sessione>` e con la chiave vuota esce subito.
    // Scriverlo lascia solo un file che nessuno raccoglie — sul disco del
    // 17/08/2026 ce n'era uno, e il Python lo chiamava pure con un altro nome
    // (`senza-sessione` contro la chiave vuota del porto): due rifiuti diversi
    // per lo stesso caso.
    if session.is_empty() {
        return;
    }
    let grab = |re: &regex::Regex| -> String {
        re.captures(detail)
            .and_then(|c| c.get(1))
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    };
    let handle = grab(&regex::Regex::new(r"\b(term_[0-9a-f-]{8,})").unwrap());
    let tab = grab(&regex::Regex::new(r#""tabId"\s*:\s*"([0-9a-f-]{8,})""#).unwrap());
    let safe: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(
        state_dir().join(format!("successore-di-{safe}")),
        hook_io::python_json::dumps_unicode(&serde_json::json!({
            "handle": handle,
            "tabId": tab,
            // Ora locale, non UTC: il Python scrive `datetime.now().isoformat()`,
            // e un marcatore due ore indietro sarebbe formalmente valido e
            // sbagliato — lo stesso inganno già visto nel registro Linear.
            "quando": hook_io::local_time::now_local_iso8601(),
        })),
    );
}

fn cap(var: &str, default: usize) -> usize {
    std::env::var(var)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// L'ora locale adesso, dalla stessa fonte che data i registri.
fn hour_now() -> u32 {
    // L'ora si lascia fissare dal banco, come `CONSEGNA_ANCHE_A_META` fissa la
    // pienezza: il freno orario sta PRIMA dei tetti e li copre, quindi di notte
    // i casi che provano i tetti si fermano qui e risultano rossi senza che
    // niente sia rotto — misurato sul Python alle 03:20 del 18/08/2026. La
    // valvola sta anche qui perché il contratto fra i due porti è che rispondano
    // uguale a parità di ambiente, e una valvola da un lato solo lo rompe nel
    // silenzio: nessun confronto la esercitava.
    if let Ok(fixed) = std::env::var("CONSEGNA_ORA") {
        if let Ok(h) = fixed.parse::<u32>() {
            return h;
        }
    }
    // `now_local_iso8601` dà `2026-08-17T14:05:12+0200`: l'ora sono due cifre a
    // offset fisso. Estrarle di lì evita una seconda strada verso il fuso —
    // e due strade verso il fuso divergono, come è già successo altrove.
    hook_io::local_time::now_local_iso8601()
        .get(11..13)
        .and_then(|h| h.parse().ok())
        .unwrap_or(12)
}

/// Il messaggio su **entrambi** i canali, perché uno solo non basta.
///
/// `systemMessage` va all'utente e Claude non lo vede; `additionalContext`
/// raggiunge l'assistente e va annidato in `hookSpecificOutput`, perché al
/// livello superiore Claude Code lo ignora in silenzio. Il 16/08/2026 il rifiuto
/// più frequente di questo gancio era muto da entrambe le parti: 97 «troppe
/// sessioni» contro 3 aperture, e nessuno dei 97 è arrivato a qualcuno.
fn speak(message: &str) -> String {
    hook_io::python_json::dumps_unicode(&serde_json::json!({
        "systemMessage": message,
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": message,
        },
    }))
}

/// Come è finita la richiesta di armare, e cosa c'è da dire.
///
/// Le tre voci sono quelle dell'originale (`ferma`, `apre`, `fallisce`), e il
/// messaggio può essere vuoto: i freni silenziosi non parlano. La **forma**
/// dell'uscita non si decide qui, perché i due eventi la vogliono diversa — un
/// PostToolUse stampa JSON, uno Stop scrive su stderr.
#[derive(Debug, PartialEq, Eq)]
pub enum ArmOutcome {
    Stop(String),
    Open(String),
    Failed(String),
}

/// I quattro freni e, se passano tutti, la scheda che parte da sola.
///
/// ESTRATTA PER UN SECONDO CHIAMANTE, come nell'originale: fino al 16/08/2026
/// l'unico innesco era la **scrittura** di una consegna, quindi una sessione
/// piena che aveva già consegnato e proseguiva senza toccare file non armava
/// mai niente — misurata così una sessione al 106% del budget di Opus 5, viva.
/// `origin` distingue le due strade nel registro, e serve solo a quello.
///
/// IL REGISTRO FA PARTE DEL LAVORO, e questa funzione esiste anche per
/// rimetterlo: fino a oggi il porto decideva bene e **non registrava niente**,
/// mentre l'originale scrive cinque casi. Misurato il 17/08/2026 — il gancio è
/// passato al binario alle 10:57, l'ultima riga `origine=scrittura` è delle
/// 11:10 (una sessione avviata prima, che aveva ancora la riga vecchia in
/// memoria), e dopo tre consegne vere scritte alle 11:35, 13:37 e 14:21 il
/// registro non ha più una sola riga. Il freno più frequente — `troppe-sessioni`,
/// 186 righe storiche — scattava invisibile. Nessun confronto se n'era accorto
/// perché `compare-successor.py` non guardava il registro.
pub fn arm(path: &str, session: &str, origin: &str, enough_used: bool) -> ArmOutcome {
    // La cwd del PROCESSO, non quella dichiarata nel payload. Il Python usa
    // `os.getcwd()`, e le due coincidono quasi sempre — per questo il confronto
    // non se ne era accorto: i casi passavano la cwd vera in entrambi i campi.
    // Divergono la prima volta che qualcuno invoca il gancio da fuori l'albero,
    // e allora il tetto guarda un albero che non è quello in cui si sta
    // lavorando. Trovato rendendo il binario non eseguibile e confrontando le
    // due risposte sullo stesso ingresso: 0 pannelli contro 2.
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();

    let mut facts = guards::successor::ArmFacts {
        second_generation: std::env::var(guards::successor::GENERATION_ENV).is_ok(),
        hour: hour_now(),
        live_sessions: live_sessions(),
        session_cap: cap("CONSEGNA_TETTO_SESSIONI", 8),
        panes_here: panes_here(&cwd),
        pane_cap: cap("CONSEGNA_TETTO_PANNELLI", 2),
        already_armed: false,
        enough_used,
    };
    // Il consumo si valuta per ultimo perché SCRIVE il marcatore: chiederlo prima
    // brucerebbe l'unica arma di questa sessione anche quando un altro freno
    // avrebbe fermato tutto comunque.
    let (reason, message) = match guards::successor::decide(&facts) {
        guards::successor::Outcome::StopQuiet(r) => (r, String::new()),
        guards::successor::Outcome::StopLoud { reason, message } => (reason, message),
        guards::successor::Outcome::Open => {
            // Il freno del già-armato non lascia riga, come nell'originale: si
            // consuma leggendo, e registrarlo raddoppierebbe ogni consegna.
            facts.already_armed = already_armed(path, session);
            if facts.already_armed {
                return ArmOutcome::Stop(String::new());
            }
            // L'inventario si raccoglie qui e non prima: costa due `lsof`, e
            // ogni altro ramo di questa funzione si ferma senza aprire niente.
            let (ok, detail) = open_tab(path, &inherited_clause(&cwd));
            if ok {
                note_successor(session, &detail);
            }
            journal::record(
                "consegna-arma-successore",
                if ok { "apre" } else { "fallisce" },
                "tab-che-parte-da-sola",
                &[
                    ("path_", Field::Text(path.to_string())),
                    ("dettaglio", Field::Text(detail.clone())),
                    ("origine", Field::Text(origin.to_string())),
                ],
            );
            return if ok {
                ArmOutcome::Open(
                    "Consegna raccolta: ho aperto una tab col mandato dentro. Parte da \
                     sola fra mezzo minuto — Ctrl-C in quella tab per fermarla."
                        .to_string(),
                )
            } else {
                ArmOutcome::Failed(format!(
                    "Consegna scritta, ma non sono riuscito ad aprire la tab: {detail}"
                ))
            };
        }
    };

    // I campi accanto al motivo sono quelli dell'originale, uno per uno: chi
    // legge il registro somma `vive` e `pannelli`, e una riga con le chiavi
    // sbagliate è una riga persa anche quando la decisione è giusta.
    let extra: Vec<(&str, Field)> = match reason {
        "fuori-orario" => vec![
            ("ora", Field::Number(facts.hour as i64)),
            ("origine", Field::Text(origin.to_string())),
        ],
        "troppe-sessioni" => vec![
            (
                "vive",
                Field::Number(facts.live_sessions.unwrap_or(0) as i64),
            ),
            ("origine", Field::Text(origin.to_string())),
        ],
        "albero-affollato" => vec![
            ("pannelli", Field::Number(facts.panes_here.unwrap_or(0) as i64)),
            ("origine", Field::Text(origin.to_string())),
        ],
        // `seconda-generazione` e ogni motivo futuro: il percorso e la strada.
        _ => vec![
            ("path_", Field::Text(path.to_string())),
            ("origine", Field::Text(origin.to_string())),
        ],
    };
    journal::record("consegna-arma-successore", "ferma", reason, &extra);
    ArmOutcome::Stop(message)
}

/// La sessione ha consumato abbastanza da essere davvero al capolinea?
///
/// Stessa misura del gancio Stop, dalla stessa fonte: il transcript arriva anche
/// nel payload di PostToolUse — `consegna-obbligatoria` lo legge di lì e scrive
/// `percento` e `token` veri nel registro, quindi è un fatto verificato e non
/// un'assunzione sul formato dell'evento.
///
/// LA SOGLIA È QUELLA DELL'AVVISO (78%), NON QUELLA DELL'OBBLIGO (90%), e le due
/// scelte si separano su casi veri del 17/08/2026. Con l'obbligo, la sessione
/// all'88% che quel giorno doveva passare il testimone — e non l'ha passato,
/// tanto che il successore l'ha avviato Theo a voce — sarebbe rimasta ferma:
/// 440k contro 450k. Con l'avviso passa, mentre `tautog` alle 16:28, a 352k su
/// 390k, resta ferma: ed era la consegna scritta a metà lavoro che ha aperto la
/// seconda sessione sullo stesso albero. L'avviso è il punto in cui la
/// configurazione stessa dice «è ora di consegnare»: una consegna scritta oltre
/// quella riga sta davvero chiudendo, una scritta prima è un salvataggio.
///
/// Fail-safe verso il **basso**: senza transcript non si sa quanto sia piena, e
/// una sessione che non si sa piena non si sostituisce. È il verso opposto agli
/// altri freni — lì «non lo so» lascia passare, perché il dubbio riguardava una
/// misura esterna. Qui riguarda la ragione stessa per cui si aprirebbe.
///
/// La valvola `CONSEGNA_ANCHE_A_META` serve a Theo per riprendere il vecchio
/// comportamento su una consegna scritta apposta per passare il lavoro.
fn session_is_full(input: &hook_io::HookInput, session: &str) -> bool {
    if std::env::var("CONSEGNA_ANCHE_A_META").is_ok() {
        return true;
    }
    let transcript = input.transcript_path.clone().unwrap_or_default();
    if transcript.is_empty() {
        return false;
    }
    let short: String = session.chars().take(8).collect();
    let used = crate::handoff::context_used(&transcript, &short);
    let t = crate::handoff::thresholds(&transcript);
    guards::successor::is_full_enough(used, t.warn)
}

/// Il gancio vero: PostToolUse, decide e — se tutti i freni sono liberi — apre.
///
/// Fail-open in ogni ramo: l'uscita è sempre 0. Un gancio che rompe la scrittura
/// di una consegna è peggio del problema che risolve.
pub fn run(input: &hook_io::HookInput) -> i32 {
    let path = input
        .tool_input
        .as_ref()
        .and_then(|v| v.get("file_path"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if path.is_empty() || !is_doc(path) {
        return 0;
    }
    let session = input.session_id.clone().unwrap_or_default();
    match arm(path, &session, "scrittura", session_is_full(input, &session)) {
        ArmOutcome::Stop(m) | ArmOutcome::Open(m) | ArmOutcome::Failed(m) => {
            // Un freno silenzioso resta silenzioso: stampare un JSON vuoto
            // riempirebbe di rumore ogni consegna scritta.
            if !m.is_empty() {
                println!("{}", speak(&m));
            }
        }
    }
    0
}

/// Risponde alle domande che lo strumento di equivalenza pone al Python.
///
/// Non è un gancio: è il punto d'aggancio del confronto. Senza un modo di
/// interrogare il Rust dall'esterno, il porting si proverebbe solo sui casi
/// scritti a mano — e sono proprio quelli a non trovare i difetti.
pub fn probe(verb: &str, a: &str, b: &str) -> i32 {
    match verb {
        "doc" => println!("{}", if is_doc(a) { "True" } else { "False" }),
        // Il mandato nudo, senza inventario: è ciò che il confronto col Python
        // esercita, e la clausola dipende da cosa gira sulla macchina in quel
        // momento — un ingresso che non si può fissare in un caso di prova.
        "mandate" => print!("{}", mandate(a, "")),
        // L'inventario da solo, per poterlo guardare senza aprire una scheda.
        "inherited" => print!("{}", inherited_clause(a)),
        "fingerprint" => println!("{}", guards::successor::armed_fingerprint(a, b)),
        "hours" => println!(
            "{}",
            match a.parse::<u32>() {
                Ok(h) if guards::successor::within_hours(h) => "True",
                _ => "False",
            }
        ),
        // I due conteggi che parlano con la macchina. Esposti qui perché il
        // confronto li eserciti contro il Python sullo stato reale: sono le due
        // risposte che i tetti usano, e provarle solo a tavolino vorrebbe dire
        // provare la soglia e non la misura da cui dipende.
        "panes" => println!(
            "{}",
            panes_here(a).map(|n| n.to_string()).unwrap_or("-1".into())
        ),
        "live" => println!(
            "{}",
            live_sessions().map(|n| n.to_string()).unwrap_or("-1".into())
        ),
        // Il tetto globale, nelle sue tre forme. `delta` e `cap` sono il
        // giudizio puro, con il conteggio passato da fuori, così il confronto
        // col Python esercita la decisione e non la macchina; `session-cap` è il
        // gancio vero, che il comando lo prende dal payload di `PreToolUse`.
        "delta" => println!(
            "{}",
            match guards::handoff::session_delta(a) {
                guards::handoff::SessionDelta::Adds => "Adds",
                guards::handoff::SessionDelta::Replaces => "Replaces",
                guards::handoff::SessionDelta::None => "None",
            }
        ),
        "cap" => {
            let facts = guards::handoff::CapFacts {
                delta: guards::handoff::session_delta(a),
                live: b.parse::<usize>().ok(),
                cap: cap("SESSION_CAP_LIMIT", guards::handoff::SESSION_CAP_DEFAULT),
            };
            print!(
                "{}",
                guards::handoff::session_cap_verdict(&facts).unwrap_or_default()
            );
        }
        "session-cap" => {
            let Some(input) = hook_io::read_input() else {
                return 0;
            };
            if !input.is_tool("Bash") {
                return 0;
            }
            return session_cap(input.bash_command());
        }
        "agents" => {
            let mut raw = String::new();
            if std::io::stdin().read_to_string(&mut raw).is_err() {
                println!("0");
                return 0;
            }
            let n = serde_json::from_str::<serde_json::Value>(&raw)
                .map(|v| count_agents(&v, a))
                .unwrap_or(0);
            println!("{n}");
        }
        _ => {
            eprintln!("verbo sconosciuto: {verb}");
            return 1;
        }
    }
    0
}
