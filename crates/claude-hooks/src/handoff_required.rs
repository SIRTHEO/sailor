//! Il gancio PostToolUse che pretende la consegna prima della compattazione.
//!
//! Il giudizio sta in `guards::handoff_required`; qui c'è ciò che tocca il
//! mondo: il payload su stdin, i marcatori sotto `state/`, il registro, e la
//! forma esatta della risposta.
//!
//! LA FORMA DELLA RISPOSTA È COMPORTAMENTO. `systemMessage` va all'utente e
//! **Claude non lo vede**; il canale che raggiunge l'assistente è
//! `additionalContext`, annidato in `hookSpecificOutput` — al livello superiore
//! viene ignorato in silenzio. I due rami di solo avviso parlavano una volta
//! solo all'utente, cioè all'unico che non può agire.
//!
//! L'AVVISO PARLA SOLO ALL'ASSISTENTE, dal 19/08/2026. Mandato di Theo: la
//! consegna va fatta «senza dirlo», e il lavoro continua. Finché `systemMessage`
//! c'era, ogni soglia superata compariva a schermo — 132 volte in 7 giorni — e
//! trasformava un gesto di manutenzione in una notifica da leggere. Il blocco
//! resta visibile: lì la sessione si ferma davvero, e tacere sarebbe mentire.
//!
//! FAIL-OPEN OVUNQUE: qualunque errore esce zero. Un PostToolUse in errore
//! rifiuta ogni strumento, `Read` compreso.

use guards::handoff::{is_handoff_call, thresholds_from_lines};
use guards::handoff_required::{decide, over_lockout, Decision, Facts};
use hook_io::journal::{self, Field};
use std::fs;
use std::io::Read;

use crate::handoff::state_dir;

/// Si era già avvisato — e se no, da adesso sì.
///
/// Legge **e scrive**, come l'originale: il marcatore nasce alla prima domanda.
/// Separare le due cose sembrerebbe più pulito e cambierebbe il comportamento,
/// perché il secondo strumento dello stesso turno riavviserebbe.
fn already_warned(session: &str) -> bool {
    let marker = state_dir().join(format!("consegna-avvisata-{session}"));
    if marker.exists() {
        return true;
    }
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(&marker, "1");
    false
}

/// Quanti rifiuti si sono già opposti; con `increment`, ne conta uno in più.
fn blocks_so_far(session: &str, increment: bool) -> u32 {
    let counter = state_dir().join(format!("consegna-blocchi-{session}"));
    let n = fs::read_to_string(&counter)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
        .unwrap_or(0);
    if increment {
        let _ = fs::create_dir_all(state_dir());
        let _ = fs::write(&counter, (n + 1).to_string());
    }
    n
}

/// La consegna c'è **ed è ancora quella del lavoro in corso**.
///
/// Il marcatore da solo non basta: una consegna scritta alle 16:09 disarmava i
/// presidi anche il giorno dopo, sei ripartenze più tardi — cioè proprio quando
/// servivano. Si registra a quante ripartenze era stata scritta; se da allora la
/// sessione è ripartita ancora, quel documento descrive un altro lavoro.
pub(crate) fn handoff_valid(transcript: &str, session: &str) -> bool {
    if !state_dir().join(format!("consegna-fatta-{session}")).exists() {
        return false;
    }
    let n = restarts(transcript, session);
    let riferimento = state_dir().join(format!("consegna-fatta-ripartenze-{session}"));
    match fs::read_to_string(&riferimento)
        .ok()
        .and_then(|t| t.trim().parse::<u32>().ok())
    {
        Some(allora) => n <= allora,
        None => {
            // Prima volta che si guarda un marcatore già esistente: si prende
            // adesso come riferimento. Una consegna vecchia vale un giro.
            let _ = fs::create_dir_all(state_dir());
            let _ = fs::write(&riferimento, n.to_string());
            true
        }
    }
}

/// Le ripartenze, memoizzate: il conteggio scorre la trascrizione intera.
///
/// Stessa disciplina della misura del contesto — si rimisura solo quando il file
/// è cresciuto di `MIN_GROWTH`. Una ripartenza in più fra due misure ritarda il
/// presidio di un turno, che è il prezzo giusto per non rileggere centinaia di
/// megabyte a ogni fine turno.
pub(crate) fn restarts(transcript: &str, session: &str) -> u32 {
    let memo = state_dir().join(format!("consegna-ripartenze-{session}"));
    let size = fs::metadata(transcript).map(|m| m.len()).unwrap_or(0);
    if let Ok(text) = fs::read_to_string(&memo) {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if let [old_size, old_n] = parts[..] {
            if let (Ok(old_size), Ok(old_n)) = (old_size.parse::<u64>(), old_n.parse::<u32>()) {
                if size.saturating_sub(old_size) < guards::handoff::MIN_GROWTH {
                    return old_n;
                }
            }
        }
    }
    let n = crate::restart::count_whole_file(transcript)
        .map(|r| r.restarts)
        .unwrap_or(0);
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(&memo, format!("{size} {n}"));
    n
}

/// Segna la garanzia come soddisfatta, e azzera il riferimento delle ripartenze.
fn mark_done(session: &str) {
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(state_dir().join(format!("consegna-fatta-{session}")), "1");
    let _ = fs::remove_file(state_dir().join(format!("consegna-fatta-ripartenze-{session}")));
    // Una nuova consegna descrive il lavoro di adesso: il riferimento di
    // crescita sopra il gradino appartiene alla consegna vecchia, e va tolto
    // perché il prossimo giro sopra il gradino registri quello nuovo.
    let _ = fs::remove_file(state_dir().join(format!("consegna-riferimento-lockout-{session}")));
}

/// Il contesto registrato quando la consegna ha cominciato a valere sopra il
/// gradino di blocco, o zero se non c'è ancora un riferimento.
///
/// Stesso schema di [`handoff_valid`]/[`restarts`] qui sopra: si registra
/// alla prima volta che serve, non a ogni giro — altrimenti il riferimento
/// scorrerebbe insieme al contesto e la crescita non si misurerebbe mai.
fn lockout_reference(session: &str, used: u64) -> u64 {
    let marker = state_dir().join(format!("consegna-riferimento-lockout-{session}"));
    match fs::read_to_string(&marker)
        .ok()
        .and_then(|t| t.trim().parse::<u64>().ok())
    {
        Some(v) => v,
        None => {
            let _ = fs::create_dir_all(state_dir());
            let _ = fs::write(&marker, used.to_string());
            0
        }
    }
}

/// Sopra il gradino di blocco, un gesto che passa (`Decision::Pass`) su una
/// consegna **già scritta** non è un passo verso l'aggiornarla: consuma
/// comunque un giro dal tetto dei rifiuti, o chi obbedisce — invocando
/// `Skill`, `Read`, `Write` — non si avvicina mai alla resa, e ci si avvicina
/// solo riprovando un comando negato (il difetto nº2 della segnalazione del
/// 21/08/2026).
///
/// SOLO CON `handoff_valid`. Senza una consegna, il segnale d'uscita resta
/// quello che già funzionava: i `Decision::Block` sulle chiamate negate.
/// Il verdetto del revisore del 21/08/2026: senza questo controllo, sei
/// `Read`/`Grep`/`Glob` qualunque sopra il gradino — in un turno solo, senza
/// che sia mai comparso un rifiuto — esaurivano `BLOCK_CAP` e arrendevano il
/// gancio per sempre, `Bash` compreso, su una sessione che non aveva
/// consegnato niente.
fn count_a_passing_turn(session: &str, used: u64, budget: u64, handoff_valid: bool) {
    if handoff_valid && over_lockout(used, budget) {
        blocks_so_far(session, true);
    }
}

/// Segna che la consegna è stata una **scelta**, non un ordine del presidio.
///
/// È il secondo motivo per cui la staffetta passa il testimone: chi consegna con
/// il contesto ancora largo dichiara chiuso l'ambito, e per quel caso la soglia
/// di occupazione non c'entra.
fn mark_deliberate(session: &str) {
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(
        state_dir().join(format!("consegna-volontaria-{session}")),
        "1",
    );
}

/// Stampa la risposta nella forma esatta dell'originale, e esce.
fn say(payload: serde_json::Value) -> i32 {
    // `json.dumps(..., ensure_ascii=False)`, non `serde_json::to_string`: il
    // Python separa con «, » e «: », e questa uscita è testo che l'harness legge
    // e che gli archivi conservano. Su venti scenari la sola formattazione
    // produceva venti divergenze.
    println!("{}", hook_io::python_json::dumps_unicode(&payload));
    0
}

fn avviso(testo: &str) -> serde_json::Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": testo,
        },
    })
}

pub fn run() -> i32 {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(data) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0;
    };
    // Chi legge stdin da sé deve dichiararlo, o ogni sua riga di registro esce
    // marcata come prova — e questo gancio è fra quelli che mordono di più.
    hook_io::mark_live_from_payload(&data);
    let campo = |k: &str| data.get(k).and_then(|v| v.as_str()).unwrap_or("");
    let session_intero = data
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("ignota");
    let session: String = session_intero.chars().take(8).collect();
    let transcript = campo("transcript_path");
    let tool = {
        let t = campo("tool_name");
        if t.is_empty() { "?" } else { t }
    };
    if transcript.is_empty() {
        return 0;
    }

    // DENTRO UN SUBAGENT SI ESCE SUBITO, e questa riga va prima di ogni altra
    // cosa che tocchi il disco. Il giudizio la ripete (`Facts::in_subagent`),
    // ma arrivarci costerebbe caro: qui sopra si scrivono il marcatore
    // d'avviso, il contatore dei rifiuti e il memo della misura — tutti
    // intestati alla MADRE, perché un subagent ne condivide la sessione. Erano
    // le chiamate dei subagent a bruciare i sei rifiuti della madre e a
    // lasciare il presidio arreso e verde.
    //
    // Ed è anche il grosso del costo: un `transcript_tail` legge 400 KB, e qui
    // passava ogni chiamata di ogni subagent.
    if crate::handoff::in_subagent(&data) {
        return 0;
    }

    // Se questo strumento È l'handoff, la garanzia è soddisfatta: da qui in poi
    // questo gancio e il gemello sullo Stop lasciano passare tutto.
    //
    // E si registra anche PERCHÉ è stata consegnata. Sotto la soglia del
    // presidio nessuno l'ha ordinata: è una scelta, e vuol dire che il lavoro è
    // finito o che cambia mestiere. Senza questa distinzione la staffetta guarda
    // solo l'occupazione, e una consegna deliberata resta sul disco senza che
    // nessuno la raccolga — misurato il 18/08/2026, consegnata al 67% con soglia
    // al 90%: la sessione successiva non è mai partita.
    if is_handoff_call(tool, data.get("tool_input")) {
        mark_done(&session);
        let used_ora = crate::handoff::context_used(transcript, &session);
        if used_ora > 0 {
            let coda = crate::handoff::transcript_tail(transcript);
            let soglie = thresholds_from_lines(&coda.lines().collect::<Vec<_>>());
            if used_ora < soglie.require {
                mark_deliberate(&session);
            }
        }
        return 0;
    }

    let used = crate::handoff::context_used(transcript, &session);
    if used == 0 {
        return 0;
    }
    let tail = crate::handoff::transcript_tail(transcript);
    let t = thresholds_from_lines(&tail.lines().collect::<Vec<_>>());

    // I due fatti che si leggono scrivendo — il marcatore d'avviso e il
    // contatore — si raccolgono SOLO se la decisione può arrivare a usarli.
    // Chiederli sempre lascerebbe un marcatore per ogni sessione sotto soglia,
    // ed è la stessa trappola già trovata nella staffetta: sette scenari su
    // venti con un memo scritto da chi non ci sarebbe mai arrivato.
    let sotto_avviso = used < t.warn;
    let valida = !sotto_avviso && handoff_valid(transcript, &session);
    // Sopra il gradino di blocco la consegna valida non disarma più il
    // presidio (`decide` la tratta come le sessioni senza consegna), quindi
    // qui il contatore dei rifiuti va letto e non azzerato.
    let disarma = valida && !over_lockout(used, t.budget);
    // Il riferimento di crescita ha senso solo per una consegna valida sopra
    // il gradino: altrove il giudizio non lo guarda, e leggerlo a vuoto
    // scriverebbe un marcatore per sessioni che non ci arriveranno mai.
    let context_at_handoff = if valida && !disarma {
        lockout_reference(&session, used)
    } else {
        0
    };
    let fatti = Facts {
        tool,
        thresholds: &t,
        // Falso per costruzione: sopra si è già usciti. Resta esplicito perché
        // il giudizio è la sede del contratto, non questa funzione.
        in_subagent: false,
        used,
        handoff_valid: valida,
        context_at_handoff,
        already_warned: !sotto_avviso && !disarma && used < t.require && already_warned(&session),
        blocks_so_far: if sotto_avviso || disarma || used < t.require {
            0
        } else {
            blocks_so_far(&session, false)
        },
    };
    let pct = (used as f64 / t.budget as f64 * 100.0).round() as u64;

    match decide(&fatti) {
        Decision::Silent => 0,
        Decision::Warn(testo) => {
            journal::record(
                "consegna-obbligatoria",
                "avvisa",
                "soglia-avviso",
                &[
                    ("session", Field::Text(session.clone())),
                    ("strumento", Field::Text(tool.to_string())),
                    ("percento", Field::Number(pct as i64)),
                    ("token", Field::Number(used as i64)),
                ],
            );
            say(avviso(&testo))
        }
        Decision::Pass => {
            // Sopra il gradino un gesto che passa non è un passo verso la
            // consegna: vedi `count_a_passing_turn`.
            count_a_passing_turn(&session, used, t.budget, valida);
            journal::record(
                "consegna-obbligatoria",
                "passa",
                "serve-a-consegnare",
                &[
                    ("session", Field::Text(session.clone())),
                    ("strumento", Field::Text(tool.to_string())),
                    ("percento", Field::Number(pct as i64)),
                ],
            );
            0
        }
        Decision::Surrender(testo) => {
            // Il contatore si incrementa comunque: l'originale lo fa **prima**
            // di guardare il tetto, e il numero che finisce nel registro è
            // quello di prima. Due letture diverse dello stesso file.
            let n = blocks_so_far(&session, true);
            journal::record(
                "consegna-obbligatoria",
                "passa",
                "arreso",
                &[
                    ("session", Field::Text(session.clone())),
                    ("strumento", Field::Text(tool.to_string())),
                    ("percento", Field::Number(pct as i64)),
                    ("blocchi", Field::Number(n as i64)),
                ],
            );
            say(avviso(&testo))
        }
        Decision::Block(motivo) => {
            let n = blocks_so_far(&session, true);
            journal::record(
                "consegna-obbligatoria",
                "blocca",
                "consegna-non-fatta",
                &[
                    ("session", Field::Text(session.clone())),
                    ("strumento", Field::Text(tool.to_string())),
                    ("percento", Field::Number(pct as i64)),
                    ("blocchi", Field::Number((n + 1) as i64)),
                ],
            );
            // Il rifiuto NON passa da `avviso`: `decision: block` raggiunge
            // l'assistente da sé, e avvolgerlo cambierebbe la forma che
            // l'harness legge.
            say(serde_json::json!({"decision": "block", "reason": motivo}))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mandato di Theo del 19/08/2026: la consegna si fa senza dirlo.
    ///
    /// `systemMessage` e' l'unico canale che raggiunge lo schermo per
    /// costruzione: finche' c'era, ogni soglia superata compariva a Theo (132
    /// volte in sette giorni) e trasformava una manutenzione in una notifica.
    #[test]
    fn the_warning_speaks_to_the_assistant_only() {
        let payload = avviso("contesto alto");
        assert!(
            payload.get("systemMessage").is_none(),
            "l'avviso non deve raggiungere lo schermo: {payload}"
        );
        assert_eq!(
            payload["hookSpecificOutput"]["additionalContext"],
            "contesto alto"
        );
        assert_eq!(
            payload["hookSpecificOutput"]["hookEventName"],
            "PostToolUse"
        );
    }

    #[test]
    fn lockout_reference_registers_once_and_then_holds() {
        // Stesso schema di `handoff_valid`/`restarts`: la prima lettura
        // registra il valore corrente e risponde zero (non ancora crescita),
        // le successive rispondono il valore fissato allora, non quello
        // corrente — altrimenti la crescita non si misurerebbe mai.
        let home = crate::test_home::HomeIsolata::nuova("lockout-reference-registers");
        assert_eq!(lockout_reference("sess1", 480_000), 0, "prima volta: nessun riferimento");
        assert_eq!(
            lockout_reference("sess1", 495_000),
            480_000,
            "il riferimento resta quello della prima chiamata"
        );
        drop(home);
    }

    #[test]
    fn mark_done_clears_the_lockout_reference() {
        // Una nuova consegna descrive il lavoro di adesso: il riferimento
        // vecchio va tolto, o il prossimo giro sopra il gradino confronterebbe
        // la crescita con una consegna che non c'è più.
        let home = crate::test_home::HomeIsolata::nuova("lockout-reference-mark-done");
        lockout_reference("sess2", 480_000);
        assert!(home
            .stato()
            .join("consegna-riferimento-lockout-sess2")
            .exists());
        mark_done("sess2");
        assert!(
            !home.stato().join("consegna-riferimento-lockout-sess2").exists(),
            "il riferimento vecchio non deve sopravvivere a una nuova consegna"
        );
    }

    #[test]
    fn a_passing_turn_counts_toward_the_cap_only_above_the_lockout_step() {
        // Il difetto della segnalazione del 21/08/2026: gli strumenti che
        // passano su una consegna già scritta non toccavano mai il tetto dei
        // rifiuti, quindi chi obbedisce (Skill, Read, Write...) non si
        // avvicinava mai alla resa. Sopra il gradino anche un gesto che passa
        // consuma un giro; sotto, no. `handoff_valid = true`: è lo scenario
        // del punto 3a, dove la consegna c'è ed è quella che invecchia.
        let home = crate::test_home::HomeIsolata::nuova("passing-turn-cap");
        let budget = 500_000;
        count_a_passing_turn("sess3", 400_000, budget, true); // sotto il gradino (96%)
        assert_eq!(blocks_so_far("sess3", false), 0, "sotto il gradino non si conta");
        count_a_passing_turn("sess3", 480_000, budget, true); // sul gradino esatto
        assert_eq!(blocks_so_far("sess3", false), 1, "sul gradino si conta");
        count_a_passing_turn("sess3", 481_000, budget, true);
        assert_eq!(blocks_so_far("sess3", false), 2, "ogni giro sopra il gradino ne consuma uno");
        drop(home);
    }

    #[test]
    fn passing_turns_without_a_handoff_never_move_the_cap() {
        // Il difetto trovato dal revisore del 21/08/2026: `count_a_passing_turn`
        // chiamata senza guardare `handoff_valid` esauriva `BLOCK_CAP` (6) con
        // sei `Read` qualunque sopra il gradino, senza che comparisse mai un
        // `Decision::Block` — e il gancio si arrendeva per sempre su una
        // sessione che non aveva consegnato niente. Qui si passa **da
        // `decide()`**, con `handoff_valid = false`, per dieci giri (più del
        // tetto): il contatore deve restare fermo, perché il segnale d'uscita
        // di una sessione senza consegna resta quello dei `Decision::Block`
        // su `Bash`, non le chiamate che passano comunque.
        let home = crate::test_home::HomeIsolata::nuova("passing-turn-no-handoff");
        let t = guards::handoff::Thresholds {
            model: "claude-opus-5".into(),
            budget: 500_000,
            warn: 390_000,
            require: 450_000,
        };
        let facts = Facts {
            tool: "Read",
            thresholds: &t,
            in_subagent: false,
            used: 490_000, // sopra il gradino (96% di 500_000 = 480_000)
            handoff_valid: false,
            context_at_handoff: 0,
            already_warned: false,
            blocks_so_far: 0,
        };
        for _ in 0..10 {
            assert_eq!(
                decide(&facts),
                Decision::Pass,
                "senza consegna, uno strumento della lista passa comunque"
            );
            count_a_passing_turn("sess4", facts.used, t.budget, facts.handoff_valid);
        }
        assert_eq!(
            blocks_so_far("sess4", false),
            0,
            "senza handoff_valid il tetto non deve mai muoversi"
        );
        drop(home);
    }
}
