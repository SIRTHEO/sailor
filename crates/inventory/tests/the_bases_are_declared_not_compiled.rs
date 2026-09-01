//! Le basi di lavoro sono **dichiarate**, non compilate — e una base che non si
//! è potuta leggere non si confonde con una base vuota.
//!
//! LA DOMANDA CHE QUESTE PROVE DIFENDONO è la sola che l'inventario non sapeva
//! rispondere: *«zero repo» vuol dire che non ce ne sono, o che non ho potuto
//! guardare?* Fino al 01/09/2026 voleva dire tutte e due, e non c'era modo di
//! sapere quale. `repos_under` incontrava una cartella illeggibile, faceva
//! `continue`, e restituiva un elenco più corto con uscita 0.
//!
//! Su questa macchina non si vedeva, perché le due basi erano
//! `~/gyver/work` e `~/personal` **compilate dentro il binario** e su questa
//! macchina esistono. Su qualunque altra, l'inventario avrebbe risposto «zero
//! repo» — indistinguibile da una macchina davvero vuota. È la stessa forma del
//! guasto 12: *vuoto* al posto di *non lo so*.
//!
//! Le due cure viaggiano insieme perché da sole non bastano: togliere le
//! cartelle di una persona senza saper dire «nessuna base dichiarata» spegne
//! l'inventario in silenzio, e saper dire «non ho potuto guardare» tenendo le
//! cartelle compilate lascia il difetto dov'è.

use inventory::{default_roots_from, repos_under};
use std::fs;
use std::path::PathBuf;

/// Una cartella usa-e-getta, cancellata e rifatta a ogni giro.
fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("prova-basi-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Un repo si riconosce dalla `.claude/` che porta, come nel resto del crate.
fn repo(base: &PathBuf, name: &str) {
    fs::create_dir_all(base.join(name).join(".claude")).unwrap();
}

#[test]
fn a_base_that_cannot_be_read_is_reported_not_swallowed() {
    let missing = temp("illeggibile").join("questa-non-esiste");
    let survey = repos_under(&[missing.clone()]);

    assert!(
        survey.roots.is_empty(),
        "una base che non esiste non porta repo"
    );
    assert_eq!(
        survey.unreadable.iter().map(|u| &u.path).collect::<Vec<_>>(),
        vec![&missing],
        "la base illeggibile deve comparire nel rendiconto, non sparire: \
         è la differenza fra «non ce ne sono» e «non ho potuto guardare»"
    );
    assert!(
        !survey.unreadable[0].reason.is_empty(),
        "chi legge deve sapere perché, non solo che"
    );
}

#[test]
fn an_empty_base_is_not_the_same_as_an_unreadable_one() {
    let base = temp("vuota");
    let survey = repos_under(&[base]);

    assert!(survey.roots.is_empty(), "una cartella vuota non porta repo");
    assert!(
        survey.unreadable.is_empty(),
        "una cartella vuota si è letta benissimo: dichiararla illeggibile \
         sarebbe l'errore opposto, e altrettanto grave"
    );
}

#[test]
fn the_bases_that_are_declared_are_the_ones_searched() {
    let base = temp("dichiarata");
    repo(&base, "primo");
    repo(&base, "secondo");

    let survey = repos_under(&[base]);
    let mut names: Vec<&str> = survey.roots.iter().map(|r| r.label.as_str()).collect();
    names.sort_unstable();

    assert_eq!(names, vec!["primo", "secondo"]);
    assert!(survey.unreadable.is_empty());
}

#[test]
fn with_nothing_declared_the_survey_says_so_instead_of_saying_zero() {
    let home = temp("casa-senza-dichiarazione");
    let survey = default_roots_from(&home, &[]);

    assert!(
        !survey.bases_declared,
        "senza dichiarazione l'inventario deve saperlo dire: un «zero repo» \
         che nasce dal non aver guardato è una risposta falsa"
    );
    assert_eq!(
        survey.roots.len(),
        1,
        "resta la casa, che non è una base di lavoro ma c'è sempre"
    );
    assert!(survey.roots[0].is_home);
}

/// LA PROVA CHE IMPEDISCE IL RITORNO. Le altre dicono che il comportamento è
/// giusto oggi; questa dice che non si può tornare indietro domani, ed è quella
/// che serve davvero: la violazione non era un difetto di logica ma di
/// abitudine, e le abitudini rientrano dalla porta.
///
/// Legge il sorgente dal disco perché non esiste un modo di chiedere al
/// compilatore «non nominare la cartella di nessuno».
#[test]
fn no_ones_personal_folders_are_compiled_into_the_binary() {
    let source = fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("lib.rs"),
    )
    .expect("il sorgente del crate");

    let code: String = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    for folder in ["\"gyver\"", "\"personal\"", "\"work\""] {
        assert!(
            !code.contains(&format!("join({folder})")),
            "`crates/inventory/src/lib.rs` costruisce di nuovo {folder} a mano. \
             Le basi di lavoro sono dichiarate — `SAILOR_WORK_ROOTS`, o il file \
             `work-roots` nella casa di Sailor — perché le cartelle di una \
             persona sola non sono un fatto della macchina."
        );
    }
}
