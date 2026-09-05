//! `sailor models`: elencare, mostrare la scelta corrente, cambiarla — tre
//! operazioni sul catalogo scaricato da OpenRouter. Il giudizio (filtro,
//! regola dei soli gratuiti, formattazione) vive nella libreria `models`;
//! qui solo l'interpretazione degli argomenti. Prima del 27/08/2026 questo
//! era il `main.rs` di un binario a sé (`models`).

use crate::Form;
use models::catalog::{Catalog, Filter};
use models::{command, fetch, store};

pub fn run(args: &[String]) -> i32 {
    let Some(cmd) = args.first() else {
        print_usage();
        return 2;
    };
    match cmd.as_str() {
        "list" => run_list(&args[1..]),
        "current" => run_current(&args[1..]),
        "set" => run_set(&args[1..]),
        "unpriced" => run_unpriced(),
        _ => {
            print_usage();
            2
        }
    }
}

/// Le forme di `sailor models`, una per riga. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor models list [--free-only] [--paid-only] [--modality text|image|audio|video] [--min-context N]",
        says_key: "",
    },
    Form {
        form: "sailor models current <kind>",
        says_key: "",
    },
    Form {
        form: "sailor models set <kind> <model-id>",
        says_key: "",
    },
    Form {
        form: "sailor models unpriced",
        says_key: "cli.models.unpriced_says",
    },
];

fn print_usage() {
    eprintln!("{}", catalogue::say("cli.usage_heading", &[]));
    for line in crate::forms_as_lines(USAGE) {
        eprintln!("  {line}");
    }
}

fn load_catalog() -> Result<Catalog, String> {
    let mut catalog = Catalog::parse(&fetch::catalog_body()?)?;
    let home = ledger::sailor_home()
        .map(|home| home.join("pacts.json"))
        .filter(|path| path.exists())
        .map(std::fs::read_to_string)
        .transpose()
        .map_err(|error| format!("pacts.json: {error}"))?;
    catalog.declare_pacts(&models::pact::Pacts::in_force(home.as_deref())?);
    Ok(catalog)
}

fn run_list(args: &[String]) -> i32 {
    let mut filter = Filter::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--free-only" => filter.free_only = true,
            "--paid-only" => filter.paid_only = true,
            "--modality" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    eprintln!("--modality needs a value");
                    return 2;
                };
                match command::parse_modality_arg(raw) {
                    Ok(m) => filter.modality = Some(m),
                    Err(e) => {
                        eprintln!("{e}");
                        return 2;
                    }
                }
            }
            "--min-context" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    eprintln!("--min-context needs a value");
                    return 2;
                };
                match raw.parse::<u64>() {
                    Ok(n) => filter.min_context = Some(n),
                    Err(_) => {
                        eprintln!(
                            "{}",
                            catalogue::say("cli.models.min_context_not_a_number", &[("raw", raw)])
                        );
                        return 2;
                    }
                }
            }
            other => {
                eprintln!("unknown option: {other}");
                print_usage();
                return 2;
            }
        }
        i += 1;
    }
    match load_catalog() {
        Ok(catalog) => {
            println!("{}", command::list(&catalog, &filter));
            0
        }
        Err(e) => {
            eprintln!(
                "{}",
                catalogue::say(
                    "cli.models.catalogue_unreadable",
                    &[("error", &e.to_string())]
                )
            );
            1
        }
    }
}

/// The models this machine has really been answered by, that the price list in
/// force cannot price. Read off the ledger's own rows, not guessed: a model
/// that answered and has no entry costs an unknown amount for ever, and the
/// only way anybody notices today is by reading one run's report.
fn run_unpriced() -> i32 {
    let dir = ui::gather::default_ledger_dir();
    let gathered = match ui::gather::gather(&dir) {
        Ok(Some(data)) => data,
        Ok(None) => {
            println!(
                "{}",
                catalogue::say(
                    "cli.models.no_store_here",
                    &[("path", &dir.display().to_string())]
                )
            );
            return 0;
        }
        Err(error) => {
            eprintln!("{error}");
            return 1;
        }
    };
    let absent = unpriced_among(
        gathered.calls_by_run.values().flatten(),
        &actions::current_price_list(),
    );
    if absent.is_empty() {
        println!("{}", catalogue::say("cli.models.every_model_is_priced", &[]));
        return 0;
    }
    println!("{}", catalogue::say("cli.models.unpriced_heading", &[]));
    for (name, calls) in &absent {
        println!(
            "  {}",
            catalogue::say(
                "cli.models.unpriced_line",
                &[("model", name), ("calls", &calls.to_string())],
            )
        );
    }
    0
}

/// The models among these calls the list cannot price, and how many calls each
/// answered. A model that answered without a name is not one of them: an empty
/// name is missing identity, and reporting it would send somebody looking for
/// a price list entry with no name to write.
fn unpriced_among<'a>(
    calls: impl Iterator<Item = &'a ledger::ModelCallRecord>,
    prices: &models::pricing::PriceList,
) -> std::collections::BTreeMap<String, usize> {
    let mut absent = std::collections::BTreeMap::new();
    for call in calls {
        let name = call.actual_model.trim();
        if name.is_empty() || prices.knows(name) == models::pricing::Known::Priced {
            continue;
        }
        *absent.entry(name.to_owned()).or_default() += 1;
    }
    absent
}

fn run_current(args: &[String]) -> i32 {
    let Some(kind) = args.first() else {
        print_usage();
        return 2;
    };
    match load_catalog() {
        Ok(catalog) => {
            let cfg = store::load(&store::config_path());
            println!("{}", command::current(&catalog, &cfg, kind));
            0
        }
        Err(e) => {
            eprintln!(
                "{}",
                catalogue::say(
                    "cli.models.catalogue_unreadable",
                    &[("error", &e.to_string())]
                )
            );
            1
        }
    }
}

fn run_set(args: &[String]) -> i32 {
    let (Some(kind), Some(model_id)) = (args.first(), args.get(1)) else {
        print_usage();
        return 2;
    };
    match load_catalog() {
        Ok(catalog) => {
            let path = store::config_path();
            let mut cfg = store::load(&path);
            match command::set(&mut cfg, &catalog, kind, model_id) {
                Ok(msg) => {
                    if let Err(e) = store::save(&path, &cfg) {
                        eprintln!(
                            "{}",
                            catalogue::say(
                                "cli.models.choice_not_saved",
                                &[("error", &e.to_string())]
                            )
                        );
                        return 1;
                    }
                    println!("{msg}");
                    0
                }
                Err(e) => {
                    eprintln!("{e}");
                    1
                }
            }
        }
        Err(e) => {
            eprintln!(
                "{}",
                catalogue::say(
                    "cli.models.catalogue_unreadable",
                    &[("error", &e.to_string())]
                )
            );
            1
        }
    }
}

#[cfg(test)]
mod tests {

    /// A model that answered and has no price is named with how many calls it
    /// answered; one that is priced is not, and a call with no model name is
    /// missing identity rather than an unpriced model.
    #[test]
    fn only_the_models_the_list_cannot_price_are_named() {
        let prices = models::pricing::shipped();
        let priced = prices
            .entries
            .first()
            .map(|entry| entry.id.clone())
            .expect("the shipped list has at least one entry");
        let calls = [
            a_call(&priced),
            a_call("un-modello-mai-visto"),
            a_call("un-modello-mai-visto"),
            a_call("   "),
        ];

        let absent = unpriced_among(calls.iter(), &prices);

        assert_eq!(absent.len(), 1, "one unpriced model, not more: {absent:?}");
        assert_eq!(absent.get("un-modello-mai-visto"), Some(&2), "{absent:?}");
    }

    fn a_call(actual_model: &str) -> ledger::ModelCallRecord {
        ledger::ModelCallRecord {
            call_id: format!("chiamata-{actual_model}"),
            run_id: "corsa".to_owned(),
            step_id: None,
            purpose: "prova".to_owned(),
            cli: "prova".to_owned(),
            requested_model: String::new(),
            actual_model: actual_model.to_owned(),
            input_tokens: None,
            output_tokens: None,
            cached_tokens: None,
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost_micros: None,
            declared_cost_micros: None,
            price_currency: None,
            input_price_micros_per_million: None,
            output_price_micros_per_million: None,
            cached_price_micros_per_million: None,
            cache_write_price_micros_per_million: None,
            cache_write_long_price_micros_per_million: None,
            engine_identity: ledger::EngineIdentity::default(),
            retry_chain: vec![],
            error_type: None,
            started_at: 0,
            ended_at: Some(1),
            session_id: None,
            work_kind: None,
            fell_back_from: Vec::new(),
        }
    }
    use super::*;

    fn a(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn no_subcommand_is_a_usage_error() {
        assert_eq!(run(&a(&[])), 2);
    }

    #[test]
    fn an_unknown_subcommand_is_a_usage_error() {
        assert_eq!(run(&a(&["frobnicate"])), 2);
    }
}
