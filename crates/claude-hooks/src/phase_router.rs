//! Il gancio `PreToolUse` su `Agent` che riscrive il modello dal mestiere.
//!
//! Il giudizio sta in `guards::phase_router`; qui c'è ciò che tocca il mondo:
//! il payload su stdin, la riscrittura di `tool_input` in `updatedInput` e il
//! registro — una riga per ogni decisione, anche quella che lascia tutto
//! com'è (senza il negativo il registro non direbbe quanto morde davvero).
//!
//! NON NEGA MAI, NON BLOCCA MAI: o riscrive `model`, o non stampa niente. Un
//! `PreToolUse` che nega qui vieterebbe di lanciare il subagent, non solo di
//! scegliergli il modello — il danno peggiore che questo gancio può fare.
//! Fail-open su stdin illeggibile o fuori contesto, come ogni altro gancio.

use guards::phase_router::{
    declared_model_in_frontmatter, declared_model_wins, declared_name_in_frontmatter, route_with,
    Row, TRADE_MODEL,
};
use serde_json::Value;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::handoff::state_dir;

const CEILING_BYTES: u64 = 5 * 1024 * 1024;

/// Il file che definisce un mestiere, se il nome è un nome di mestiere.
///
/// Il nome arriva dal payload, quindi non si concatena a un percorso senza
/// guardarlo: un `subagent_type` con una barra o due punti uscirebbe dalla
/// cartella degli agenti. I mestieri dei plugin (`claude-security:explore`)
/// cadono qui dentro ed è giusto — il loro file non sta in questa cartella,
/// e il gancio non ha niente da far valere per loro.
fn agent_file(trade: &str) -> Option<PathBuf> {
    if trade.is_empty()
        || !trade
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return None;
    }
    let home = std::env::var_os("HOME")?;
    Some(
        PathBuf::from(home)
            .join(".claude")
            .join("agents")
            .join(format!("{trade}.md")),
    )
}

/// Il modello che il mestiere dichiara, letto dal suo file. `None` per ogni
/// mestiere che non ne ha uno: quelli di sistema, quelli dei plugin, e
/// quelli il cui file non si legge.
///
/// IL NOME DENTRO IL FILE DEVE COMBACIARE, e non è una cintura in più: su
/// macOS il filesystem è insensibile alle maiuscole, quindi `agent_file`
/// costruisce `agents/BUILDER.md` e `fs::read_to_string` apre tranquillamente
/// `builder.md`. Senza questo confronto un `subagent_type` scritto in un caso
/// qualunque erediterebbe la dichiarazione di un mestiere che in quella forma
/// non esiste. Il nome nel frontmatter è l'unico dato che non passa dal
/// percorso, quindi l'unico che può smentirlo.
fn declared_model_for(trade: &str) -> Option<String> {
    let text = fs::read_to_string(agent_file(trade)?).ok()?;
    if declared_name_in_frontmatter(&text) != Some(trade) {
        return None;
    }
    declared_model_in_frontmatter(&text).map(str::to_string)
}

/// Una riga in `ganci.jsonl`: mestiere, modello scelto (o `null`), motivo,
/// sessione se c'è. Formato compatto, come i ganci nati in Rust senza un
/// originale Python da ricopiare byte per byte.
fn log_routing(trade: &str, model: Option<&str>, reason: &str, session: Option<&str>) {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "t".into(),
        Value::String(hook_io::journal::now_iso8601_python()),
    );
    obj.insert("gancio".into(), Value::String("phase-router".into()));
    obj.insert("mestiere".into(), Value::String(trade.to_string()));
    obj.insert(
        "modello".into(),
        model
            .map(|m| Value::String(m.to_string()))
            .unwrap_or(Value::Null),
    );
    obj.insert("motivo".into(), Value::String(reason.to_string()));
    if let Some(s) = session.filter(|s| !s.is_empty()) {
        obj.insert("session".into(), Value::String(s.chars().take(8).collect()));
    }
    let line = Value::Object(obj).to_string();

    let dir = state_dir();
    let path = dir.join("ganci.jsonl");
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > CEILING_BYTES {
            let _ = fs::rename(&path, path.with_extension("jsonl.1"));
        }
    }
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{line}");
    }
}

/// Dal payload grezzo alla riscrittura da stampare, se c'è. Registra ogni
/// decisione tranne quella della valvola, che è pensata per non lasciare
/// traccia — come `Mode::Off` altrove in questa configurazione, dove un
/// gancio spento non tocca né stato né registro. Isolata da stdin/stdout per
/// potersi provare due volte di fila senza toccare il terminale.
fn process_with(table: &[Row], payload: &Value) -> Option<Value> {
    if payload.get("tool_name").and_then(|v| v.as_str()) != Some("Agent") {
        return None; // invocato fuori dal proprio matcher: non è affar suo
    }
    let tool_input = payload.get("tool_input").filter(|v| v.is_object())?;
    let trade = tool_input.get("subagent_type").and_then(|v| v.as_str());
    let model = tool_input.get("model").and_then(|v| v.as_str());
    let prompt = tool_input
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let valve_off = hook_io::Mode::from_env("PHASE_ROUTER") == hook_io::Mode::Off;

    if valve_off {
        return None; // la valvola lascia passare tutto senza toccare il registro
    }
    let session = payload.get("session_id").and_then(|v| v.as_str());

    // Prima si fa valere la scelta che il mestiere ha già dichiarato: dove
    // c'è un `model` esplicito il router non entra mai («chi lancia ha
    // deciso»), e senza questo passaggio il frontmatter resterebbe
    // scavalcato in silenzio.
    let declared = trade.and_then(declared_model_for);
    if let Some(down) = declared
        .as_deref()
        .and_then(|d| declared_model_wins(Some(d), model))
    {
        log_routing(trade.unwrap_or(""), Some(&down.to), down.reason, session);
        return Some(rewrite_model(tool_input, &down.to));
    }

    let routing = route_with(table, trade, model, prompt, valve_off);

    // TABELLA E MESTIERE CHE SI CONTRADDICONO NON SI RISOLVONO IN SILENZIO.
    // Quando la chiamata non porta `model`, il router sceglie dalla tabella
    // senza guardare il frontmatter: se il mestiere ne dichiara uno diverso,
    // riscrivere vorrebbe dire scavalcare — dal basso, e senza che nessuno lo
    // veda — la stessa decisione che il ramo sopra difende dall'alto. Vince il
    // dichiarato, e resta la riga: due fonti in disaccordo sono un segnale che
    // una delle due va aggiornata a mano, non un caso da appianare.
    // Oggi non morde: `TRADE_MODEL` è vuota. Il giorno che una riga la
    // popolasse — che è lo scopo di `CANDIDATE_TABLE` — morderebbe subito.
    if let (Some(chosen), Some(declared)) = (routing.model, declared.as_deref()) {
        if chosen != declared {
            log_routing(
                &routing.trade,
                None,
                "tabella e mestiere in disaccordo: vince quello dichiarato",
                session,
            );
            return None;
        }
    }

    log_routing(&routing.trade, routing.model, routing.reason, session);

    let chosen = routing.model?;
    Some(rewrite_model(tool_input, chosen))
}

/// L'input della chiamata con il campo `model` sostituito: è l'unica forma
/// di uscita che questo gancio produce.
fn rewrite_model(tool_input: &Value, model: &str) -> Value {
    let mut rewritten = tool_input.clone();
    rewritten
        .as_object_mut()
        .expect("filtrato da chi chiama: è un oggetto")
        .insert("model".to_string(), Value::String(model.to_string()));
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PreToolUse",
            "updatedInput": rewritten,
        }
    })
}

pub fn run() -> i32 {
    run_from(TRADE_MODEL, &mut std::io::stdin(), &mut std::io::stdout())
}

/// Lo stesso giro di `run`, con tabella, stdin e stdout passati da fuori: è
/// l'unico modo di provare «non nega mai» su un payload rotto senza un processo.
fn run_from(table: &[Row], input: &mut dyn Read, output: &mut dyn Write) -> i32 {
    let mut raw = String::new();
    if input.read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(payload) = serde_json::from_str::<Value>(&raw) else {
        return 0;
    };
    // Chi legge stdin da sé deve dichiararlo, o ogni sua riga di registro esce
    // marcata come prova.
    hook_io::mark_live_from_payload(&payload);
    if let Some(out) = process_with(table, &payload) {
        let _ = writeln!(output, "{out}");
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;
    use guards::phase_router::CANDIDATE_TABLE;

    fn agent_payload(trade: &str, model: Option<&str>, prompt: &str) -> Value {
        let mut tool_input = serde_json::json!({
            "subagent_type": trade,
            "prompt": prompt,
        });
        if let Some(m) = model {
            tool_input["model"] = Value::String(m.to_string());
        }
        serde_json::json!({
            "hook_event_name": "PreToolUse",
            "tool_name": "Agent",
            "tool_input": tool_input,
            "session_id": "abcdef01-2345",
        })
    }

    fn hook_journal_lines(home: &HomeIsolata) -> Vec<Value> {
        fs::read_to_string(home.stato().join("ganci.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    /// Un `measurer` senza `model` viene riscritto su haiku, e la riga finisce
    /// nel registro con il mestiere e il modello scelto.
    #[test]
    fn a_measurer_without_a_model_is_rewritten_to_haiku() {
        let home = HomeIsolata::nuova("phase-router-riscrive");
        let out = process_with(
            CANDIDATE_TABLE,
            &agent_payload("measurer", None, "conta i file"),
        )
        .expect("doveva riscrivere");
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["model"], "haiku");
        assert_eq!(
            out["hookSpecificOutput"]["updatedInput"]["subagent_type"],
            "measurer"
        );
        let lines = hook_journal_lines(&home);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["gancio"], "phase-router");
        assert_eq!(lines[0]["mestiere"], "measurer");
        assert_eq!(lines[0]["modello"], "haiku");
    }

    /// Un mestiere fuori tabella non produce nessun output, ma la riga «non
    /// riscrivo» resta nel registro con `modello: null`.
    #[test]
    fn a_builder_is_passed_through_but_still_logged() {
        let home = HomeIsolata::nuova("phase-router-passa");
        assert_eq!(
            process_with(
                CANDIDATE_TABLE,
                &agent_payload("builder", None, "scrivi il codice")
            ),
            None
        );
        let lines = hook_journal_lines(&home);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["mestiere"], "builder");
        assert!(lines[0]["modello"].is_null());
    }

    /// `model` già scritto non si tocca: nessun output.
    #[test]
    fn an_explicit_model_produces_no_output() {
        let _home = HomeIsolata::nuova("phase-router-esplicito");
        assert_eq!(
            process_with(
                CANDIDATE_TABLE,
                &agent_payload("measurer", Some("opus"), "conta i file")
            ),
            None
        );
    }

    /// La valvola spegne tutto: nessuna riscrittura, nessuna riga.
    #[test]
    fn the_valve_silences_the_hook_entirely() {
        let home = HomeIsolata::nuova("phase-router-valvola");
        std::env::set_var("PHASE_ROUTER", "off");
        let outcome = process_with(
            CANDIDATE_TABLE,
            &agent_payload("measurer", None, "conta i file"),
        );
        std::env::remove_var("PHASE_ROUTER");
        assert_eq!(outcome, None);
        assert!(!home.stato().join("ganci.jsonl").exists());
    }

    /// L'invariante che conta di più: su qualunque stdin rotto il gancio esce
    /// 0 e non stampa niente — mai un `deny`, mai un panico. Un lettore che
    /// fallisce copre anche lo stdin chiuso male.
    #[test]
    fn a_broken_stdin_never_denies() {
        let _home = HomeIsolata::nuova("phase-router-stdin-rotto");
        struct Broken;
        impl Read for Broken {
            fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
                Err(std::io::Error::other("stdin chiuso"))
            }
        }
        let mut out = Vec::new();
        assert_eq!(run_from(CANDIDATE_TABLE, &mut Broken, &mut out), 0);
        for raw in [
            "",
            "non è json",
            "[1,2]",
            "\"stringa\"",
            r#"{"tool_name":"Agent"}"#,
            r#"{"tool_name":"Agent","tool_input":"non un oggetto"}"#,
            r#"{"tool_name":"Agent","tool_input":{"subagent_type":7,"model":[],"prompt":null}}"#,
        ] {
            let mut out = Vec::new();
            assert_eq!(
                run_from(CANDIDATE_TABLE, &mut raw.as_bytes(), &mut out),
                0,
                "{raw:?}"
            );
            assert!(out.is_empty(), "{raw:?} non doveva stampare: {out:?}");
        }
        // Il caso sano, dallo stesso ingresso: stampa la riscrittura e basta.
        let ok = agent_payload("measurer", None, "conta i file").to_string();
        let mut out = Vec::new();
        assert_eq!(run_from(CANDIDATE_TABLE, &mut ok.as_bytes(), &mut out), 0);
        let printed: Value = serde_json::from_slice(&out).expect("una riga JSON");
        assert_eq!(
            printed["hookSpecificOutput"]["updatedInput"]["model"],
            "haiku"
        );
    }

    /// Scrive il file di un mestiere nella casa isolata, col modello che
    /// dichiara: è il frontmatter che il gancio deve andare a leggere.
    fn declare_trade(home: &HomeIsolata, trade: &str, model: &str) {
        let dir = home.dir.join(".claude").join("agents");
        fs::create_dir_all(&dir).expect("la cartella degli agenti");
        fs::write(
            dir.join(format!("{trade}.md")),
            format!("---\nname: {trade}\nmodel: {model}\n---\n\n# {trade}\n"),
        )
        .expect("il file del mestiere");
    }

    /// Il caso misurato il 26/08/2026: `builder` dichiara sonnet e la
    /// chiamata chiede opus. Si torna al dichiarato, e il registro dice
    /// perché.
    #[test]
    fn a_costlier_model_than_the_trade_declares_is_brought_back_to_it() {
        let home = HomeIsolata::nuova("phase-router-declassa");
        declare_trade(&home, "builder", "sonnet");
        let out = process_with(
            CANDIDATE_TABLE,
            &agent_payload("builder", Some("opus"), "scrivi il codice"),
        )
        .expect("doveva riportare al modello dichiarato");
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["model"], "sonnet");
        assert_eq!(
            out["hookSpecificOutput"]["updatedInput"]["subagent_type"],
            "builder"
        );
        let lines = hook_journal_lines(&home);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["mestiere"], "builder");
        assert_eq!(lines[0]["modello"], "sonnet");
        assert_eq!(
            lines[0]["motivo"],
            "il mestiere aveva già scelto il proprio modello"
        );
    }

    /// Verso il basso non si tocca, e nemmeno quando il modello chiesto è
    /// quello già dichiarato: resta la riga «chi lancia ha deciso».
    #[test]
    fn a_cheaper_or_equal_model_is_left_to_whoever_asked() {
        let home = HomeIsolata::nuova("phase-router-non-declassa");
        declare_trade(&home, "builder", "sonnet");
        for asked in ["haiku", "sonnet"] {
            assert_eq!(
                process_with(
                    CANDIDATE_TABLE,
                    &agent_payload("builder", Some(asked), "scrivi il codice")
                ),
                None,
                "{asked} non doveva essere riscritto"
            );
        }
        let lines = hook_journal_lines(&home);
        assert_eq!(lines.len(), 2);
        assert!(lines
            .iter()
            .all(|l| l["motivo"] == "chi lancia ha deciso" && l["modello"].is_null()));
    }

    /// Un mestiere che non dichiara niente non ha una scelta da far valere:
    /// gli agenti di sistema e quelli dei plugin passano intatti. Il nome coi
    /// due punti non deve nemmeno diventare un percorso.
    #[test]
    fn a_trade_that_declares_nothing_is_passed_through() {
        let home = HomeIsolata::nuova("phase-router-senza-dichiarazione");
        for trade in ["general-purpose", "claude-security:explore", "Explore"] {
            assert_eq!(
                process_with(CANDIDATE_TABLE, &agent_payload(trade, Some("opus"), "x")),
                None,
                "{trade} non dichiara niente: nessuna riscrittura"
            );
        }
        assert!(hook_journal_lines(&home)
            .iter()
            .all(|l| l["modello"].is_null()));
    }

    /// La tabella del router manda `measurer` su haiku; il file reale ne
    /// dichiara un altro. Vince il dichiarato, e la riga lo dice.
    ///
    /// Trovato in revisione il 26/08/2026 come conflitto dormiente: oggi
    /// `TRADE_MODEL` è vuota, ma `CANDIDATE_TABLE` esiste apposta per essere
    /// promossa. Il giorno che quella riga entrasse in servizio, un `measurer`
    /// senza `model` esplicito sarebbe finito su haiku scavalcando in silenzio
    /// la scelta del mestiere — lo stesso scavalcamento che il ramo del
    /// declassamento difende, preso dall'altro verso.
    #[test]
    fn a_table_row_disagreeing_with_the_trade_loses_to_it() {
        let home = HomeIsolata::nuova("phase-router-disaccordo");
        declare_trade(&home, "measurer", "sonnet");
        assert_eq!(
            process_with(CANDIDATE_TABLE, &agent_payload("measurer", None, "conta")),
            None,
            "la tabella direbbe haiku, il mestiere dice sonnet: non si riscrive"
        );
        let lines = hook_journal_lines(&home);
        assert_eq!(lines.len(), 1);
        assert!(lines[0]["modello"].is_null());
        assert_eq!(
            lines[0]["motivo"],
            "tabella e mestiere in disaccordo: vince quello dichiarato"
        );
    }

    /// Il differenziale della prova sopra: quando la tabella e il mestiere
    /// dicono la stessa cosa il router riscrive come sempre — il disaccordo
    /// non è un modo per spegnerlo del tutto.
    ///
    /// UNA PROVA A SÉ, E NON È STILE. `HomeIsolata::nuova` prende un lucchetto
    /// globale, e `std::sync::Mutex` non è rientrante: due case nella stessa
    /// funzione bloccano il thread su sé stesso e il lucchetto **non torna
    /// più**. Il 26/08/2026 questo ha fermato l'intera batteria — dodici prove
    /// appese, sei in questo modulo e sei in `queue_mandate`, che non
    /// c'entrava niente ed è stata la prima sospettata. Il primo `home` è vivo
    /// fino alla fine del blocco, non fino al suo ultimo uso.
    #[test]
    fn a_table_row_agreeing_with_the_trade_is_still_applied() {
        let home = HomeIsolata::nuova("phase-router-accordo");
        declare_trade(&home, "measurer", "haiku");
        let out = process_with(CANDIDATE_TABLE, &agent_payload("measurer", None, "conta"))
            .expect("concordi: si riscrive");
        assert_eq!(out["hookSpecificOutput"]["updatedInput"]["model"], "haiku");
    }

    /// Il nome scritto in un caso diverso non è quel mestiere, anche se il
    /// filesystem di macOS apre lo stesso file.
    ///
    /// Trovato in revisione il 26/08/2026, verificato sulla macchina: con
    /// `builder.md` sul disco, `agent_file("BUILDER")` costruisce un percorso
    /// che si apre, e senza il confronto sul nome il gancio ereditava la
    /// dichiarazione di un mestiere che in quella forma non esiste.
    #[test]
    fn a_trade_named_in_a_different_case_is_not_that_trade() {
        let home = HomeIsolata::nuova("phase-router-maiuscole");
        declare_trade(&home, "builder", "sonnet");
        for trade in ["BUILDER", "Builder", "bUiLdEr"] {
            assert_eq!(
                declared_model_for(trade),
                None,
                "{trade} non è il mestiere dichiarato in builder.md"
            );
            assert_eq!(
                process_with(CANDIDATE_TABLE, &agent_payload(trade, Some("opus"), "x")),
                None,
                "{trade} non doveva ereditare una dichiarazione altrui"
            );
        }
        // Il differenziale: lo stesso file, chiesto col proprio nome, declassa.
        assert_eq!(declared_model_for("builder").as_deref(), Some("sonnet"));
    }

    /// Il nome del mestiere arriva dal payload: non deve poter uscire dalla
    /// cartella degli agenti. Il file esiste davvero, un gradino più su, e
    /// resta irraggiungibile.
    #[test]
    fn a_trade_name_cannot_escape_the_agents_directory() {
        let home = HomeIsolata::nuova("phase-router-fuga");
        declare_trade(&home, "builder", "sonnet");
        let outside = home.dir.join(".claude").join("outside.md");
        fs::write(&outside, "---\nmodel: haiku\n---\n").expect("il file un gradino su");
        for trade in ["../outside", "../../etc/passwd", "builder/../outside", "."] {
            assert_eq!(agent_file(trade), None, "{trade} doveva essere respinto");
            assert_eq!(
                process_with(CANDIDATE_TABLE, &agent_payload(trade, Some("opus"), "x")),
                None,
                "{trade} non doveva leggere niente"
            );
        }
        assert!(agent_file("builder").is_some());
    }

    /// Uno strumento diverso da `Agent` non è affar suo: nessun output, nessuna
    /// riga — è il caso di un gancio invocato fuori dal proprio matcher.
    #[test]
    fn a_non_agent_tool_is_ignored_and_not_logged() {
        let home = HomeIsolata::nuova("phase-router-altro-strumento");
        let payload = serde_json::json!({
            "tool_name": "Bash",
            "tool_input": {"command": "echo ciao"},
        });
        assert_eq!(process_with(CANDIDATE_TABLE, &payload), None);
        assert!(!home.stato().join("ganci.jsonl").exists());
    }
}
