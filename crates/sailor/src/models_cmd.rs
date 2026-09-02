//! `sailor models`: elencare, mostrare la scelta corrente, cambiarla — tre
//! operazioni sul catalogo scaricato da OpenRouter. Il giudizio (filtro,
//! regola dei soli gratuiti, formattazione) vive nella libreria `models`;
//! qui solo l'interpretazione degli argomenti. Prima del 27/08/2026 questo
//! era il `main.rs` di un binario a sé (`models`).

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
        _ => {
            print_usage();
            2
        }
    }
}

/// Le forme di `sailor models`, una per riga. Vedi `flow_cmd::USAGE`.
pub const USAGE: &[&str] = &[
    "sailor models list [--free-only] [--paid-only] [--modality text|image|audio|video] [--min-context N]",
    "sailor models current <kind>",
    "sailor models set <kind> <model-id>",
];

fn print_usage() {
    eprintln!("usage:");
    for line in USAGE {
        eprintln!("  {line}");
    }
}

fn load_catalog() -> Result<Catalog, String> {
    Catalog::parse(&fetch::catalog_body()?)
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
