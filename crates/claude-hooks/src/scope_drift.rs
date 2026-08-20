//! Il gancio PostToolUse che avvisa quando la sessione cambia mestiere.
//!
//! Il giudizio sta in `guards::scope_drift`; qui c'è ciò che tocca il mondo: il
//! payload su stdin, il file di stato per sessione, il registro e l'avviso.
//!
//! È IL GANCIO PIÙ CALDO DEL PARCO INSIEME A `observe`: sta su `PostToolUse` con
//! matcher `*`, quindi parte dopo **ogni** chiamata a strumento. È l'unica
//! ragione per cui vale la pena portarlo — il suo lavoro è quasi niente, e
//! quasi tutto il suo costo era l'avvio dell'interprete Python.
//!
//! IL REGISTRO QUI NON È QUELLO DI `journal::record`, e non è una svista.
//! L'originale scrive tramite `scripts/hook_log.py`, cioè con `json.dumps(...,
//! ensure_ascii=False)`: separatori `", "` e `": "`, con lo spazio.
//! `journal::record` riproduce invece `scripts/hook-log.js`, che essendo
//! `JSON.stringify` non ha spazi. Le due forme convivono già in `ganci.jsonl` —
//! misurate il 17/08/2026: 2093 righe con lo spazio, 3517 senza — e chi porta un
//! gancio deve guardare quale delle due usa il suo originale, non indovinare.
//! Uniformarle è legittimo, ma è un cambio di formato su un archivio storico: si
//! decide e si dichiara, non lo si fa di straforo dentro un porto.
//!
//! NON BLOCCA MAI. Esce sempre 0, e l'avviso va su **stdout**, come
//! nell'originale: è un fatto messo davanti a chi lavora, non un rifiuto.

use guards::scope_drift::{areas_in_text, decide, notice, tool_text};
use std::collections::BTreeSet;
use std::fs;
use std::io::{Read, Write};
use std::path::PathBuf;

use crate::handoff::state_dir;

const CEILING_BYTES: u64 = 5 * 1024 * 1024;

/// Dove la sessione tiene il conto delle aree che ha toccato.
///
/// Il nome del file è l'identificativo di sessione **grezzo**, come
/// nell'originale: nessuna sanificazione. Un identificativo con una barra
/// dentro produce un percorso inesistente e la scrittura fallisce in silenzio —
/// la sessione ripete l'avviso invece di tacerlo, che è il verso giusto in cui
/// sbagliare per un gancio che non blocca niente.
fn state_file(session: &str) -> PathBuf {
    state_dir().join("scope-drift").join(format!("{session}.json"))
}

/// Le aree già viste e se l'avviso è già stato dato.
///
/// TRE ESITI, non due. File assente o JSON illeggibile valgono «nessuna area»:
/// il gancio ricomincia a contare, e al massimo ripete un avviso — l'originale
/// li cattura entrambi con `except (OSError, ValueError)`.
///
/// `None` è il terzo, e significa «il gancio si ferma qui». Nell'originale non è
/// un ramo scritto: è un'eccezione che sfugge ai due `except` e arriva al
/// `try` esterno, che esce 0 senza scrivere e senza stampare. Succede quando lo
/// stato non è un oggetto (`state.get` su una lista è un `AttributeError`) o
/// quando fra le aree c'è un valore non testuale (`sorted` su `{"suite", None}`
/// è un `TypeError`). Trovati dal confronto, non a mente: entrambi lasciavano il
/// porto a scrivere uno stato dove l'originale non toccava niente.
///
/// RESIDUO DICHIARATO: un'area non testuale **da sola** non farebbe crollare
/// `sorted` — un insieme di un elemento non confronta niente — e lì l'originale
/// scriverebbe `[null]` mentre qui ci si ferma. È irraggiungibile, perché a
/// scrivere quel file è solo questo gancio e questo gancio scrive solo stringhe.
fn read_state(path: &PathBuf) -> Option<(BTreeSet<String>, bool)> {
    let Ok(text) = fs::read_to_string(path) else {
        return Some((BTreeSet::new(), false));
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return Some((BTreeSet::new(), false));
    };
    if !v.is_object() {
        return None;
    }
    // `bool(state.get("detto"))` di Python: vero anche per un numero diverso da
    // zero o una stringa non vuota, non solo per `true`.
    let said = match v.get("detto") {
        None | Some(serde_json::Value::Null) => false,
        Some(serde_json::Value::Bool(b)) => *b,
        Some(serde_json::Value::String(s)) => !s.is_empty(),
        Some(serde_json::Value::Number(n)) => n.as_f64().unwrap_or(0.0) != 0.0,
        Some(serde_json::Value::Array(a)) => !a.is_empty(),
        Some(serde_json::Value::Object(o)) => !o.is_empty(),
    };
    let areas = match v.get("aree") {
        Some(serde_json::Value::Array(items)) => {
            if items.iter().any(|x| !x.is_string()) {
                return None; // `sorted` mescolando i tipi è un TypeError
            }
            items
                .iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        }
        // `set("suite")` in Python è l'insieme dei suoi caratteri, non
        // `{"suite"}`. Capita solo su un file corrotto o scritto a mano, ma è la
        // differenza fra «i due si comportano uguale» e «quasi».
        Some(serde_json::Value::String(s)) => s.chars().map(|c| c.to_string()).collect(),
        _ => BTreeSet::new(),
    };
    Some((areas, said))
}

/// La riga del registro, nella forma che scrive `hook_log.py`.
///
/// Senza, «il gancio è partito» resta un'impressione. Questo avviso esce **una
/// volta per sessione** e solo per sessioni che toccano tre aree: senza una riga
/// qui, per sapere quante volte ha parlato bisognerebbe rileggere le
/// trascrizioni — che è esattamente il mezzo giorno di lavoro per cui il
/// registro esiste.
///
/// L'UNICA DIVERGENZA VOLUTA DAL PYTHON, e non è una scelta di stile: **nel
/// Python questo registro non esiste**. `scope-drift.py` importa `log_hook` e
/// `log_failure` da `hook_log`, che esporta invece `registra` e
/// `registra_guasto` — la rinomina in inglese non è arrivata al chiamante, e
/// l'`except ImportError` che doveva essere una rete trasforma i due in funzioni
/// vuote. Misurato il 17/08/2026: `ganci.jsonl` ha 5610 righe e **zero**
/// `scope-drift`, e per la stessa ragione nemmeno un guasto di questo gancio
/// lascerebbe traccia. Qui la riga si scrive, perché è ciò che l'intestazione
/// dell'originale promette; il confronto continua a verificarla byte per byte
/// chiedendo il formato a `hook_log.registra` in persona, invece che al gancio
/// rotto. Correggere l'originale è una riga sola, ma sta in `skills/hooks/`, che
/// altre sessioni stanno usando: va fatto sapendo che tocca a chi ci lavora.
fn log_notice(session: &str, areas: &BTreeSet<String>) {
    let line = hook_io::python_json::dumps_unicode(&serde_json::json!({
        "t": hook_io::journal::now_iso8601_python(),
        "gancio": "scope-drift",
        "decisione": "avvisa",
        "motivo": "terza-area",
        // `session[:8]` dell'originale: **caratteri**, non byte.
        "session": session.chars().take(8).collect::<String>(),
        "areas": areas.iter().cloned().collect::<Vec<_>>(),
    }));
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

/// `str(ev.get("session_id") or "")` dell'originale.
///
/// NON È `as_str()`, ed è la differenza che si vede solo leggendo il Python:
/// `or ""` scarta i valori **falsi** — assente, `null`, stringa vuota, zero,
/// `false` — e `str()` accetta tutto il resto, numero compreso. Senza stato
/// questo gancio non sa dire se ha già parlato, quindi un identificativo vuoto
/// fa uscire subito; ma uno numerico è un identificativo valido per l'originale,
/// e trattarlo come assente farebbe tacere il porto dove l'oracolo parla.
///
/// I contenitori restano fuori: `str([1])` di Python è `"[1]"`, cioè un nome di
/// file, e nessun evento di Claude Code manda una lista come `session_id`.
/// Riprodurre il `repr` di Python per un caso che non esiste costa più di quanto
/// vale — è dichiarato qui invece che scoperto dopo.
fn session_id(v: Option<&serde_json::Value>) -> String {
    match v {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Number(n)) => {
            if n.as_f64() == Some(0.0) {
                String::new() // `0 or ""` è `""`
            } else {
                n.to_string()
            }
        }
        Some(serde_json::Value::Bool(true)) => "True".to_string(),
        _ => String::new(),
    }
}

pub fn run() -> i32 {
    if std::env::var("SCOPE_DRIFT").as_deref() == Ok("off") {
        return 0;
    }
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        return 0;
    }
    let Ok(ev) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return 0;
    };
    process(&ev)
}

/// Il gancio a valle di stdin, così la provenienza si può provare in batteria.
fn process(ev: &serde_json::Value) -> i32 {
    // DENTRO UN SUBAGENT SI TACE, e va prima di ogni cosa che tocchi il disco.
    //
    // Le aree si contano **per sessione**, e un subagent condivide la sessione
    // della madre: le sue aree finivano nel conto di lei. Il danno non è il
    // conto gonfiato, è che questo avviso esce **una volta sola** — scritto
    // `detto: true`, non torna più. Misurato il 21/08/2026 sul binario in
    // servizio: chiamata da subagent e chiamata dalla madre con le stesse tre
    // aree, la prima riceve l'avviso e la seconda tace. Cioè lo legge chi non
    // può chiudere la sessione, e chi può non lo vede mai.
    //
    // È lo stesso difetto del presidio della consegna, in un altro punto: il
    // controllo giusto applicato al soggetto sbagliato.
    if crate::handoff::in_subagent(ev) {
        return 0;
    }

    let session = session_id(ev.get("session_id"));
    let Some(args) = ev.get("tool_input").filter(|v| v.is_object()) else {
        return 0;
    };
    if session.is_empty() {
        return 0;
    }

    let fresh = areas_in_text(&tool_text(args));
    // Il filtro che rende il caso normale gratuito: la stragrande maggioranza
    // delle chiamate non nomina nessuna area, e lì non si tocca il disco.
    if fresh.is_empty() {
        return 0;
    }

    let path = state_file(&session);
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let Some((seen, already_said)) = read_state(&path) else {
        return 0; // stato inutilizzabile: l'originale muore qui, in silenzio
    };
    let esito = decide(&seen, &fresh, already_said);
    if !esito.write {
        return 0;
    }

    let _ = fs::write(
        &path,
        hook_io::python_json::dumps(&serde_json::json!({
            "aree": esito.areas.iter().cloned().collect::<Vec<_>>(),
            "detto": already_said || esito.speak,
        })),
    );

    if !esito.speak {
        return 0;
    }
    log_notice(&session, &esito.areas);
    println!("{}", notice(&esito.areas));
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    fn scrivi(casa: &HomeIsolata, sessione: &str, json: &str) {
        let dir = casa.stato().join("scope-drift");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{sessione}.json")), json).unwrap();
    }

    /// Il file che il gancio ha scritto davvero, byte per byte.
    ///
    /// Si guarda il **testo**, non l'oggetto rianalizzato: la forma è ciò che
    /// l'altra implementazione deve produrre uguale, e un confronto fra oggetti
    /// non vedrebbe né i separatori né l'ordine delle chiavi.
    #[test]
    fn lo_stato_si_scrive_nella_forma_di_json_dumps() {
        let casa = HomeIsolata::nuova("scope-drift-forma");
        scrivi(&casa, "S1", r#"{"aree": ["suite"], "detto": false}"#);
        let (aree, detto) = read_state(&state_file("S1")).unwrap();
        assert_eq!(aree.len(), 1);
        assert!(!detto);
        let esito = decide(&aree, &areas_in_text("/Users/theo/gyver/work/packages/x"), detto);
        let scritto = hook_io::python_json::dumps(&serde_json::json!({
            "aree": esito.areas.iter().cloned().collect::<Vec<_>>(),
            "detto": detto || esito.speak,
        }));
        // Gli spazi dopo `:` e `,` sono quelli di `json.dumps` predefinito.
        assert_eq!(scritto, r#"{"aree": ["packages", "suite"], "detto": false}"#);
    }

    /// Uno stato illeggibile non deve far esplodere niente né mentire sul «detto».
    #[test]
    fn uno_stato_corrotto_vale_come_stato_vuoto() {
        let casa = HomeIsolata::nuova("scope-drift-corrotto");
        scrivi(&casa, "S2", "non sono json");
        assert_eq!(read_state(&state_file("S2")), Some((BTreeSet::new(), false)));
        // Un file che non c'è: stessa risposta.
        assert_eq!(
            read_state(&state_file("mai-vista")),
            Some((BTreeSet::new(), false))
        );
    }

    /// I due stati su cui l'originale muore in silenzio: qui ci si ferma uguale.
    ///
    /// Trovati dal confronto col Python, non a mente. Prima di questi due casi
    /// il porto riscriveva lo stato dove l'originale non toccava niente.
    #[test]
    fn uno_stato_inutilizzabile_ferma_il_gancio() {
        let casa = HomeIsolata::nuova("scope-drift-inutilizzabile");
        // `state.get("detto")` su una lista è un AttributeError.
        scrivi(&casa, "L1", "[1, 2, 3]");
        assert_eq!(read_state(&state_file("L1")), None);
        // `sorted({"suite", None})` è un TypeError.
        scrivi(&casa, "L2", r#"{"aree": ["suite", null], "detto": false}"#);
        assert_eq!(read_state(&state_file("L2")), None);
    }

    /// `set("suite")` di Python sono i suoi caratteri: il quirk si conserva,
    /// altrimenti su un file scritto a mano le due implementazioni divergono.
    #[test]
    fn aree_come_stringa_diventano_i_suoi_caratteri() {
        let casa = HomeIsolata::nuova("scope-drift-stringa");
        scrivi(&casa, "S3", r#"{"aree": "abc", "detto": 1}"#);
        let (aree, detto) = read_state(&state_file("S3")).unwrap();
        assert_eq!(aree.len(), 3);
        assert!(aree.contains("a"));
        // `bool(1)` è vero: l'avviso risulta già dato.
        assert!(detto);
    }

    /// La riga del registro esce con gli spazi di `json.dumps`, non senza.
    #[test]
    fn il_registro_usa_la_forma_del_python_non_quella_del_javascript() {
        let casa = HomeIsolata::nuova("scope-drift-registro");
        let aree: BTreeSet<String> = ["configurazione", "packages", "suite"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        log_notice("abcdefgh-1234", &aree);
        let riga = fs::read_to_string(casa.stato().join("ganci.jsonl")).unwrap();
        assert!(riga.contains(r#""gancio": "scope-drift""#), "{riga}");
        assert!(riga.contains(r#""decisione": "avvisa""#), "{riga}");
        assert!(riga.contains(r#""motivo": "terza-area""#), "{riga}");
        // La sessione si tronca a otto caratteri, e le aree sono ordinate.
        assert!(riga.contains(r#""session": "abcdefgh""#), "{riga}");
        assert!(
            riga.contains(r#""areas": ["configurazione", "packages", "suite"]"#),
            "{riga}"
        );
        // MUTANTE: con `journal::record` qui non ci sarebbe nessuno spazio.
        assert!(!riga.contains(r#""gancio":"scope-drift""#), "{riga}");
    }

    /// Differenziale a variabile unica: cambia solo `agent_id`, e l'esito si
    /// ribalta. Senza il caso dalla madre, un gancio spento del tutto passerebbe.
    #[test]
    fn inside_a_subagent_the_hook_writes_no_state_of_the_mother() {
        let _home = HomeIsolata::nuova("scope-drift-subagent");
        let event = |agent: Option<&str>| {
            let mut ev = serde_json::json!({
                "session_id": "S9",
                "tool_name": "Bash",
                "tool_input": {
                    "file_path": "/Users/theo/gyver/work/suite/src/x.ts",
                    "command": "pnpm --dir /Users/theo/gyver/work/matching-engine test \
                                /Users/theo/gyver/work/whatsapp/src",
                },
            });
            if let Some(a) = agent {
                ev["agent_id"] = serde_json::json!(a);
            }
            ev
        };
        // MUTANTE: tolto il ramo `in_subagent` da `process`, questo caso va in rosso.
        assert_eq!(process(&event(Some("agent-figlio"))), 0);
        assert!(
            !state_file("S9").exists(),
            "le aree di un subagent non entrano nel conto della madre"
        );
        // Stessi identici fatti, solo la provenienza cambia: qui lo stato nasce.
        assert_eq!(process(&event(None)), 0);
        assert!(
            state_file("S9").exists(),
            "dalla conversazione principale il gancio lavora come prima"
        );
    }
}
