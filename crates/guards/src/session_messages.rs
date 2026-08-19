//! Chi puo' ricevere un messaggio senza chiedere il permesso a Theo.
//!
//! E' la parte del gancio che non tocca il mondo: dato un destinatario e i nomi
//! delle copie di lavoro che Orca conosce, dice se e' una sessione **locale
//! viva**. Il resto — la chiamata a Orca, il registro, la risposta — sta in
//! `claude-hooks/src/session_messages.rs`.
//!
//! L'ELENCO E' DI CIO' CHE PASSA, non di cio' che si vieta. `SendMessage`
//! raggiunge anche sessioni nel cloud e su altre macchine: quelle escono da
//! questo computer e restano una decisione di Theo. Ogni destinatario che non si
//! riesce a riconoscere lascia la domanda dov'era.

use std::collections::BTreeSet;

/// I destinatari interni al processo: non escono di qui, e non passano per la
/// rete. `main` e' la conversazione principale vista da un subagent.
pub const IN_PROCESS: &[&str] = &["main"];

/// Il verdetto, con il motivo che finisce nel registro e nel messaggio.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub allow: bool,
    pub reason: String,
}

fn no(reason: &str) -> Verdict {
    Verdict { allow: false, reason: reason.into() }
}

fn yes(reason: String) -> Verdict {
    Verdict { allow: true, reason }
}

/// Il suffisso che distingue due sessioni sullo stesso albero: `tautog-3a`.
///
/// Corto e alfanumerico. La regex dell'originale e' `^-[0-9a-z]{1,8}$`, e qui e'
/// scritta a mano perche' una dipendenza da `regex` per otto caratteri non la
/// merita nessuno.
fn is_session_suffix(rest: &str) -> bool {
    let Some(tail) = rest.strip_prefix('-') else {
        return false;
    };
    !tail.is_empty()
        && tail.len() <= 8
        && tail.chars().all(|c| c.is_ascii_digit() || c.is_ascii_lowercase())
}

/// Lo spazio dei socket delle sessioni locali. Fuori di qui non si concede.
const SOCKET_DIR: &str = "/tmp/cc-socks/";

/// `uds:/tmp/cc-socks/24334.sock` — l'indirizzo che `ListAgents` stampa per una
/// sessione locale, e la forma che il confronto sui nomi delle copie non vede.
///
/// Misurato il 19/08/2026: dei 256 messaggi fra sessioni di sette giorni, 143
/// erano nomi di copia e 64 indirizzi come questo. Il gancio taceva su tutti e
/// 64, e tacere lascia aperto il dialogo di permesso.
///
/// LA FORMA E' ESATTA, non un prefisso: una sottocartella o un `..` porterebbero
/// fuori dallo spazio locale, e un percorso arbitrario concesso e' chiunque che
/// parla con chiunque. Il binario di Claude Code usa la stessa cautela e la
/// chiama «outside our socket namespace».
fn is_local_socket(recipient: &str) -> bool {
    let Some(path) = recipient.strip_prefix("uds:") else {
        return false;
    };
    let Some(file) = path.strip_prefix(SOCKET_DIR) else {
        return false;
    };
    let Some(stem) = file.strip_suffix(".sock") else {
        return false;
    };
    !stem.is_empty() && stem.chars().all(|c| c.is_ascii_digit())
}

/// Il destinatario e' una sessione viva su questa macchina?
///
/// `names` a `None` vuol dire «Orca non ha risposto», che **non** e' un insieme
/// vuoto: se non si sa niente non si concede al buio. Il verso sarebbe comunque
/// prudente con l'insieme vuoto, ma dirlo con un valore diverso lo rende
/// leggibile a chi passa dopo.
///
/// IL CONFRONTO E' ESATTO SUL NOME DELLA COPIA. Un prefisso qualunque non basta:
/// altrimenti `tautog` aprirebbe la strada a `tautog-in-the-cloud`, che locale
/// non e'. E' il mutante che la batteria dell'originale tiene scritto.
pub fn is_local(recipient: &str, names: Option<&BTreeSet<String>>) -> Verdict {
    if recipient.is_empty() {
        return no("destinatario assente");
    }
    if IN_PROCESS.contains(&recipient) {
        return yes("destinatario interno al processo".into());
    }
    // Prima del confronto sui nomi: un socket locale si riconosce dalla propria
    // forma, senza chiedere niente a Orca. Vale anche quando Orca non risponde.
    if is_local_socket(recipient) {
        return yes("socket di una sessione locale".into());
    }
    let Some(names) = names else {
        return no("Orca non risponde: non si concede al buio");
    };
    // Il ` [ref]` che disambigua due omonimi non fa parte del nome.
    let name = recipient.split(" [").next().unwrap_or("").trim();
    if names.contains(name) {
        return yes(format!("sessione locale sulla copia `{name}`"));
    }
    for copy in names {
        if let Some(rest) = name.strip_prefix(copy.as_str()) {
            if is_session_suffix(rest) {
                return yes(format!("sessione locale sulla copia `{copy}`"));
            }
        }
    }
    no("destinatario non riconosciuto fra le copie locali")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> BTreeSet<String> {
        ["tautog", "suite-229-tabella-residui", "general"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn a_live_local_session_is_allowed() {
        assert!(is_local("tautog", Some(&names())).allow);
        assert!(is_local("suite-229-tabella-residui", Some(&names())).allow);
    }

    #[test]
    fn the_suffix_that_tells_two_apart_still_counts() {
        assert!(is_local("tautog-3a", Some(&names())).allow);
        assert!(is_local("suite-229-tabella-residui-de", Some(&names())).allow);
    }

    #[test]
    fn a_disambiguating_ref_is_not_part_of_the_name() {
        assert!(is_local("tautog-3a [c23e5e]", Some(&names())).allow);
    }

    #[test]
    fn a_name_that_merely_starts_the_same_is_not_local() {
        // I due mutanti che l'originale tiene scritti: col confronto per
        // prefisso semplice passerebbero entrambi.
        assert!(!is_local("tautog-in-the-cloud", Some(&names())).allow);
        assert!(!is_local("generaledelesercito", Some(&names())).allow);
    }

    #[test]
    fn silence_from_orca_is_not_an_empty_list() {
        let v = is_local("tautog", None);
        assert!(!v.allow);
        assert!(v.reason.contains("al buio"));
    }

    #[test]
    fn an_unknown_recipient_leaves_the_question_where_it_was() {
        assert!(!is_local("qualcun-altro", Some(&names())).allow);
        assert!(!is_local("", Some(&names())).allow);
    }

    #[test]
    fn in_process_recipients_need_no_list() {
        assert!(is_local("main", None).allow);
    }

    #[test]
    fn a_socket_in_the_local_namespace_is_granted_without_asking_orca() {
        let v = is_local("uds:/tmp/cc-socks/24334.sock", None);
        assert!(v.allow, "un socket locale si riconosce anche senza i nomi");
        assert!(v.reason.contains("socket"));
    }

    #[test]
    fn a_socket_outside_the_namespace_is_never_granted() {
        // Un percorso arbitrario concesso e' chiunque che parla con chiunque.
        assert!(!is_local_socket("uds:/tmp/altrove/24334.sock"));
        assert!(!is_local_socket("uds:/tmp/cc-socks/../altrove/24334.sock"));
        assert!(!is_local_socket("uds:/tmp/cc-socks/sotto/24334.sock"));
        assert!(!is_local_socket("uds:/tmp/cc-socks/24334.txt"));
        assert!(!is_local_socket("uds:/tmp/cc-socks/nome.sock")); // non numerico
        assert!(!is_local_socket("uds:/tmp/cc-socks/.sock")); // vuoto
        assert!(!is_local_socket("/tmp/cc-socks/24334.sock")); // senza schema
        assert!(!is_local_socket("uds:"));
    }

    #[test]
    fn the_suffix_is_short_and_alphanumeric() {
        assert!(is_session_suffix("-3a"));
        assert!(is_session_suffix("-12345678"));
        assert!(!is_session_suffix("-123456789")); // nove: troppo lungo
        assert!(!is_session_suffix("-"));
        assert!(!is_session_suffix("-A3")); // maiuscola
        assert!(!is_session_suffix("-in-the-cloud"));
        assert!(!is_session_suffix("3a")); // senza trattino
    }
}
