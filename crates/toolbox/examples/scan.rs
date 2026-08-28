//! Esegue il rilevamento su questa macchina e lo stampa.
//!
//! È un esempio e non un sottocomando perché il posto del sottocomando è il
//! binario `sailor`, e quell'aggancio non appartiene a questo crate: qui serve
//! il modo di guardare l'esito senza aspettare che qualcuno lo agganci.
//!
//!     cargo run -p toolbox --example scan -- [famiglia] [--json]

use toolbox::{Catalog, Machine, Presence, VersionReading};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let as_json = args.iter().any(|a| a == "--json");
    let family = args.iter().find(|a| !a.starts_with("--")).cloned();

    let machine = Machine::current();
    let catalog = Catalog::load(&toolbox::default_sources(&machine));
    let report = toolbox::detect(&catalog, &machine);

    if as_json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
        return;
    }

    for f in &report.findings {
        if family.as_deref().is_some_and(|want| want != f.family) {
            continue;
        }
        let (mark, why) = match &f.presence {
            Presence::Present(why) => ("c'è      ", why.as_str()),
            Presence::Absent(why) => ("non c'è  ", why.as_str()),
            Presence::Undetermined(why) => ("non so   ", why.as_str()),
        };
        let version = match &f.version {
            VersionReading::Declared(v) => v.clone(),
            VersionReading::Unavailable(why) => format!("versione non ottenuta — {why}"),
            VersionReading::NotAsked(_) => String::new(),
        };
        println!("{mark} [{}] {:<24} {}", f.family, f.name, version);
        println!("           da: {} ({})", f.descriptor_id, f.descriptor_source);
        if let Some(bin) = &f.executable {
            println!("           eseguibile: {bin}");
        }
        for c in f.config.iter().filter(|c| c.presence.is_present()) {
            println!("           configurazione: {}", c.path);
        }
        if !f.presence.is_present() {
            println!("           perché: {why}");
        }
    }
    println!();
    println!(
        "{} voci, {} presenti, {} descrittori non letti",
        report.findings.len(),
        report.present().len(),
        report.problems.len()
    );
    for p in &report.problems {
        println!("  segnalazione: {} in {} — {}", p.about, p.source, p.reason);
    }
}
