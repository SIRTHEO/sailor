//! La parte con disco e registro del freno che nega l'uscita a chi lascia un
//! filo scoperto. Il giudizio puro sta in `guards::uncovered_exit`.
//!
//! Si inserisce nel ramo `Decision::Settle` di `handoff_on_stop::run`, subito
//! dopo il tentativo di armare un successore: è nello Stop che QUESTA
//! sessione può scoprire, mentre sta per fermarsi, che il suo filo è rimasto
//! senza nessuno. Il marcatore che legge (`uncovered_thread::read_own`) non
//! nasce solo qui — lo scrive anche `successor::run` sul `PostToolUse` che
//! scatta alla scrittura della consegna, probabilmente la strada più comune —
//! ma non serve distinguere chi l'ha scritto: si rilegge dal disco per
//! session-id, e questo è l'unico istante in cui la sessione può ancora
//! scegliere di non lasciarlo scoperto.
//!
//! IL TETTO VALE PER FILO, NON PER SESSIONE. Trovato in revisione il
//! 25/08/2026: una sessione lunga può lasciare scoperto un filo A, farsi
//! bloccare una volta ed esaurire il tetto, e ore dopo lasciare scoperto un
//! filo B completamente diverso — che a un contatore per sola sessione non
//! direbbe più niente, proprio il silenzio per cui questo freno esiste. Il
//! contatore porta quindi l'IMPRONTA del filo (`thread_fingerprint`) accanto
//! al numero, e torna a zero da sola quando l'impronta cambia — stesso schema
//! di `handoff_on_stop::lockout_reference`, che registra un riferimento e lo
//! rifà da capo quando il marcatore sotto cambia. NESSUNO CANCELLA IL FILE
//! VECCHIO: come il suo gemello, diventa inerte da solo quando l'impronta non
//! combacia più — la domanda «chi lo butta» ha qui la stessa risposta.
//!
//! L'IMPRONTA È `declared_at_epoch`, NON IL PERCORSO — 2º giro di revisione,
//! 25/08/2026 sera. Il percorso non regge: la skill `/handoff` **aggiorna**
//! la consegna esistente invece di crearne una nuova quando la riconosce come
//! propria (`commands/handoff.md`, punto 4), quindi due fili della stessa
//! sessione finiscono spesso sullo stesso file — il caso concreto era una
//! sessione che, dopo essersi vista negare per il filo A, riprendeva un filo B
//! del tutto diverso sullo stesso percorso, e trovava il tetto già esaurito.
//! `uncovered_thread::declare` ora tiene fermo `declared_at_epoch` finché
//! consegna e motivo restano gli stessi (vedi `stable_stamp` in quel modulo):
//! l'istante torna a essere l'identità giusta, perché cambia solo quando il
//! filo cambia davvero — non il percorso su cui capita di atterrare.

use crate::handoff::state_dir;
use guards::uncovered_exit::{decide, Decision, Facts};
use hook_io::journal::{self, Field};
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn counter_path(short_session: &str) -> PathBuf {
    state_dir().join(format!("filo-uscita-blocchi-{short_session}"))
}

/// L'identità del filo dentro il marcatore, per distinguere «lo stesso filo
/// ritentato» da «un filo nuovo preso in carico dalla stessa sessione».
///
/// `declared_at_epoch`, non il percorso della consegna: scartato due volte
/// prima di arrivare qui. La prima (istante grezzo) perché `declare`
/// riscriveva l'istante a ogni tentativo, anche sullo stesso filo. La
/// seconda (percorso) perché la skill `/handoff` aggiorna in place la
/// consegna esistente — due fili della stessa sessione finiscono spesso sullo
/// stesso file. Ora `declare` tiene fermo l'istante finché consegna e motivo
/// non cambiano (vedi `uncovered_thread::stable_stamp`), quindi l'istante è
/// tornato a essere l'identità giusta.
fn thread_fingerprint(marker: &Value) -> String {
    marker.get("declared_at_epoch").map(|v| v.to_string()).unwrap_or_default()
}

/// Quante volte questo freno ha già negato per QUESTO filo di questa
/// sessione. Se l'impronta salvata non combacia più con quella corrente — un
/// filo diverso ha preso il posto di quello negato l'ultima volta — il
/// conteggio riparte da zero.
fn blocks_so_far(short_session: &str, fingerprint: &str) -> u32 {
    let Ok(text) = fs::read_to_string(counter_path(short_session)) else {
        return 0;
    };
    let mut righe = text.splitn(2, '\n');
    let impronta_salvata = righe.next().unwrap_or("");
    let n_salvato = righe.next().and_then(|n| n.trim().parse::<u32>().ok()).unwrap_or(0);
    if impronta_salvata == fingerprint {
        n_salvato
    } else {
        0
    }
}

fn record_block(short_session: &str, fingerprint: &str) {
    let n = blocks_so_far(short_session, fingerprint);
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(counter_path(short_session), format!("{fingerprint}\n{}", n + 1));
}

/// Nega lo Stop se questa sessione sta per sparire lasciando un filo suo
/// senza nessuno. Se nega, stampa già il messaggio su stderr: il chiamante
/// deve solo uscire con 2 invece di proseguire verso il congedo.
///
/// `session_intero` è l'identificativo completo, lo stesso passato ad
/// `arm_successor` — `uncovered_thread` applica da sé il troncamento a otto
/// caratteri con cui `declare` ha scritto il marcatore.
pub fn deny_if_own_thread_uncovered(session_intero: &str, stop_hook_active: bool) -> bool {
    if session_intero.is_empty() {
        return false;
    }
    let short: String = session_intero.chars().take(8).collect();
    let marker = crate::uncovered_thread::read_own(session_intero);
    let reason = marker
        .as_ref()
        .and_then(|m| m.get("reason"))
        .and_then(|v| v.as_str())
        .unwrap_or("ignoto")
        .to_string();
    // L'impronta serve solo quando c'è un marcatore da leggere: senza,
    // `decide` risponde `Pass` a prescindere e il contatore non si consulta.
    let fingerprint = marker.as_ref().map(thread_fingerprint).unwrap_or_default();
    let facts = Facts {
        own_thread_uncovered: marker.is_some(),
        stop_hook_active,
        blocks_so_far: blocks_so_far(&short, &fingerprint),
        reason,
    };
    match decide(&facts) {
        Decision::Pass => false,
        Decision::Surrender => {
            journal::record(
                "filo-uscita",
                "passa",
                "arreso",
                &[("session", Field::Text(short))],
            );
            false
        }
        Decision::Block(messaggio) => {
            record_block(&short, &fingerprint);
            journal::record(
                "filo-uscita",
                "blocca",
                "filo-scoperto",
                &[("session", Field::Text(short)), ("motivo", Field::Text(facts.reason))],
            );
            eprint!("{messaggio}");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    #[test]
    fn no_marker_means_no_denial() {
        let _home = HomeIsolata::nuova("filo-uscita-senza-marcatore");
        assert!(!deny_if_own_thread_uncovered("aaaabbbb-1111", false));
    }

    #[test]
    fn a_declared_thread_denies_once_then_surrenders() {
        let _home = HomeIsolata::nuova("filo-uscita-nega-una-volta");
        crate::uncovered_thread::declare(
            "aaaabbbb-1111",
            "/albero/uno",
            "/consegna.md",
            "fuori-orario",
        );
        assert!(
            deny_if_own_thread_uncovered("aaaabbbb-1111", false),
            "la prima volta nega"
        );
        assert!(
            !deny_if_own_thread_uncovered("aaaabbbb-1111", false),
            "la seconda volta, tetto raggiunto, lascia andare"
        );
    }

    #[test]
    fn a_new_thread_gets_a_fresh_chance_after_a_different_one_was_capped() {
        // IL DIFETTO GRAVE DELLA PRIMA STESURA: il tetto valeva per sessione,
        // non per filo — dopo la prima negazione una sessione lunga che
        // lasciava scoperto un secondo filo, diverso dal primo, non riceveva
        // più niente. Qui i due fili hanno la stessa sessione ma consegne
        // diverse: la seconda deve negare comunque.
        //
        // L'IMPRONTA È `declared_at_epoch`, IN SECONDI: una pausa vera fra i
        // due fili non è un vezzo, è ciò che garantisce che i due istanti non
        // coincidano per caso — due dichiarazioni a microsecondi di distanza
        // atterrerebbero quasi sempre sullo stesso secondo, e il caso
        // proverebbe poco.
        let _home = HomeIsolata::nuova("filo-uscita-filo-nuovo");
        crate::uncovered_thread::declare(
            "aaaabbbb-1111",
            "/albero/uno",
            "/consegna-A.md",
            "fuori-orario",
        );
        assert!(deny_if_own_thread_uncovered("aaaabbbb-1111", false), "il filo A nega");
        assert!(
            !deny_if_own_thread_uncovered("aaaabbbb-1111", false),
            "il filo A, ritentato, ha gia' esaurito il suo tetto"
        );
        // Lo stesso filo A si chiude (il successore si arma altrove), e la
        // sessione ne prende in carico un altro: consegna diversa.
        crate::uncovered_thread::clear("aaaabbbb-1111");
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        crate::uncovered_thread::declare(
            "aaaabbbb-1111",
            "/albero/uno",
            "/consegna-B.md",
            "albero-affollato",
        );
        assert!(
            deny_if_own_thread_uncovered("aaaabbbb-1111", false),
            "il filo B e' un lavoro diverso: merita la sua prima forzatura"
        );
    }

    #[test]
    fn a_new_thread_on_the_same_handoff_path_still_gets_a_fresh_chance() {
        // LO SCENARIO ESATTO DEL 2º GIRO: la skill `/handoff` aggiorna in
        // place la consegna esistente (`commands/handoff.md`, punto 4), quindi
        // il filo B atterra sullo STESSO percorso del filo A gia' negato — a
        // differenza del caso sopra, che usa due percorsi gia' diversi in
        // partenza e per questo non basta a provare la correzione: con
        // un'impronta sul percorso i due test morirebbero comunque, con
        // un'impronta stabile ma per-percorso no. Solo `declared_at_epoch`
        // (che cambia perche' il motivo cambia, vedi `stable_stamp`) distingue
        // i due fili qui.
        let _home = HomeIsolata::nuova("filo-uscita-stesso-percorso");
        crate::uncovered_thread::declare(
            "aaaabbbb-1111",
            "/albero/uno",
            "/consegna.md",
            "fuori-orario",
        );
        assert!(deny_if_own_thread_uncovered("aaaabbbb-1111", false), "il filo A nega");
        assert!(
            !deny_if_own_thread_uncovered("aaaabbbb-1111", false),
            "il filo A, ritentato, ha gia' esaurito il suo tetto"
        );
        crate::uncovered_thread::clear("aaaabbbb-1111");
        std::thread::sleep(std::time::Duration::from_millis(1_100));
        crate::uncovered_thread::declare(
            "aaaabbbb-1111",
            "/albero/uno",
            "/consegna.md", // STESSO percorso del filo A: la skill riscrive in place.
            "troppe-sessioni",
        );
        assert!(
            deny_if_own_thread_uncovered("aaaabbbb-1111", false),
            "il filo B ha un motivo diverso sullo stesso percorso: merita la sua prima forzatura"
        );
    }

    #[test]
    fn an_induced_chain_never_gets_denied() {
        let _home = HomeIsolata::nuova("filo-uscita-catena-indotta");
        crate::uncovered_thread::declare(
            "aaaabbbb-1111",
            "/albero/uno",
            "/consegna.md",
            "fuori-orario",
        );
        assert!(!deny_if_own_thread_uncovered("aaaabbbb-1111", true));
    }

    #[test]
    fn an_empty_session_is_never_denied() {
        let _home = HomeIsolata::nuova("filo-uscita-sessione-vuota");
        assert!(!deny_if_own_thread_uncovered("", false));
    }

    /// Il cablaggio, non solo il modulo: un freno perfetto che nessuno chiama
    /// è il difetto di oggi (`uncovered_thread::both_ends_of_the_marker_are_
    /// wired_where_they_belong`) — qui si verifica che `handoff_on_stop`
    /// interpelli davvero questo freno, PRIMA del congedo, dentro il ramo
    /// `Settle`.
    #[test]
    fn the_stop_hook_wires_this_brake_before_the_farewell() {
        let source = include_str!("handoff_on_stop.rs");
        assert!(
            source.contains("uncovered_exit::deny_if_own_thread_uncovered("),
            "il gancio Stop non interpella piu' questo freno: un filo scoperto \
             non fermerebbe piu' nessuno"
        );
        let settle = source
            .split("Decision::Settle => {")
            .nth(1)
            .expect("il ramo Settle non si legge piu' da qui");
        let deny_at = settle
            .find("uncovered_exit::deny_if_own_thread_uncovered(")
            .expect("il freno non e' piu' dentro il ramo Settle");
        let farewell_at = settle.find("farewell(").unwrap_or(usize::MAX);
        assert!(
            deny_at < farewell_at,
            "il freno deve venire prima del congedo: chiudere la propria \
             scheda renderebbe la scelta impossibile"
        );
    }
}
