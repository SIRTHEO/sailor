//! Le scritture della tela: creare, modificare, cancellare un flusso.
//!
//! La sola conoscenza che vive qui è dove stanno i file e come proteggerli da
//! un nome che esce dalla cartella o da una scrittura a metà. Se un flusso è
//! accettabile lo decide il motore — `flow::FlowFile` che si deserializza
//! attraverso la stessa validazione di `flow::Graph`, e `actions::register_default`
//! / `register_store`, la stessa lista di azioni note che usa `sailor flow check`
//! — mai una copia riscritta qui.

use flow::{ActionRegistry, FlowFile};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ui::gather::{default_ledger_dir, ledger_present};

/// Il comando che la tela chiama per creare o modificare un flusso.
#[tauri::command]
pub(crate) fn save_flow(flow: serde_json::Value) -> Result<(), String> {
    save_flow_in(&super::default_flows_dir(), flow)
}

/// Il comando che la tela chiama per cancellare un flusso.
#[tauri::command]
pub(crate) fn delete_flow(name: String) -> Result<(), String> {
    delete_flow_in(&super::default_flows_dir(), &name)
}

/// Cuore di `save_flow`, con la cartella passata invece che letta
/// dall'ambiente: le prove scrivono in una cartella usa-e-getta, non nei
/// quattordici flussi veri.
fn save_flow_in(flows_dir: &Path, flow_json: serde_json::Value) -> Result<(), String> {
    // Deserializzare `FlowFile` richiama `Graph::try_from`, che chiama
    // `Graph::validate`: cicli, dipendenze mancanti o incompatibili sono
    // rifiutati qui dentro, non ricontrollati a mano.
    let flow: FlowFile = serde_json::from_value(flow_json)
        .map_err(|error| format!("il flusso non supera la validazione del motore: {error}"))?;
    let id = safe_flow_id(&flow.id)?;
    reject_unknown_actions(&flow)?;

    fs::create_dir_all(flows_dir)
        .map_err(|error| format!("non riesco a preparare la cartella dei flussi: {error}"))?;
    let target = flows_dir.join(format!("{id}.flow.json"));
    let text = serde_json::to_string_pretty(&flow)
        .map_err(|error| format!("non riesco a comporre il flusso in JSON: {error}"))?;
    write_atomically(&target, text.as_bytes())
}

/// Cuore di `delete_flow`, stessa ragione della cartella passata a mano.
fn delete_flow_in(flows_dir: &Path, name: &str) -> Result<(), String> {
    let id = safe_flow_id(name)?;
    let target = flows_dir.join(format!("{id}.flow.json"));
    match fs::remove_file(&target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(format!("il flusso \"{name}\" non esiste"))
        }
        Err(error) => Err(format!(
            "non riesco a cancellare {}: {error}",
            target.display()
        )),
    }
}

/// Un id che uscirebbe dalla cartella dei flussi (vuoto, con `/` o `\`, o con
/// `..`) è un percorso di attraversamento: si nega, non si ripulisce in
/// silenzio — chi guarda la finestra deve vedere che il nome è stato rifiutato.
fn safe_flow_id(id: &str) -> Result<&str, String> {
    if id.is_empty() {
        return Err("il nome del flusso non può essere vuoto".to_owned());
    }
    if id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err(format!(
            "\"{id}\" non è un nome di flusso sicuro: niente separatori di percorso"
        ));
    }
    Ok(id)
}

/// Le azioni note al motore, per rifiutare al salvataggio un flusso che una
/// corsa vera respingerebbe con `azione sconosciuta`. Il deposito entra nel
/// registro solo se esiste già — una verifica statica non deve crearne uno,
/// per la stessa ragione di `sailor flow check`.
fn action_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    let ledger_dir = default_ledger_dir();
    if ledger_present(&ledger_dir) {
        if let Ok(ledger) = ledger::Ledger::open(&ledger_dir) {
            actions::store::register_store(&mut registry, ledger);
        }
    }
    registry
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

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Scrittura atomica: file temporaneo accanto al bersaglio, poi `rename`. Chi
/// rilegge la cartella (la finestra, o una corsa) non deve poter vedere un
/// file a metà scritto — `rename` sullo stesso filesystem è indivisibile,
/// una `write` diretta sul bersaglio no.
fn write_atomically(target: &Path, contents: &[u8]) -> Result<(), String> {
    let temp_path = temp_path_for(target);
    fs::write(&temp_path, contents).map_err(|error| {
        format!(
            "non riesco a scrivere il file temporaneo {}: {error}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, target).map_err(|error| {
        let _ = fs::remove_file(&temp_path);
        format!("non riesco a sostituire {}: {error}", target.display())
    })
}

fn temp_path_for(target: &Path) -> PathBuf {
    let file_name = target
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("flow");
    let unique = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    target.with_file_name(format!(
        ".{file_name}.tmp-{}-{unique}",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
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

    // ── l'id non può uscire dalla cartella ──────────────────────────────

    #[test]
    fn save_flow_rejects_an_empty_id_and_writes_nothing() {
        let dir = scratch_dir("empty-id");
        let error = save_flow_in(&dir, valid_flow("")).expect_err("id vuoto rifiutato");
        assert!(error.contains("vuoto"), "{error}");
        assert!(entries(&dir).is_empty(), "nessun file deve comparire");
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: senza il controllo su `..` questo
    /// id scriverebbe fuori dalla cartella dei flussi, in `dir/..` cioè nel suo
    /// genitore. Tolto il controllo in `safe_flow_id` la prova diventa rossa
    /// perché il file compare nel genitore invece di essere rifiutato.
    #[test]
    fn save_flow_rejects_an_id_that_climbs_out_of_the_flows_directory() {
        let dir = scratch_dir("traversal");
        // Il bersaglio dell'evasione è fuori dalla cartella usa-e-getta della
        // prova: va ripulito prima e dopo, o un mutante che la lascia passare
        // sporca `$TMPDIR` per i giri successivi invece di farsi vedere qui.
        let escaped = dir.parent().expect("la prova ha un genitore").join("evaso.flow.json");
        let _ = fs::remove_file(&escaped);

        let error =
            save_flow_in(&dir, valid_flow("../evaso")).expect_err("id con .. rifiutato");
        assert!(error.contains("percorso"), "{error}");
        assert!(entries(&dir).is_empty(), "la cartella dei flussi resta vuota");
        assert!(
            !escaped.exists(),
            "il flusso non deve essere uscito dalla cartella"
        );
        let _ = fs::remove_file(&escaped);
    }

    #[test]
    fn save_flow_rejects_an_id_with_a_path_separator() {
        let dir = scratch_dir("slash-id");
        let error =
            save_flow_in(&dir, valid_flow("sotto/cartella")).expect_err("id con / rifiutato");
        assert!(error.contains("percorso"), "{error}");
        assert!(entries(&dir).is_empty());
    }

    #[test]
    fn delete_flow_rejects_an_id_that_climbs_out_of_the_flows_directory() {
        let dir = scratch_dir("delete-traversal");
        let error = delete_flow_in(&dir, "../evaso").expect_err("id con .. rifiutato");
        assert!(error.contains("percorso"), "{error}");
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

    // ── la scrittura sostituisce davvero il contenuto, in modo atomico ──

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: la seconda scrittura usa una
    /// descrizione diversa dalla prima. Un mutante che salti `fs::rename` (o
    /// che scriva sempre sul file temporaneo senza mai sostituire il
    /// bersaglio) lascerebbe la prima descrizione sul disco, e questa prova
    /// diventerebbe rossa.
    #[test]
    fn save_flow_overwrites_existing_content_instead_of_leaving_it_untouched() {
        let dir = scratch_dir("overwrite");
        save_flow_in(&dir, valid_flow("stesso-id")).expect("prima scrittura riuscita");
        let mut second = valid_flow("stesso-id");
        second["description"] = json!("seconda versione, diversa dalla prima");
        save_flow_in(&dir, second).expect("seconda scrittura riuscita");

        let text = fs::read_to_string(dir.join("stesso-id.flow.json")).expect("file leggibile");
        assert!(
            text.contains("seconda versione, diversa dalla prima"),
            "il contenuto sul disco deve essere quello dell'ultima scrittura: {text}"
        );
        assert!(
            !text.contains("\"flusso di prova\""),
            "non deve restare traccia della prima descrizione: {text}"
        );
    }

    #[test]
    fn save_flow_leaves_no_temporary_file_behind() {
        let dir = scratch_dir("no-temp-leftover");
        save_flow_in(&dir, valid_flow("pulito")).expect("scrittura riuscita");
        let leftovers: Vec<String> = entries(&dir)
            .into_iter()
            .filter(|name| name.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "file temporanei rimasti: {leftovers:?}");
    }

    // ── cancellare ────────────────────────────────────────────────────

    #[test]
    fn delete_flow_removes_a_flow_that_exists() {
        let dir = scratch_dir("delete-existing");
        save_flow_in(&dir, valid_flow("da-cancellare")).expect("scrittura riuscita");
        assert!(dir.join("da-cancellare.flow.json").exists());

        delete_flow_in(&dir, "da-cancellare").expect("cancellazione riuscita");
        assert!(!dir.join("da-cancellare.flow.json").exists());
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: cancellare un flusso mai scritto
    /// deve tornare un errore, non un successo silenzioso. Un mutante che
    /// tratti `NotFound` come `Ok(())` fa sparire questo errore, e questa
    /// prova diventerebbe rossa.
    #[test]
    fn delete_flow_on_a_missing_flow_reports_the_absence_instead_of_succeeding_silently() {
        let dir = scratch_dir("delete-missing");
        let error = delete_flow_in(&dir, "mai-esistito").expect_err("cancellazione di un assente");
        assert!(error.contains("non esiste"), "{error}");
    }
}
