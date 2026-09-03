//! I flussi spediti col prodotto, provati eseguendoli.
//!
//! **PERCHÉ QUI E NON NEL CRATE DEL FLUSSO.** `crates/flow` sa che i due file
//! incorporati sono flussi validi, e lo prova. Non sa se le azioni che nominano
//! esistono: il vocabolario lo compone il programma, un pezzo per crate, e
//! l'unico posto dove si vede intero è questo. Un flusso spedito che nomina
//! un'azione che nessuno registra è il guasto peggiore possibile per un
//! prodotto — chi lo installa non può ripararlo, perché il file sta dentro il
//! binario — e deve cadere qui, prima di uscire di casa.
//!
//! **E POI SI ESEGUONO DAVVERO.** Un flusso che si carica e non gira è un file
//! JSON ben scritto. Queste prove costruiscono lo stesso registro di
//! `sailor flow run`, eseguono i due flussi in un deposito in memoria e
//! guardano cosa hanno prodotto.

use flow::system;
use flow::{
    ActionRegistry, Clock, Decision, Execution, ExecutionRequest, Executor, FlowError, FlowFile,
    InMemoryRecordStore, InProcessExecutor, Outcome, RecordStore, SharedState,
};
use serde_json::Value;
use std::collections::BTreeMap;

/// Lo stesso vocabolario che compone `sailor flow`, meno i nodi del deposito.
///
/// I due nodi di `store` nascono da un deposito aperto, e aprire un deposito
/// **crea file sul disco**: una prova che li volesse dovrebbe scrivere. Nessuno
/// dei flussi spediti li nomina, e la prova qui sotto è esattamente ciò che
/// impedisce che qualcuno ce li metta senza accorgersene.
fn product_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    toolbox::register_default(&mut registry);
    toolbox::register_needs(&mut registry);
    trigger::register_default(&mut registry);
    registry.register(
        actions::EXTERNAL_ENGINE_ACTION,
        actions::ExternalEngineAction::resolving_with(toolbox::Tools::current()),
    );
    registry.register(
        actions::handoff::HANDED_TO_AGENT_ACTION,
        actions::handoff::HandoffAction::new(),
    );
    registry
}

/// Un orologio finto che avanza di uno a ogni domanda. Il contatore è atomico
/// perché ora l'orologio è condiviso da più fili: un `i64` mutabile qui non
/// compilerebbe, ed è la stessa ragione per cui il tratto chiede `&self`.
struct Tick(std::sync::atomic::AtomicI64);

impl Tick {
    fn new(start: i64) -> Self {
        Tick(std::sync::atomic::AtomicI64::new(start))
    }
}

impl Clock for Tick {
    fn now(&self) -> Result<i64, FlowError> {
        Ok(self.0.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1)
    }
}

fn shipped(name: &str) -> FlowFile {
    system::builtin_registry()
        .remove(name)
        .unwrap_or_else(|| panic!("il flusso spedito «{name}» non c'è"))
        .expect("il flusso spedito si carica")
}

fn run(flow: &FlowFile) -> (Execution, Vec<flow::StepRecord>) {
    let mut store = InMemoryRecordStore::default();
    let run_id = format!("prova-{}", flow.id);
    let request = ExecutionRequest {
        run_id: run_id.clone(),
        root_inputs: flow.inputs.clone().into_iter().collect(),
        gates: Vec::new(),
        shared: SharedState::new(),
        spend_cap_micros: None,
    };
    let execution = InProcessExecutor
        .execute(
            &flow.graph,
            request,
            &mut store,
            &product_registry(),
            &mut Tick::new(0),
        )
        .expect("l'esecuzione non deve rompersi");
    let records = store.records(&run_id).expect("le tracce della corsa");
    (execution, records)
}

/// L'uscita di un passo, o il motivo per cui non ce n'è una.
fn output_of(records: &[flow::StepRecord], step: &str) -> Value {
    let record = records
        .iter()
        .filter(|record| record.step_id == step)
        .max_by_key(|record| (record.attempt, record.epoch))
        .unwrap_or_else(|| panic!("il passo «{step}» non ha lasciato traccia"));
    assert_eq!(
        record.outcome,
        Some(Outcome::Went),
        "il passo «{step}» non è andato a buon fine: {:?}",
        record.failure_class
    );
    record
        .output
        .clone()
        .unwrap_or_else(|| panic!("il passo «{step}» non ha prodotto niente"))
}

// ── il vocabolario ───────────────────────────────────────────────────────

/// IL GUASTO CHE QUESTA PROVA ESISTE PER PRENDERE: un flusso spedito che nomina
/// un'azione che il programma non registra. Chi lo installa vedrebbe il flusso
/// nell'elenco, lo lancerebbe, e riceverebbe «azione mancante» su un file che
/// non può correggere perché sta dentro il binario.
#[test]
fn every_action_named_by_a_shipped_flow_is_in_the_vocabulary() {
    let registry = product_registry();
    for (name, entry) in system::builtin_registry() {
        let flow = entry.expect("il flusso spedito si carica");
        for step in flow.graph.steps() {
            assert!(
                registry.get(&step.action).is_some(),
                "il flusso spedito «{name}» chiede l'azione «{}» al passo «{}», \
                 che nessun crate registra",
                step.action,
                step.id
            );
        }
    }
}

/// I flussi spediti non devono nominare un binario: girerebbero solo dove quel
/// nome è nel percorso di chi esegue, e un flusso di sistema deve girare su una
/// macchina qualunque.
#[test]
fn no_shipped_flow_names_a_binary() {
    for (name, entry) in system::builtin_registry() {
        let flow = entry.expect("il flusso spedito si carica");
        for step in flow.graph.steps() {
            let named = step
                .with
                .as_ref()
                .and_then(|with| with.get("bin"))
                .or_else(|| flow.inputs.get(&step.id).and_then(|input| input.get("bin")));
            assert!(
                named.is_none(),
                "il flusso spedito «{name}» nomina un binario al passo «{}»",
                step.id
            );
        }
    }
}

/// **NESSUN FLUSSO SPEDITO PORTA UN PERCORSO DI UNA MACCHINA SOLA.**
///
/// La garanzia c'era e stava nel posto sbagliato: sorvegliava i flussi di
/// sviluppo di questo progetto, che dall'01/09/2026 non stanno più nel repo —
/// sono andati dove vanno i flussi di chiunque, nella casa di Sailor. Quelli
/// erano nostri e potevano permettersi un percorso assoluto; **questi vengono
/// installati su macchine che non conosciamo**, ed è qui che la regola serve.
#[test]
fn no_shipped_flow_carries_a_path_from_one_machine() {
    for (name, _) in system::FLOWS {
        let text = system::FLOWS
            .iter()
            .find(|(id, _)| id == name)
            .map(|(_, body)| *body)
            .expect("il flusso spedito ha un corpo");
        for home in ["/Users/", "/home/", "C:\\Users\\"] {
            assert!(
                !text.contains(home),
                "il flusso spedito «{name}» porta un percorso di una macchina sola ({home}): \
                 su qualunque altra non parte"
            );
        }
    }
}

// ── il primo flusso: cosa c'è qui, e cosa manca ──────────────────────────

/// SI ESEGUE, E LA SECONDA META' È LA RISPOSTA. Il rilevamento da solo è un
/// elenco; il passo dopo dice quali strumenti i flussi di questa macchina
/// chiedono, e quali di quelli non ci sono.
#[test]
fn the_tools_flow_runs_and_answers_which_flows_would_stop() {
    let flow = shipped("what-this-machine-has");
    let (execution, records) = run(&flow);

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "la corsa deve chiudersi: {:?}",
        execution.decisions
    );

    let detected = output_of(&records, "rileva");
    let total = detected["total"]
        .as_u64()
        .expect("un conto dei descrittori");
    assert!(
        total >= 30,
        "il rilevamento ha guardato solo {total} descrittori: il catalogo spedito ne ha molti di più"
    );

    let answer = output_of(&records, "cosa-chiedono-i-flussi");
    let flows_seen = answer["flows_seen"].as_u64().expect("un conto dei flussi");
    assert!(
        flows_seen >= 2,
        "i due flussi spediti si vedono sempre, e ne ho visti {flows_seen}"
    );
    let report = answer["report"].as_str().expect("una risposta da leggere");
    assert!(
        report.contains("flows read in"),
        "la risposta deve essere leggibile da una persona: {report}"
    );
    // GLI STRUMENTI CHIESTI SONO UN INSIEME SOLO: uno strumento non può essere
    // insieme presente e mancante, e chi legge due elenchi che si sovrappongono
    // installa due volte la stessa cosa.
    let names = |key: &str| -> Vec<String> {
        answer[key]
            .as_array()
            .expect("un elenco")
            .iter()
            .map(|need| need["tool"].as_str().expect("un nome").to_owned())
            .collect()
    };
    let mut all = names("present");
    all.extend(names("missing"));
    all.extend(names("unknown"));
    let mut unique = all.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(
        all.len(),
        unique.len(),
        "uno strumento sta in un elenco solo"
    );
}

/// «NON CHIESTA» NON È «NON C'È»: chi spegne i comandi di versione deve vedere
/// ogni versione diventare una domanda non fatta, non una risposta vuota.
#[test]
fn the_tools_flow_asks_for_versions_only_if_told_to() {
    let flow = shipped("what-this-machine-has");
    let asked = flow.inputs["rileva"]["version_probes"]
        .as_bool()
        .expect("il flusso dichiara se chiedere le versioni");
    assert!(
        asked,
        "il flusso di sistema chiede le versioni: è la parte del rilevamento che \
         costa, e spegnerla di nascosto renderebbe l'elenco più povero senza dirlo"
    );
}

// ── il secondo flusso: cosa automatizzi già ──────────────────────────────

/// SI ESEGUE, E LE QUATTRO FAMIGLIE SONO IL VERDETTO. Ogni passo guarda una
/// famiglia di automazioni, e il nome del passo dice cosa se ne potrebbe fare.
#[test]
fn the_migration_flow_runs_and_looks_at_four_families() {
    let flow = shipped("migrate-to-sailor");
    let (execution, records) = run(&flow);

    assert_eq!(
        execution.decisions.last(),
        Some(&Decision::Complete),
        "la corsa deve chiudersi: {:?}",
        execution.decisions
    );

    let steps = [
        "ganci",
        "pianificate",
        "script-sparsi",
        "viste-ma-non-lette",
    ];
    let mut families: Vec<String> = Vec::new();
    for step in steps {
        let output = output_of(&records, step);
        let findings = output["findings"].as_array().expect("un elenco");
        assert!(
            !findings.is_empty(),
            "il passo «{step}» non ha guardato niente: un descrittore che non trova \
             nulla lascia comunque la riga che dice dove ha guardato"
        );
        for finding in findings {
            families.push(finding["family"].as_str().expect("una famiglia").to_owned());
        }
    }
    families.sort();
    families.dedup();
    assert_eq!(
        families,
        vec![
            "automation_hook",
            "automation_opaque",
            "automation_schedule",
            "automation_script"
        ],
        "ogni passo guarda la sua famiglia e nessun'altra"
    );
}

/// GUARDARE NON DEVE VOLER DIRE AVVIARE. Questo flusso legge la configurazione
/// personale di chi lo esegue: se un descrittore del catalogo dichiarasse un
/// comando di versione, il rilevamento **eseguirebbe** un programma per il solo
/// fatto di essere stato guardato.
#[test]
fn the_migration_flow_never_runs_anything() {
    let flow = shipped("migrate-to-sailor");
    let (_, records) = run(&flow);

    for step in [
        "ganci",
        "pianificate",
        "script-sparsi",
        "viste-ma-non-lette",
    ] {
        for finding in output_of(&records, step)["findings"]
            .as_array()
            .expect("un elenco")
        {
            assert_eq!(
                finding["version"]["state"], "notasked",
                "il passo «{step}» ha interrogato «{}»",
                finding["name"]
            );
        }
    }
}

/// LE AUTOMAZIONI ALTRUI NON SONO STRUMENTI CHE UN PASSO PUÒ INVOCARE. Stanno in
/// un catalogo separato apposta: se finissero in quello degli strumenti, il loro
/// identificativo comparirebbe fra quelli che Sailor propone a chi ne ha scritto
/// uno sbagliato, e un passo potrebbe nominarne uno come se fosse un binario.
#[test]
fn the_automations_catalog_does_not_leak_into_the_tools() {
    let tools = toolbox::Tools::current();
    let automations: BTreeMap<String, ()> =
        toolbox::Catalog::load(&[toolbox::Source::BuiltinNamed("automations".to_owned())])
            .live()
            .into_iter()
            .map(|loaded| (loaded.descriptor.id.clone(), ()))
            .collect();

    assert!(
        !automations.is_empty(),
        "il catalogo delle automazioni non è vuoto"
    );
    for id in automations.keys() {
        assert!(
            !tools.declares(id),
            "«{id}» è un'automazione e compare fra gli strumenti invocabili"
        );
    }
}

/// Un nome di catalogo sbagliato deve diventare una segnalazione, non un elenco
/// vuoto: i due si leggono uguale — «qui non c'è niente» — e uno dei due è un
/// errore di chi ha scritto il passo.
#[test]
fn a_misspelled_catalog_is_a_problem_not_an_empty_list() {
    let catalog =
        toolbox::Catalog::load(&[toolbox::Source::BuiltinNamed("automazioni".to_owned())]);
    assert!(catalog.live().is_empty());
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    assert!(
        catalog.problems[0].reason.contains("automations"),
        "la segnalazione deve dire quali cataloghi esistono: {}",
        catalog.problems[0].reason
    );
}

/// LA REGOLA CHE VIETA DI ESEGUIRE STA NEL CATALOGO, NON SOLO NEL FLUSSO — e
/// questa prova esiste perché il 29/08/2026 non c'era.
///
/// Il flusso di migrazione spegne i comandi di versione con
/// `"version_probes": false`, e finché lo fa nessuno esegue niente. Ma quella
/// riga è una scelta scritta in un file, e chi sovrascrive il flusso con uno
/// suo se la perde senza saperlo. Misurato aggiungendo un `version` a un
/// descrittore del catalogo delle automazioni: **nessuna prova diventava
/// rossa**. Questo catalogo guarda la configurazione personale di chi lo
/// esegue, e non deve poter avviare niente per costruzione, non per
/// gentilezza di chi scrive il passo.
#[test]
fn no_automation_descriptor_may_run_anything() {
    let catalog =
        toolbox::Catalog::load(&[toolbox::Source::BuiltinNamed("automations".to_owned())]);
    assert!(!catalog.live().is_empty(), "il catalogo non è vuoto");
    for loaded in catalog.live() {
        assert!(
            loaded.descriptor.version.is_none(),
            "«{}» dichiara un comando di versione: guardare le automazioni di una \
             persona diventerebbe eseguire qualcosa sulla sua macchina",
            loaded.descriptor.id
        );
    }
}

// ── il cuore dell'incrocio, su flussi costruiti qui ──────────────────────

/// L'INCROCIO STESSO, PROVATO SU FLUSSI NOSTRI — e questa prova esiste perché il
/// 29/08/2026 non c'era.
///
/// Le prove che eseguono il flusso di sistema guardano la macchina vera, dove i
/// due flussi spediti non chiedono nessuno strumento: svuotando la raccolta
/// degli strumenti chiesti — misurato, rompendola apposta — **nessuna diventava
/// rossa**, perché «zero strumenti chiesti» è la risposta giusta su una macchina
/// pulita e quella sbagliata qui. Qui i flussi li scriviamo noi, quindi la
/// risposta è nota e la prova poteva venire diversa.
///
/// I tre casi che non vanno confusi stanno tutti in un colpo solo: uno strumento
/// che c'è, uno che nessun descrittore dichiara — che non si ripara installando
/// niente — e un passo che nomina un binario, che nessun elenco di strumenti
/// mancanti potrebbe mai vedere.
#[test]
fn the_crossing_says_who_asked_for_what() {
    let dir = std::env::temp_dir().join(format!("sailor-incrocio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di prova");
    std::fs::write(
        dir.join("chiede.flow.json"),
        serde_json::json!({
            "id": "chiede",
            "description": "un flusso che chiede",
            "graph": { "steps": [
                {"id": "con-strumento", "deps": [], "action": "external_engine", "max_attempts": 1,
                 "when": null, "with": {"tool": "codex"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "con-nome-inventato", "deps": [], "action": "external_engine", "max_attempts": 1,
                 "when": null, "with": {"tool": "arnese-che-non-esiste"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}},
                {"id": "con-binario", "deps": [], "action": "shell_check", "max_attempts": 1,
                 "when": null, "with": {"bin": "echo"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}}
            ]},
            "inputs": {}
        })
        .to_string(),
    )
    .expect("scrittura del flusso");

    let registry = product_registry();
    let action = registry.get("tool_needs").expect("l'azione è registrata");
    let outcome = action
        .execute(
            &serde_json::json!({
                "include_default_sources": false,
                "flows_dirs": [dir.to_string_lossy()],
                "findings": [{
                    "name": "codex",
                    "family": "ai_cli",
                    "label": "Codex",
                    "descriptor_id": "codex",
                    "descriptor_source": "incorporato",
                    "presence": {"state": "present", "reason": "trovato"},
                    "executable": "/da/qualche/parte/codex",
                    "version": {"state": "notasked", "detail": "non chiesta"},
                    "config": [],
                    "note": "si installa così"
                }]
            }),
            &mut SharedState::new(),
        )
        .expect("l'azione non deve rompersi");

    let flow::ActionOutcome::Went(answer) = outcome else {
        panic!("l'azione deve produrre una risposta");
    };
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(answer["flows_seen"], 1);
    assert_eq!(answer["present"][0]["tool"], "codex");
    assert_eq!(answer["present"][0]["asked_by"][0], "chiede/con-strumento");
    assert_eq!(
        answer["present"][0]["executable"],
        "/da/qualche/parte/codex"
    );
    assert_eq!(answer["missing"].as_array().expect("un elenco").len(), 0);
    assert_eq!(answer["unknown"][0]["tool"], "arnese-che-non-esiste");
    assert_eq!(
        answer["unknown"][0]["asked_by"][0],
        "chiede/con-nome-inventato"
    );
    assert_eq!(answer["steps_naming_a_binary"][0], "chiede/con-binario");

    let report = answer["report"].as_str().expect("una risposta da leggere");
    for atteso in [
        "codex",
        "arnese-che-non-esiste",
        "chiede/con-strumento",
        "chiede/con-binario",
    ] {
        assert!(report.contains(atteso), "manca «{atteso}» in:\n{report}");
    }
}

/// UNO STRUMENTO RILEVATO E ASSENTE NON È UNO STRUMENTO SCONOSCIUTO, e le due
/// riparazioni sono opposte: la prima si installa, la seconda si scrive. Un
/// elenco che le mescola manda a installare un nome che non esiste.
#[test]
fn a_tool_that_is_absent_is_not_a_tool_that_is_unknown() {
    let dir = std::env::temp_dir().join(format!("sailor-assente-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("cartella di prova");
    std::fs::write(
        dir.join("chiede.flow.json"),
        serde_json::json!({
            "id": "chiede",
            "description": "un flusso che chiede uno strumento non installato",
            "graph": { "steps": [
                {"id": "passo", "deps": [], "action": "external_engine", "max_attempts": 1,
                 "when": null, "with": {"tool": "docker"},
                 "input_schema": {"type": "any"}, "output_schema": {"type": "any"}}
            ]},
            "inputs": {}
        })
        .to_string(),
    )
    .expect("scrittura del flusso");

    let registry = product_registry();
    let outcome = registry
        .get("tool_needs")
        .expect("l'azione è registrata")
        .execute(
            &serde_json::json!({
                "include_default_sources": false,
                "flows_dirs": [dir.to_string_lossy()],
                "findings": [{
                    "name": "docker",
                    "family": "tool",
                    "label": "Docker",
                    "descriptor_id": "docker",
                    "descriptor_source": "incorporato",
                    "presence": {"state": "absent", "reason": "nessun `docker` nelle cartelle del percorso"},
                    "version": {"state": "notasked", "detail": "non è qui"},
                    "config": [],
                    "note": "si prende da docker.com"
                }]
            }),
            &mut SharedState::new(),
        )
        .expect("l'azione non deve rompersi");

    let flow::ActionOutcome::Went(answer) = outcome else {
        panic!("l'azione deve produrre una risposta");
    };
    let _ = std::fs::remove_dir_all(&dir);

    assert_eq!(answer["unknown"].as_array().expect("un elenco").len(), 0);
    assert_eq!(answer["missing"][0]["tool"], "docker");
    assert_eq!(answer["missing"][0]["note"], "si prende da docker.com");
    let report = answer["report"].as_str().expect("una risposta");
    assert!(report.contains("MISSING HERE"), "{report}");
    assert!(report.contains("si prende da docker.com"), "{report}");
}

/// UN INGRESSO SENZA RILEVAMENTO È UN ERRORE DI CHI HA SCRITTO IL PASSO, non un
/// elenco vuoto: senza `findings` ogni strumento chiesto sembrerebbe sconosciuto,
/// e chi legge andrebbe a scrivere descrittori per strumenti che ha installati.
#[test]
fn without_a_detection_the_step_refuses_instead_of_guessing() {
    let registry = product_registry();
    let error = registry
        .get("tool_needs")
        .expect("l'azione è registrata")
        .execute(&serde_json::json!({}), &mut SharedState::new())
        .expect_err("senza rilevamento il passo non può rispondere");
    assert_eq!(error.class, "invalid_input");
}
