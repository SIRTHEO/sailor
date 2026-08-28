//! Il marcatore «un successore è già stato armato per questa sessione».
//!
//! Il presidio che apriva davvero un pannello successore è stato tolto il
//! 28/08/2026 — la staffetta ne aveva già abbandonato la funzione dal 19/08.
//! Quello che resta qui non decide più niente in servizio: `already_armed`
//! sopravvive perché `marker_sweep.rs` la usa nel proprio collaudo, per
//! scrivere un marcatore vero — non un contenuto fabbricato a mano — e
//! provare che chi legge e chi scrive concordano sul formato. Per questo
//! l'intero file è compilato solo in prova.

#![cfg(test)]

use crate::handoff::state_dir;
use std::fs;

/// Il marcatore «per questa sessione un successore c'è già», e se c'era.
///
/// Legge **e scrive** in una volta sola: due letture ravvicinate non devono
/// passare entrambe.
///
/// SCRIVE ANCHE LA SESSIONE, non solo il percorso — decisione del capitano,
/// 21/08/2026 15:55. Il nome del marcatore porta solo l'impronta, e
/// dall'impronta non si torna indietro: senza l'identificativo dentro, chi
/// raccoglie questi marcatori non ha altra via che ricalcolarla per ogni
/// sessione viva.
pub(crate) fn already_armed(path: &str, session: &str) -> bool {
    let marker = state_dir().join(format!(
        "successore-armato-{}",
        guards::successor::armed_fingerprint(path, session)
    ));
    if marker.exists() {
        return true;
    }
    let _ = fs::create_dir_all(state_dir());
    let _ = fs::write(&marker, format!("{path}\n{session}\n"));
    false
}

mod tests {
    use super::*;
    use crate::test_home::HomeIsolata;

    #[test]
    fn already_armed_arms_only_once() {
        let _home = HomeIsolata::nuova("successor-already-armed");
        assert!(!already_armed("/x/consegna.md", "sessione-armo"));
        // Idempotenza: lo stesso path e la stessa sessione non riarmano.
        assert!(already_armed("/x/consegna.md", "sessione-armo"));
        let marker = state_dir().join(format!(
            "successore-armato-{}",
            guards::successor::armed_fingerprint("/x/consegna.md", "sessione-armo")
        ));
        assert!(marker.exists());
    }

    #[test]
    fn already_armed_distinguishes_sessions() {
        let _home = HomeIsolata::nuova("successor-already-armed-sessions");
        assert!(!already_armed("/x/consegna.md", "sessione-a"));
        assert!(!already_armed("/x/consegna.md", "sessione-b"));
    }
}
