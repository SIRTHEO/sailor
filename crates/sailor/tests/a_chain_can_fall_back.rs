//! Una catena di motori serve solo se il ripiego può scattare.
//!
//! **IL GUASTO 31.** Un passo che scrive `"tool": ["claude-code", "agy",
//! "codex"]` sembra avere due ripieghi. Ne ha quanti sono i motori che
//! **dichiarano come dicono di non poter lavorare**: `says_it_cannot_work` su
//! un elenco vuoto è `false`, quindi un motore che tace fa morire il passo sul
//! proprio fallimento e i motori dopo di lui non partono mai. `agy` tace, e sta
//! in mezzo a tutte e tre le catene di questo albero.
//!
//! Il meccanismo è provato in modo ermetico in `crates/actions`
//! (`an_engine_that_declares_no_exhaustion_words_kills_the_chain`, in coppia
//! con `an_engine_that_says_it_is_out_hands_the_work_to_the_next_one`): elenco
//! popolato, il secondo parte; elenco vuoto, il secondo non parte. Qui si
//! guarda se quel difetto è **in casa nostra**, sui flussi e sui descrittori
//! spediti.

use std::collections::BTreeSet;
use std::path::PathBuf;

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

/// Un motore nominato in una catena, con il posto che occupa.
struct InChain {
    flow: String,
    step: String,
    tool: String,
    last: bool,
}

/// Tutti i motori nominati da una catena in tutti i flussi dell'albero.
///
/// **L'ORDINE CONTA E SI LEGGE DAL FILE.** Un `BTreeSet` direbbe *quali*
/// motori, mai *quale viene dopo quale* — e la domanda qui è esattamente
/// quella: l'ultimo della catena non ha nessuno a cui passare il lavoro, e
/// pretendere da lui una dichiarazione di esaurimento sarebbe pretendere una
/// misura che non serve a niente.
fn engines_in_chains() -> Vec<InChain> {
    let dir = repository_root().join("flows");
    let mut found = Vec::new();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", dir.display()));
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.to_string_lossy().ends_with(".flow.json") {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("leggere il flusso");
        let Ok(file) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let flow = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(steps) = file["graph"]["steps"].as_array() else {
            continue;
        };
        for step in steps {
            let Some(chain) = step["with"]["tool"].as_array() else {
                continue;
            };
            let names: Vec<&str> = chain.iter().filter_map(|id| id.as_str()).collect();
            for (place, tool) in names.iter().enumerate() {
                found.push(InChain {
                    flow: flow.clone(),
                    step: step["id"].as_str().unwrap_or_default().to_owned(),
                    tool: (*tool).to_owned(),
                    last: place + 1 == names.len(),
                });
            }
        }
    }
    found
}

/// Le parole di `unusable_when` che un motore spedito dichiara.
fn exhaustion_words(tool: &str) -> Option<Vec<String>> {
    let catalog = toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]);
    catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == tool)
        .map(|loaded| {
            loaded
                .descriptor
                .ask
                .as_ref()
                .map(|ask| ask.unusable_when.clone())
                .unwrap_or_default()
        })
}

/// **LA REGOLA, E RESTA ROSSA FINCHÉ QUALCUNO NON MISURA `agy`.**
///
/// Ogni motore che compare in una catena e **non è l'ultimo** deve dichiarare
/// almeno una parola con cui dice di non poter lavorare. Chi non la dichiara
/// non è un ripiego: è un tappo.
///
/// **PERCHÉ È `#[ignore]` E NON SEMPLICEMENTE ROSSA.** La cura non si può
/// scrivere: le parole con cui `agy` dice di aver finito la quota non sono mai
/// state viste su questa macchina, e inventarle sarebbe peggio del difetto —
/// una parola sbagliata in `unusable_when` fa scendere un mandato malformato
/// per tutta la catena finché qualcuno risponde comunque, che è il motivo per
/// cui quel campo pretende **le parole del fornitore**. Una prova rossa per
/// sempre, invece, ha due conseguenze concrete e nessuna delle due è la
/// diagnosi: la batteria intera diventa rossa, e il passo `prove` del flusso
/// `sviluppa-sailor` — che esegue `cargo test` — fallirebbe a ogni corsa,
/// bloccando ogni lavoro futuro dietro un difetto che non riguarda quel lavoro.
/// Peggio: chi la trovasse rossa avrebbe l'incentivo a farla passare, e il modo
/// più rapido è inventare la parola.
///
/// **IL DEBITO STA DOVE SI LEGGE, NON QUI.** È il guasto 31 in
/// `docs/guasti-incontrati.md`, dichiarato **aperto**, in una tabella che ha un
/// suo controllo — `the_fault_table_holds_together` — il quale pretende che i
/// conteggi in prosa dicano il vero. Un guasto aperto lì non si può togliere
/// senza che qualcuno se ne accorga; un `#[ignore]` senza quella riga sarebbe
/// una dimenticanza travestita da decisione.
///
/// Si toglie l'`#[ignore]` il giorno in cui si vede `agy` dire di essere
/// esaurito, e si scrivono le sue parole nel descrittore.
#[test]
#[ignore = "guasto 31 aperto: agy non dichiara `unusable_when`, e le sue parole non sono state misurate. Inventarle sarebbe peggio del difetto"]
fn every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted() {
    let mut silent: BTreeSet<String> = BTreeSet::new();
    for engine in engines_in_chains() {
        if engine.last {
            continue;
        }
        let words = exhaustion_words(&engine.tool).unwrap_or_default();
        if words.iter().all(|word| word.trim().is_empty()) {
            silent.insert(format!(
                "{} · {} · {}",
                engine.flow, engine.step, engine.tool
            ));
        }
    }
    assert!(
        silent.is_empty(),
        "questi motori stanno in mezzo a una catena senza dichiarare come dicono \
         di non poter lavorare: quando si esauriscono uccidono il passo, e i \
         motori dopo di loro non partono. È il guasto 31.\n{}",
        silent.into_iter().collect::<Vec<_>>().join("\n")
    );
}

/// **LA REGOLA QUI SOPRA NON DEVE POTER DIVENTARE VUOTA IN SILENZIO.**
///
/// Questa prova gira sempre, e dice una cosa sola: nei flussi di questo albero
/// esistono catene con un motore che non è l'ultimo. Il giorno in cui non ce ne
/// fossero più — perché i flussi cambiano — la prova ignorata qui sopra
/// passerebbe senza guardare niente, e nessuno lo saprebbe. È il difetto del
/// guasto 22 applicato a un controllo invece che a un totale: uno zero mai
/// calcolato si presenta come una misura.
#[test]
fn there_are_chains_whose_fallback_can_actually_be_needed() {
    let engines = engines_in_chains();
    let not_last = engines.iter().filter(|engine| !engine.last).count();
    assert!(
        not_last > 0,
        "nessuna catena ha un motore prima dell'ultimo: la regola sul ripiego \
         non guarderebbe più niente, e resterebbe verde per vuoto"
    );
}

/// E la misura di **quanto** è aperto il guasto 31, oggi, senza giudicarlo.
///
/// Non pretende niente da `agy`: dice che ogni motore in mezzo a una catena o
/// dichiara le sue parole, o è uno dei motori che la tabella dei guasti già
/// nomina. Serve perché il giorno in cui un **quarto** motore muto entrasse in
/// una catena, quello non sarebbe più il guasto 31 registrato: sarebbe un
/// difetto nuovo, e passerebbe inosservato dietro un `#[ignore]` che parla
/// d'altro.
#[test]
fn no_engine_other_than_the_ones_already_registered_is_silent_in_a_chain() {
    // `agy` è il guasto 31, aperto e scritto in `docs/guasti-incontrati.md`.
    let registered = ["agy"];
    let mut unexpected: BTreeSet<String> = BTreeSet::new();
    for engine in engines_in_chains() {
        if engine.last || registered.contains(&engine.tool.as_str()) {
            continue;
        }
        let words = exhaustion_words(&engine.tool).unwrap_or_default();
        if words.iter().all(|word| word.trim().is_empty()) {
            unexpected.insert(engine.tool);
        }
    }
    assert!(
        unexpected.is_empty(),
        "un motore muto in mezzo a una catena che la tabella dei guasti non \
         nomina: {unexpected:?}. Non è il guasto 31, è uno nuovo — registralo, \
         o misura le sue parole"
    );
}
