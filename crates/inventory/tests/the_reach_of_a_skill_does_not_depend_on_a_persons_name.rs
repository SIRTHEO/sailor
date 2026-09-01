//! Una raccolta di competenze installata **come cartella** è raggiungibile, e
//! per questo non serve conoscerne il nome.
//!
//! **COSA C'ERA PRIMA, E PERCHÉ ERA PEGGIO DI UN NOME DI TROPPO.** Fino al
//! 01/09/2026 l'inventario nominava una raccolta sola, `mattpocock-skills`, in
//! tre punti: il percorso da cui leggerla, il prefisso con cui invocarla, e —
//! il peggiore — la condizione che decideva se una competenza fosse
//! raggiungibile:
//!
//! ```ignore
//! } else if on.contains(&plugin) || plugin.contains("mattpocock") {
//! ```
//!
//! Cioè **il nome di una persona decideva cosa Sailor considera installato**.
//! Su questa macchina la riga dormiva, perché quella cartella non esiste; il
//! giorno che comparisse tornerebbe viva senza che nessuno l'abbia chiesto.
//!
//! E dormiva anche per un secondo motivo, che è la ragione per cui non l'aveva
//! vista nessuno: stava dentro un `if` i cui **primi due rami tornavano la
//! stessa cosa**. Una condizione il cui esito non cambia niente non si legge —
//! si scorre.
//!
//! **IL PRINCIPIO CHE STAVA SOTTO IL NOME.** Una raccolta installata come
//! cartella sotto `.claude/skills/` non è un plugin: non compare in
//! `enabledPlugins`, e chiedere a quell'elenco se sia accesa risponde «no» per
//! una domanda che non le si applica. La regola giusta è sull'**origine**, non
//! sul nome: dalla cache dei plugin si chiede l'elenco degli accesi, da una
//! cartella no.

use inventory::{collect, Kind, Reach, Root};
use std::fs;
use std::path::{Path, PathBuf};

fn fake_home(name: &str) -> PathBuf {
    let home = std::env::temp_dir().join(format!("prova-raggiungibilita-{name}"));
    let _ = fs::remove_dir_all(&home);
    fs::create_dir_all(&home).unwrap();
    home
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn skill(home: &Path, at: &str, name: &str) {
    write(
        &home.join(at),
        &format!("---\nname: {name}\ndescription: che cosa fa {name}\n---\n\n# {name}\n"),
    );
}

fn found<'a>(entries: &[&'a inventory::Entry], name: &str) -> &'a inventory::Entry {
    entries
        .iter()
        .copied()
        .find(|entry| entry.name.ends_with(name))
        .unwrap_or_else(|| {
            panic!(
                "«{name}» non è nell'inventario; ci sono: {:?}",
                entries.iter().map(|e| &e.name).collect::<Vec<_>>()
            )
        })
}

/// **UNA RACCOLTA QUALUNQUE, CON UN NOME CHE NESSUNO HA COMPILATO DENTRO.**
/// Se passa, vuol dire che la regola guarda la forma dell'installazione e non
/// l'identità di chi l'ha pubblicata.
#[test]
fn a_collection_installed_as_a_folder_is_reachable_without_being_a_plugin() {
    let home = fake_home("raccolta-cartella");
    // Nessun plugin acceso: se la raggiungibilità dipendesse da `enabledPlugins`,
    // questa competenza risulterebbe spenta.
    write(&home.join(".claude/settings.json"), r#"{"enabledPlugins": {}}"#);
    skill(
        &home,
        ".claude/skills/raccolta-di-qualcuno/skills/tagliare/SKILL.md",
        "tagliare",
    );

    let inventory = collect(&[Root::home(&home)]);
    let entry = found(&inventory.of(Kind::Skill), "tagliare");

    assert_eq!(
        entry.reach,
        Reach::Active,
        "una raccolta installata come cartella è raggiungibile: non è un plugin, \
         e chiedere a `enabledPlugins` se sia accesa è una domanda che non le si applica"
    );
    assert!(
        entry.name.starts_with("raccolta-di-qualcuno:"),
        "il prefisso si ricava dalla cartella che la contiene, non da un elenco \
         di nomi conosciuti: {}",
        entry.name
    );
}

/// La regola vale ancora dove serve: un **plugin** spento resta spento, e dice
/// perché. Senza questa, «tutto raggiungibile» passerebbe le altre prove.
#[test]
fn a_plugin_that_is_switched_off_is_still_switched_off() {
    let home = fake_home("plugin-spento-resta-spento");
    write(
        &home.join(".claude/settings.json"),
        r#"{"enabledPlugins": {"spento@1.0.0": false}}"#,
    );
    skill(
        &home,
        ".claude/plugins/cache/mercato/spento/skills/cucire/SKILL.md",
        "cucire",
    );

    let inventory = collect(&[Root::home(&home)]);
    let entry = found(&inventory.of(Kind::Skill), "cucire");

    match &entry.reach {
        Reach::Inactive(reason) => assert!(reason.contains("spento"), "{reason}"),
        other => panic!("un plugin spento non è raggiungibile, e qui risulta {other:?}"),
    }
}

/// **LA GUARDIA CHE IMPEDISCE IL RITORNO.** Le altre due dicono che oggi la
/// regola è giusta; questa dice che non si può tornare indietro domani — e
/// serve, perché la violazione non era un errore di logica ma un'abitudine:
/// far entrare nel codice il nome di ciò che si aveva sotto mano.
///
/// Legge i sorgenti dal disco perché non esiste modo di chiedere al compilatore
/// «non nominare nessuno».
#[test]
fn no_ones_name_decides_what_is_reachable() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src");
    for file in ["lib.rs", "discovery.rs"] {
        let source = fs::read_to_string(crate_root.join(file)).expect("il sorgente");
        let code: String = source
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with("//") && !trimmed.starts_with("///")
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(
            !code.contains("mattpocock"),
            "`{file}` nomina di nuovo una raccolta sola. La raggiungibilità si \
             decide sull'**origine** — cache dei plugin, oppure cartella — non \
             sull'identità di chi ha pubblicato le competenze."
        );
    }
}
