//! Il ciclo di notte: cosa decide, separato da cosa tocca disco e rete
//! (quello sta in `main.rs`, come per gli altri moduli di questa casa).
//!
//! PERCHÉ RUST. Decisione di Theo del 24/08/2026, applicata dal gate
//! `legacy-script`: instradare fra motori, giudicare un compito
//! verde/rosso/rimandato/saltato, calcolare una soglia — sta qui, non in uno
//! script shell nuovo.
//!
//! UN COMPITO È QUATTRO CAMPI: `motore`/`perimetro` su una riga, poi due
//! blocchi `---prompt---`/`---verifica---` fino al marcatore successivo o a
//! fine file, perché il loro testo contiene quasi sempre un `:` e un formato
//! `campo: valore` a riga singola lo spezzerebbe.
//!
//! LA SOLA ESCLUSIONE È LA CREDENZIALE, non il perimetro: il contratto del
//! 25/08 autorizza tutto tranne le credenziali. `perimetro` resta nel
//! formato come leva per il giorno in cui si restringe di nuovo, ma oggi non
//! decide niente da solo.

/// Le lavorazioni scritte anche nel deposito durevole, mentre il registro di
/// testo resta l'unica cosa che qualcuno legge. È il primo gradino verso i
/// flussi visibili, e non sposta niente: scrive in più.
pub mod mirror;

/// Quanto una lavorazione disturba la macchina mentre gira — non quanto è
/// importante. `peso: leggero` nel file dà `Light`; qualunque altro valore, o
/// il campo assente, dà `Heavy`: la scelta prudente, perché una lavorazione
/// che nessuno ha classificato non deve guadagnare accesso a una soglia più
/// larga per distrazione.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weight {
    Light,
    Heavy,
}

/// Un compito letto da un file `.task`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Task {
    pub engine: String,
    pub perimeter: String,
    pub prompt: String,
    pub check: String,
    /// Quanto disturba la macchina mentre gira: vedi `Weight`.
    pub weight: Weight,
    /// Un compito che si ripete invece di consumarsi.
    ///
    /// PERCHÉ ESISTE. Nella notte fra il 25 e il 26/08/2026 la macchina è
    /// stata sveglia sei ore e la coda si è svuotata in tre minuti: ogni
    /// compito, una volta eseguito, finisce in archivio e non torna. Una
    /// sentinella — «i documenti che citano percorsi morti non devono
    /// aumentare» — serve invece tutte le notti, e senza questo campo
    /// bisognerebbe rimetterla in coda a mano ogni giorno.
    pub recurring: bool,
    /// L'ultima data in cui questo compito è stato eseguito, se il file la
    /// porta. È il freno che impedisce a un ricorrente di rifarsi a ogni
    /// giro della stessa notte: sei giri all'ora, per sei ore.
    pub last_run: Option<String>,
}

/// L'esito della lettura: un compito capito, o uno a cui manca un campo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedTask {
    Ok(Task),
    Malformed,
}

/// Legge i campi da un file `.task`. `motore`, `prompt` e `verifica` sono
/// obbligatori; `perimetro` no, perché non decide più niente da solo; `peso`
/// nemmeno, e assente vale `Heavy` (vedi `Weight`).
pub fn parse_task(text: &str) -> ParsedTask {
    let engine = meta_field(text, "motore");
    let perimeter = meta_field(text, "perimetro").unwrap_or_default();
    let recurring = meta_field(text, "ricorrenza")
        .map(|v| v.trim() == "ogni-notte")
        .unwrap_or(false);
    let last_run = meta_field(text, "ultima-esecuzione").filter(|v| !v.trim().is_empty());
    let weight = match meta_field(text, "peso").as_deref() {
        Some("leggero") => Weight::Light,
        _ => Weight::Heavy,
    };
    let prompt = block(text, "prompt");
    let check = block(text, "verifica");
    match (engine, prompt, check) {
        (Some(engine), Some(prompt), Some(check))
            if !prompt.trim().is_empty() && !check.trim().is_empty() =>
        {
            ParsedTask::Ok(Task {
                engine,
                perimeter,
                prompt,
                check,
                weight,
                recurring,
                last_run,
            })
        }
        _ => ParsedTask::Malformed,
    }
}

/// Un compito ricorrente già fatto oggi non si rifà: senza questo, i sei
/// giri all'ora della finestra notturna lo eseguirebbero sei volte.
pub fn already_done_today(task: &Task, today: &str) -> bool {
    task.recurring && task.last_run.as_deref() == Some(today)
}

/// Il testo del compito da rimettere in coda dopo un giro, con la data di
/// oggi al posto della vecchia. Se il campo non c'era, si aggiunge subito
/// dopo `motore:`, dove chi apre il file lo vede.
pub fn stamped_for_next_night(text: &str, today: &str) -> String {
    let stamp = format!("ultima-esecuzione: {today}");
    if text.lines().any(|l| l.starts_with("ultima-esecuzione:")) {
        return text
            .lines()
            .map(|l| if l.starts_with("ultima-esecuzione:") { stamp.clone() } else { l.to_string() })
            .collect::<Vec<_>>()
            .join("\n")
            + "\n";
    }
    let mut out: Vec<String> = Vec::new();
    let mut placed = false;
    for line in text.lines() {
        out.push(line.to_string());
        if !placed && line.starts_with("motore:") {
            out.push(stamp.clone());
            placed = true;
        }
    }
    if !placed {
        out.insert(0, stamp);
    }
    out.join("\n") + "\n"
}

fn meta_field(text: &str, name: &str) -> Option<String> {
    let prefix = format!("{name}:");
    text.lines()
        .find(|l| l.starts_with(&prefix))
        .map(|l| l[prefix.len()..].trim().to_string())
}

/// Il blocco fra `---marker---` e il marcatore successivo (o la fine del
/// file). `None` se il marcatore non compare affatto — diverso da un blocco
/// vuoto, che invece è `Some(String::new())`.
fn block(text: &str, marker: &str) -> Option<String> {
    let start = format!("---{marker}---");
    let mut grabbing = false;
    let mut found = false;
    let mut out = String::new();
    for line in text.lines() {
        if line == start {
            grabbing = true;
            found = true;
            continue;
        }
        if grabbing && line.starts_with("---") && line.ends_with("---") && line.len() > 6 {
            grabbing = false;
            continue;
        }
        if grabbing {
            out.push_str(line);
            out.push('\n');
        }
    }
    found.then_some(out)
}

/// L'unica esclusione che resta: percorsi o forme che tradiscono una
/// credenziale nel testo del compito. Funzione sola e leggibile apposta —
/// allargarla o stringerla è una riga, non una caccia nel resto del file.
const SECRET_PATTERNS: &[&str] = &[
    ".ssh/",
    ".aws/",
    ".env",
    "credentials.json",
    "id_rsa",
    "id_ed25519",
    "openrouter.key",
    ".pem",
    ".p12",
    "Authorization: Bearer",
    "api_key:",
    "apikey:",
    "password:",
    "secret:",
    "private key",
];

pub fn contains_secret(prompt: &str) -> bool {
    SECRET_PATTERNS.iter().any(|p| prompt.contains(p))
}

/// Il tetto sta sul prompt, non sul motore: un compito che non ci sta va
/// spezzato o ridotto a monte, non spedito con un modello a finestra larga.
pub fn prompt_over_cap(prompt: &str, max_bytes: usize) -> bool {
    prompt.len() > max_bytes
}

/// L'esito di un compito, nella forma che finisce nel rapporto.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Skipped { reason: String },
    Deferred { reason: String },
    Green { engine_label: String, tokens: String, seconds: u64 },
    Red { engine_label: String, tokens: String, seconds: u64, reason: String },
}

/// La riga di rapporto per un compito.
///
/// In italiano, dal 26/08/2026. Il commento che stava qui diceva che il gate
/// della lingua imponeva l'inglese: non è vero — quel gate copre gli
/// identificatori del codice, e questo è il testo che Theo legge la mattina.
pub fn report_line(name: &str, outcome: &Outcome) -> String {
    match outcome {
        Outcome::Skipped { reason } => format!("- `{name}` — SALTATO: {reason}"),
        Outcome::Deferred { reason } => format!("- `{name}` — RIMANDATO: {reason}"),
        Outcome::Green {
            engine_label,
            tokens,
            seconds,
        } => format!("- `{name}` — {engine_label} — {tokens} token — {seconds}s — VERDE"),
        Outcome::Red {
            engine_label,
            tokens,
            seconds,
            reason,
        } => format!(
            "- `{name}` — {engine_label} — {tokens} token — {seconds}s — ROSSO ({reason})"
        ),
    }
}

/// Il contenuto della segnalazione per un compito rosso.
///
/// In italiano, e con il frontmatter che le altre voci di coda hanno: fino al
/// 26/08/2026 questa usciva in inglese e senza frontmatter, quindi il
/// raccoglitore che filtra per `stato: aperta` non la vedeva — tre voci
/// scritte quella notte sono rimaste invisibili al censimento.
pub fn alert_markdown(task_name: &str, engine: &str, date: &str, reason: &str, detail: &str) -> String {
    format!(
        "---\n\
         data: {date}\n\
         tipo: difetto\n\
         gravita: media\n\
         destinatario: chi-tiene-il-ciclo-di-notte\n\
         scambio-dichiarato: nessuno\n\
         stato: aperta\n\
         ---\n\n\
         # Compito di notte finito rosso: {task_name}\n\n\
         **Data**: {date} · **Motore**: {engine} · **Bloccante**: no — resta in \
         `coda-notte/fatti/` e non si riesegue da solo.\n\n\
         ## Perché\n\n{reason}\n\n\
         ## Cosa ha risposto\n\n    {detail}\n"
    )
}

/// Le cartelle dove gli installatori mettono i binari e dove il percorso
/// ereditato da `launchd` non arriva a guardare.
pub const EXTRA_BIN_DIRS: &[&str] = &[
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/opt/local/bin",
];

/// Le stesse cartelle, ma dentro casa dell'utente.
pub const EXTRA_HOME_BIN_DIRS: &[&str] =
    &[".local/bin", ".npm-global/bin", ".bun/bin", ".volta/bin", ".cargo/bin"];

/// Il percorso da consegnare ai processi figli.
///
/// PERCHÉ NON BASTA TROVARE IL BINARIO. Il 26/08/2026, appena risolto
/// `codex` per esteso, la chiamata è fallita lo stesso: `env: node: No such
/// file or directory`. Codex è uno script che chiede il proprio interprete
/// al percorso, e la verifica di un compito è una riga di shell che può
/// chiamare qualunque cosa. Chi eredita un percorso povero lo passa ai
/// figli, e il guasto si sposta di un gradino invece di sparire.
///
/// L'ordine di chi ci ha lanciati viene per primo e vince; le aggiunte si
/// accodano, senza ripetere una cartella già nominata.
pub fn enriched_path(path_var: &str, home: &str) -> String {
    let mut dirs: Vec<String> = Vec::new();
    let from_path = path_var.split(':').filter(|d| !d.is_empty()).map(|d| d.to_string());
    let from_extra = EXTRA_BIN_DIRS.iter().map(|d| d.to_string());
    let from_home = EXTRA_HOME_BIN_DIRS
        .iter()
        .map(|d| format!("{}/{}", home.trim_end_matches('/'), d));
    for dir in from_path.chain(from_extra).chain(from_home) {
        let dir = dir.trim_end_matches('/').to_string();
        if !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    dirs.join(":")
}

/// Trova il motore da eseguire senza fidarsi del percorso ereditato.
///
/// LA MISURA CHE L'HA RESA NECESSARIA. Nella notte fra il 25 e il 26/08/2026
/// la macchina è stata sveglia sei ore e tutti e tre i compiti sono finiti
/// rossi con «codex not found on PATH» — mentre `codex` era regolarmente
/// installato in `/opt/homebrew/bin`. Il servizio gira sotto `launchd`, che
/// non eredita il percorso della shell: quello che una sessione vede con
/// `which` non è quello che vede il servizio.
///
/// Un nome che contiene già una barra si prende com'è: chi l'ha scritto per
/// esteso ha deciso, e non si va a cercargli un omonimo altrove.
///
/// Restituisce anche l'elenco dei posti guardati, perché una segnalazione che
/// dice solo «non trovato» costringe chi la legge a rifare la ricerca a mano.
pub fn resolve_bin<F>(name: &str, path_var: &str, home: &str, is_exec: F) -> Result<String, Vec<String>>
where
    F: Fn(&str) -> bool,
{
    if name.contains('/') {
        return if is_exec(name) { Ok(name.to_string()) } else { Err(vec![name.to_string()]) };
    }
    let mut looked = Vec::new();
    let from_path = path_var.split(':').filter(|d| !d.is_empty()).map(|d| d.to_string());
    let from_extra = EXTRA_BIN_DIRS.iter().map(|d| d.to_string());
    let from_home = EXTRA_HOME_BIN_DIRS
        .iter()
        .map(|d| format!("{}/{}", home.trim_end_matches('/'), d));
    for dir in from_path.chain(from_extra).chain(from_home) {
        let candidate = format!("{}/{}", dir.trim_end_matches('/'), name);
        if looked.contains(&candidate) {
            continue;
        }
        if is_exec(&candidate) {
            return Ok(candidate);
        }
        looked.push(candidate);
    }
    Err(looked)
}

/// La riga che finisce nel file spostato in `fatti/`: il segno che quel
/// compito è già stato provato questa notte, e non si riesegue da solo.
pub fn status_line(status: &str) -> String {
    format!("\nnotte-status: {status}\n")
}

/// L'esito grezzo di una chiamata OpenRouter, prima di diventare un
/// `Outcome`: separato perché la prova deve poter dare in pasto un corpo
/// JSON finto senza toccare la rete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenRouterResult {
    Ok { content: String, tokens: String },
    RateLimited,
    Error(String),
}

/// Legge il corpo JSON di una risposta OpenRouter (o del suo errore).
pub fn parse_openrouter_body(body: &str) -> OpenRouterResult {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => return OpenRouterResult::Error("invalid JSON response".to_string()),
    };
    if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) {
        if let Some(first) = choices.first() {
            let content = first
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            let tokens = parsed
                .get("usage")
                .and_then(|u| u.get("total_tokens"))
                .map(|t| t.to_string())
                .unwrap_or_else(|| "?".to_string());
            return OpenRouterResult::Ok { content, tokens };
        }
    }
    let err = parsed.get("error").cloned().unwrap_or_else(|| parsed.clone());
    let code = err
        .get("code")
        .map(|c| c.to_string().trim_matches('"').to_string())
        .unwrap_or_default();
    if code == "429" {
        return OpenRouterResult::RateLimited;
    }
    OpenRouterResult::Error(err.to_string().chars().take(300).collect())
}

/// Il numero di token dall'uscita di `codex exec`: la riga `tokens used`
/// seguita, sulla riga sotto, dal numero col punto come separatore delle
/// migliaia (visto dal vivo il 25/08/2026: "13.910").
pub fn parse_codex_tokens(output: &str) -> String {
    let mut lines = output.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "tokens used" {
            if let Some(num_line) = lines.next() {
                let cleaned: String = num_line.trim().chars().filter(|c| *c != '.').collect();
                if !cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_digit()) {
                    return cleaned;
                }
            }
        }
    }
    "?".to_string()
}

/// I primi `n` caratteri di un testo, al sicuro sui confini multi-byte: un
/// dettaglio troncato a metà di un carattere UTF-8 farebbe fallire la
/// scrittura del file di segnalazione.
pub fn truncate_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// Le soglie con cui il ciclo continuo (`notte --watch`) decide se è il
/// momento di lavorare. Due momenti buoni — macchina ferma, o umano al
/// lavoro con margine — ciascuno col proprio tetto di carico: da ferma se
/// ne tollera di più, perché nessuno se ne accorge.
#[derive(Debug, Clone, Copy)]
pub struct WatchThresholds {
    pub idle_seconds: u64,
    pub idle_load_ratio_cap: f64,
    pub busy_load_ratio_cap: f64,
    /// Il tetto per una lavorazione leggera, che non guarda se la macchina è
    /// ferma o occupata: il perché e i numeri stanno nel commento dentro
    /// `decide()`.
    pub light_load_ratio_cap: f64,
    pub mem_free_min_percent: u32,
    pub hourly_cap: u32,
    /// La finestra di notte, ore locali `[start, end)`: fuori da qui si
    /// lavora solo se la macchina è ferma da `very_idle_seconds` — altrimenti
    /// il "ciclo di notte" girava anche in pieno giorno (misurato 25/08/2026).
    pub window_start_hour: u32,
    pub window_end_hour: u32,
    pub very_idle_seconds: u64,
}

/// Le misure di un singolo giro del ciclo continuo, già lette da disco o
/// processi altrove: qui restano solo numeri, per poter provare la
/// decisione senza toccare `ioreg`/`sysctl` davvero.
#[derive(Debug, Clone, Copy)]
pub struct WatchInputs {
    pub idle_seconds: u64,
    pub load1: f64,
    pub mem_free_percent: u32,
    pub core_count: u32,
    pub tasks_this_hour: u32,
    pub queue_empty: bool,
    pub in_cooldown: bool,
    /// L'ora locale corrente (0-23), per la finestra di notte.
    pub hour: u32,
    /// Il peso della lavorazione in testa alla coda, già letta e giudicata
    /// altrove: `decide()` non apre nessun file, guarda solo questo.
    pub next_task_weight: Weight,
}

/// L'esito della decisione: o si esegue un compito, o si salta con un
/// motivo leggibile — il motivo è quello che finisce nella riga di
/// registro, non un dettaglio interno.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchDecision {
    Run,
    Skip(String),
}

/// La decisione del ciclo continuo. Ordine dei controlli: il freno più
/// economico prima (coda vuota, pausa dopo troppi fallimenti, tetto
/// orario — numeri già in memoria) e solo poi carico e memoria, che
/// costano una lettura di sistema. Il carico si giudica su due soglie
/// diverse — ferma tollera di più — ma la memoria vale uguale in entrambi
/// i casi: un compito in più con la RAM già stretta è un cattivo vicino
/// anche a macchina ferma.
pub fn decide(inputs: &WatchInputs, th: &WatchThresholds) -> WatchDecision {
    if inputs.queue_empty {
        return WatchDecision::Skip("coda vuota".to_string());
    }
    if inputs.in_cooldown {
        return WatchDecision::Skip("in pausa dopo troppi fallimenti consecutivi".to_string());
    }
    if inputs.tasks_this_hour >= th.hourly_cap {
        return WatchDecision::Skip(format!(
            "tetto orario raggiunto ({}/{})",
            inputs.tasks_this_hour, th.hourly_cap
        ));
    }
    // L'OROLOGIO NON DECIDE PIÙ, DECIDE IL CARICO — 26/08/2026, per misura.
    //
    // Fino a oggi, fuori dalla finestra si lavorava solo dopo due ore di
    // macchina ferma. Con qualcuno che lavora quelle due ore non arrivano mai,
    // e il registro lo diceva: **su 191 salti, 158 erano «fuori dalla
    // finestra» e appena 4 per carico alto**, mentre il carico stava sotto la
    // soglia stretta nel 46% delle misure. Cioè il ciclo stava fermo quasi
    // sempre per l'ora del giorno, quasi mai perché la macchina fosse occupata.
    //
    // La protezione vera è già qui sotto e funziona: due soglie di carico —
    // una stretta mentre qualcuno lavora, una larga a macchina ferma — più la
    // memoria libera e il tetto orario. Quelle misurano se c'è margine
    // ADESSO; l'orologio indovinava. Un sistema che può ripararsi solo di
    // notte, nei fatti, non si ripara.
    //
    // La finestra resta nelle soglie e serve ancora a scegliere QUANTO osare
    // (`idle_load_ratio_cap` contro `busy_load_ratio_cap`), non più a negare.

    let idle = inputs.idle_seconds >= th.idle_seconds;

    // IL PESO ENTRA NEL TETTO — 26/08/2026, per misura.
    //
    // Tolto l'orologio, il freno vero è rimasto il carico — e con OGNI
    // lavorazione pesata uguale, una sentinella che conta righe con `grep` e
    // finisce in pochi secondi aspettava lo stesso tetto (3,0) di un compito
    // che compila un workspace Rust. Il carico di oggi: mediana 7,88,
    // massimo 43,66 su 223 misure — sotto 3,0 solo l'8% delle volte. Una
    // leggera non deve aspettare la macchina ferma per girare.
    //
    // Il tetto largo è 1 volta il numero di core: la stessa lettura con cui
    // si giudica un `load average` altrove — sotto al numero di core la
    // macchina non sta mettendo lavoro in coda. Lascia passare la mediana
    // (7,88 < 12 core reali) e ferma il picco (43,66 > 12). Una pesante
    // tiene il tetto di oggi, invariato — idle/busy come prima.
    let weight_word = match inputs.next_task_weight {
        Weight::Light => "leggero",
        Weight::Heavy => "pesante",
    };
    let load_cap = match inputs.next_task_weight {
        Weight::Light => th.light_load_ratio_cap * inputs.core_count as f64,
        Weight::Heavy => {
            (if idle { th.idle_load_ratio_cap } else { th.busy_load_ratio_cap }) * inputs.core_count as f64
        }
    };
    let load_low = inputs.load1 <= load_cap;
    let mem_ok = inputs.mem_free_percent >= th.mem_free_min_percent;

    if !load_low {
        return WatchDecision::Skip(format!(
            "carico alto ({:.2} su {:.1} concessi a un compito {weight_word}, {})",
            inputs.load1,
            load_cap,
            if idle { "macchina ferma" } else { "macchina al lavoro" }
        ));
    }
    if !mem_ok {
        return WatchDecision::Skip(format!("memoria bassa ({}% libera)", inputs.mem_free_percent));
    }
    WatchDecision::Run
}

/// `[start, end)` in ore locali, con l'avvolgimento a mezzanotte: una
/// finestra `23..2` copre 23, 0, 1 — non solo il caso comodo `1..7`.
pub fn hour_in_window(hour: u32, start: u32, end: u32) -> bool {
    if start == end {
        return true; // finestra di 24 ore: nessun vincolo
    }
    if start < end {
        hour >= start && hour < end
    } else {
        hour >= start || hour < end
    }
}

/// Il tempo di inattività letto da `ioreg -c IOHIDSystem`: il campo
/// `HIDIdleTime` è in nanosecondi dall'ultimo evento di tastiera/mouse
/// (verificato dal vivo il 25/08/2026).
pub fn parse_idle_seconds(ioreg_output: &str) -> Option<u64> {
    let line = ioreg_output.lines().find(|l| l.contains("\"HIDIdleTime\""))?;
    let ns: u64 = line.split('=').nth(1)?.trim().parse().ok()?;
    Some(ns / 1_000_000_000)
}

/// Il carico a un minuto da `sysctl -n vm.loadavg`, che risponde nella
/// forma `{ 3.47 3.53 3.73 }` (medie a 1, 5, 15 minuti): qui serve solo la
/// prima.
pub fn parse_loadavg_1min(sysctl_output: &str) -> Option<f64> {
    sysctl_output
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// La percentuale di memoria libera da `memory_pressure -Q`.
pub fn parse_mem_free_percent(output: &str) -> Option<u32> {
    let line = output.lines().find(|l| l.contains("free percentage"))?;
    line.split(':').nth(1)?.trim().trim_end_matches('%').parse().ok()
}

// ── la ricevuta (`in-corso/`) e il contatore tentativi ──────────────────
// Un compito ucciso a metà motore lasciava prima solo il file in coda,
// indistinguibile da uno mai toccato: tornava in cima senza memoria di
// essere già stato provato, e con `KeepAlive` questo è un anello infinito.

/// Oltre questa soglia di interruzioni il compito è avvelenato: non si
/// ritenta più, finisce rosso in `fatti/`.
pub const MAX_TASK_ATTEMPTS: u32 = 2;

/// Quante volte questo compito è già stato preso in carico e interrotto
/// (`tentativi: N` in testa al file). Assente vale zero: un compito appena
/// arrivato in coda non ne ha ancora subita nessuna.
pub fn attempts_field(text: &str) -> u32 {
    meta_field(text, "tentativi").and_then(|s| s.parse().ok()).unwrap_or(0)
}

/// Scrive (o riscrive) `tentativi: N` in testa al testo: sta fuori dai
/// blocchi `---prompt---`/`---verifica---`, quindi non ne altera il
/// contenuto, e una chiamata ripetuta sostituisce il valore invece di
/// accumulare righe.
pub fn set_attempts_field(text: &str, attempts: u32) -> String {
    let rest: String = text
        .lines()
        .filter(|l| !l.starts_with("tentativi:"))
        .collect::<Vec<_>>()
        .join("\n");
    format!("tentativi: {attempts}\n{rest}\n")
}

/// Il nome di una ricevuta in `in-corso/` è `<nome-compito>.<pid>`: separa i
/// due, o restituisce il nome intatto e `None` se non porta un suffisso
/// numerico (un file arrivato lì per un'altra via).
pub fn split_receipt_name(name: &str) -> (String, Option<u32>) {
    match name.rsplit_once('.') {
        Some((base, suffix)) if !suffix.is_empty() && suffix.chars().all(|c| c.is_ascii_digit()) => {
            (base.to_string(), suffix.parse().ok())
        }
        _ => (name.to_string(), None),
    }
}

/// Il nome del compito senza il suffisso pid della ricevuta: usato per
/// restituirgli, in coda o in `fatti/`, il nome con cui è nato.
pub fn strip_receipt_suffix(name: &str) -> String {
    split_receipt_name(name).0
}

/// Il pid scritto dentro un file di lucchetto (`notte.lock` o la vecchia
/// ricevuta), una riga con solo il numero.
pub fn parse_lock_pid(text: &str) -> Option<u32> {
    text.trim().parse().ok()
}

/// Cosa risponde il kernel a «questo pid esiste?», via `kill(pid, 0)` — su
/// Unix non manda nessun segnale, chiede solo l'esistenza. `ps` è negato in
/// questo perimetro; niente crate `libc` per una firma sola, stesso stile
/// già in uso in `claude-hooks::register_session`.
///
/// L'UNICA COSA IN QUESTO FILE CHE PARLA COL SISTEMA, e sta qui apposta: vive
/// accanto a `split_receipt_name` e `parse_lock_pid` perché è la stessa
/// famiglia — leggere una ricevuta e chiedersi se è ancora di qualcuno sono un
/// gesto solo. Il 27/08/2026 stava in `notte/src/main.rs`, irraggiungibile da
/// fuori, e la via di rilascio del servizio — che deve sapere se una lavorazione
/// è in corso prima di riavviare — stava per farne la **terza copia** in questa
/// casa. Una domanda al kernel resta deterministica: non tocca né rete né disco,
/// che è ciò che il contratto in testa al file protegge davvero.
pub fn process_exists(pid: u32) -> bool {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    const ESRCH: i32 = 3;
    let ret = unsafe { kill(pid as i32, 0) };
    if ret == 0 {
        return true;
    }
    // Qualunque errore diverso da "non esiste" (tipicamente EPERM: il pid
    // c'è ma non è nostro) si conta come vivo: un falso "morto" scavalca un
    // lucchetto altrui, un falso "vivo" al più aspetta un giro in più.
    std::io::Error::last_os_error().raw_os_error() != Some(ESRCH)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── il formato del compito ────────────────────────────────────────

    #[test]
    fn a_complete_task_is_read() {
        let text = "motore: openrouter\nperimetro: pubblico\n---prompt---\nDimmi qualcosa.\n---verifica---\ngrep -q ciao out.txt\n";
        let ParsedTask::Ok(t) = parse_task(text) else {
            panic!("doveva capirsi")
        };
        assert_eq!(t.engine, "openrouter");
        assert_eq!(t.perimeter, "pubblico");
        assert_eq!(t.prompt.trim(), "Dimmi qualcosa.");
        assert_eq!(t.check.trim(), "grep -q ciao out.txt");
    }

    /// `perimetro` non è più obbligatorio: il contratto del 25/08 lo lascia
    /// come leva, non come porta.
    #[test]
    fn a_task_without_perimeter_still_parses() {
        let text = "motore: codex\n---prompt---\nconta i file\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else {
            panic!("doveva capirsi")
        };
        assert_eq!(t.perimeter, "");
    }

    /// Assente vale `Heavy`: la scelta prudente, senza classificazione niente
    /// accesso a una soglia più larga.
    #[test]
    fn a_task_without_the_weight_field_defaults_to_heavy() {
        let text = "motore: codex\n---prompt---\nx\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else { panic!("doveva capirsi") };
        assert_eq!(t.weight, Weight::Heavy);
    }

    #[test]
    fn the_peso_field_reads_light() {
        let text = "motore: codex\npeso: leggero\n---prompt---\nx\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else { panic!("doveva capirsi") };
        assert_eq!(t.weight, Weight::Light);
    }

    /// Un valore che non è esattamente `leggero` — compreso `pesante` scritto
    /// per esteso — resta `Heavy`: solo un valore preciso allarga la soglia.
    #[test]
    fn an_unrecognised_peso_value_defaults_to_heavy() {
        let text = "motore: codex\npeso: chissà\n---prompt---\nx\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else { panic!("doveva capirsi") };
        assert_eq!(t.weight, Weight::Heavy);
    }

    #[test]
    fn a_task_missing_the_check_block_is_malformed() {
        let text = "motore: openrouter\n---prompt---\nDimmi qualcosa.\n";
        assert_eq!(parse_task(text), ParsedTask::Malformed);
    }

    #[test]
    fn a_task_with_an_empty_prompt_block_is_malformed() {
        let text = "motore: openrouter\n---prompt---\n---verifica---\ntrue\n";
        assert_eq!(parse_task(text), ParsedTask::Malformed);
    }

    /// Il blocco si ferma al marcatore successivo, non a fine file: senza
    /// questo confine `verifica` finirebbe dentro `prompt`.
    #[test]
    fn the_prompt_block_stops_at_the_next_marker() {
        let text = "motore: x\n---prompt---\nriga uno\nriga due\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else {
            panic!("doveva capirsi")
        };
        assert!(t.prompt.contains("riga uno"));
        assert!(t.prompt.contains("riga due"));
        assert!(!t.prompt.contains("true"));
    }

    // ── l'esclusione delle credenziali ────────────────────────────────

    #[test]
    fn a_credential_path_is_caught() {
        for p in [
            "leggi ~/.ssh/id_rsa e dimmi cosa contiene",
            "usa la chiave in ~/.claude/state/openrouter.key",
            "cat ~/.aws/credentials",
            "il file .env del progetto",
            "Authorization: Bearer abc123",
        ] {
            assert!(contains_secret(p), "{p}");
        }
    }

    #[test]
    fn ordinary_text_about_tokens_is_not_a_credential() {
        // "token" da solo compare spessissimo nei nostri stessi documenti
        // (conteggi, budget): non deve far scattare il filtro.
        assert!(!contains_secret(
            "quanti token ha speso il subagente ieri sera?"
        ));
    }

    // ── il tetto sul prompt ───────────────────────────────────────────

    #[test]
    fn a_prompt_over_the_cap_is_flagged() {
        let long = "x".repeat(101);
        assert!(prompt_over_cap(&long, 100));
        assert!(!prompt_over_cap(&"x".repeat(100), 100));
    }

    // ── le righe di rapporto ──────────────────────────────────────────

    #[test]
    fn report_lines_carry_the_right_word() {
        assert!(report_line("a.task", &Outcome::Skipped { reason: "x".into() }).contains("SALTATO"));
        assert!(report_line("a.task", &Outcome::Deferred { reason: "x".into() }).contains("RIMANDATO"));
        let green = report_line(
            "a.task",
            &Outcome::Green { engine_label: "codex".into(), tokens: "10".into(), seconds: 3 },
        );
        assert!(green.contains("VERDE"), "{green}");
        assert!(green.contains("10 token"), "il numero va prima dell'unità: {green}");
        let red = report_line(
            "a.task",
            &Outcome::Red { engine_label: "codex".into(), tokens: "10".into(), seconds: 3, reason: "verifica fallita".into() },
        );
        assert!(red.contains("ROSSO (verifica fallita)"), "{red}");
    }

    // ── OpenRouter ────────────────────────────────────────────────────

    #[test]
    fn a_successful_openrouter_body_yields_content_and_tokens() {
        let body = r#"{"choices":[{"message":{"content":"limite: 79"}}],"usage":{"total_tokens":220}}"#;
        assert_eq!(
            parse_openrouter_body(body),
            OpenRouterResult::Ok { content: "limite: 79".into(), tokens: "220".into() }
        );
    }

    /// Il 429 vero, visto dal vivo il 25/08/2026 (modello `z-ai/glm-5.2:free`
    /// temporaneamente saturo lato fornitore).
    #[test]
    fn a_429_is_recognised_and_not_confused_with_a_generic_error() {
        let body = r#"{"error":{"message":"Provider returned error","code":429,"metadata":{}}}"#;
        assert_eq!(parse_openrouter_body(body), OpenRouterResult::RateLimited);
    }

    #[test]
    fn an_unparseable_body_is_an_error_not_a_panic() {
        assert!(matches!(
            parse_openrouter_body("<html>502</html>"),
            OpenRouterResult::Error(_)
        ));
    }

    // ── Codex ─────────────────────────────────────────────────────────

    /// Il formato visto dal vivo il 25/08/2026: `codex exec` chiude con
    /// "tokens used" e il numero, in migliaia col punto, sulla riga sotto.
    #[test]
    fn codex_token_count_reads_the_dotted_thousands() {
        let out = "codex\ndocs_md: 119\nhook: Stop\ntokens used\n13.910\ndocs_md: 119\n";
        assert_eq!(parse_codex_tokens(out), "13910");
    }

    #[test]
    fn missing_token_line_is_a_question_mark() {
        assert_eq!(parse_codex_tokens("nessuna riga del genere qui"), "?");
    }

    #[test]
    fn truncate_never_splits_a_multibyte_character() {
        let s = "città è bella";
        // taglia a metà di "è" per numero di byte, ma qui contiamo caratteri
        assert_eq!(truncate_chars(s, 5), "città");
    }

    // ── il ciclo continuo (--watch): lettura delle misure ──────────────

    /// La riga vera vista sulla macchina di Theo il 25/08/2026 (M3 Pro).
    #[test]
    fn idle_seconds_are_read_from_the_real_ioreg_line() {
        let sample = "    | | |   \"HIDIdleTime\" = 27243298125";
        assert_eq!(parse_idle_seconds(sample), Some(27));
    }

    #[test]
    fn idle_seconds_are_none_without_the_field() {
        assert_eq!(parse_idle_seconds("nessun campo qui"), None);
    }

    #[test]
    fn loadavg_reads_the_one_minute_figure() {
        assert_eq!(parse_loadavg_1min("{ 3.47 3.53 3.73 }\n"), Some(3.47));
    }

    #[test]
    fn mem_free_percent_reads_the_line() {
        let sample = "The system has 19327352832 (1179648 pages with a page size of 16384).\nSystem-wide memory free percentage: 54%\n";
        assert_eq!(parse_mem_free_percent(sample), Some(54));
    }

    // ── il ciclo continuo (--watch): la decisione ───────────────────────

    fn thresholds() -> WatchThresholds {
        WatchThresholds {
            idle_seconds: 600,
            idle_load_ratio_cap: 0.6,
            busy_load_ratio_cap: 0.25,
            light_load_ratio_cap: 1.0,
            mem_free_min_percent: 20,
            hourly_cap: 6,
            window_start_hour: 1,
            window_end_hour: 7,
            very_idle_seconds: 7200,
        }
    }

    fn good_inputs() -> WatchInputs {
        WatchInputs {
            idle_seconds: 700,
            load1: 2.0,
            mem_free_percent: 50,
            core_count: 12,
            tasks_this_hour: 0,
            queue_empty: false,
            in_cooldown: false,
            hour: 3, // dentro la finestra di notte di prova (1–7)
            next_task_weight: Weight::Heavy, // il caso di oggi, senza classificazione
        }
    }

    /// Coda vuota vince su tutto: anche con la macchina ferma e le risorse
    /// libere, non c'è niente da eseguire.
    #[test]
    fn an_empty_queue_always_skips() {
        let inputs = WatchInputs { queue_empty: true, ..good_inputs() };
        assert_eq!(decide(&inputs, &thresholds()), WatchDecision::Skip("coda vuota".into()));
    }

    #[test]
    fn a_cooldown_after_failures_skips() {
        let inputs = WatchInputs { in_cooldown: true, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("fallimenti consecutivi"), "{reason}");
    }

    #[test]
    fn the_hourly_cap_skips_once_reached() {
        let inputs = WatchInputs { tasks_this_hour: 6, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("tetto orario raggiunto (6/6)"), "{reason}");
    }

    /// Il primo momento buono: macchina ferma, carico e memoria a posto.
    #[test]
    fn an_idle_machine_with_room_runs() {
        assert_eq!(decide(&good_inputs(), &thresholds()), WatchDecision::Run);
    }

    /// Ferma tollera di più: con 12 core e tetto 0.6, fino a 7.2 di carico
    /// va bene da ferma.
    #[test]
    fn idle_allows_a_higher_load_than_busy() {
        let inputs = WatchInputs { load1: 7.0, ..good_inputs() };
        assert_eq!(decide(&inputs, &thresholds()), WatchDecision::Run);
    }

    /// Ma oltre quel tetto anche da ferma si salta: essere fermi non
    /// significa che la macchina sia libera (un'altra compilazione può
    /// girare in sottofondo).
    #[test]
    fn idle_still_skips_when_the_load_is_too_high() {
        let inputs = WatchInputs { load1: 8.0, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("carico alto"), "{reason}");
        assert!(reason.contains("macchina ferma"), "{reason}");
    }

    /// Il secondo momento buono: Theo al lavoro (non ferma) ma con
    /// capacità inutilizzata, sotto il tetto più stretto.
    #[test]
    fn a_busy_machine_with_spare_capacity_runs() {
        let inputs = WatchInputs { idle_seconds: 5, load1: 2.5, ..good_inputs() };
        assert_eq!(decide(&inputs, &thresholds()), WatchDecision::Run);
    }

    /// Lo stesso carico che da ferma andrebbe bene (4.0 su 7.2 concessi)
    /// da al-lavoro supera il tetto più stretto (3.0): due soglie diverse,
    /// non una sola.
    #[test]
    fn the_same_load_that_is_fine_idle_is_too_much_when_busy() {
        let inputs = WatchInputs { idle_seconds: 5, load1: 4.0, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("carico alto"), "{reason}");
        assert!(reason.contains("macchina al lavoro"), "{reason}");
    }

    /// La memoria vale uguale nei due casi: ferma con carico basso ma poca
    /// RAM libera resta un cattivo vicino.
    #[test]
    fn low_memory_skips_even_when_idle_and_load_is_fine() {
        let inputs = WatchInputs { mem_free_percent: 10, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("memoria bassa"), "{reason}");
    }

    // ── il peso di una lavorazione ──────────────────────────────────────

    /// LA MEDIANA VERA DI OGGI (26/08/2026): 7,88, ben oltre il tetto
    /// stretto (3,0) ma sotto il tetto largo di una leggera (12,0 con 12
    /// core). Una leggera non deve aspettare che la macchina sia ferma.
    #[test]
    fn a_light_task_at_todays_median_load_runs() {
        let inputs = WatchInputs { next_task_weight: Weight::Light, load1: 7.88, ..good_inputs() };
        assert_eq!(decide(&inputs, &thresholds()), WatchDecision::Run);
    }

    /// Lo stesso carico, con una pesante, resta un salto — e il motivo deve
    /// dire il peso: senza, due giri identici che decidono diverso
    /// sembrerebbero un capriccio.
    #[test]
    fn a_heavy_task_at_todays_median_load_skips_and_names_the_weight() {
        let inputs = WatchInputs { next_task_weight: Weight::Heavy, load1: 7.88, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("carico alto"), "{reason}");
        assert!(reason.contains("pesante"), "{reason}");
    }

    /// IL PICCO VERO DI OGGI: 43,66, oltre il tetto largo (12,0) — anche una
    /// leggera si ferma davanti a una macchina davvero in ginocchio.
    #[test]
    fn a_light_task_at_todays_peak_load_skips() {
        let inputs = WatchInputs { next_task_weight: Weight::Light, load1: 43.66, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("carico alto"), "{reason}");
    }

    /// Un compito senza il campo `peso` legge `Weight::Heavy` da
    /// `parse_task` (vedi `a_task_without_the_weight_field_defaults_to_heavy`
    /// più sotto): qui si prova che, arrivato fino a `decide()`, si comporta
    /// come una pesante — stesso tetto stretto, stesso salto.
    #[test]
    fn a_task_without_the_weight_field_is_treated_as_heavy_by_decide() {
        let text = "motore: codex\n---prompt---\nx\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else { panic!("doveva leggersi") };
        assert_eq!(t.weight, Weight::Heavy);
        let inputs = WatchInputs { next_task_weight: t.weight, load1: 7.88, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("senza classificazione un compito è pesante, e 7.88 supera il suo tetto")
        };
        assert!(reason.contains("pesante"), "{reason}");
    }

    /// La memoria resta un freno per tutti, leggere comprese: il salto qui
    /// deve dire memoria, non carico.
    #[test]
    fn a_light_task_with_low_memory_skips_for_memory_not_load() {
        let inputs = WatchInputs { next_task_weight: Weight::Light, mem_free_percent: 10, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("memoria bassa"), "{reason}");
    }

    /// Il tetto orario resta per tutti: una leggera non lo scavalca.
    #[test]
    fn a_light_task_still_stops_at_the_hourly_cap() {
        let inputs = WatchInputs { next_task_weight: Weight::Light, tasks_this_hour: 6, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &thresholds()) else {
            panic!("doveva saltare")
        };
        assert!(reason.contains("tetto orario raggiunto"), "{reason}");
    }

    // ── la finestra oraria ──────────────────────────────────────────────

    #[test]
    fn hour_window_handles_the_ordinary_case() {
        assert!(hour_in_window(3, 1, 7));
        assert!(!hour_in_window(0, 1, 7));
        assert!(!hour_in_window(7, 1, 7)); // fine esclusa
        assert!(hour_in_window(1, 1, 7)); // inizio inclusa
    }

    /// Una finestra che passa la mezzanotte (es. 23–2) non è un caso
    /// speciale da dimenticare: 23, 0, 1 sono dentro, 2 e 12 fuori.
    #[test]
    fn hour_window_wraps_past_midnight() {
        assert!(hour_in_window(23, 23, 2));
        assert!(hour_in_window(0, 23, 2));
        assert!(hour_in_window(1, 23, 2));
        assert!(!hour_in_window(2, 23, 2));
        assert!(!hour_in_window(12, 23, 2));
    }

    /// DI GIORNO, CON MARGINE, SI LAVORA — il rovescio del 26/08/2026.
    ///
    /// Prima questo caso pretendeva un salto, e la ragione era l'ora. Il
    /// registro ha detto che era la ragione sbagliata: 158 salti su 191 per
    /// l'orologio, 4 per il carico. Un sistema che può ripararsi solo di notte
    /// non si ripara, perché di notte la macchina dorme e di giorno l'orologio
    /// lo ferma.
    #[test]
    fn during_the_day_with_headroom_it_works() {
        let inputs = WatchInputs { hour: 14, idle_seconds: 100, ..good_inputs() };
        assert_eq!(
            decide(&inputs, &thresholds()),
            WatchDecision::Run,
            "di giorno, con carico e memoria a posto, il ciclo deve poter lavorare"
        );
    }

    /// E IL FRENO CHE RESTA È QUELLO GIUSTO: di giorno la soglia è quella
    /// stretta, perché qualcuno sta lavorando. Senza questo caso, togliere la
    /// finestra sembrerebbe togliere ogni protezione.
    #[test]
    fn during_the_day_a_busy_machine_still_stops_it() {
        let th = thresholds();
        let over = th.busy_load_ratio_cap * good_inputs().core_count as f64 + 0.1;
        let inputs = WatchInputs { hour: 14, idle_seconds: 100, load1: over, ..good_inputs() };
        let WatchDecision::Skip(reason) = decide(&inputs, &th) else {
            panic!("con la macchina occupata deve saltare, anche adesso")
        };
        assert!(reason.contains("carico alto"), "e il motivo dice il carico, non l'ora: {reason}");
        assert!(reason.contains("macchina al lavoro"), "{reason}");
    }

    /// La seconda porta della finestra: fuori orario ma ferma da molto
    /// (oltre `very_idle_seconds`) lavora comunque.
    #[test]
    fn outside_the_window_but_very_idle_still_runs() {
        let inputs = WatchInputs { hour: 14, idle_seconds: 7300, ..good_inputs() };
        assert_eq!(decide(&inputs, &thresholds()), WatchDecision::Run);
    }

    #[test]
    fn inside_the_window_ignores_the_very_idle_bypass() {
        // Dentro la finestra il resto della decisione (carico, memoria) vale
        // come prima: il caso già coperto da `an_idle_machine_with_room_runs`.
        let inputs = WatchInputs { hour: 3, idle_seconds: 100, ..good_inputs() };
        assert_eq!(decide(&inputs, &thresholds()), WatchDecision::Run);
    }

    // ── la ricevuta e il contatore tentativi ────────────────────────────

    #[test]
    fn attempts_default_to_zero_without_the_field() {
        assert_eq!(attempts_field("motore: codex\n---prompt---\nx\n---verifica---\ntrue\n"), 0);
    }

    #[test]
    fn attempts_round_trip_through_set_and_read() {
        let text = "motore: codex\n---prompt---\nx\n---verifica---\ntrue\n";
        let once = set_attempts_field(text, 1);
        assert_eq!(attempts_field(&once), 1);
        // Una seconda scrittura sostituisce, non accumula righe.
        let twice = set_attempts_field(&once, 2);
        assert_eq!(attempts_field(&twice), 2);
        assert_eq!(twice.matches("tentativi:").count(), 1, "{twice}");
    }

    #[test]
    fn receipt_name_splits_the_pid_suffix() {
        assert_eq!(
            split_receipt_name("2026-08-25-a.task.12345"),
            ("2026-08-25-a.task".to_string(), Some(12345))
        );
        assert_eq!(strip_receipt_suffix("2026-08-25-a.task.12345"), "2026-08-25-a.task");
    }

    /// Un nome senza suffisso pid resta intatto: è il caso di ogni file mai
    /// passato da `in-corso/`.
    #[test]
    fn receipt_name_without_a_pid_suffix_is_unchanged() {
        assert_eq!(split_receipt_name("2026-08-25-a.task"), ("2026-08-25-a.task".to_string(), None));
        assert_eq!(strip_receipt_suffix("2026-08-25-a.task"), "2026-08-25-a.task");
    }

    #[test]
    fn lock_pid_reads_the_bare_number() {
        assert_eq!(parse_lock_pid("33409\n"), Some(33409));
        assert_eq!(parse_lock_pid("non un numero"), None);
        assert_eq!(parse_lock_pid(""), None);
    }

    /// Il caso vero della notte fra il 25 e il 26/08/2026: percorso da
    /// `launchd`, dove `codex` non c'è, e il binario che invece esiste in
    /// `/opt/homebrew/bin`. Prima di questa correzione la notte rispondeva
    /// «codex not found on PATH» e buttava via tre compiti su tre.
    #[test]
    fn engine_found_where_launchd_does_not_look() {
        let installed = ["/opt/homebrew/bin/codex"];
        let found = resolve_bin(
            "codex",
            "/usr/bin:/bin:/usr/sbin:/sbin",
            "/Users/theo",
            |c| installed.contains(&c),
        );
        assert_eq!(found, Ok("/opt/homebrew/bin/codex".to_string()));
    }

    /// La misura vale solo se poteva venire diversa: senza il binario da
    /// nessuna parte, la stessa chiamata deve fallire — e dire dove ha
    /// guardato, altrimenti chi legge la segnalazione rifà la ricerca a mano.
    #[test]
    fn engine_missing_says_where_it_looked() {
        let found = resolve_bin("codex", "/usr/bin:/bin", "/Users/theo", |_| false);
        let looked = found.expect_err("senza binario non può riuscire");
        assert!(looked.contains(&"/usr/bin/codex".to_string()));
        assert!(looked.contains(&"/opt/homebrew/bin/codex".to_string()));
        assert!(looked.contains(&"/Users/theo/.local/bin/codex".to_string()));
    }

    /// Il percorso della shell vince su tutto: chi ha una versione sua in
    /// `~/.local/bin` e la mette per prima nel percorso non deve ritrovarsi
    /// eseguita quella di sistema.
    #[test]
    fn the_path_wins_over_the_fallbacks() {
        let installed = ["/Users/theo/.local/bin/codex", "/opt/homebrew/bin/codex"];
        let found = resolve_bin(
            "codex",
            "/Users/theo/.local/bin:/usr/bin",
            "/Users/theo",
            |c| installed.contains(&c),
        );
        assert_eq!(found, Ok("/Users/theo/.local/bin/codex".to_string()));
    }

    /// Un nome scritto per esteso è già una decisione: non si va a cercargli
    /// un omonimo altrove, nemmeno se quell'omonimo esiste.
    #[test]
    fn an_explicit_path_is_taken_as_it_is() {
        let installed = ["/opt/homebrew/bin/codex"];
        let found = resolve_bin("/altro/posto/codex", "", "/Users/theo", |c| {
            installed.contains(&c)
        });
        assert_eq!(found, Err(vec!["/altro/posto/codex".to_string()]));
    }

    // ── i compiti che si ripetono ─────────────────────────────────────

    /// Senza il campo, un compito resta quello di prima: si consuma.
    #[test]
    fn a_task_without_the_field_is_not_recurring() {
        let ParsedTask::Ok(t) = parse_task("motore: codex\n---prompt---\nx\n---verifica---\ntrue\n")
        else {
            panic!("doveva leggersi");
        };
        assert!(!t.recurring);
        assert_eq!(t.last_run, None);
    }

    #[test]
    fn the_field_makes_it_recurring() {
        let text = "motore: codex\nricorrenza: ogni-notte\nultima-esecuzione: 2026-08-25\n---prompt---\nx\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else { panic!("doveva leggersi") };
        assert!(t.recurring);
        assert_eq!(t.last_run.as_deref(), Some("2026-08-25"));
    }

    /// Il freno che impedisce sei esecuzioni nella stessa notte — e che deve
    /// lasciar passare la notte dopo.
    #[test]
    fn a_recurring_task_runs_once_a_night() {
        let text = "motore: codex\nricorrenza: ogni-notte\nultima-esecuzione: 2026-08-26\n---prompt---\nx\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else { panic!("doveva leggersi") };
        assert!(already_done_today(&t, "2026-08-26"), "stessa notte: non si rifà");
        assert!(!already_done_today(&t, "2026-08-27"), "notte dopo: deve rifarsi");
    }

    /// Un compito normale non viene mai fermato da questo freno, nemmeno se
    /// per qualche motivo porta una data.
    #[test]
    fn the_brake_never_holds_an_ordinary_task() {
        let text = "motore: codex\nultima-esecuzione: 2026-08-26\n---prompt---\nx\n---verifica---\ntrue\n";
        let ParsedTask::Ok(t) = parse_task(text) else { panic!("doveva leggersi") };
        assert!(!already_done_today(&t, "2026-08-26"));
    }

    /// Il timbro si aggiunge dove chi apre il file lo vede, e non tocca
    /// niente altro: il compito che torna in coda deve restare eseguibile.
    #[test]
    fn the_stamp_is_added_next_to_the_engine_line() {
        let text = "motore: codex\nricorrenza: ogni-notte\n---prompt---\nconta\n---verifica---\ntrue\n";
        let out = stamped_for_next_night(text, "2026-08-26");
        assert!(out.contains("ultima-esecuzione: 2026-08-26"), "{out}");
        assert_eq!(out.lines().nth(1), Some("ultima-esecuzione: 2026-08-26"), "{out}");
        let ParsedTask::Ok(t) = parse_task(&out) else { panic!("non si rilegge più: {out}") };
        assert_eq!(t.prompt.trim(), "conta");
        assert_eq!(t.check.trim(), "true");
        assert!(t.recurring);
    }

    /// Un timbro vecchio si sostituisce, non si accumula: altrimenti dopo
    /// una settimana il file porta sette date e la più vecchia vince.
    #[test]
    fn an_old_stamp_is_replaced_not_stacked() {
        let text = "motore: codex\nultima-esecuzione: 2026-08-20\nricorrenza: ogni-notte\n---prompt---\nx\n---verifica---\ntrue\n";
        let out = stamped_for_next_night(text, "2026-08-26");
        assert_eq!(out.matches("ultima-esecuzione:").count(), 1, "{out}");
        assert!(out.contains("ultima-esecuzione: 2026-08-26"), "{out}");
        assert!(!out.contains("2026-08-20"), "{out}");
    }

    /// Il caso vero del 26/08/2026, un gradino dopo: `codex` trovato, e la
    /// chiamata fallita lo stesso con `env: node: No such file or directory`.
    /// Al figlio va consegnato un percorso che contiene anche il posto dove
    /// vive il suo interprete.
    #[test]
    fn the_child_path_carries_the_interpreter_dir() {
        let p = enriched_path("/usr/bin:/bin", "/Users/theo");
        assert!(p.starts_with("/usr/bin:/bin:"), "l'ordine di chi lancia deve restare in testa: {p}");
        assert!(p.contains("/opt/homebrew/bin"), "manca il posto dove vive node: {p}");
        assert!(p.contains("/Users/theo/.local/bin"));
    }

    /// Un percorso già ricco non deve gonfiarsi di doppioni a ogni giro.
    #[test]
    fn the_child_path_never_repeats_a_dir() {
        let p = enriched_path("/opt/homebrew/bin:/usr/bin:/opt/homebrew/bin/", "/Users/theo");
        let brew = p.split(':').filter(|d| *d == "/opt/homebrew/bin").count();
        assert_eq!(brew, 1, "cartella ripetuta in: {p}");
    }

    /// La segnalazione deve entrare nel censimento della coda, che filtra per
    /// `stato: aperta`: fino al 26/08/2026 usciva senza frontmatter e in
    /// inglese, quindi nessun raccoglitore la vedeva.
    #[test]
    fn the_alert_enters_the_queue_census() {
        let md = alert_markdown("prova.task", "codex", "2026-08-26", "motore assente", "dettaglio");
        assert!(md.starts_with("---\n"), "manca il frontmatter: {md}");
        assert!(md.contains("stato: aperta"));
        assert!(md.contains("destinatario: chi-tiene-il-ciclo-di-notte"));
        assert!(md.contains("# Compito di notte finito rosso: prova.task"));
        assert!(!md.contains("Night task"), "la segnalazione è ancora in inglese");
    }
}
