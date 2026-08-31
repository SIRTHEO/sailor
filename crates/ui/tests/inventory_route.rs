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
//! Le radici sono quelle vere di questa macchina (`$HOME`), non un deposito
//! finto: l'inventario non ha un punto di iniezione, e verificarne la forma
//! non richiede contarne il contenuto esatto.

fn census() -> serde_json::Value {
    let found = inventory::collect(&inventory::default_roots());
    serde_json::to_value(&found).expect("il censimento si serializza")
}

#[test]
fn the_census_answers_with_the_shape_the_window_reads() {
    let body = census();

    let entries = body["entries"].as_array().expect("array di voci");
    let roots = body["roots"].as_array().expect("array di radici");
    assert!(!roots.is_empty(), "la casa è sempre una radice");
    let stale = body["stale_plugin_copies"].as_u64().expect("numero di copie in cache");

    // Sulla macchina di prova esistono davvero competenze e ganci: se
    // l'elenco fosse vuoto la rotta risponderebbe con la forma giusta ma un
    // contenuto sbagliato, e questo controllo lo prenderebbe.
    assert!(!entries.is_empty(), "ci si aspetta almeno una voce sulla macchina di prova");

    let known_kinds = ["skill", "agent", "command", "rule", "hook"];
    for entry in entries {
        let kind = entry["kind"].as_str().expect("ogni voce dichiara un genere");
        assert!(known_kinds.contains(&kind), "genere inatteso: {kind}");
        assert!(entry["name"].as_str().is_some(), "ogni voce ha un nome");
        assert!(entry["origin"].as_str().is_some(), "ogni voce dichiara l'origine");
        let state = entry["reach"]["state"].as_str().expect("ogni voce dichiara reach.state");
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

    // stale_plugin_copies è un conteggio, non parte dell'elenco: non deve
    // superare in modo assurdo il numero di voci trovate — è solo una difesa
    // contro un errore grossolano di lettura, non un valore atteso preciso.
    assert!(stale < 1_000_000, "numero di copie in cache implausibile: {stale}");
}
