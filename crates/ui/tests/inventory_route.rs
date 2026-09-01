//! La forma con cui il censimento della macchina arriva a chi guarda.
//!
//! **ERA UNA PROVA SU UN SOCKET, ADESSO E' UNA PROVA SULLA FORMA.** Fino al
//! 31/08/2026 questa apriva `127.0.0.1` e chiedeva `/api/inventory`. Il
//! servitore non c'e' piu' — la finestra chiama `machine_inventory` — ma cio'
//! che la prova difendeva non era il trasporto: era che ogni voce dichiari un
//! genere noto, un nome, un'origine, e che chi non e' raggiungibile porti il
//! motivo scritto. Senza quel motivo, «spenta» e' una parola che non si puo'
//! correggere.
//!
//! **LE RADICI NON SONO PIÙ QUELLE DI CHI ESEGUE, ED È IL GUASTO 5.** Fino al
//! 01/09/2026 chiamava `inventory::default_roots()`, cioè `$HOME`, e pretendeva
//! che l'elenco non fosse vuoto — «ci si aspetta almeno una voce sulla macchina
//! di prova». Il commento in testa diceva che l'inventario non ha un punto di
//! iniezione: **ce l'ha**, ed è il parametro di `collect`. Misurato il
//! 01/09/2026 rieseguendo l'intera batteria con una casa vuota: questa prova
//! diventava rossa a codice invariato, che è parola per parola la riga del
//! guasto 5. La cura scritta accanto al guasto è «ciò che serve si versiona»:
//! le radici adesso sono un albero che questa prova costruisce da sé.
//!
//! **COSA SI PERDE, DICHIARATO.** Non prova più che su questa macchina esistano
//! davvero competenze e ganci: quello è un fatto del mondo, e non è quello che
//! la finestra deve poter contare su di lei. Prova che *dato* un albero noto la
//! forma esce giusta — e con l'albero noto può pretendere ciò che prima non
//! poteva, cioè che tutti e tre gli stati di raggiungibilità compaiano davvero.
//! Prima, «attivo» dappertutto avrebbe lasciato la riga sul motivo non
//! eseguita, e nessuno l'avrebbe saputo.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

/// Un contatore, non il solo orologio: viene dal guasto 21. `cargo test` manda
/// le prove sullo stesso processo e l'orologio di macOS non ha la risoluzione
/// del nanosecondo, quindi due cartelle nate nello stesso istante si rubavano il
/// posto a vicenda.
static NEXT_SCRATCH: AtomicU32 = AtomicU32::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let serial = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "ui-inventory-{}-{serial}-{label}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().expect("ogni file ha una cartella"))
        .expect("creare la cartella del file di prova");
    std::fs::write(path, text).expect("scrivere il file di prova");
}

/// Una casa e un repo costruiti apposta: due comandi, una regola e due ganci,
/// scelti perché coprono tutti e tre gli stati che la finestra sa disegnare.
///
/// Il gancio rotto punta a un file che non esiste, ed è l'unico modo di far
/// nascere uno stato «spento» con il proprio motivo: sulla casa la
/// raggiungibilità è «attiva» per costruzione, su un repo è «ignota».
fn fixture_roots(label: &str) -> Vec<inventory::Root> {
    let base = scratch_dir(label);
    let home = base.join("casa");
    let repo = base.join("repo-di-lavoro");

    write(
        &home.join(".claude/commands/saluta.md"),
        "# Saluta\n\nUn comando di prova.\n",
    );
    write(
        &home.join(".claude/rules/una-regola.md"),
        "# Una regola\n\nIl testo della regola.\n",
    );
    write(
        &home.join(".claude/settings.json"),
        r#"{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          { "type": "command", "command": "/non/esiste/gancio-morto.sh --controlla" }
        ]
      }
    ]
  }
}
"#,
    );
    write(
        &repo.join(".claude/commands/lavora.md"),
        "# Lavora\n\nUn comando che vale solo dentro questo repo.\n",
    );

    vec![inventory::Root::home(&home), inventory::Root::repo(&repo)]
}

/// **RICEVE LE RADICI, E NON LE CERCA.** L'altro ramo aveva scritto qui
/// `collect_survey(&default_roots(ledger::sailor_home()))`: è la forma giusta
/// per chi guarda la macchina vera — la riga di comando e la finestra — ma non
/// per una prova, che tornerebbe a dipendere da `$HOME`. `collect` esiste
/// apposta accanto a `collect_survey`, e il suo commento lo dice: chi
/// costruisce le radici a mano non ha nessun rendiconto da passare.
fn census(roots: &[inventory::Root]) -> serde_json::Value {
    let found = inventory::collect(roots);
    serde_json::to_value(&found).expect("il censimento si serializza")
}

#[test]
fn the_census_answers_with_the_shape_the_window_reads() {
    let roots = fixture_roots("forma");
    let body = census(&roots);

    let entries = body["entries"].as_array().expect("array di voci");
    let declared = body["roots"].as_array().expect("array di radici");
    assert_eq!(declared.len(), roots.len(), "ogni radice si dichiara");
    let stale = body["stale_plugin_copies"]
        .as_u64()
        .expect("numero di copie in cache");

    // L'albero di prova porta quattro voci: se l'elenco fosse vuoto la risposta
    // avrebbe la forma giusta e un contenuto sbagliato, e questo controllo lo
    // prenderebbe. È la stessa guardia di prima, ma su un numero che sappiamo.
    assert_eq!(
        entries.len(),
        4,
        "l'albero di prova porta due comandi, una regola e un gancio: {entries:?}"
    );

    let known_kinds = ["skill", "agent", "command", "rule", "hook"];
    for entry in entries {
        let kind = entry["kind"].as_str().expect("ogni voce dichiara un genere");
        assert!(known_kinds.contains(&kind), "genere inatteso: {kind}");
        assert!(entry["name"].as_str().is_some(), "ogni voce ha un nome");
        assert!(
            entry["origin"].as_str().is_some(),
            "ogni voce dichiara l'origine"
        );
        let state = entry["reach"]["state"]
            .as_str()
            .expect("ogni voce dichiara reach.state");
        assert!(
            ["active", "inactive", "unknown"].contains(&state),
            "stato di raggiungibilità inatteso: {state}"
        );
        if state != "active" {
            assert!(
                entry["reach"]["reason"].as_str().is_some(),
                "chi non è attivo porta il motivo scritto"
            );
        }
    }

    // Nessuna cache di plugin in un albero costruito adesso: prima questo era
    // un «non è un numero assurdo», che è tutto quello che si poteva chiedere a
    // una macchina sconosciuta.
    assert_eq!(stale, 0, "l'albero di prova non ha copie in cache");
}

/// **TUTTI E TRE GLI STATI COMPAIONO, E QUESTO PRIMA NON SI POTEVA PRETENDERE.**
/// Il ciclo qui sopra controlla «se non è attivo allora c'è il motivo»: su un
/// elenco tutto attivo resterebbe verde senza aver mai eseguito quella riga —
/// due copie che sbagliano insieme, nella forma in cui una prova si conferma da
/// sola. Con l'albero versionato si può chiedere che gli stati ci siano.
#[test]
fn every_reachability_state_carries_what_the_window_needs() {
    let roots = fixture_roots("stati");
    let body = census(&roots);
    let entries = body["entries"].as_array().expect("array di voci");

    let state_of = |wanted: &str| -> Vec<&serde_json::Value> {
        entries
            .iter()
            .filter(|entry| entry["reach"]["state"].as_str() == Some(wanted))
            .collect()
    };

    let active = state_of("active");
    assert!(
        !active.is_empty(),
        "la casa è raggiungibile: qualcosa deve essere attivo"
    );

    let inactive = state_of("inactive");
    assert_eq!(
        inactive.len(),
        1,
        "un solo gancio punta a un file che non c'è: {inactive:?}"
    );
    let reason = inactive[0]["reach"]["reason"]
        .as_str()
        .expect("uno spento porta il motivo");
    assert!(
        reason.contains("gancio-morto.sh"),
        "il motivo nomina il file che manca: {reason}"
    );

    let unknown = state_of("unknown");
    assert!(
        !unknown.is_empty(),
        "ciò che vive in un repo vale solo lì, e la finestra deve dirlo"
    );
    assert!(
        unknown[0]["reach"]["reason"].as_str().is_some(),
        "anche «ignoto» porta il proprio motivo"
    );
}
