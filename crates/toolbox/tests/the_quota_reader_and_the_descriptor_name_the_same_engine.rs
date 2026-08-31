//! Chi legge la quota e chi la dichiara devono nominare **lo stesso motore**.
//!
//! **PERCHÉ QUESTA PROVA ESISTE.** Leggere la quota funziona su un motore solo,
//! e il vincolo permanente «indipendenza dal modello» dice cosa farne: la si
//! dichiara come **capacità di quello strumento**, e chi non ce l'ha continua a
//! funzionare pagando di più. Una dichiarazione che nessuno interroga però è
//! decorazione: il descrittore direbbe `claude-code` e il lettore leggerebbe
//! quello che vuole, e le due cose divergerebbero senza che niente diventi
//! rosso. È il guasto 10 — la stessa lista scritta in due punti — applicato a un
//! nome solo.
//!
//! Qui il nome che `models::remaining` scrive dentro ogni lettura deve essere
//! l'`id` di un descrittore **spedito** che dichiara `read_remaining_quota`
//! disponibile. Se qualcuno rinomina l'uno senza l'altro, questa cade.

use toolbox::descriptor::{CapabilityState, Catalog, Source};

/// Il nome della capacità, come i descrittori la scrivono.
const READ_REMAINING_QUOTA: &str = "read_remaining_quota";

fn shipped() -> Catalog {
    Catalog::load(&[Source::Builtin])
}

#[test]
fn the_engine_the_reader_writes_is_a_shipped_descriptor_that_declares_the_capability() {
    let catalog = shipped();
    let found = catalog
        .descriptors
        .iter()
        .find(|loaded| loaded.descriptor.id == models::remaining::CLAUDE_CODE)
        .unwrap_or_else(|| {
            panic!(
                "«{}» non è l'id di nessun descrittore spedito: il lettore firmerebbe \
                 le proprie letture con un motore che non esiste",
                models::remaining::CLAUDE_CODE
            )
        });

    assert_eq!(
        found.descriptor.capability(READ_REMAINING_QUOTA),
        CapabilityState::Available,
        "il motore che il lettore interroga deve dichiarare di saperlo dire"
    );
}

/// **CHI NON CE L'HA NON DEVE TACERE.** Un motore che non sa dire la propria
/// quota e non lo dichiara è indistinguibile da uno che nessuno ha mai
/// guardato — sono i tre stati del blocco `capabilities`, e servono tutti e
/// tre. Codex è stato guardato il 01/09/2026 e non ci si è arrivati: sta
/// scritto `false`, e il perché sta nella sua nota.
#[test]
fn an_engine_that_was_looked_at_and_cannot_says_so_instead_of_staying_silent() {
    let catalog = shipped();
    let codex = catalog
        .descriptors
        .iter()
        .find(|loaded| loaded.descriptor.id == "codex")
        .expect("codex è spedito");

    assert_eq!(
        codex.descriptor.capability(READ_REMAINING_QUOTA),
        CapabilityState::Absent,
        "«provato e non riuscito» non è «nessuno ha guardato»: si scrive `false`"
    );
    assert!(
        codex.descriptor.note.contains("account/rateLimits/read"),
        "chi riprova deve trovare scritto fin dove si era arrivati, o ricomincia da zero"
    );
}
