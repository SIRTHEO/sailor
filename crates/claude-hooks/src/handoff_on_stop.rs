//! Il gancio Stop che pretende la consegna quando il turno finisce in prosa.
//!
//! Il giudizio sta in `guards::handoff_on_stop`; qui c'è ciò che tocca il mondo:
//! il payload su stdin, i marcatori sotto `state/`, il registro, e i due effetti
//! su Orca — armare chi prosegue e congedarsi.
//!
//! DUE EFFETTI CHE NON SONO SIMMETRICI. Armare apre una scheda, congedarsi ne
//! chiude una: la seconda è irreversibile per chi ci stava lavorando, quindi
//! pretende tre condizioni tutte vere invece di una. Se una manca non si chiude
//! niente e non si dice niente — una sessione che resta aperta costa una riga
//! nella plancia, una che si chiude per sbaglio costa il lavoro in corso.
//!
//! L'ORDINE È COMPORTAMENTO: prima si arma, poi ci si congeda. Il congedo
//! pretende un successore **vivo**, quindi nell'ordine inverso non scatterebbe
//! mai; e in quest'ordine, se l'apertura fallisce, questa sessione resta viva
//! invece di lasciare l'albero scoperto.
//!
//! FAIL-OPEN OVUNQUE: qualunque errore esce zero. Uno Stop hook che si rompe
//! blocca la sessione per sempre, che è peggio della compattazione che evita.

use guards::handoff::{resolve_terminal_handle, Terminal};
use guards::handoff_on_stop::{decide, Decision, Facts, RESTART_CAP_DEFAULT};
use guards::handoff_required::over_lockout;
use hook_io::journal::{self, Field};
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::time::UNIX_EPOCH;

use crate::handoff::state_dir;
use crate::handoff_required::{handoff_valid, restarts};

/// Da quando data il marcatore della consegna corrente, in nanosecondi
/// dall'epoca — 0 se illeggibile.
///
/// Il marcatore (`consegna-fatta-{session}`) è lo stesso che `handoff_valid`
/// già richiede per esistere: una data di modifica diversa da quella salvata
/// vuol dire che nel frattempo è stata scritta una consegna nuova, quindi il
/// riferimento sotto va rifatto da capo. Precisione al nanosecondo, non al
/// secondo: due consegne scritte nello stesso secondo — normale in prova, e
/// non impossibile in servizio — non devono sembrare la stessa. Stesso
/// percorso di `handoff_required::mark_done`, che qui non si importa: è
/// privato al suo modulo, e leggere una data di modifica non è scrivere
/// quel file.
fn handoff_marker_epoch(session: &str) -> u64 {
    state_dir()
        .join(format!("consegna-fatta-{session}"))
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

/// Il contesto registrato quando l'attuale consegna ha cominciato a valere
/// sopra il gradino di blocco — 0 se non c'è ancora, o se nel frattempo è
/// arrivata una consegna diversa da quella per cui era stato registrato.
///
/// Aggiorna il file appena scopre che il riferimento non vale più: la
/// registrazione ("nel momento in cui la consegna vale, si segna quanto
/// contesto c'era") avviene qui, al primo Stop che la osserva sopra il
/// gradino — non c'è un evento più vicino nel perimetro di questo gancio.
fn lockout_reference(session: &str, used: u64) -> u64 {
    let marker_epoch = handoff_marker_epoch(session);
    let path = state_dir().join(format!("consegna-stop-riferimento-{session}"));
    if let Ok(text) = fs::read_to_string(&path) {
        let parts: Vec<&str> = text.split_whitespace().collect();
        if let [saved_epoch, saved_used] = parts[..] {
            if let (Ok(saved_epoch), Ok(saved_used)) =
                (saved_epoch.parse::<u64>(), saved_used.parse::<u64>())
            {
                if saved_epoch == marker_epoch {
                    return saved_used;
                }
            }
        }
    }
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(&path, format!("{marker_epoch} {used}"));
    0
}

/// Quante forzature si sono già opposte; con `increment`, ne conta una in più.
fn stop_blocks(session: &str, increment: bool) -> u32 {
    let counter = state_dir().join(format!("consegna-stop-{session}"));
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

/// Dove una sessione lascia detto chi prosegue.
///
/// Gemello di quello che scrive `successor::note_successor`: se i due divergono
/// il congedo non scatta mai e nessuna prova se ne accorge — successo il
/// 16/08/2026. Il nome si sanifica perché un identificativo vero porta dentro
/// caratteri che in un nome di file non ci stanno: è la stessa famiglia di
/// difetto che ha reso inesistenti la tregua e il segnale della staffetta.
fn successor_marker(session: &str) -> PathBuf {
    let safe: String = session
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
        .take(64)
        .collect();
    state_dir().join(format!("successore-di-{safe}"))
}

/// L'handle del successore, se ne è stato armato uno **e sta girando**.
///
/// L'elenco arriva da fuori perché il chiamante ne ha bisogno anche per
/// ritrovare sé stesso, e questo gancio ha quindici secondi in tutto: due
/// letture da venti l'una ci stanno dentro solo per fortuna. `None` significa
/// «non si è potuto leggere», e da lì non si conclude niente — un congedo contro
/// un'intenzione invece che contro una prova spegne chi lavora.
///
/// SI RISOLVE DALLA `tabId`, come fa la staffetta. Il 17/08/2026 lo scrittore ha
/// cominciato a salvarla accanto all'handle proprio perché l'handle scade al
/// primo riattacco del pannello, ma la correzione era arrivata solo all'altro
/// lettore (`relay::armed_successor`): qui si continuava a confrontare
/// l'identificatore che invecchia, e un successore vivo risultava morto. Nello
/// stesso periodo: 14 successori aperti, **una** sessione vecchia congedata.
fn successor_alive(session: &str, terminals: Option<&[Terminal]>) -> String {
    if session.is_empty() {
        return String::new();
    }
    let Ok(text) = fs::read_to_string(successor_marker(session)) else {
        return String::new();
    };
    let Ok(d) = serde_json::from_str::<serde_json::Value>(&text) else {
        return String::new();
    };
    let Some(list) = terminals else {
        return String::new();
    };
    let get = |k: &str| d.get(k).and_then(|v| v.as_str()).unwrap_or("");
    // Il worktree resta vuoto di proposito: qui si cerca **quel** successore, e
    // dedurlo dall'albero adotterebbe una scheda qualsiasi che ci abita.
    let handle = resolve_terminal_handle(get("tabId"), "", get("handle"), list);
    if handle.is_empty() {
        return handle;
    }
    // UNA SCHEDA APERTA NON È UN SUCCESSORE. La tab nasce trenta secondi prima
    // della sessione che dovrà ospitare — `sleep 30; exec claude` — e in quella
    // finestra esiste, risponde all'elenco, e dentro non c'è nessuno. Congedarsi
    // contro di essa chiuderebbe l'unica sessione viva del lavoro senza lasciarne
    // nessuna: è il caso che questo gancio esiste per non fare. Orca antepone un
    // marcatore al titolo quando un agente c'è davvero, ed è la sola prova
    // disponibile qui — sull'elenco già letto, senza una seconda chiamata da
    // venti secondi dentro un gancio che ne ha quindici in tutto.
    let ha_agente = list
        .iter()
        .find(|t| t.handle == handle)
        .map(|t| {
            t.title
                .chars()
                .any(|c| guards::successor::AGENT_MARKS.contains(&c))
        })
        .unwrap_or(false);
    if ha_agente {
        handle
    } else {
        String::new()
    }
}

/// Chiude la propria scheda, ma solo se qualcuno prosegue davvero.
///
/// La propria scheda si chiede a `resolve_terminal_handle`, che parte da
/// `ORCA_TAB_ID`: la tab sopravvive al riavvio del terminale, l'handle no. Prima
/// qui c'era `ORCA_TERMINAL_HANDLE`, ed è il motivo per cui questo congedo —
/// scritto, provato 20/20 e registrato — non ha mai chiuso una sola scheda.
/// Verde e inerte, che è il modo peggiore di essere rotti.
fn farewell(session: &str, terminals: Option<&[Terminal]>, orca: crate::relay::OrcaFn) -> bool {
    let Some(list) = terminals else {
        return false; // elenco illeggibile: non è «non c'è nessuno»
    };
    if list.is_empty() {
        return false;
    }
    let mine = resolve_terminal_handle(
        &std::env::var("ORCA_TAB_ID").unwrap_or_default(),
        &std::env::var("ORCA_WORKTREE_ID").unwrap_or_default(),
        &std::env::var("ORCA_TERMINAL_HANDLE").unwrap_or_default(),
        list,
    );
    let heir = successor_alive(session, terminals);
    if mine.is_empty() || heir.is_empty() || heir == mine {
        return false;
    }
    // L'esito del comando non prova la chiusura — su questa CLI un `ok: false`
    // ha già accompagnato una chiusura riuscita e un `ok: true` una mancata. Si
    // riferisce l'intenzione; la verifica la fa chi rilegge l'elenco.
    let _ = orca(&["terminal", "close", "--terminal", &mine]);
    journal::record(
        "consegna-allo-stop",
        "congeda",
        "successore-vivo",
        &[
            ("successore", Field::Text(heir)),
            ("propria", Field::Text(mine)),
        ],
    );
    true
}

/// Su Stop: se la sessione è piena e ha consegnato, apre chi prosegue.
///
/// Non decide niente da sola — i quattro freni restano quelli di
/// `successor::arm`, che è la sede unica. Qui si decide solo **quando**
/// chiederglielo, e senza un documento da citare non si chiede: il successore
/// partirebbe cieco.
///
/// IL BUCO CHE CHIUDE, misurato il 16/08/2026: l'unico innesco era la scrittura
/// di una consegna, quindi una sessione piena che aveva già consegnato e
/// proseguiva senza toccare file non armava mai niente — trovata così una
/// sessione al 106% del budget di Opus 5, consegna fatta, ancora viva.
fn arm_successor(session: &str, used: u64, require: u64) -> bool {
    if used == 0 || used < require {
        return false; // consegnata ma non piena: non si rigenera per sport
    }
    let cwd = std::env::current_dir()
        .unwrap_or_default()
        .display()
        .to_string();
    let doc = crate::relay::latest_handoff(&cwd);
    if doc.is_empty() {
        return false;
    }
    matches!(
        // `true`: la pienezza l'ha già verificata la guardia qui sopra, ed è la
        // condizione d'ingresso di questo percorso. `false`: qui non si arriva
        // mai da un subagent, perché il gancio dello Stop esce prima di tutto
        // se la chiamata viene da uno (vedi `handoff::in_subagent`).
        crate::successor::arm(&doc, session, "stop", true, false),
        crate::successor::ArmOutcome::Open(_)
    )
}

fn orca_real(args: &[&str]) -> (i32, String) {
    match std::process::Command::new("orca").args(args).output() {
        Ok(o) => (
            o.status.code().unwrap_or(1),
            String::from_utf8_lossy(&o.stdout).into_owned(),
        ),
        Err(_) => (1, String::new()),
    }
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
    // marcata come prova.
    hook_io::mark_live_from_payload(&data);
    let session_intero = data
        .get("session_id")
        .and_then(|v| v.as_str())
        .unwrap_or("ignota")
        .to_string();
    let session: String = session_intero.chars().take(8).collect();
    let transcript = data
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let stop_hook_active = data
        .get("stop_hook_active")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // I due anti-loop si valutano prima di toccare il disco: senza, ogni catena
    // indotta lascerebbe dietro di sé i memo della misura e delle ripartenze.
    // Per lo stesso motivo il subagent esce di qui: i marcatori che si
    // scriverebbero sotto sono intestati alla madre. Vedi `handoff::in_subagent`
    // per cui si guarda `agent_id` e non il percorso del transcript.
    if stop_hook_active || transcript.is_empty() || crate::handoff::in_subagent(&data) {
        return 0;
    }

    let used = crate::handoff::context_used(transcript, &session);
    let t = crate::handoff::thresholds(transcript);
    let valida = handoff_valid(transcript, &session);
    let n_restarts = restarts(transcript, &session);
    let restart_cap = std::env::var("RIPARTENZA_TETTO_COMPATT")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(RESTART_CAP_DEFAULT);
    // Il riferimento si legge (e si registra, se manca) solo quando può
    // servire: sotto il gradino o senza consegna valida `decide` non lo
    // guarda, e leggerlo comunque scriverebbe un file per sessione a ogni
    // Stop di lavoro normale.
    let context_at_handoff = if valida && over_lockout(used, t.budget) {
        lockout_reference(&session, used)
    } else {
        0
    };

    let fatti = Facts {
        stop_hook_active,
        // Falso per costruzione: sopra si è già usciti.
        in_subagent: false,
        has_transcript: true,
        thresholds: &t,
        used,
        handoff_valid: valida,
        context_at_handoff,
        restarts: n_restarts,
        restart_cap,
        // Il contatore si legge senza incrementare: l'incremento è l'effetto di
        // una forzatura, e va fatto solo se la decisione arriva davvero lì.
        stop_blocks_so_far: stop_blocks(&session, false),
    };
    let pct = guards::handoff::percent(used, t.budget);

    match decide(&fatti) {
        Decision::Pass => 0,
        Decision::Settle => {
            // L'elenco si legge UNA volta e serve a due domande: chi prosegue, e
            // qual è la propria scheda. Chiederlo due volte a venti secondi
            // l'una sfonderebbe il timeout del gancio.
            let mut orca = orca_real;
            let terminals = crate::relay::read_terminals(&mut orca);
            if successor_alive(&session_intero, terminals.as_deref()).is_empty() {
                arm_successor(&session_intero, used, t.require);
            }
            farewell(&session_intero, terminals.as_deref(), &mut orca);
            0
        }
        Decision::Surrender => {
            let n = stop_blocks(&session, true);
            journal::record(
                "consegna-allo-stop",
                "passa",
                "arreso",
                &[
                    ("session", Field::Text(session.clone())),
                    ("percento", Field::Number(pct as i64)),
                    ("blocchi", Field::Number(n as i64)),
                ],
            );
            0
        }
        Decision::Block(messaggio) => {
            // `blocchi` è il conteggio **dopo** questa forzatura, come
            // nell'originale: la riga della resa cita quante ne erano già state
            // opposte, quella del blocco quante ne conta insieme a sé.
            let n = stop_blocks(&session, true);
            journal::record(
                "consegna-allo-stop",
                "blocca",
                "consegna-non-fatta",
                &[
                    ("session", Field::Text(session.clone())),
                    ("percento", Field::Number(pct as i64)),
                    ("blocchi", Field::Number((n + 1) as i64)),
                    ("ripartenze", Field::Number(n_restarts as i64)),
                ],
            );
            eprint!("{messaggio}");
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    /// Un pannello con un agente dentro: il marcatore lo mette Orca.
    fn term(handle: &str, tab: &str) -> Terminal {
        Terminal {
            handle: handle.into(),
            tab_id: tab.into(),
            worktree_id: String::new(),
            title: "◑ al lavoro".into(),
        }
    }

    /// Una scheda aperta e ancora vuota: esiste, ma dentro non c'è nessuno.
    ///
    /// È lo stato normale nei trenta secondi fra `terminal create` e `exec
    /// claude`. Congedarsi contro di essa chiude l'unica sessione viva del lavoro
    /// senza lasciarne nessuna.
    fn term_vuoto(handle: &str, tab: &str) -> Terminal {
        Terminal {
            title: "consegna raccolta (parte da sola)".into(),
            ..term(handle, tab)
        }
    }

    #[test]
    fn una_scheda_ancora_vuota_non_e_un_successore() {
        let casa = HomeIsolata::nuova("congedo-vuota");
        scrivi_marcatore(&casa, "S5", r#"{"handle":"term_a","tabId":"tab-1"}"#);
        assert_eq!(successor_alive("S5", Some(&[term_vuoto("term_a", "tab-1")])), "");
        // Trenta secondi dopo, con l'agente dentro, il congedo può procedere.
        assert_eq!(successor_alive("S5", Some(&[term("term_a", "tab-1")])), "term_a");
    }

    fn scrivi_marcatore(casa: &HomeIsolata, sessione: &str, json: &str) {
        fs::write(casa.stato().join(format!("successore-di-{sessione}")), json).unwrap();
    }

    /// Il caso che il congedo sbagliava: la scheda è viva, l'handle è scaduto.
    ///
    /// È lo scenario normale dopo un riattacco del pannello. Prima il successore
    /// risultava morto e la sessione vecchia restava aperta accanto a lui.
    #[test]
    fn la_tab_ritrova_il_successore_quando_l_handle_e_scaduto() {
        let casa = HomeIsolata::nuova("congedo-tab");
        scrivi_marcatore(
            &casa,
            "S1",
            r#"{"handle":"term_vecchio","tabId":"tab-42","quando":"x"}"#,
        );
        assert_eq!(
            successor_alive("S1", Some(&[term("term_nuovo", "tab-42")])),
            "term_nuovo"
        );
    }

    /// I marcatori scritti prima del 17/08/2026 non hanno la tab: l'handle resta
    /// l'unica strada, e vale solo se è ancora fra i vivi.
    #[test]
    fn senza_tab_si_ricade_sull_handle_ma_solo_se_vivo() {
        let casa = HomeIsolata::nuova("congedo-handle");
        scrivi_marcatore(&casa, "S2", r#"{"handle":"term_a","quando":"x"}"#);
        assert_eq!(successor_alive("S2", Some(&[term("term_a", "t1")])), "term_a");
        assert_eq!(successor_alive("S2", Some(&[term("term_b", "t1")])), "");
    }

    /// Una scheda chiusa non congeda nessuno: se la tab non è più fra i vivi la
    /// risposta è vuota anche quando il vecchio handle per caso combacia.
    #[test]
    fn una_tab_sparita_non_autorizza_il_congedo() {
        let casa = HomeIsolata::nuova("congedo-morto");
        scrivi_marcatore(
            &casa,
            "S3",
            r#"{"handle":"term_a","tabId":"tab-sparita","quando":"x"}"#,
        );
        assert_eq!(
            successor_alive("S3", Some(&[term("term_a", "tab-altra")])),
            ""
        );
    }

    /// Elenco illeggibile e marcatore assente non sono «non c'è nessuno».
    #[test]
    fn nel_dubbio_non_si_congeda() {
        let casa = HomeIsolata::nuova("congedo-dubbio");
        scrivi_marcatore(&casa, "S4", r#"{"handle":"term_a","tabId":"tab-1"}"#);
        assert_eq!(successor_alive("S4", None), "");
        assert_eq!(
            successor_alive("mai-armata", Some(&[term("term_a", "tab-1")])),
            ""
        );
        assert_eq!(successor_alive("", Some(&[term("term_a", "tab-1")])), "");
    }

    /// Il riferimento si registra una volta e resta fermo finché il
    /// marcatore della consegna non cambia — non a ogni chiamata.
    ///
    /// MUTANTE: se `lockout_reference` restituisse `used` invece del valore
    /// salvato al momento della registrazione, la seconda riga passerebbe lo
    /// stesso ma con un numero diverso da quello atteso; è la terza riga,
    /// dopo la consegna nuova, a morire da sola se il confronto sull'istante
    /// del marcatore sparisce.
    #[test]
    fn lockout_reference_registers_once_then_holds_until_the_handoff_changes() {
        let home = HomeIsolata::nuova("riferimento-registra");
        let marker = home.stato().join("consegna-fatta-S9");
        fs::write(&marker, "1").unwrap();
        assert_eq!(lockout_reference("S9", 480_000), 0, "prima volta: si registra ora");
        assert_eq!(
            lockout_reference("S9", 495_000),
            480_000,
            "il riferimento resta quello della registrazione, non l'used corrente"
        );
        // Una consegna nuova tocca il marcatore: il vecchio riferimento non vale più.
        std::thread::sleep(std::time::Duration::from_millis(2));
        fs::write(&marker, "1").unwrap();
        assert_eq!(
            lockout_reference("S9", 495_000),
            0,
            "consegna nuova: si registra di nuovo"
        );
        assert_eq!(lockout_reference("S9", 500_000), 495_000);
    }

    /// Senza marcatore la data vale zero: non si spaccia un errore per un
    /// riferimento legittimo.
    #[test]
    fn a_missing_marker_gives_a_zero_stamp() {
        let _home = HomeIsolata::nuova("riferimento-senza-marcatore");
        assert_eq!(handoff_marker_epoch("nessuno"), 0);
    }
}
