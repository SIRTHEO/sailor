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
    let file_name = format!("{id}.flow.json");
    reject_a_name_that_collides_only_by_case(flows_dir, &file_name)?;
    let target = flows_dir.join(&file_name);
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

/// DUE NOMI CHE DIFFERISCONO SOLO PER LE MAIUSCOLE SONO LO STESSO FILE, e il
/// disco non lo dice. Su APFS come lo installa macOS — e su Windows — salvare
/// «mioflusso» sopra un «MioFlusso» esistente non dà nessun errore: sostituisce
/// il contenuto e lascia il nome vecchio. Chi salva crede di aver creato un
/// flusso nuovo, e ne ha cancellato un altro.
///
/// Il controllo non sta in `safe_flow_id`, che giudica il nome da solo: qui
/// serve guardare cosa c'è già nella cartella. E si nega invece di scegliere
/// per conto di chi salva — «volevi sovrascrivere quello?» è una domanda che
/// deve fare la finestra, non un file system.
fn reject_a_name_that_collides_only_by_case(
    flows_dir: &Path,
    file_name: &str,
) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(flows_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let existing = entry.file_name();
        let existing = existing.to_string_lossy();
        if existing.as_ref() != file_name && existing.eq_ignore_ascii_case(file_name) {
            return Err(format!(
                "esiste già «{existing}», che su questo disco è lo stesso file di \
                 «{file_name}»: scrivendolo lo sostituiresti senza accorgertene. \
                 Scegli un altro nome, o modifica quello che c'è."
            ));
        }
    }
    Ok(())
}

/// Le azioni note al motore, per rifiutare al salvataggio un flusso che una
/// corsa vera respingerebbe con `azione sconosciuta`. Il deposito entra nel
/// registro solo se esiste già — una verifica statica non deve crearne uno,
/// per la stessa ragione di `sailor flow check`.
fn action_registry() -> ActionRegistry {
    let mut registry = ActionRegistry::default();
    actions::register_default(&mut registry);
    // Senza questa riga un flusso che comincia con un nodo di innesco non si
    // salva: il pannello lo rifiuterebbe come «azione sconosciuta» pur essendo
    // un flusso che il motore esegue.
    trigger::register_default(&mut registry);
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

    /// IL FILE SYSTEM NON DICE CHE SONO LO STESSO FILE. Su APFS come lo
    /// installa macOS, salvare «MioFlusso» sopra un «mioflusso» esistente
    /// sostituisce il contenuto senza un errore e lascia il nome vecchio: chi
    /// salva crede di aver creato un flusso, e ne ha cancellato un altro.
    #[test]
    fn save_flow_refuses_a_name_that_differs_only_by_case() {
        let dir = scratch_dir("case-collision");
        save_flow_in(&dir, valid_flow("MioFlusso")).expect("il primo si scrive");

        let error = save_flow_in(&dir, valid_flow("mioflusso"))
            .expect_err("il secondo deve essere rifiutato");
        assert!(error.contains("MioFlusso"), "{error}");

        // E quello che c'era resta intero: il rifiuto non deve aver toccato
        // niente, che è il motivo per cui esiste.
        let written = fs::read_to_string(dir.join("MioFlusso.flow.json")).expect("il primo c'è");
        assert!(written.contains("\"MioFlusso\""), "{written}");
        assert_eq!(entries(&dir).len(), 1);
    }

    /// Riscrivere lo stesso flusso, con lo stesso nome, deve continuare a
    /// passare: la difesa è contro i nomi che collidono, non contro il
    /// salvataggio.
    #[test]
    fn saving_the_same_name_twice_is_still_allowed() {
        let dir = scratch_dir("same-name-twice");
        save_flow_in(&dir, valid_flow("stesso-nome")).expect("prima scrittura");
        save_flow_in(&dir, valid_flow("stesso-nome")).expect("seconda scrittura");
        assert_eq!(entries(&dir).len(), 1);
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
