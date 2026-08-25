//! Dichiara un filo di lavoro rimasto senza esecutore, leggibile da fuori.
//!
//! Nasce dalla segnalazione del 25/08/2026
//! (`state/plancia/segnalazioni/2026-08-25-il-successore-non-si-arma-quasi-mai-e-il-filo-muore-col-pannello.md`):
//! il gancio che apre il successore **apre nel 4,4% dei casi** — 66 aperture
//! contro 1.438 rinvii, ricontati il 25/08 su `state/ganci.jsonl`. I rinvii non
//! sono un guasto: i tre più frequenti sono tetti di risorse, cioè decisioni
//! prese apposta. Il guasto è che **dopo un rinvio non resta traccia di un filo
//! scoperto**: la consegna viene scritta, nessuno la raccoglie, e la sola riga
//! che lo dice sta nel registro dei ganci, che nessuno legge. Il 25/08 il filo
//! Sailor è rimasto senza nessuno per 2 ore e 40, e se n'è accorto Theo.
//!
//! IL RINVIO NON È IL MOMENTO IN CUI IL FILO SI SCOPRE, ed è la differenza che
//! regge tutto il modulo. Quando il successore non si arma, chi ha consegnato è
//! ancora lì: il filo ce l'ha lei. Diventa scoperto quando **quella sessione
//! sparisce senza che nessuno abbia preso il suo posto** — che è esattamente
//! ciò che è successo il 25/08 alle 10:16. Perciò il marcatore si scrive al
//! rinvio, ma conta solo dopo, e a dirlo è il registro delle sessioni vive.
//!
//! CHI LO BUTTA, la domanda che il libro di bordo impone a chi scrive un
//! marcatore («chi lo aggiorna quando il fatto cambia, e chi lo butta quando
//! scade? Se non c'è risposta, hai scritto un interruttore e credi sia un
//! freno»). Due strade, tutte e due cablate:
//!   1. `clear()` — il successore per quella sessione si arma davvero: il filo
//!      ha un esecutore, il marcatore non ha più niente da dire;
//!   2. `clear_tree()` — una sessione parte in quell'albero di lavoro: chiunque
//!      sia, quel filo adesso ha qualcuno davanti. Gira su `SessionStart`, che
//!      è l'unico evento che dice «qui c'è di nuovo qualcuno».
//!
//! Senza la seconda il marcatore non sarebbe un freno ma un interruttore: una
//! volta acceso resterebbe acceso, e un elenco che non cala smette di essere
//! letto dopo il terzo giorno.

use serde_json::Value;
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// I motivi di rinvio che lasciano davvero un filo scoperto.
///
/// NON SONO TUTTI, e sceglierli è metà del lavoro. Un elenco che contasse ogni
/// rinvio direbbe millecinquecento fili scoperti e non lo leggerebbe nessuno.
/// Entrano i **tetti di risorse** — la consegna è valida, il lavoro è finito, e
/// il successore non parte solo perché non c'è posto o non è ora — e la
/// **seconda generazione**. Ne restano fuori tre, e non per la stessa ragione:
/// - `non-piena` — `/handoff` si invoca anche a metà lavoro: la sessione sta
///   ancora lavorando, il filo ce l'ha lei;
/// - `subagent` — un figlio che consegna dentro il suo perimetro non chiude il
///   lavoro della madre, che è viva e continua;
/// - `non-dichiarata` — e questo è l'unico dei tre che non parla di chi tiene il
///   filo: qui **non c'è un filo**. Nessuno ha chiuso la consegna, e ciò che sta
///   sul disco è un documento che le somiglia.
///
/// `seconda-generazione` È DENTRO, ED È UN CAMBIO DI IDEA. La prima stesura la
/// escludeva con la ragione «tace per progetto, e chi non poteva armare non ha
/// lasciato scoperto niente» — che sono due affermazioni diverse spacciate per
/// una. Tacere è giusto: una figlia che si lamentasse di non poter armare
/// riempirebbe il contesto di ogni sessione nata così. Ma **non aver lasciato
/// scoperto niente è falso**: se quella sessione ha finito un lavoro vero e
/// dichiarato, il filo non ha nessuno — e qui è pure garantito che nessun
/// successore arriverà, perché il tetto della generazione è assoluto. Sono 118
/// rinvii su 1.438 (l'8%), non un'eccezione. Registrare in silenzio è
/// esattamente ciò che serve: nessuno lo dice a lei, ma il filo si vede.
const UNCOVERING_REASONS: &[&str] =
    &["fuori-orario", "troppe-sessioni", "albero-affollato", "seconda-generazione"];

/// Oltre questa età un filo si mostra anche se la sua sessione risulta viva.
///
/// PERCHÉ NON BASTA CHIEDERE «È VIVA». `sessioni-vive/` non si svuota in modo
/// affidabile: `forget_with` toglie il record solo quando la morte è **vista**
/// (`Gone`); se `ps` non risponde o non riconosce il nome, la liveness è
/// `Unknown` e il record resta un giorno intero — e nessuno ripassa, perché
/// `marker_sweep` esiste e non è in servizio. Una sessione finita che lascia il
/// proprio record dietro risulterebbe viva, e il suo filo **sparirebbe
/// dall'elenco proprio mentre è scoperto**: un falso negativo, cioè l'unico
/// errore che questo modulo non si può permettere.
///
/// Sei ore è la stessa misura che `guards::chain` usa per dire «fermo
/// abbastanza». Una sessione che dopo sei ore non ha né armato un successore né
/// ripreso il lavoro non lo sta tenendo, qualunque cosa dica il registro. Ci si
/// sbaglia verso il mostrare: una riga di troppo costa una riga, una di meno
/// costa il lavoro di una giornata.
const SHOW_ANYWAY_AFTER_SECONDS: i64 = 6 * 60 * 60;

fn home() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/Users/theo".into()))
}

fn markers_dir() -> PathBuf {
    home().join(".claude").join("state").join("fili-scoperti")
}

fn live_dir() -> PathBuf {
    home().join(".claude").join("state").join("sessioni-vive")
}

/// Gli stessi primi otto caratteri con cui `register_session` nomina
/// `sessioni-vive/<sess>.json`: le due cartelle si interrogano a vicenda, e con
/// due chiavi diverse il filtro «è ancora viva» mancherebbe sempre.
fn short(session_id: &str) -> String {
    session_id.chars().take(8).collect()
}

fn marker_path(session_id: &str) -> PathBuf {
    markers_dir().join(format!("{}.json", short(session_id)))
}

fn now_epoch() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

/// Questo motivo di rinvio lascia un filo scoperto? PURA.
pub fn uncovers(reason: &str) -> bool {
    UNCOVERING_REASONS.contains(&reason)
}

/// Registra che una consegna è rimasta senza successore. Chiamata dal ramo che
/// ferma di `consegna-arma-successore`, subito dopo la riga di registro.
///
/// Scrive anche quando la sessione è ancora viva: è il momento in cui si sanno
/// il motivo e il percorso della consegna, e sono i due dati che servono a chi
/// legge dopo. Il filtro «viva o no» è di chi legge, non di chi scrive.
pub fn declare(session_id: &str, cwd: &str, handoff_path: &str, reason: &str) {
    if !uncovers(reason) || session_id.is_empty() {
        return;
    }
    if fs::create_dir_all(markers_dir()).is_err() {
        return;
    }
    let body = serde_json::json!({
        "session_id": session_id,
        "cwd": cwd,
        "handoff": handoff_path,
        "reason": reason,
        "declared_at": hook_io::journal::now_iso8601_seconds(),
        "declared_at_epoch": now_epoch(),
    });
    let _ = fs::write(marker_path(session_id), body.to_string());
}

/// Il successore si è armato: quel filo ha un esecutore.
pub fn clear(session_id: &str) {
    if session_id.is_empty() {
        return;
    }
    let _ = fs::remove_file(marker_path(session_id));
}

/// Qualcuno è partito in questo albero di lavoro: i fili che vi erano rimasti
/// scoperti hanno di nuovo qualcuno davanti.
///
/// Si confronta il `cwd` per intero e non un prefisso: due alberi possono
/// annidarsi, e chi apre il figlio non ha preso in carico il padre.
///
/// I DUE PERCORSI VENGONO DA FONTI DIVERSE, E VANNO NORMALIZZATI. Chi scrive il
/// marcatore legge `std::env::current_dir()`; chi lo cancella riceve il campo
/// `cwd` del messaggio di `SessionStart`. Le due strade possono dare la stessa
/// cartella scritta in due modi — un collegamento risolto da una parte e non
/// dall'altra, una barra finale, un `..` — e allora il confronto per stringa
/// fallisce, il marcatore non viene tolto **mai** e l'elenco tiene una riga
/// morta per sempre. In questa casa quel difetto è già costato una volta: è il
/// motivo per cui `orca_cleanup` normalizza, e da lì viene la funzione riusata
/// qui invece di riscriverne una seconda.
pub fn clear_tree(cwd: &str) {
    if cwd.is_empty() {
        return;
    }
    let wanted = crate::orca_cleanup::realpath(cwd);
    let Ok(entries) = fs::read_dir(markers_dir()) else { return };
    for e in entries.flatten() {
        let path = e.path();
        let Ok(text) = fs::read_to_string(&path) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        let theirs = v.get("cwd").and_then(|x| x.as_str()).unwrap_or_default();
        if !theirs.is_empty() && crate::orca_cleanup::realpath(theirs) == wanted {
            let _ = fs::remove_file(&path);
        }
    }
}

/// Un filo che adesso non ha nessuno.
#[derive(Debug, Clone, PartialEq)]
pub struct UncoveredThread {
    pub session_id: String,
    pub cwd: String,
    pub handoff: String,
    pub reason: String,
    pub declared_at: String,
    pub seconds_uncovered: i64,
}

/// Il giudizio puro: quali marcatori contano come «scoperto», adesso.
///
/// ALL'OPPOSTO DI `permission_stall::decide_stalled`, che tiene solo le sessioni
/// **vive**: qui una sessione ancora viva è la prova che il filo NON è scoperto,
/// perché è lei a tenerlo. Conta chi non c'è più.
///
/// MA «VIVA» NON SI CREDE PER SEMPRE. Il registro delle vive tiene i record che
/// non ha potuto smentire, fino a un giorno intero, e un record rimasto dietro
/// farebbe sparire dall'elenco proprio il filo che è scoperto. Oltre
/// `SHOW_ANYWAY_AFTER_SECONDS` il marcatore si mostra comunque: chi non ha né
/// armato né ripreso in sei ore non sta tenendo niente.
/// LA SOGLIA SI MISURA SULL'ULTIMA ATTIVITÀ, NON SULLA DICHIARAZIONE, e la
/// differenza è un falso positivo intero. Il modo più comune di riprendere un
/// filo non è né armare un successore né aprire un pannello nuovo: è
/// **continuare a lavorarci nello stesso pannello**, che è quello che fa una
/// sessione a cui il successore è stato negato. Contando dalla dichiarazione,
/// dopo sei ore quella sessione comparirebbe fra i fili scoperti mentre ci sta
/// lavorando sopra in quel momento. `last_activity` risponde «quando quella
/// sessione ha toccato il suo transcript l'ultima volta», ed è iniettata per la
/// ragione di sempre: leggere il disco qui dentro renderebbe improvabili i tre
/// esiti.
pub fn decide_uncovered_with(
    markers: &[Value],
    alive_short_ids: &BTreeSet<String>,
    now: i64,
    last_activity: &dyn Fn(&str) -> Option<i64>,
) -> Vec<UncoveredThread> {
    let mut out = Vec::new();
    for m in markers {
        let Some(session_id) = m.get("session_id").and_then(|v| v.as_str()) else { continue };
        let since = m.get("declared_at_epoch").and_then(|v| v.as_i64()).unwrap_or(now);
        if alive_short_ids.contains(&short(session_id)) {
            // Da quando non si muove: se il transcript non si legge, si torna
            // alla dichiarazione — non si inventa un'attività che non si è vista.
            let quiet_since = last_activity(session_id).unwrap_or(since);
            if (now - quiet_since) < SHOW_ANYWAY_AFTER_SECONDS {
                continue; // è ancora lei a tenere il filo, e lo sta toccando
            }
        }
        out.push(UncoveredThread {
            session_id: session_id.to_string(),
            cwd: m.get("cwd").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            handoff: m.get("handoff").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            reason: m.get("reason").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            declared_at: m.get("declared_at").and_then(|v| v.as_str()).unwrap_or("").to_string(),
            seconds_uncovered: (now - since).max(0),
        });
    }
    out.sort_by(|a, b| b.seconds_uncovered.cmp(&a.seconds_uncovered));
    out
}

/// Quando la sessione ha toccato il proprio transcript l'ultima volta.
///
/// Il percorso lo porta già il suo record in `sessioni-vive/`: non serve
/// indovinarlo dal nome dell'albero, e se il record non c'è la domanda non ha
/// risposta — il che è coerente, perché senza record la sessione non risulta
/// nemmeno viva.
fn last_activity_of(session_id: &str) -> Option<i64> {
    let raw = fs::read_to_string(live_dir().join(format!("{}.json", short(session_id)))).ok()?;
    let record = serde_json::from_str::<Value>(&raw).ok()?;
    let transcript = record.get("transcript_path").and_then(|v| v.as_str())?;
    let modified = fs::metadata(transcript).ok()?.modified().ok()?;
    modified.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// La forma comoda: stessa decisione, col disco già interrogato.
pub fn decide_uncovered(
    markers: &[Value],
    alive_short_ids: &BTreeSet<String>,
    now: i64,
) -> Vec<UncoveredThread> {
    decide_uncovered_with(markers, alive_short_ids, now, &last_activity_of)
}

fn alive_short_ids() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    if let Ok(entries) = fs::read_dir(live_dir()) {
        for e in entries.flatten() {
            if let Some(stem) = e.path().file_stem().and_then(|s| s.to_str()) {
                out.insert(stem.to_string());
            }
        }
    }
    out
}

fn read_markers() -> Vec<Value> {
    let Ok(entries) = fs::read_dir(markers_dir()) else { return Vec::new() };
    entries
        .flatten()
        .filter_map(|e| fs::read_to_string(e.path()).ok())
        .filter_map(|s| serde_json::from_str::<Value>(&s).ok())
        .collect()
}

/// La riga che una sessione appena aperta si trova davanti, se c'è del lavoro
/// che non ha nessuno. Vuota quando non c'è niente da dire.
///
/// È QUI CHE IL MODULO SERVE A QUALCOSA, e senza questa funzione non serviva.
/// La segnalazione da cui nasce chiedeva che un filo scoperto fosse **visibile**
/// — «una riga in coda o sulla plancia, non solo nel registro dei ganci». Un
/// comando da riga di comando che nessuno lancia è il registro dei ganci con un
/// altro nome: il dato esiste, e lo si scopre solo se si è già sospettato che ci
/// fosse qualcosa da scoprire. Il 25/08/2026 quel sospetto è venuto a Theo dopo
/// due ore e quaranta.
///
/// L'AVVIO DI UNA SESSIONE È IL MOMENTO GIUSTO perché chi arriva è esattamente
/// chi potrebbe raccogliere il filo, ed è l'unico istante in cui qualcuno sta
/// per scegliere cosa fare. Si dice il minimo: quanti, e il più vecchio. Chi
/// vuole l'elenco intero lo chiede a `claude-hooks fili-scoperti`.
pub fn opening_notice() -> String {
    let threads = decide_uncovered(&read_markers(), &alive_short_ids(), now_epoch());
    let Some(first) = threads.first() else { return String::new() };
    let quanti = if threads.len() == 1 {
        "C'e' 1 filo di lavoro che non ha nessuno".to_string()
    } else {
        format!("Ci sono {} fili di lavoro che non hanno nessuno", threads.len())
    };
    format!(
        "{quanti}. Il piu' vecchio aspetta da {} h: {} (consegna: {}).\n\
         Non e' un ordine — se stai per fare altro, lascialo. L'elenco: `claude-hooks fili-scoperti`.",
        first.seconds_uncovered / 3600,
        if first.cwd.is_empty() { "(albero ignoto)" } else { &first.cwd },
        if first.handoff.is_empty() { "(nessuna)" } else { &first.handoff },
    )
}

/// `claude-hooks fili-scoperti`: cosa non ha nessuno, adesso.
///
/// Esce 0 anche con dei fili scoperti: è un rapporto, non un cancello. Chi
/// vuole un cancello guarda il conto.
pub fn run_report() -> i32 {
    let threads = decide_uncovered(&read_markers(), &alive_short_ids(), now_epoch());
    if threads.is_empty() {
        println!("nessun filo scoperto");
        return 0;
    }
    println!("{} filo/fili senza nessuno:", threads.len());
    for t in &threads {
        println!(
            "  {} · fermo da {} min · {} · consegna: {}",
            t.session_id,
            t.seconds_uncovered / 60,
            if t.cwd.is_empty() { "(albero ignoto)" } else { &t.cwd },
            if t.handoff.is_empty() { "(nessuna)" } else { &t.handoff },
        );
        println!("      il successore non parti' per: {}", t.reason);
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker(session: &str, cwd: &str, since: i64) -> Value {
        serde_json::json!({
            "session_id": session,
            "cwd": cwd,
            "handoff": "/consegna.md",
            "reason": "albero-affollato",
            "declared_at": "2026-08-25T10:00:00",
            "declared_at_epoch": since,
        })
    }

    fn alive(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_thread_whose_session_is_still_alive_is_not_uncovered() {
        // È il caso normale subito dopo il rinvio: la sessione ha consegnato ma
        // sta ancora lì. Contarlo scoperto riempirebbe l'elenco di righe vere
        // per un istante e false per tutto il resto del tempo.
        let m = [marker("aaaabbbb-1111", "/albero", 100)];
        assert!(decide_uncovered(&m, &alive(&["aaaabbbb"]), 1000).is_empty());
    }

    #[test]
    fn a_thread_whose_session_is_gone_is_uncovered() {
        let m = [marker("aaaabbbb-1111", "/albero", 100)];
        let out = decide_uncovered(&m, &alive(&["ccccdddd"]), 1000);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].session_id, "aaaabbbb-1111");
        assert_eq!(out[0].seconds_uncovered, 900);
    }

    #[test]
    fn the_longest_uncovered_comes_first() {
        // Chi legge un elenco ne guarda la prima riga: deve essere quella che
        // aspetta da piu' tempo, non quella che il disco ha restituito per prima.
        let m = [marker("aaaabbbb", "/a", 900), marker("ccccdddd", "/c", 100)];
        let out = decide_uncovered(&m, &alive(&[]), 1000);
        assert_eq!(out[0].session_id, "ccccdddd");
        assert_eq!(out[1].session_id, "aaaabbbb");
    }

    #[test]
    fn a_refusal_leaves_a_thread_uncovered_when_nobody_is_left_holding_it() {
        // I tetti di risorse: consegna valida, nessun posto per il successore.
        assert!(uncovers("fuori-orario"));
        assert!(uncovers("troppe-sessioni"));
        assert!(uncovers("albero-affollato"));
        // E la seconda generazione, che non armera' MAI un successore: il
        // silenzio verso di lei e' giusto, dare per scontato che non abbia
        // lasciato niente scoperto non lo era.
        assert!(uncovers("seconda-generazione"));
        // I due che non contano, perche' il lavoro ha ancora qualcuno: chi sta
        // ancora lavorando, e il figlio che consegna mentre la madre continua.
        assert!(!uncovers("non-piena"));
        assert!(!uncovers("non-dichiarata"));
        assert!(!uncovers("subagent"));
    }

    /// Nessuna attività nota: `last_activity` non risponde, come per una
    /// sessione il cui transcript non si legge.
    fn silent(_: &str) -> Option<i64> {
        None
    }

    #[test]
    fn a_live_session_stops_counting_after_six_hours() {
        // IL FALSO NEGATIVO CHE IL MODULO NON PUO' PERMETTERSI. Il registro
        // delle vive tiene i record che non ha potuto smentire, fino a un
        // giorno: senza questa soglia, una sessione finita che lascia dietro il
        // proprio record farebbe sparire dall'elenco il filo proprio mentre e'
        // scoperto.
        //
        // LE SEI ORE SONO SCRITTE QUI IN SECONDI, NON PRESE DALLA COSTANTE. Un
        // caso che calcolasse l'atteso sommando `SHOW_ANYWAY_AFTER_SECONDS` a
        // se stesso proverebbe soltanto che la diramazione scatta a qualunque
        // valore essa abbia: cambiarla in sei minuti o sei giorni non farebbe
        // rosso niente. La costante e' una decisione, e una decisione si
        // ancora a un numero.
        assert_eq!(SHOW_ANYWAY_AFTER_SECONDS, 21_600, "la soglia non e' piu' di sei ore");
        let alive_now = alive(&["aaaabbbb"]);
        let m = [marker("aaaabbbb-1111", "/albero", 1000)];
        assert!(
            decide_uncovered_with(&m, &alive_now, 1000 + 21_599, &silent).is_empty(),
            "un minuto prima delle sei ore il filo ce l'ha ancora lei"
        );
        assert_eq!(
            decide_uncovered_with(&m, &alive_now, 1000 + 21_600, &silent).len(),
            1,
            "dopo sei ore senza armare ne' riprendere, 'viva' non basta piu'"
        );
    }

    #[test]
    fn a_session_that_kept_working_on_it_still_holds_the_thread() {
        // IL FALSO POSITIVO SIMMETRICO, e il modo piu' comune di riprendere un
        // filo: non armare un successore ne' aprire un pannello, ma continuare
        // a lavorarci nello stesso. Misurato dalla dichiarazione, dopo sei ore
        // questa sessione comparirebbe fra i fili scoperti mentre ci sta
        // lavorando sopra in quel momento.
        let alive_now = alive(&["aaaabbbb"]);
        let m = [marker("aaaabbbb-1111", "/albero", 1000)];
        let now = 1000 + 21_600 * 3;
        let busy = |_: &str| Some(now - 60); // ha toccato il transcript un minuto fa
        assert!(
            decide_uncovered_with(&m, &alive_now, now, &busy).is_empty(),
            "chi ci sta lavorando adesso e' stato dato per sparito"
        );
        // E la controprova: la stessa sessione, ferma da sette ore.
        let quiet = |_: &str| Some(now - 21_600 - 3600);
        assert_eq!(decide_uncovered_with(&m, &alive_now, now, &quiet).len(), 1);
    }

    #[test]
    fn the_same_tree_written_two_ways_is_still_the_same_tree() {
        // Le due fonti del percorso sono diverse — `current_dir()` da una parte,
        // il campo del messaggio dall'altra — e una barra di troppo o un `..`
        // basterebbero a non togliere mai piu' quel marcatore.
        let _home = crate::test_home::HomeIsolata::nuova("filo-percorso");
        declare("aaaabbbb-1111", "/albero/uno/", "/consegna.md", "fuori-orario");
        assert!(marker_path("aaaabbbb-1111").exists());
        clear_tree("/albero/due/../uno");
        assert!(
            !marker_path("aaaabbbb-1111").exists(),
            "lo stesso albero scritto in un altro modo non ha chiuso il filo"
        );
    }

    #[test]
    fn the_opening_notice_says_nothing_when_there_is_nothing_to_say() {
        // Una riga in piu' all'avvio di OGNI sessione, quando non c'e' niente da
        // dire, e' il modo piu' rapido di far smettere di leggerla.
        let _home = crate::test_home::HomeIsolata::nuova("filo-avviso-vuoto");
        assert_eq!(opening_notice(), "");
    }

    #[test]
    fn the_opening_notice_names_the_oldest_thread() {
        let _home = crate::test_home::HomeIsolata::nuova("filo-avviso");
        declare("aaaabbbb-1111", "/albero/uno", "/consegna.md", "fuori-orario");
        let notice = opening_notice();
        assert!(notice.contains("/albero/uno"), "l'avviso non dice dove: {notice}");
        assert!(notice.contains("/consegna.md"), "l'avviso non dice cosa leggere: {notice}");
        assert!(
            notice.contains("Non e' un ordine"),
            "l'avviso deve restare un'offerta, non un incarico: {notice}"
        );
    }

    /// I due capi del filo sono cablati davvero, e non solo scritti qui.
    ///
    /// SI LEGGE IL SORGENTE, come già fa `register_session` per le famiglie di
    /// marcatori: le due chiamate vivono dentro funzioni che vogliono `stdin`,
    /// un pannello e un giro di `orca`, e un caso che le esercitasse per intero
    /// proverebbe soprattutto l'impalcatura. Quello che deve non poter sparire
    /// in silenzio è **la giunzione**: un modulo perfetto che nessuno chiama è
    /// il difetto di stamattina, e la batteria non se n'era accorta.
    #[test]
    fn both_ends_of_the_marker_are_wired_where_they_belong() {
        let arms = include_str!("successor.rs");
        assert!(
            arms.contains("uncovered_thread::declare("),
            "chi rinvia il successore non dichiara piu' il filo scoperto: nessuno sapra' che quel lavoro non ha nessuno"
        );
        assert!(
            arms.contains("uncovered_thread::clear("),
            "il successore che parte non toglie piu' il marcatore: l'elenco terrebbe righe gia' risolte"
        );
        let starts = include_str!("register_session.rs");
        assert!(
            starts.contains("uncovered_thread::clear_tree("),
            "l'avvio di una sessione non ripulisce piu' l'albero: l'elenco crescerebbe e basta, e smetterebbe di essere letto"
        );
        assert!(
            starts.contains("uncovered_thread::opening_notice("),
            "nessuno mostra piu' i fili scoperti a chi apre una sessione: resta un dato che si trova solo se lo si cerca, ed e' il difetto da cui il modulo nasce"
        );
        // E L'AVVISO NON CALPESTA UN MANDATO. Chi riceve una ripartenza
        // automatica legge un canale che per progetto non porta narrazione:
        // l'avviso deve stare sotto il ramo che esce quando il mandato c'e'.
        let after_mandate = starts
            .split("let msg = resume_message();")
            .nth(1)
            .expect("il punto di ripresa non si legge piu' da qui");
        let notice_at = after_mandate
            .find("uncovered_thread::opening_notice(")
            .expect("l'avviso non e' piu' dopo il mandato");
        let return_at = after_mandate.find("return 0;").unwrap_or(usize::MAX);
        assert!(
            return_at < notice_at,
            "l'avviso dei fili scoperti si stampa anche a chi ha gia' un mandato di ripartenza"
        );
    }

    #[test]
    fn a_thread_declared_then_taken_over_leaves_nothing_behind() {
        // Il ciclo intero sul disco: si dichiara, e chi arriva in quell'albero
        // lo chiude. È la prova che il marcatore è un freno e non un
        // interruttore — la domanda che il libro di bordo impone.
        let _home = crate::test_home::HomeIsolata::nuova("filo-scoperto");
        declare("aaaabbbb-1111", "/albero/uno", "/consegna.md", "albero-affollato");
        assert!(marker_path("aaaabbbb-1111").exists(), "la dichiarazione non ha scritto niente");

        // Un albero diverso non tocca questo marcatore.
        clear_tree("/albero/due");
        assert!(marker_path("aaaabbbb-1111").exists(), "un altro albero si e' preso il filo");

        clear_tree("/albero/uno");
        assert!(!marker_path("aaaabbbb-1111").exists(), "chi e' arrivato non ha chiuso il filo");
    }

    #[test]
    fn a_refusal_that_leaves_nobody_stranded_writes_nothing() {
        // `non-piena` vuol dire che chi ha consegnato sta ancora lavorando:
        // scrivere qui riempirebbe l'elenco di fili che hanno gia' qualcuno.
        let _home = crate::test_home::HomeIsolata::nuova("filo-non-scoperto");
        declare("ccccdddd-2222", "/albero", "/consegna.md", "non-piena");
        assert!(!marker_path("ccccdddd-2222").exists());
    }

    #[test]
    fn a_marker_without_its_timestamp_is_not_counted_as_ancient() {
        // Un marcatore scritto da una versione che non aveva il campo non deve
        // presentarsi in testa all'elenco con cinquant'anni di attesa: senza
        // data si assume adesso, cioe' il caso meno allarmante.
        let m = [serde_json::json!({"session_id": "aaaabbbb", "cwd": "/a"})];
        let out = decide_uncovered(&m, &alive(&[]), 1000);
        assert_eq!(out[0].seconds_uncovered, 0);
    }
}
