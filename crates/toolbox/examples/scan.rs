//! Runs the detection on this machine and prints it.
//!
//! An example, not a subcommand: that wiring belongs to the `sailor` binary.
//! Here what is needed is to see the outcome before anybody wires it up.
//!
//!     cargo run -p toolbox --example scan -- [family] [--json]

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
            Presence::Present(why) => ("here    ", why.as_str()),
            Presence::Absent(why) => ("missing ", why.as_str()),
            Presence::Undetermined(why) => ("unknown ", why.as_str()),
        };
        let version = match &f.version {
            VersionReading::Declared(v) => v.clone(),
            VersionReading::Unavailable(why) => format!("version not obtained — {why}"),
            VersionReading::NotAsked(_) => String::new(),
        };
        println!("{mark} [{}] {:<24} {}", f.family, f.name, version);
        println!(
            "           from: {} ({})",
            f.descriptor_id, f.descriptor_source
        );
        if let Some(bin) = &f.executable {
            println!("           executable: {bin}");
        }
        for c in f.config.iter().filter(|c| c.presence.is_present()) {
            println!("           configuration: {}", c.path);
        }
        if !f.presence.is_present() {
            println!("           why: {why}");
        }
    }
    println!();
    println!(
        "{} entries, {} present, {} descriptors unread",
        report.findings.len(),
        report.present().len(),
        report.problems.len()
    );
    for p in &report.problems {
        println!("  problem: {} in {} — {}", p.about, p.source, p.reason);
    }
}
