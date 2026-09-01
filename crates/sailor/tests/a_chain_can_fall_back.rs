//! Una catena di motori serve solo se il ripiego può scattare.
//!
//! **IL GUASTO 31.** Un passo che scrive `"tool": ["claude-code", "agy",
//! "codex"]` sembra avere due ripieghi. Ne ha quanti sono i motori che
//! **dichiarano come dicono di non poter lavorare**: `says_it_cannot_work` su
//! un elenco vuoto è `false`, quindi un motore che tace fa morire il passo sul
//! proprio fallimento e i motori dopo di lui non partono mai. `agy` tace, e
//! stava **in mezzo** a dodici catene di questo albero: `codex`, che dichiara
//! il proprio 401, non partiva mai da nessuna di esse.
//!
//! **CHIUSO IL 01/09/2026, E NON MISURANDO `agy`.** La regola ha due modi di
//! essere rispettata — misurare le parole di chi sta in mezzo, oppure non
//! mettere in mezzo chi non le ha — e il secondo non chiede nessun dato che non
//! esista. `agy` sta in fondo; la regola la scrive
//! `toolbox::Descriptor::cannot_be_a_fallback`, in un posto solo, e la legge
//! anche `sailor flow check` sui flussi di chi lo lancia.
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

/// Perché un motore spedito non può fare da ripiego, se non può.
///
/// **LA REGOLA NON È SCRITTA QUI**, ed è la differenza che conta: sta in
/// `toolbox::Descriptor::cannot_be_a_fallback`, in un posto solo, e da lì la
/// legge anche `sailor flow check` sui flussi e sui descrittori di chi lancia.
/// Una copia scritta dentro questa prova avrebbe sorvegliato i quattro flussi di
/// questo albero e nessuno di quelli di nessun altro.
fn why_it_cannot_be_a_fallback(tool: &str) -> Option<String> {
    let catalog = toolbox::Catalog::load(&[toolbox::descriptor::Source::Builtin]);
    catalog
        .live()
        .into_iter()
        .find(|loaded| loaded.descriptor.id == tool)
        .and_then(|loaded| loaded.descriptor.cannot_be_a_fallback())
}

/// **LA REGOLA, E DA OGGI GIRA.**
///
/// Ogni motore che compare in una catena e **non è l'ultimo** deve dichiarare
/// almeno una parola con cui dice di non poter lavorare. Chi non la dichiara
/// non è un ripiego: è un tappo.
///
/// **PERCHÉ ERA `#[ignore]`, E PERCHÉ NON LO È PIÙ.** La ragione scritta qui
/// fino al 01/09/2026 era buona e resta vera: le parole con cui `agy` dice di
/// aver finito la quota non sono mai state viste su questa macchina, e
/// inventarle sarebbe peggio del difetto. Ma quella era la ragione per non
/// **inventare un dato**, ed era diventata la ragione per non **avere un
/// controllo**: sono due cose diverse, e la seconda non discende dalla prima. La
/// regola ha due modi di essere rispettata — misurare le parole di chi sta in
/// mezzo, oppure non mettere in mezzo chi non le ha — e il secondo non richiede
/// nessuna misura che non esista. Le catene di questo albero mettevano `agy`
/// **fra** `claude-code` e `codex`: `codex`, che dichiara il proprio 401, non
/// partiva mai. Adesso `agy` sta in fondo, dove la sua reticenza non toglie il
/// lavoro a nessuno.
///
/// **QUELLO CHE RESTA APERTO, E VA DETTO QUI PERCHÉ QUI SI LEGGE.** Un `agy`
/// esaurito **in fondo** a una catena fa ancora morire il passo invece di dire
/// «sono esaurito»: si vede nel motivo dell'errore, non nel comportamento, e
/// nessun ripiego si perde perché dietro di lui non c'è nessuno. Il giorno in
/// cui qualcuno lo vede dire di essere esaurito, si scrivono le sue parole nel
/// descrittore e torna a poter stare in mezzo.
#[test]
fn every_engine_that_is_not_last_in_a_chain_says_how_it_is_exhausted() {
    let mut silent: BTreeSet<String> = BTreeSet::new();
    for engine in engines_in_chains() {
        if engine.last {
            continue;
        }
        if let Some(why) = why_it_cannot_be_a_fallback(&engine.tool) {
            silent.insert(format!(
                "{} · {} · {}: {why}",
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

/// **E LA REGOLA NON SI RISPETTA SVUOTANDO LE CATENE.** Ogni motore che dichiara
/// le proprie parole deve poter continuare a stare in mezzo: se
/// `cannot_be_a_fallback` cominciasse a rispondere «no» a tutti, la prova qui
/// sopra resterebbe verde su catene che non ripiegano più.
#[test]
fn an_engine_that_declares_its_words_is_still_allowed_in_the_middle() {
    let engines = engines_in_chains();
    let in_the_middle: BTreeSet<String> = engines
        .iter()
        .filter(|engine| !engine.last)
        .map(|engine| engine.tool.clone())
        .collect();
    assert!(
        in_the_middle.contains("claude-code") && in_the_middle.contains("codex"),
        "i due motori che dichiarano come si esauriscono non stanno più in mezzo a \
         nessuna catena: la regola è stata rispettata togliendo i ripieghi invece \
         che dichiarandoli. Trovati: {in_the_middle:?}"
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

/// **UN MOTORE CHE NON RIPIEGA HA ANCORA UN POSTO, E DEVE RESTARE IN FONDO.**
///
/// La misura di quanto la regola stringe, oggi: `agy` è nelle catene di questo
/// albero, e ci sta come ultimo. Fino al 01/09/2026 questa prova sorvegliava un
/// **elenco di eccezioni** — «nessun motore muto oltre a quello registrato» —
/// che è la forma che prende una regola quando la si scrive prima di poterla
/// rispettare. Adesso la regola non ha eccezioni, e ciò che resta da sorvegliare
/// è l'opposto: che `agy` non sia stato tolto di mezzo cancellandolo, perché un
/// ripiego in meno non è la cura di un ripiego che non scatta.
#[test]
fn the_engine_that_cannot_fall_back_is_still_used_last() {
    let engines = engines_in_chains();
    let last: Vec<&InChain> = engines
        .iter()
        .filter(|engine| engine.tool == "agy" && engine.last)
        .collect();
    assert!(
        !last.is_empty(),
        "«agy» non compare più in fondo a nessuna catena: la regola sul ripiego è \
         stata rispettata togliendo un motore invece di spostarlo"
    );
    assert!(
        why_it_cannot_be_a_fallback("agy").is_some(),
        "«agy» dichiara adesso come si esaurisce: se la misura è stata fatta, \
         questa prova non serve più e le catene possono rimetterlo in mezzo"
    );
}
