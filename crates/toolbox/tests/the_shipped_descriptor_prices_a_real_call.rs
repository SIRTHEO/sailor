//! Il descrittore **spedito** legge un'uscita **vera** e ne calcola il costo.
//!
//! **PERCHÉ QUESTA PROVA VALE PIÙ DELLE ALTRE SUL CONSUMO.** Le altre provano un
//! pezzo per volta con dati costruiti apposta, e passano anche quando i pezzi non
//! si toccano fra loro. Qui i tre pezzi sono quelli veri: il descrittore che
//! viene spedito col prodotto, l'uscita che `claude -p --output-format json` ha
//! davvero scritto su questa macchina il 30/08/2026, e un listino coi prezzi
//! pubblicati. Il numero che ne esce si confronta col costo che il motore ha
//! dichiarato di suo — due strade indipendenti verso la stessa cifra.
//!
//! Fino a stamattina questa catena non arrivava in fondo: `claude-code` non
//! dichiarava nessun blocco `usage`, quindi ogni chiamata al motore che si usa
//! di più finiva nel deposito senza un solo conteggio.

use actions::ToolResolver;
use toolbox::descriptor::{Catalog, Source};
use toolbox::probe::Machine;
use toolbox::resolver::Tools;

/// L'uscita vera, ridotta ai campi che il descrittore nomina. I numeri non sono
/// inventati: 2 token d'ingresso, 4 d'uscita, 9.922 letti dalla cache, 12.347
/// scritti in una cache a lunga durata, e 0,128541 dollari dichiarati.
const REAL_OUTPUT: &str = r#"{
  "stop_reason": "end_turn",
  "total_cost_usd": 0.128541,
  "usage": {
    "input_tokens": 2,
    "cache_creation_input_tokens": 12347,
    "cache_read_input_tokens": 9922,
    "output_tokens": 4,
    "cache_creation": {
      "ephemeral_1h_input_tokens": 12347,
      "ephemeral_5m_input_tokens": 0
    }
  },
  "modelUsage": {
    "claude-opus-5[1m]": {
      "inputTokens": 2,
      "costUSD": 0.128541,
      "canonicalModel": "claude-opus-5"
    }
  },
  "result": "ok",
  "type": "result"
}"#;

/// Un listino coi prezzi pubblicati per quel modello. Il prezzo della cache a
/// lunga durata è il doppio dell'ingresso: non è una supposizione, è ciò che
/// rende il conto uguale a quello del motore.
const PRICES: &str = r#"{
  "currency": "USD",
  "models": [{
    "id": "claude-opus-5",
    "aliases": ["claude-opus-5[1m]"],
    "input_per_million": 5.0,
    "output_per_million": 25.0,
    "cached_per_million": 0.5,
    "cache_write_per_million": 6.25,
    "cache_write_long_per_million": 10.0
  }]
}"#;

/// Solo i descrittori spediti col prodotto: quello che ha chiunque lo installi,
/// senza niente di questa macchina attorno.
fn shipped_only() -> Tools {
    let machine = Machine {
        path_dirs: Vec::new(),
        home: std::path::PathBuf::from("/home/nessuno"),
        env: Default::default(),
        version_probes: false,
    };
    Tools::new(Catalog::load(&[Source::Builtin]), machine)
}

#[test]
fn the_shipped_claude_descriptor_reads_a_real_call_and_prices_it() {
    let recipe = shipped_only()
        .ask_recipe("claude-code")
        .expect("claude-code dichiara come gli si fa una domanda");
    let usage = recipe
        .usage
        .expect("e dichiara anche come si legge quanto ha consumato");

    let reading = models::usage::read_declared(REAL_OUTPUT, &usage.declared);

    // I conteggi, uno per uno, dall'uscita vera.
    assert_eq!(reading.input_tokens, Some(2));
    assert_eq!(reading.output_tokens, Some(4));
    assert_eq!(reading.cached_tokens, Some(9_922));
    assert_eq!(reading.cache_write_tokens, Some(0), "cache breve: nessuna");
    assert_eq!(reading.cache_write_long_tokens, Some(12_347));
    // Il modello è la CHIAVE di `modelUsage`, non un campo.
    assert_eq!(reading.model.as_deref(), Some("claude-opus-5[1m]"));
    // E l'uscita del passo resta la risposta, non l'involucro.
    assert_eq!(reading.answer.as_deref(), Some("ok"));

    let prices = models::pricing::PriceList::parse(PRICES).unwrap();
    let entry = prices
        .find(reading.model.as_deref().unwrap())
        .expect("l'alias porta alla voce di listino");
    let cost = models::pricing::cost_micros(
        models::pricing::TokenCounts {
            input: reading.input_tokens,
            output: reading.output_tokens,
            cached: reading.cached_tokens,
            cache_write: reading.cache_write_tokens,
            cache_write_long: reading.cache_write_long_tokens,
        },
        entry.micros(),
    )
    .expect("il costo si calcola");

    // Il motore aveva dichiarato 0,128541 $. Il nostro conto, partito dai soli
    // conteggi e dal listino, arriva alla stessa cifra al micro.
    let declared = (reading.declared_cost.unwrap() * 1_000_000.0).round() as i64;
    assert_eq!(declared, 128_541);
    assert_eq!(
        cost, declared,
        "il conto sul listino e quello del motore devono coincidere"
    );
}

/// La stessa catena su un motore che dichiara **meno**: agy dà i token ma non
/// il modello. Il costo deve restare sconosciuto — non zero, e non un modello
/// indovinato.
#[test]
fn a_engine_that_names_no_model_leaves_the_cost_unknown_not_zero() {
    let recipe = shipped_only().ask_recipe("agy").expect("agy è spedito");
    let usage = recipe.usage.expect("e dichiara il proprio consumo");

    let said = r#"{"status":"SUCCESS","response":"ok\n",
        "usage":{"input_tokens":14514,"output_tokens":195,
                 "cache_read_tokens":0,"total_tokens":14709}}"#;
    let reading = models::usage::read_declared(said, &usage.declared);

    assert_eq!(reading.input_tokens, Some(14_514));
    assert_eq!(reading.total_tokens, Some(14_709));
    assert_eq!(reading.answer.as_deref(), Some("ok\n"));
    assert_eq!(
        reading.model, None,
        "agy non nomina nessun modello, e nessuno lo inventa per lui"
    );

    let prices = models::pricing::PriceList::parse(PRICES).unwrap();
    assert!(
        reading
            .model
            .as_deref()
            .and_then(|name| prices.find(name))
            .is_none(),
        "senza nome non c'è voce di listino, quindi nessun prezzo da applicare"
    );
}

/// La riga di comando che il descrittore **spedito** compone per `agy`, dove il
/// prompt è un argomento e non l'ingresso standard.
///
/// **QUESTA È LA PROVA CHE MANCAVA AL GUASTO 1, E CHE LO HA LASCIATO TORNARE.**
/// Allora l'ordine sbagliato erano due opzioni di `ask` fra loro, e la cura
/// scritta accanto — «una prova che esegue davvero ogni riga di comando prima
/// che finisca in un flusso» — non è mai stata costruita. Il 31/08/2026 il
/// difetto è ricomparso da un'altra porta: le opzioni di `usage`, accodandosi
/// dopo quelle di `ask`, si infilavano fra `--print` e la domanda, e `agy`
/// rispondeva «--print took "--output-format" as its prompt» ignorando il testo
/// vero. I due blocchi erano giusti separatamente e sbagliati insieme, che è
/// esattamente ciò che una prova per blocco non può vedere.
///
/// Non esegue niente: la sequenza la decide il codice, quindi è identica su una
/// macchina carica, senza rete e senza `agy` installato. Ciò che resta scoperto
/// — eseguire davvero ogni riga composta — è ancora la cura del guasto 1.
#[test]
fn the_prompt_flag_stays_glued_to_the_prompt_it_introduces() {
    let recipe = shipped_only().ask_recipe("agy").expect("agy è spedito");
    assert_eq!(
        recipe.prompt,
        actions::PromptVia::LastArg,
        "questa prova ha senso solo se la domanda è un argomento"
    );

    let line = actions::command_line(&recipe);
    assert_eq!(
        line,
        vec!["--mode", "plan", "--output-format", "json", "--print"],
        "le opzioni del consumo vanno prima di quella che introduce la domanda"
    );
    assert_eq!(
        line.last().map(String::as_str),
        Some("--print"),
        "fra la bandiera che introduce la domanda e la domanda non entra niente"
    );
}
