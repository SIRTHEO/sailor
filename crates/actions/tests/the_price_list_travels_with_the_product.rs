//! **IL GUASTO 35, PROVATO DOVE VIVEVA.**
//!
//! `load_pricing` leggeva `~/.config/sailor/pricing.json` e nient'altro. Su una
//! macchina appena installata quel file non c'è: ogni `cost_micros` restava
//! `None`, la spesa registrata di ogni corsa restava zero, e un flusso con
//! `spend_cap_micros` girava fino in fondo senza che il tetto scattasse mai —
//! senza nessun errore e senza nessun avviso. Un freno che non frena.
//!
//! Qui la scena è quella: **niente in casa**, e il costo si deve sapere lo
//! stesso. I conteggi non sono inventati — sono quelli che
//! `claude -p --output-format json` ha dichiarato davvero su questa macchina il
//! 30/08/2026. Che il descrittore spedito li sappia *leggere* da quell'uscita lo
//! prova `toolbox/tests/the_shipped_descriptor_prices_a_real_call.rs`, che dal
//! 01/09/2026 fa il conto sul listino spedito invece che su uno scritto a mano:
//! là la catena è intera, qui c'è il solo anello che questo guasto riguarda.
//!
//! **PERCHÉ NON SI PROVA PUNTANDO `SAILOR_PRICING` DA QUALCHE PARTE.** Una
//! variabile d'ambiente è di **processo**: `cargo test` manda le prove di uno
//! stesso binario su più fili dello stesso processo, e una prova che la scrive
//! deciderebbe il listino di tutte le altre mentre girano. Per questo la regola
//! sta in `price_list_from`, che prende il testo di casa come argomento.

use models::pricing::{cost_micros, Known, Price, TokenCounts};

/// I conteggi veri di quella chiamata: 2 token d'ingresso, 4 d'uscita, 9.922
/// letti dalla cache e 12.347 scritti in una cache a lunga durata. Il motore
/// dichiarò 0,128541 dollari.
const MEASURED: TokenCounts = TokenCounts {
    input: Some(2),
    output: Some(4),
    cached: Some(9_922),
    cache_write: None,
    cache_write_long: Some(12_347),
};

/// Il nome con cui quel motore si è dichiarato. Non è l'`id` di listino: è un
/// alias, e se il listino spedito lo perdesse il costo tornerebbe sconosciuto
/// senza che nessun prezzo sia cambiato.
const AS_THE_ENGINE_NAMED_IT: &str = "claude-opus-5[1m]";

/// **LA PROVA CHE CHIUDE IL GUASTO 35.** Nessun file in casa, e il costo di una
/// chiamata vera si sa lo stesso — alla micro-unità.
///
/// Rimetti `price_list_from` a restituire il solo listino di casa e questa
/// diventa rossa: è il difetto originale, non una sua imitazione.
#[test]
fn with_nothing_in_the_users_home_a_real_call_still_gets_a_cost() {
    let prices = actions::price_list_from(None);
    let entry = prices
        .find(AS_THE_ENGINE_NAMED_IT)
        .unwrap_or_else(|| panic!("il listino spedito non conosce «{AS_THE_ENGINE_NAMED_IT}»"));
    assert_eq!(
        cost_micros(MEASURED, entry.micros()),
        Some(128_541),
        "senza un listino spedito questa chiamata costava zero, e nessun tetto scattava"
    );
    assert_eq!(prices.currency, "USD");
}

/// Il file di casa continua a vincere: è il punto della cura, non un effetto
/// collaterale. Chi corregge un prezzo lo fa con un editor di testo, e vale
/// dalla chiamata dopo senza ricompilare niente.
#[test]
fn what_the_user_writes_at_home_still_beats_what_is_shipped() {
    let home = r#"{"currency":"USD","models":[
        {"id":"claude-opus-5","aliases":["claude-opus-5[1m]"],
         "input_per_million":1.0,"output_per_million":1.0,
         "cached_per_million":1.0,"cache_write_long_per_million":1.0}
    ]}"#;
    let prices = actions::price_list_from(Some(home));
    let entry = prices.find(AS_THE_ENGINE_NAMED_IT).expect("la voce di casa");
    assert_eq!(entry.input_per_million, Some(1.0), "ha vinto quella spedita");
    // E ciò che il file di casa non nomina arriva ancora da quello spedito.
    assert_eq!(prices.knows("claude-haiku-4-5"), Known::Priced);
}

/// **UN FILE DI CASA SCRITTO MALE NON DEVE SPEGNERE IL LISTINO.** È il guasto 35
/// nella sua forma più silenziosa: un errore di battitura in un file JSON che
/// spegne un tetto di spesa senza dire niente a nessuno.
#[test]
fn a_broken_file_at_home_falls_back_to_what_is_shipped() {
    let prices = actions::price_list_from(Some("questo non è JSON"));
    assert_eq!(prices.knows("claude-opus-5"), Known::Priced);
}

/// Chi non ha un prezzo per un modello deve saperlo. I modelli di OpenAI e di
/// Google non stanno nel listino spedito **di proposito** — nessuno ne ha
/// verificato i prezzi — e restano dichiaratamente sconosciuti invece di
/// diventare zero.
#[test]
fn a_model_nobody_priced_is_reported_as_absent_not_as_free() {
    let prices = actions::price_list_from(None);
    assert_eq!(prices.knows("gpt-5-codex"), Known::Absent);
    assert_eq!(
        cost_micros(
            TokenCounts {
                input: Some(1_000_000),
                output: Some(1_000_000),
                ..TokenCounts::default()
            },
            prices.find("gpt-5-codex").map(Price::micros).unwrap_or_default()
        ),
        None,
        "un modello senza prezzo non costa zero: non si sa"
    );
}
