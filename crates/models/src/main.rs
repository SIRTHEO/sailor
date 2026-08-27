//! L'eseguibile del crate: solo I/O — ambiente, rete, disco, stampa. Il
//! giudizio (filtro, regola dei soli gratuiti, formattazione) vive nel resto
//! del crate, dove le prove lo controllano senza toccare nessuno di questi
//! tre. Pensato per diventare `sailor models`: le tre operazioni sotto sono
//! già quelle del mandato, non un sottoinsieme provvisorio.

use models::catalog::{Catalog, Filter};
use models::{command, fetch, store};
use std::path::PathBuf;

/// `MODELS_CONFIG_PATH`, se presente, altrimenti `~/.claude/state/modelli.json`.
/// Mai cablato altrove nel crate: è la riga del mandato.
fn config_path() -> PathBuf {
    if let Ok(p) = std::env::var("MODELS_CONFIG_PATH") {
        return PathBuf::from(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{home}/.claude/state/modelli.json"))
}

fn load_catalog() -> Result<Catalog, String> {
    let body = fetch::fetch_catalog_body();
    Catalog::parse(&body)
}

fn usage() -> ! {
    eprintln!("uso:");
    eprintln!("  models list [--free-only] [--paid-only] [--modality text|image|audio|video] [--min-context N]");
    eprintln!("  models current <genere>");
    eprintln!("  models set <genere> <model-id>");
    std::process::exit(2);
}

fn run_list(args: &[String]) {
    let mut filter = Filter::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--free-only" => filter.free_only = true,
            "--paid-only" => filter.paid_only = true,
            "--modality" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    eprintln!("--modality richiede un valore");
                    std::process::exit(2);
                };
                match command::parse_modality_arg(raw) {
                    Ok(m) => filter.modality = Some(m),
                    Err(e) => {
                        eprintln!("{e}");
                        std::process::exit(2);
                    }
                }
            }
            "--min-context" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    eprintln!("--min-context richiede un valore");
                    std::process::exit(2);
                };
                match raw.parse::<u64>() {
                    Ok(n) => filter.min_context = Some(n),
                    Err(_) => {
                        eprintln!("--min-context vuole un numero, letto \"{raw}\"");
                        std::process::exit(2);
                    }
                }
            }
            other => {
                eprintln!("opzione sconosciuta: {other}");
                usage();
            }
        }
        i += 1;
    }
    match load_catalog() {
        Ok(catalog) => println!("{}", command::list(&catalog, &filter)),
        Err(e) => {
            eprintln!("catalogo non leggibile: {e}");
            std::process::exit(1);
        }
    }
}

fn run_current(args: &[String]) {
    let Some(kind) = args.first() else { usage() };
    match load_catalog() {
        Ok(catalog) => {
            let cfg = store::load(&config_path());
            println!("{}", command::current(&catalog, &cfg, kind));
        }
        Err(e) => {
            eprintln!("catalogo non leggibile: {e}");
            std::process::exit(1);
        }
    }
}

fn run_set(args: &[String]) {
    let (Some(kind), Some(model_id)) = (args.first(), args.get(1)) else { usage() };
    match load_catalog() {
        Ok(catalog) => {
            let path = config_path();
            let mut cfg = store::load(&path);
            match command::set(&mut cfg, &catalog, kind, model_id) {
                Ok(msg) => {
                    if let Err(e) = store::save(&path, &cfg) {
                        eprintln!("scelta accettata ma non salvata: {e}");
                        std::process::exit(1);
                    }
                    println!("{msg}");
                }
                Err(e) => {
                    eprintln!("{e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("catalogo non leggibile: {e}");
            std::process::exit(1);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = args.first() else { usage() };
    match cmd.as_str() {
        "list" => run_list(&args[1..]),
        "current" => run_current(&args[1..]),
        "set" => run_set(&args[1..]),
        _ => usage(),
    }
}
