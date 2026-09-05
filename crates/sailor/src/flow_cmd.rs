//! `sailor flow`: carica i file dichiarativi da `flows/`, mostra anche quelli
//! guasti, controlla che le azioni nominate esistano ed esegue il grafo nel
//! deposito durevole comune di Sailor.

// Il formato del file vive nel crate del flusso: qui si importa, non si
// ridichiara. Averlo scritto due volte, il 28/08/2026, li ha fatti coincidere
// per fortuna e non per costruzione.
use crate::Form;
use flow::{ActionRegistry, FlowFile, Graph};
use ledger::Ledger;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use ui::gather::FlowSource;

mod beat;
mod cap_and_schedule;
mod check;
mod cost;
mod engines;
mod extensions;
mod hazards;
mod relocate;
mod run_and_resume;
#[cfg(test)]
mod test_support;

use beat::{due_flows, tick_flows, waiting_report};
use cap_and_schedule::{cap_of, schedule_of, set_cap, set_schedule};
use check::check_flow;
use cost::cost_of;
use relocate::relocate_flow;
use run_and_resume::{resume_run, run_flow};

pub use run_and_resume::{resume_run_in, resume_run_with};

pub fn run(args: &[String]) -> i32 {
    match dispatch(args, &ui::gather::flow_sources()) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor flow: {message}");
            1
        }
    }
}

fn dispatch(args: &[String], sources: &[FlowSource]) -> Result<String, String> {
    match args {
        [command] if command == "list" => list_flows(sources),
        [command, words @ ..] if command == "search" && !words.is_empty() => {
            search_flows(sources, &words.join(" "))
        }
        [command] if command == "due" => due_flows(sources),
        [command] if command == "tick" => tick_flows(sources),
        [command, name] if command == "check" => check_flow(sources, name, true),
        [command, name, flag] if command == "check" && flag == "--no-engines" => {
            check_flow(sources, name, false)
        }
        [command, name] if command == "run" => run_flow(sources, name, None),
        [command, name, text] if command == "run" => run_flow(sources, name, Some(text)),
        [command, run_id] if command == "resume" => resume_run(run_id),
        [command, name] if command == "cost" => cost_of(name),
        [command, name] if command == "relocate" => relocate_flow(sources, name, None),
        [command, name, from] if command == "relocate" => relocate_flow(sources, name, Some(from)),
        [command, name] if command == "cap" => cap_of(sources, name),
        [command, name, value] if command == "cap" => set_cap(sources, name, value),
        [command, name] if command == "schedule" => schedule_of(sources, name),
        [command, name, value] if command == "schedule" => set_schedule(sources, name, value, None),
        [command, name, value, weight] if command == "schedule" => {
            set_schedule(sources, name, value, Some(weight))
        }
        [command] if command == "publish" => crate::publish_cmd::publish_flows(sources, None),
        [command, remote] if command == "publish" => {
            crate::publish_cmd::publish_flows(sources, Some(remote))
        }
        _ => Err(usage()),
    }
}

/// I flussi che questa macchina vede, con l'origine di ciascuno.
///
/// **LA RIGA DI COMANDO E LA FINESTRA DEVONO GUARDARE NEGLI STESSI POSTI.** Fino
/// al 29/08/2026 questo comando leggeva `flows/` sotto la cartella corrente e
/// nient'altro: su una macchina appena installata rispondeva «nessun flusso
/// trovato in flows/» mentre la finestra, dallo stesso binario, ne mostrava due
/// spediti dentro di esso. Due risposte alla stessa domanda non danno un errore
/// da leggere — danno due persone che si dicono cose diverse guardando lo stesso
/// prodotto.
///
/// **UN FLUSSO SI NOMINA, NON SI PERCORRE.** Prima il nome diventava un percorso
/// e serviva un controllo perché non uscisse dalla cartella. Adesso il nome si
/// cerca in un elenco già costruito: un nome che quell'elenco non contiene non
/// apre niente, e non c'è nessun posto da cui scappare.
fn known_flows(sources: &[FlowSource]) -> Vec<(String, &'static str, Result<FlowFile, String>)> {
    ui::gather::load_all_flows(sources)
}

/// Il flusso che si chiama così, con l'origine da cui viene.
fn one_flow(sources: &[FlowSource], name: &str) -> Result<(FlowFile, &'static str), String> {
    let known = known_flows(sources);
    match known.iter().find(|(known, _, _)| known == name) {
        Some((_, origin, Ok(flow))) => Ok((flow.clone(), origin)),
        Some((_, origin, Err(reason))) => Err(catalogue::say(
            "cli.flow.does_not_load",
            &[("flow", name), ("origin", origin), ("reason", reason)],
        )),
        None => {
            let names: Vec<&str> = known.iter().map(|(name, _, _)| name.as_str()).collect();
            let in_sight = match names.is_empty() {
                true => catalogue::say("cli.flow.none_in_sight", &[]),
                false => names.join(", "),
            };
            Err(catalogue::say(
                "cli.flow.no_flow_by_that_name",
                &[("flow", name), ("names", &in_sight)],
            ))
        }
    }
}

/// Dove si è guardato, sempre in coda a un elenco vuoto: una lista vuota che non
/// dice dove ha cercato è indistinguibile da un guasto.
fn nothing_found(sources: &[FlowSource]) -> String {
    catalogue::say(
        "cli.flow.nothing_found",
        &[(
            "places",
            &sources
                .iter()
                .map(|source| format!("{}: {}", source.origin, source.dir.display()))
                .collect::<Vec<_>>()
                .join("\n  "),
        )],
    )
}

/// Le forme di `sailor flow`, una per riga.
///
/// **L'ELENCO DEI GESTI STA QUI, IN UN POSTO SOLO.** Una prova pretende che
/// ogni sottocomando che `dispatch` accetta compaia in questo elenco: un gesto
/// che il programma sa fare e nessuno sa di poter chiedere è un gesto che non
/// esiste, ed è per non trovarlo che il guasto 15 è stato aggirato con
/// `python3`.
///
/// **ED È UN `const` E NON UNA STRINGA DENTRO `usage()` PERCHÉ LA LEGGE ANCHE
/// LA FINESTRA.** Una stringa stampata da una funzione privata non è
/// interrogabile da un programma: la pagina d'aiuto della finestra sarebbe
/// stata una seconda copia che diverge alla prima opzione aggiunta.
/// `Command::usage` punta qui.
///
/// Le due regole sono nate lo stesso giorno su due rami diversi e non si
/// escludono: la prima dice che l'elenco è completo, la seconda che è uno solo.
/// `schedule` viene dalla prima, la forma a righe dalla seconda.
pub const USAGE: &[Form] = &[
    Form {
        form: "sailor flow list",
        says_key: "",
    },
    Form {
        form: "sailor flow search <words>",
        says_key: "",
    },
    Form {
        form: "sailor flow due",
        says_key: "",
    },
    Form {
        form: "sailor flow tick",
        says_key: "",
    },
    Form {
        form: "sailor flow check <name> [--no-engines]",
        says_key: "",
    },
    Form {
        form: "sailor flow run <name> [mandate]",
        says_key: "",
    },
    Form {
        form: "sailor flow resume <run>",
        says_key: "",
    },
    Form {
        form: "sailor flow cost <name>",
        says_key: "",
    },
    // **`micro`, `nessuno`, `leggero` E `pesante` RESTANO COSÌ, E NON È UNA
    // DIMENTICANZA.** Non sono segnaposto: sono le parole che l'utente batte
    // davvero e che il codice confronta, e una `schedule` già scritta le
    // conserva nel deposito. Tradurle qui senza toccare il parser farebbe
    // mentire l'aiuto; tradurle in tutti e due i posti romperebbe le
    // pianificazioni già registrate — che è la stessa ragione per cui gli `id`
    // dei flussi restano in italiano (`AGENTS.md`, la riga sui dati del
    // deposito). Se un giorno si vogliono in inglese, la strada è accettarle
    // in tutte e due le lingue e mostrare la nuova, mai sostituirle.
    Form {
        form: "sailor flow cap <name> [micros|none]",
        says_key: "",
    },
    Form {
        form: "sailor flow schedule <name> [3600s|07:30|none] [light|heavy]",
        says_key: "",
    },
    Form {
        form: "sailor flow relocate <name> [prefix-to-strip]",
        says_key: "",
    },
    Form {
        form: "sailor flow publish [remote]",
        says_key: "",
    },
];

/// The flows that mention the words, best first, with the line that matched.
fn search_flows(sources: &[FlowSource], query: &str) -> Result<String, String> {
    let hits = actions::search::rank_flows(&known_flows(sources), query)?;
    if hits.is_empty() {
        return Ok(catalogue::say("cli.flow.search_nothing", &[("query", query)]));
    }
    let mut lines = vec![catalogue::say(
        "cli.flow.search_found",
        &[("count", &hits.len().to_string()), ("query", query)],
    )];
    for hit in &hits {
        lines.push(format!(
            "  {} · {}\n      {}",
            hit["flow"].as_str().unwrap_or_default(),
            hit["origin"].as_str().unwrap_or_default(),
            hit["excerpt"].as_str().unwrap_or_default().replace('\n', " ")
        ));
    }
    Ok(lines.join("\n"))
}

fn usage() -> String {
    format!(
        "{}\n  {}",
        catalogue::say("cli.usage_heading", &[]),
        crate::forms_as_lines(USAGE).join("\n  ")
    )
}

fn list_flows(sources: &[FlowSource]) -> Result<String, String> {
    let known = known_flows(sources);
    if known.is_empty() {
        return Ok(nothing_found(sources));
    }
    let mut report = String::new();
    // L'ORIGINE STA NELL'ELENCO, e non è ornamento: due flussi con lo stesso
    // nome in due posti sono uno solo qui dentro — vince il piu' specifico — e
    // chi non vede da dove viene quello che gira modifica l'altro.
    for (name, origin, entry) in known {
        match entry {
            Ok(flow) => {
                let _ = writeln!(
                    report,
                    "{}\t{} steps\t{origin}\t{}",
                    flow.id,
                    flow.graph.steps().len(),
                    flow.description
                );
            }
            Err(error) => {
                let _ = writeln!(report, "{name}\t{origin}\tnon caricabile: {error}");
            }
        }
    }
    let _ = write!(report, "{}", waiting_report());
    Ok(report)
}

/// Il deposito predefinito se si apre, `None` se non c'è o non si apre.
///
/// Non riporta l'errore: chi la chiama sta facendo un controllo statico, e un
/// deposito assente non è un guasto del flusso che sta guardando. Chi invece
/// deve *eseguire* apre il deposito da sé e pretende che riesca.
fn open_default_ledger() -> Option<Ledger> {
    let dir = default_ledger_dir().ok()?;
    if !dir.exists() {
        return None;
    }
    Ledger::open(&dir).ok()
}

fn missing_actions(graph: &Graph, registry: &ActionRegistry) -> BTreeSet<String> {
    graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.clone())
        .collect()
}

fn default_ledger_dir() -> Result<PathBuf, String> {
    ledger::default_directory()
        .ok_or_else(|| "HOME non è definita: non so dove aprire il deposito".to_owned())
}

fn now_secs() -> Result<i64, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .map_err(|error| format!("l'orologio di sistema precede Unix epoch: {error}"))
}

fn new_run_id(flow_id: &str) -> Result<String, String> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| format!("{flow_id}-{}", duration.as_nanos()))
        .map_err(|error| format!("l'orologio di sistema precede Unix epoch: {error}"))
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use registry::default_registry;
    use std::fs;

    #[test]
    fn list_keeps_an_unloadable_flow_visible_with_its_reason() {
        let directory = TestDirectory::new();
        directory.write("buono.flow.json", &flow_json("shell_check", "[]", "{}"));
        directory.write("rotto.flow.json", "{ non-json");

        let report = list_flows(&[FlowSource {
            origin: "di prova",
            dir: directory.0.clone(),
        }])
        .expect("elencare i flussi");

        assert!(report.contains("prova\t1 steps\tdi prova"), "{report}");
        assert!(
            report.contains("rotto\tdi prova\tnon caricabile:"),
            "{report}"
        );
        assert!(report.contains("rotto.flow.json"), "{report}");
    }

    #[test]
    fn a_cycle_is_rejected_while_loading_the_file() {
        let json = r#"{
            "id": "ciclo",
            "description": "non deve caricarsi",
            "graph": {
                "steps": [
                    {"id":"a","deps":["b"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}},
                    {"id":"b","deps":["a"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}}
                ]
            },
            "inputs": {}
        }"#;

        let error =
            serde_json::from_str::<FlowFile>(json).expect_err("il ciclo deve essere rifiutato");

        assert!(error.to_string().contains("backward dependency"), "{error}");
    }

    /// **OGNI GESTO CHE `dispatch` SA FARE È SCRITTO NELL'USO.**
    ///
    /// Un comando che il programma esegue e che nessuno sa di poter chiedere è
    /// un comando che non esiste: chi non lo trova esce dal sistema, ed è
    /// esattamente come il guasto 15 è successo — `python3` al posto di un
    /// gesto che nessuno sapeva di avere. La riga dell'uso è l'unica interfaccia
    /// di chi sta al terminale.
    ///
    /// **SI LEGGE IL SORGENTE INVECE DI ESEGUIRE, E LA RAGIONE È IL GUASTO 5.**
    /// Chiamare `dispatch` per ogni parola farebbe aprire a `cost` e a `resume`
    /// il deposito **di questa macchina**: una prova che legge lo stato di chi
    /// la esegue diventa rossa per una pulizia, a codice invariato. Qui si
    /// contano i bracci dov'è scritto quali sono.
    ///
    /// Il mutante che la fa cadere è aggiungere un braccio a `dispatch` senza
    /// nominarlo in `usage()` — cioè il modo in cui un gesto diventa invisibile.
    #[test]
    fn every_arm_of_the_dispatcher_is_written_in_the_usage_line() {
        let source = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/flow_cmd.rs");
        let text = fs::read_to_string(&source).expect("questo file si rilegge");
        let body = text
            .split_once("fn dispatch(")
            .and_then(|(_, after)| after.split_once("\nfn "))
            .map(|(body, _)| body)
            .expect("il corpo di dispatch");

        let mut arms: BTreeSet<String> = BTreeSet::new();
        for piece in body.split("command == \"").skip(1) {
            let word = piece
                .split_once('"')
                .map(|(word, _)| word.to_owned())
                .expect("una parola fra virgolette");
            arms.insert(word);
        }
        assert!(
            arms.len() >= 8,
            "i bracci trovati sono troppo pochi, il modo di leggerli si è rotto: {arms:?}"
        );
        assert!(arms.contains("schedule"), "il braccio nuovo c'è: {arms:?}");

        let usage = usage();
        let missing: Vec<&String> = arms.iter().filter(|arm| !usage.contains(*arm)).collect();
        assert!(
            missing.is_empty(),
            "questi gesti esistono e non sono scritti da nessuna parte: {missing:?}\n{usage}\n\
             Un gesto che nessuno sa di poter chiedere è un gesto che non c'è, e chi \
             non lo trova esce da Sailor per farlo a mano"
        );
    }

    /// I FLUSSI DI CHI USA SAILOR NON SONO UNA FIXTURE. Fino al 28/08/2026
    /// questa prova includeva `flows/prima-corsa.flow.json` a tempo di
    /// compilazione: il giorno in cui la cartella dei flussi è stata svuotata —
    /// un gesto legittimo di chi usa il programma — **il crate ha smesso di
    /// compilare**. Una batteria non può dipendere dai dati dell'utente.
    ///
    /// Quello che la prova voleva dire resta, e vale per tutti: ogni flusso
    /// presente si carica nella forma decisa e non nomina azioni che il motore
    /// non sa eseguire. Una cartella vuota non è un fallimento — non c'è niente
    /// da verificare — ma non si spaccia per una verifica riuscita: il
    /// conteggio si stampa, così chi legge il verde sa su quanti file è passato.
    #[test]
    fn every_flow_on_disk_loads_and_names_only_registered_actions() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../flows");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            println!("nessuna cartella dei flussi: niente da verificare");
            return;
        };
        let mut checked = 0;
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.to_string_lossy().ends_with(".flow.json") {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("leggere il flusso");
            let flow: FlowFile = serde_json::from_str(&text)
                .unwrap_or_else(|e| panic!("{} non si carica: {e}", path.display()));
            let unknown = missing_actions(&flow.graph, &default_registry(None, None));
            assert!(
                unknown.is_empty(),
                "{} nomina azioni che il motore non conosce: {unknown:?}",
                path.display()
            );
            checked += 1;
        }
        println!("flussi verificati: {checked}");
    }

    /// La forma decisa del file, su una fixture nostra: qui la prova deve
    /// fallire se cambia il formato, non se qualcuno cancella un file suo.
    #[test]
    fn the_decided_file_shape_still_loads() {
        let inputs = r#"{"solo":{"command":"true","env":{},"timeout_secs":1}}"#;
        let json = flow_json("shell_check", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare la forma decisa");
        assert_eq!(flow.graph.steps().len(), 1);
        assert!(missing_actions(&flow.graph, &default_registry(None, None)).is_empty());
    }

    /// UN NOME NON DIVENTA PIÙ UN PERCORSO, e la protezione cambia di natura:
    /// prima `../segreto` veniva unito alla cartella e serviva un controllo che
    /// lo rifiutasse; adesso il nome si cerca in un elenco già costruito, quindi
    /// non apre niente perché non c'è niente che si chiami così. La prova resta
    /// perché la garanzia deve restare: nessun nome deve poter far leggere un
    /// file che non è un flusso di questa macchina.
    #[test]
    fn a_name_that_is_not_a_known_flow_opens_nothing() {
        let directory = TestDirectory::new();
        directory.write("buono.flow.json", &flow_json("shell_check", "[]", "{}"));
        let sources = [FlowSource {
            origin: "di prova",
            dir: directory.0.clone(),
        }];

        for name in [
            "../segreto",
            "cartella/segreto",
            "",
            "..",
            "buono.flow.json",
        ] {
            let refused = one_flow(&sources, name).expect_err(&format!(
                "«{name}» non è un flusso di questa macchina e non deve aprirsi"
            ));
            assert!(refused.contains("no flow is called"), "«{name}»: {refused}");
        }
        assert!(
            one_flow(&sources, "buono").is_ok(),
            "il flusso vero si apre"
        );
    }

    /// I FLUSSI SPEDITI SI VEDONO ANCHE DALLA RIGA DI COMANDO. Il difetto che
    /// questa prova esiste per prendere: `sailor flow list` rispondeva «nessun
    /// flusso» su una macchina appena installata mentre la finestra, dallo
    /// stesso binario, ne mostrava due.
    #[test]
    fn the_command_line_sees_the_shipped_flows_too() {
        let report = list_flows(&[FlowSource::builtin()]).expect("elencare i flussi");
        for (name, _) in flow::system::FLOWS {
            assert!(report.contains(name), "manca «{name}» in:\n{report}");
        }
        assert!(report.contains("built in"), "{report}");
    }
}
