//! Le scritture della tela: creare, modificare, cancellare un flusso.
//!
//! **QUI NON VIVE PIÙ NESSUNA CONOSCENZA SUL DISCO.** Dove stanno i file, quale
//! nome è sicuro, come si sostituisce un file senza farlo vedere a metà: tutto
//! questo è passato in `flow::system` il 31/08/2026, perché `sailor flow cap`
//! deve riscrivere un `.flow.json` e questo modulo sta **fuori dal workspace
//! Rust** — la riga di comando non lo può chiamare. Le due copie che ne
//! sarebbero nate sono il guasto 10.
//!
//! Quel che resta è ciò che appartiene davvero al guscio: prendere il JSON che
//! arriva dalla tela, farlo passare per la validazione del motore
//! (`flow::FlowFile`, e con lui `flow::Graph::validate`) e rifiutare un flusso
//! che nomina azioni che il motore non conosce — `registry::default_registry`,
//! la stessa lista che usa `sailor flow check`, mai una copia riscritta qui.
//!
//! **QUESTA FRASE ERA GIÀ SCRITTA, E FINO AL 01/09/2026 ERA FALSA.** Diceva
//! «`actions::register_default` / `register_store`», che sono due righe delle
//! sedici del registro: il guscio ne era la quinta copia. Il modo in cui una
//! regola smette di essere un'affermazione e diventa una difesa è che una
//! prova la interroghi — qui `the_window_knows_every_action_the_engine_can_run`.

use flow::{ActionRegistry, FlowFile};
use std::path::Path;

use ui::gather::{default_ledger_dir, ledger_present};

/// Il comando che la tela chiama per creare o modificare un flusso.
#[tauri::command]
pub(crate) fn save_flow(flow: serde_json::Value) -> Result<(), String> {
    // L'identificativo si legge qui solo per sapere DOVE scrivere. Che sia
    // valido lo decide `save_flow_in`, che ha già tutte le sue regole: un id
    // assente o non testuale finisce nella cartella dei flussi nuovi e viene
    // rifiutato là, col messaggio giusto, invece di essere rifiutato qui con un
    // messaggio peggiore.
    let id = flow.get("id").and_then(|id| id.as_str()).unwrap_or_default();
    save_flow_in(&super::flows_dir_for(id), flow)
}

/// Il comando che la tela chiama per cancellare un flusso.
#[tauri::command]
pub(crate) fn delete_flow(name: String) -> Result<(), String> {
    delete_flow_in(&super::flows_dir_for(&name), &name)
}

/// Cuore di `save_flow`, con la cartella passata invece che letta
/// dall'ambiente: le prove scrivono in una cartella usa-e-getta, non fra i
/// flussi veri.
///
/// Qui c'era il numero di quei flussi scritto a mano — «i quattordici flussi
/// veri» — e affermava il falso. Non si aggiorna: si toglie. Un conteggio
/// copiato in un commento invecchia da solo, e `docs/decisioni.md` lo vieta
/// proprio per questo — dove un fatto è già registrato, il testo ci rimanda
/// invece di copiarlo. Il numero lo dice `sailor flow list`, che li conta tutti
/// e tre i posti da cui vengono.
fn save_flow_in(flows_dir: &Path, flow_json: serde_json::Value) -> Result<(), String> {
    // Deserializzare `FlowFile` richiama `Graph::try_from`, che chiama
    // `Graph::validate`: cicli, dipendenze mancanti o incompatibili sono
    // rifiutati qui dentro, non ricontrollati a mano.
    let flow: FlowFile = serde_json::from_value(flow_json)
        .map_err(|error| format!("il flusso non supera la validazione del motore: {error}"))?;
    reject_unknown_actions(&flow)?;
    flow::system::save_in(flows_dir, &flow)
}

/// Cuore di `delete_flow`, stessa ragione della cartella passata a mano.
fn delete_flow_in(flows_dir: &Path, name: &str) -> Result<(), String> {
    flow::system::delete_in(flows_dir, name)
}

/// Le azioni note al motore, per rifiutare al salvataggio un flusso che una
/// corsa vera respingerebbe con `azione sconosciuta`. Il deposito entra nel
/// registro solo se esiste già — una verifica statica non deve crearne uno,
/// per la stessa ragione di `sailor flow check`.
fn action_registry() -> ActionRegistry {
    let ledger_dir = default_ledger_dir();
    let ledger = ledger_present(&ledger_dir)
        .then(|| ledger::Ledger::open(&ledger_dir).ok())
        .flatten();
    action_registry_with(ledger)
}

/// Il registro **senza deposito**: la forma che una prova può costruire senza
/// leggere la macchina di chi la esegue (guasto 5).
#[cfg(test)]
fn action_registry_without_deposit() -> ActionRegistry {
    action_registry_with(None)
}

/// **LA LISTA È UNA SOLA, E FINO AL 01/09/2026 QUESTA ERA LA QUINTA COPIA.**
///
/// Qui stavano tre righe scelte a mano — `register_default`, `trigger`, e lo
/// `store` se il deposito esiste — mentre `crates/registry` ne registra
/// sedici. Il risultato è che la finestra **rifiutava al salvataggio** cinque
/// azioni che dal terminale girano: `detect_tools`, `tool_needs`,
/// `history_ask`, `subflow` e — la più grave — `handed_to_agent`, cioè il
/// passo che consegna il lavoro a chi è già vivo nel terminale. Una funzione
/// centrale del prodotto non si poteva comporre dalla finestra.
///
/// È il guasto 10 alla quinta occorrenza, e stavolta il commento in testa al
/// file dichiarava già la regola giusta — «la stessa lista che usa `sailor
/// flow check`, mai una copia riscritta qui» — sopra il codice che la
/// violava. Una regola scritta e non verificata non è una difesa.
fn action_registry_with(ledger: Option<ledger::Ledger>) -> ActionRegistry {
    registry::default_registry(ledger, None)
}

fn reject_unknown_actions(flow: &FlowFile) -> Result<(), String> {
    let registry = action_registry();
    let missing: Vec<&str> = flow
        .graph
        .steps()
        .iter()
        .filter(|step| registry.get(&step.action).is_none())
        .map(|step| step.action.as_str())
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "il flusso usa azioni che il motore non conosce: {}",
            missing.join(", ")
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    /// Una cartella usa-e-getta per ogni prova: si vede da sola quando resta
    /// vuota, cosa che una cartella condivisa fra prove non garantirebbe.
    fn scratch_dir(label: &str) -> PathBuf {
        let sequence = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "sailor-desktop-flows-test-{label}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("creazione cartella di prova");
        dir
    }

    fn valid_flow(id: &str) -> serde_json::Value {
        json!({
            "id": id,
            "description": "flusso di prova",
            "graph": {
                "steps": [{
                    "id": "solo",
                    "deps": [],
                    "action": "shell_check",
                    "max_attempts": 1,
                    "when": null,
                    "input_schema": {"type": "any"},
                    "output_schema": {"type": "any"}
                }]
            },
            "inputs": {"solo": {"command": "true", "timeout_secs": 5}}
        })
    }

    fn entries(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("cartella leggibile")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    // ── ciò che il guscio decide da sé ──────────────────────────────────
    //
    // Le prove su nomi pericolosi, collisioni di maiuscole, sostituzione
    // atomica e cancellazione sono passate in `crates/flow/src/system.rs`
    // insieme al codice che provano — e da lì `cargo test --workspace` le
    // esegue, cosa che qui non faceva: questo modulo sta fuori dal workspace.
    // Quella qui sotto resta perché prova il **collegamento**: se un giorno
    // `save_flow_in` smettesse di passare da `flow::system::save_in`, nessuna
    // delle prove laggiù se ne accorgerebbe.

    #[test]
    fn save_flow_rejects_an_empty_id_and_writes_nothing() {
        let dir = scratch_dir("empty-id");
        let error = save_flow_in(&dir, valid_flow("")).expect_err("id vuoto rifiutato");
        assert!(error.contains("vuoto"), "{error}");
        assert!(entries(&dir).is_empty(), "nessun file deve comparire");
    }

    // ── un grafo che il motore rifiuterebbe non tocca il disco ──────────

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: `a` e `b` dipendono l'uno
    /// dall'altro. `Graph::validate` lo respinge con un ciclo. Se la
    /// validazione venisse saltata (mutante: scrivere `text` senza prima
    /// passare da `serde_json::from_value::<FlowFile>`), il file comparirebbe
    /// comunque nella cartella e questa prova diventerebbe rossa.
    #[test]
    fn save_flow_rejects_a_cyclic_graph_and_writes_nothing() {
        let dir = scratch_dir("cyclic-graph");
        let cyclic = json!({
            "id": "ciclico",
            "description": "due passi che si aspettano a vicenda",
            "graph": {
                "steps": [
                    {
                        "id": "a", "deps": ["b"], "action": "shell_check", "max_attempts": 1,
                        "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                    },
                    {
                        "id": "b", "deps": ["a"], "action": "shell_check", "max_attempts": 1,
                        "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                    }
                ]
            },
            "inputs": {}
        });
        let error = save_flow_in(&dir, cyclic).expect_err("grafo ciclico rifiutato");
        assert!(error.contains("validazione"), "{error}");
        assert!(entries(&dir).is_empty(), "un grafo rifiutato non deve toccare il disco");
    }

    #[test]
    fn save_flow_rejects_a_missing_dependency_and_writes_nothing() {
        let dir = scratch_dir("missing-dependency");
        let broken = json!({
            "id": "guasto",
            "description": "dipende da un passo che non esiste",
            "graph": {
                "steps": [{
                    "id": "solo", "deps": ["fantasma"], "action": "shell_check", "max_attempts": 1,
                    "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                }]
            },
            "inputs": {}
        });
        let error = save_flow_in(&dir, broken).expect_err("dipendenza mancante rifiutata");
        assert!(error.contains("validazione"), "{error}");
        assert!(entries(&dir).is_empty());
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: `azione-mai-registrata` non è né
    /// `shell_check` né `external_engine` né una delle azioni del deposito.
    /// Senza `reject_unknown_actions` (mutante: farla tornare sempre `Ok`) il
    /// file comparirebbe comunque, e questa prova diventerebbe rossa.
    #[test]
    fn save_flow_rejects_an_unknown_action_and_writes_nothing() {
        let dir = scratch_dir("unknown-action");
        let mut flow = valid_flow("azione-ignota");
        flow["graph"]["steps"][0]["action"] = json!("azione-mai-registrata");
        let error = save_flow_in(&dir, flow).expect_err("azione sconosciuta rifiutata");
        assert!(error.contains("azione-mai-registrata"), "{error}");
        assert!(entries(&dir).is_empty());
    }

    #[test]
    fn save_flow_accepts_the_two_actions_the_engine_always_knows() {
        let dir = scratch_dir("known-actions");
        assert!(save_flow_in(&dir, valid_flow("shell-ok")).is_ok());
        let mut with_engine = valid_flow("engine-ok");
        with_engine["graph"]["steps"][0]["action"] = json!("external_engine");
        with_engine["inputs"]["solo"] = json!({"bin": "true", "timeout_secs": 5});
        assert!(save_flow_in(&dir, with_engine).is_ok());
    }

    /// **LA FINESTRA DEVE SAPER SALVARE TUTTO CIÒ CHE IL MOTORE SA ESEGUIRE.**
    ///
    /// Il commento in testa a questo file dichiara «la stessa lista che usa
    /// `sailor flow check`, mai una copia riscritta qui». `action_registry`
    /// era invece la copia: `register_default` più due righe scelte a mano,
    /// mentre il motore passa da `registry::default_registry`. È il guasto 10
    /// per la quinta volta, e stavolta rifiutava al salvataggio dei flussi che
    /// dal terminale partono — il contrario del difetto gemello, che offriva
    /// nella tavolozza azioni che il motore non conosce.
    ///
    /// Il confronto è **senza deposito da tutte e due le parti**, o la prova
    /// leggerebbe lo stato della macchina di chi la esegue (guasto 5).
    #[test]
    fn the_window_knows_every_action_the_engine_can_run() {
        let engine = registry::default_registry(None, None);
        let window = action_registry_without_deposit();
        let known: BTreeSet<&str> = window.names().into_iter().collect();
        let missing: Vec<&str> = engine
            .names()
            .into_iter()
            .filter(|name| !known.contains(name))
            .collect();
        assert!(
            missing.is_empty(),
            "il motore sa eseguire {} azioni che la finestra rifiuta al salvataggio: {}",
            missing.len(),
            missing.join(", ")
        );
    }

    /// **LA TAVOLOZZA NON DEVE OFFRIRE NIENTE CHE IL MOTORE RIFIUTI.**
    ///
    /// Il difetto gemello del precedente, e quello che si vede a mano: fino al
    /// 01/09/2026 `ACTION_KIND` nominava sei azioni che non esistono in nessun
    /// crate — `pane_until_idle`, `signal_is_gone`, `deposit_write`,
    /// `pane_send`, `hand_to_human`, `pane_read` — e quattro erano nella
    /// cassetta dei passi: premere «attesa», «deposito», «gesto» o «a una
    /// persona» creava un nodo che poi non si salvava.
    ///
    /// L'ancora sta **fuori da tutte e due le copie**: il vocabolario della
    /// finestra si legge dal suo file e si confronta col registro del motore.
    /// Confrontare due mappe scritte a mano le lascerebbe sbagliare insieme.
    #[test]
    fn the_window_vocabulary_names_only_actions_the_engine_registers() {
        let source = fs::read_to_string(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/flow.ts"),
        )
        .expect("il vocabolario della finestra si legge da desktop/src/flow.ts");
        let named = action_names_in(&source, "const ACTION_KIND");
        assert!(
            named.len() > 4,
            "il vocabolario non è stato letto: {} nomi trovati",
            named.len()
        );
        // Col deposito **aperto**, perché sei azioni si registrano solo allora
        // e la finestra ha ragione a saperle disegnare comunque.
        let dir = scratch_dir("vocabolario");
        let ledger = ledger::Ledger::open(&dir).expect("un deposito di prova");
        let engine = registry::default_registry(Some(ledger), None);
        let known: BTreeSet<&str> = engine.names().into_iter().collect();
        let invented: Vec<&String> = named
            .iter()
            .filter(|name| !known.contains(name.as_str()))
            .collect();
        assert!(
            invented.is_empty(),
            "la finestra nomina {} azioni che il motore non registra: {:?}",
            invented.len(),
            invented
        );

        // E l'altro verso, che è come si nascondeva il difetto peggiore:
        // `kindOf` ripiega su «verifica» per un nome che non conosce, quindi
        // i sette passi `trigger` dei flussi veri si disegnavano come nodi di
        // controllo, in silenzio e senza che niente diventasse rosso.
        let undrawn: Vec<&str> = engine
            .names()
            .into_iter()
            .filter(|name| !named.iter().any(|k| k == name))
            .collect();
        assert!(
            undrawn.is_empty(),
            "il motore registra {} azioni che la finestra non sa disegnare, e che \
             ricadrebbero in silenzio su «verifica»: {}",
            undrawn.len(),
            undrawn.join(", ")
        );
    }

    /// I nomi di azione dentro un blocco `const NOME: … = { chiave: valore }`
    /// del sorgente della finestra. Sta qui e non in un lettore generico
    /// perché è una lettura sola e deve restare leggibile: se un giorno il
    /// blocco cambia forma, la prova sopra fallisce sul conto minimo invece di
    /// passare su un elenco vuoto.
    fn action_names_in(source: &str, header: &str) -> Vec<String> {
        let Some(start) = source.find(header) else {
            return Vec::new();
        };
        let body = &source[start..];
        let Some(open) = body.find('{') else {
            return Vec::new();
        };
        let Some(close) = body.find("};") else {
            return Vec::new();
        };
        body[open + 1..close]
            .lines()
            .filter_map(|line| line.split(':').next())
            .map(|name| name.trim().trim_matches('"').to_string())
            .filter(|name| !name.is_empty() && !name.starts_with("//"))
            .collect()
    }

    /// La cancellazione passa dallo stesso posto: se il guscio smettesse di
    /// delegare, questa riga se ne accorgerebbe.
    #[test]
    fn delete_flow_reports_a_flow_that_was_never_written() {
        let dir = scratch_dir("delete-missing");
        let error = delete_flow_in(&dir, "mai-esistito").expect_err("cancellazione di un assente");
        assert!(error.contains("non esiste"), "{error}");
    }
}
