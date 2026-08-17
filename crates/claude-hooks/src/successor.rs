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

/// I primi 400 byte del file, come li legge il Python.
///
/// La lunghezza è la stessa di proposito: il frontmatter sta in testa, e un
/// limite diverso farebbe divergere le due implementazioni su un file che
/// dichiara `type: project` al byte 401 — improbabile, ma il confronto lo
/// vedrebbe e nessuno saprebbe perché.
fn head(path: &str) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 400];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// È un documento di consegna? Legge il file solo quando serve davvero.
pub fn is_doc(path: &str) -> bool {
    is_handoff_doc(path, head(path).as_deref())
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

/// Le sessioni Claude vive adesso. `None` se non si è potuto sapere.
///
/// Si chiede al binario invece di contare i processi: un `ps | grep claude`
/// conta anche i subagent e i wrapper della shell, e sovrastima di molto.
pub fn live_sessions() -> Option<usize> {
    let out = std::process::Command::new("claude")
        .args(["agents", "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.as_array().map(|a| a.len())
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

/// Apre la scheda col mandato dentro, e la fa partire dopo un conto alla rovescia.
///
/// L'attesa non è una conferma da dare: è una via d'uscita per chi sta
/// guardando. Prima qui c'era un Invio da premere, e nessuno lo premeva — 21
/// schede armate e zero avviate. L'inerzia deve lavorare nel verso giusto.
fn open_tab(path: &str) -> (bool, String) {
    let wait_s = std::env::var("CONSEGNA_ATTESA_S")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(30);
    // Il mandato entra in una stringa fra apici singoli della shell: l'unico
    // carattere da neutralizzare è l'apice stesso.
    let text = mandate(path).replace('\'', r"'\''");
    let command = format!(
        "printf '%s\\n\\n' '{text}'; \
         printf 'Starting in {wait_s}s. Ctrl-C to cancel.\\n'; \
         sleep {wait_s}; exec claude '{text}'"
    );
    let out = std::process::Command::new("orca")
        .args([
            "terminal",
            "create",
            "--command",
            &command,
            "--title",
            "consegna raccolta (parte da sola)",
            "--json",
        ])
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
pub fn arm(path: &str, session: &str, origin: &str) -> ArmOutcome {
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
            let (ok, detail) = open_tab(path);
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
    match arm(path, &session, "scrittura") {
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
        "mandate" => print!("{}", mandate(a)),
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
