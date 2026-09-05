//! `sailor flow relocate`: strips a tree's prefix from the paths a flow wrote down.

use flow::FlowFile;
use serde_json::Value;
use std::path::{Path, PathBuf};
use ui::gather::FlowSource;

use super::hazards::{hardcoded_paths, POSITION_FIELDS};
use super::run_and_resume::workspace_root;

/// Toglie da un flusso i percorsi assoluti che stanno sotto la radice.
///
/// **PERCHÉ LO FA UN COMANDO E NON UNO SCRIPT.** È il guasto 15: il 29/08/2026
/// per cambiare l'innesco di un flusso è stato usato uno script Python che
/// riscriveva il JSON, perché `sailor flow` aveva solo `list`, `due`, `check`
/// e `run`. Uno strumento che si aggira non registra niente di ciò che gli
/// succede intorno.
///
/// **RISCRIVE I CAMPI, NON I PROMPT.** Un `workdir` è un campo: il suo valore
/// ha un significato solo per il programma, e sostituirlo è una traduzione. Il
/// testo di un prompt è un'istruzione scritta da una persona per un'altra
/// intelligenza: riscriverlo è riscrivere l'istruzione, e nessuno ha chiesto a
/// questo comando di farlo. Quelli li **stampa** e basta.
///
/// **IL PREFISSO SI PUÒ DICHIARARE, PERCHÉ IL CASO NORMALE È UN ALTRO ALBERO.**
/// Un flusso da spostare quasi sempre nomina la copia su cui è stato scritto —
/// un altro clone, o la macchina di qualcun altro — e quel percorso **non sta
/// sotto** la radice di chi lo sta spostando. Senza dirlo, il comando non può
/// sapere se `/Users/tizio/progetto` volesse dire «la radice» o una cartella
/// vera che deve restare dov'è: indovinare qui vorrebbe dire riscrivere un
/// percorso legittimo. Si dichiara come secondo argomento — posizionale come
/// il mandato di `run`, che è la forma di questa riga di comando — e quello che
/// non combacia si vede nel rapporto invece di sparire.
pub(super) fn relocate_flow(sources: &[FlowSource], name: &str, from: Option<&str>) -> Result<String, String> {
    let root = workspace_root().ok_or_else(|| {
        catalogue::say(
            "cli.flow.no_workspace_root_here",
            &[("marker", flow::workspace::MARKER)],
        )
    })?;
    // Il prefisso da togliere: quello dichiarato, o la radice stessa quando il
    // flusso è stato scritto proprio qui.
    let old_root = from.map(PathBuf::from).unwrap_or_else(|| root.clone());
    let path = flow_file_path(sources, name)?;
    let text = std::fs::read_to_string(&path).map_err(|error| {
        catalogue::say(
            "cli.flow.cannot_read_file",
            &[("path", &path.display().to_string()), ("error", &error.to_string())],
        )
    })?;
    // Si lavora sul documento grezzo e non sul `FlowFile` tipato: un flusso può
    // avere campi che questa versione non conosce, e riscriverlo dal tipo li
    // perderebbe in silenzio — è il guasto 8 applicato a un file dell'utente.
    let mut document: Value = serde_json::from_str(&text).map_err(|error| {
        catalogue::say(
            "cli.flow.not_valid_json",
            &[("path", &path.display().to_string()), ("error", &error.to_string())],
        )
    })?;

    let (mut moved, mut left_alone) = relocate_workdirs(&mut document, &old_root).ok_or_else(|| {
        catalogue::say(
            "cli.flow.no_steps_to_relocate",
            &[("path", &path.display().to_string())],
        )
    })?;
    let (from_inputs, kept) = relocate_declared_inputs(&mut document, &old_root);
    moved.extend(from_inputs);
    left_alone.extend(kept);

    if !moved.is_empty() {
        let mut rewritten = serde_json::to_string_pretty(&document).map_err(|error| {
            catalogue::say(
                "cli.flow.cannot_recompose_flow",
                &[("error", &error.to_string())],
            )
        })?;
        rewritten.push('\n');
        std::fs::write(&path, rewritten).map_err(|error| {
            catalogue::say(
                "cli.flow.cannot_write_file",
                &[("path", &path.display().to_string()), ("error", &error.to_string())],
            )
        })?;
    }

    let mut report = catalogue::say(
        "cli.flow.relocate_heading",
        &[
            ("root", &root.display().to_string()),
            ("prefix", &old_root.display().to_string()),
            ("path", &path.display().to_string()),
        ],
    );
    let moved_said = if moved.is_empty() {
        catalogue::say("cli.flow.no_field_moved", &[])
    } else {
        format!("{}\n  {}", moved.len(), moved.join("\n  "))
    };
    report.push_str(&catalogue::say(
        "cli.flow.fields_moved",
        &[("moved", &moved_said)],
    ));
    if !left_alone.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.outside_the_prefix_left_alone",
            &[("fields", &left_alone.join("; "))],
        ));
    }
    // I percorsi dentro i testi si mostrano e non si toccano: chi legge decide.
    let flow: FlowFile = serde_json::from_str(&text).map_err(|error| {
        catalogue::say(
            "cli.flow.not_a_valid_flow",
            &[("path", &path.display().to_string()), ("error", &error.to_string())],
        )
    })?;
    let in_text: Vec<String> = hardcoded_paths(&flow)
        .iter()
        .filter(|found| !found.fatal)
        .map(|found| format!("{} in «{}» ({})", found.step, found.field, found.value))
        .collect();
    if !in_text.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.paths_inside_text_to_fix_by_hand",
            &[
                ("count", &in_text.len().to_string()),
                ("paths", &in_text.join("\n  ")),
            ],
        ));
    }
    Ok(report)
}

/// The same, on the inputs a person writes by hand.
///
/// **THE CURE LOOKS WHERE THE JUDGE LOOKS**: `hardcoded_paths` reads the
/// declared inputs too, so a flow could be refused for a field this command
/// never walked. A field equal to the root becomes `.` and is not removed:
/// an input is read by name, and taking it away changes what arrives.
fn relocate_declared_inputs(document: &mut Value, old_root: &Path) -> (Vec<String>, Vec<String>) {
    let mut moved = Vec::new();
    let mut left_alone = Vec::new();
    let Some(inputs) = document.get_mut("inputs").and_then(Value::as_object_mut) else {
        return (moved, left_alone);
    };
    for (name, declared) in inputs.iter_mut() {
        walk_inputs_for_places(name, declared, old_root, &mut moved, &mut left_alone);
    }
    (moved, left_alone)
}

fn walk_inputs_for_places(
    name: &str,
    value: &mut Value,
    old_root: &Path,
    moved: &mut Vec<String>,
    left_alone: &mut Vec<String>,
) {
    let Some(fields) = value.as_object_mut() else {
        return;
    };
    for (key, inner) in fields.iter_mut() {
        if !POSITION_FIELDS.contains(&key.as_str()) {
            walk_inputs_for_places(name, inner, old_root, moved, left_alone);
            continue;
        }
        let Value::String(declared) = inner else {
            continue;
        };
        if !(declared.starts_with('/') || declared.starts_with("~/")) {
            continue;
        }
        match relative_to(old_root, declared) {
            Some(rest) => {
                let rest = if rest.is_empty() { ".".to_owned() } else { rest };
                moved.push(format!("{name}: «{declared}» → «{rest}»"));
                *inner = Value::String(rest);
            }
            None => left_alone.push(format!("{name}: «{declared}»")),
        }
    }
}

/// Toglie il prefisso dai `workdir` del documento, senza toccare il disco.
///
/// Sta separata dal comando perché è la parte che si può provare: quella
/// intorno legge la cartella corrente e scrive un file, e una prova che
/// cambiasse la cartella del processo rovinerebbe le altre che girano insieme
/// — è il guasto 21, che qui si evita non avendo bisogno del processo.
///
/// Torna `None` se il documento non ha nemmeno un elenco di passi.
fn relocate_workdirs(document: &mut Value, old_root: &Path) -> Option<(Vec<String>, Vec<String>)> {
    let mut moved = Vec::new();
    let mut left_alone = Vec::new();
    let steps = document
        .get_mut("graph")
        .and_then(|graph| graph.get_mut("steps"))
        .and_then(Value::as_array_mut)?;
    for step in steps {
        let step_id = step
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| catalogue::say("cli.flow.step_without_id", &[]), str::to_owned);
        let Some(with) = step.get_mut("with").and_then(Value::as_object_mut) else {
            continue;
        };
        // Solo un testo: un `{"$from": …}` è un rinvio, e va risolto a
        // esecuzione da chi sa contro cosa. Riscriverlo sarebbe inventare.
        let Some(Value::String(declared)) = with.get(WORKDIR_KEY).cloned() else {
            continue;
        };
        match relative_to(old_root, &declared) {
            // Coincide con la radice: il campo non serve più, e un `workdir`
            // che vale la radice è rumore che invita a riscriverlo assoluto.
            Some(rest) if rest.is_empty() => {
                // **`shift_remove` E NON `remove`.** Con `preserve_order`
                // acceso — e lo è — `remove` è uno *swap*: tira l'ultima
                // chiave dentro il buco e riordina il file. Un comando che
                // toglie un campo e in cambio rimescola l'oggetto produce un
                // diff illeggibile, e chi lo rilegge non distingue più ciò che
                // è stato deciso da ciò che è stato spostato. Misurato il
                // 31/08/2026: 62 righe cambiate al posto di 7.
                with.shift_remove(WORKDIR_KEY);
                moved.push(catalogue::say(
                    "cli.flow.workdir_removed_was_the_root",
                    &[("step", &step_id)],
                ));
            }
            Some(rest) => {
                with.insert(WORKDIR_KEY.to_owned(), Value::String(rest.clone()));
                moved.push(format!("{step_id}: «{declared}» → «{rest}»"));
            }
            // Fuori dal prefisso: non è questo comando a decidere cosa voleva
            // dire chi l'ha scritto.
            None => left_alone.push(format!("{step_id}: «{declared}»")),
        }
    }
    Some((moved, left_alone))
}

/// Il file da cui viene un flusso, cercato nelle sorgenti che sono cartelle.
fn flow_file_path(sources: &[FlowSource], name: &str) -> Result<PathBuf, String> {
    // Si guarda dalla più specifica alla meno: è quella che vince a esecuzione,
    // e riscrivere una che non gira lascerebbe il guasto dov'era.
    for source in sources.iter().rev() {
        if source.is_builtin() {
            continue;
        }
        let candidate = source.dir.join(format!("{name}.flow.json"));
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(catalogue::say(
        "cli.flow.flow_is_not_a_file_on_disk",
        &[("name", name)],
    ))
}

/// Il resto di `path` sotto `root`, o `None` se non ci sta sotto.
///
/// Restituisce la stringa vuota quando i due coincidono: è il caso in cui il
/// campo va tolto, non riscritto.
fn relative_to(root: &Path, path: &str) -> Option<String> {
    let candidate = Path::new(path);
    let rest = candidate.strip_prefix(root).ok()?;
    Some(rest.display().to_string())
}

/// Il campo che dice dove un passo lavora. Il nome sta nel crate del flusso:
/// due costanti con lo stesso valore in due crate sono il guasto 10 in piccolo.
const WORKDIR_KEY: &str = flow::WORKDIR_FIELD;

#[cfg(test)]
mod tests {
    use super::*;

    // ── spostare un flusso da un albero all'altro ─────────────────────

    fn document_with_workdir(workdir: &str) -> Value {
        serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "external_engine",
                "max_attempts": 1, "when": null,
                "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                // **`workdir` NON È IL PENULTIMO, E LA POSIZIONE È LA PROVA.**
                // Togliendo il penultimo campo, lo swap e lo scorrimento danno
                // lo stesso ordine: una fixture così lascia passare il difetto.
                // Qui ne restano due dopo, quindi i due modi divergono.
                "with": {
                    "tool": "git", "workdir": workdir,
                    "timeout_secs": 5, "args": ["status"]
                }
            }]},
            "inputs": {}
        })
    }

    /// Coincide con la radice: il campo sparisce. Tenerlo come `"."` sarebbe
    /// rumore che invita il prossimo a riscriverlo assoluto.
    #[test]
    fn a_workdir_equal_to_the_root_is_removed() {
        let mut document = document_with_workdir("/vecchio/albero");

        let (moved, left) =
            relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert_eq!(moved.len(), 1);
        assert!(left.is_empty());
        assert!(document["graph"]["steps"][0]["with"]
            .get("workdir")
            .is_none());
    }

    /// Sotto la radice: resta il pezzo relativo, che è ciò che rende il flusso
    /// eseguibile su un clone qualunque.
    #[test]
    fn a_workdir_under_the_root_keeps_only_the_rest() {
        let mut document = document_with_workdir("/vecchio/albero/desktop");

        relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert_eq!(document["graph"]["steps"][0]["with"]["workdir"], "desktop");
    }

    /// **TOGLIERE UN CAMPO NON DEVE RIORDINARE IL FILE.** Con `preserve_order`
    /// acceso `Map::remove` è uno *swap*: tira l'ultima chiave dentro il buco.
    /// Misurato il 31/08/2026 sul flusso vero: 62 righe cambiate invece di 7,
    /// e un diff in cui non si distingue più ciò che è stato deciso da ciò che
    /// è stato spostato.
    #[test]
    fn removing_a_workdir_does_not_reorder_the_other_fields() {
        let mut document = document_with_workdir("/vecchio/albero");

        relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        let keys: Vec<&str> = document["graph"]["steps"][0]["with"]
            .as_object()
            .expect("un oggetto")
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            vec!["tool", "timeout_secs", "args"],
            "l'ordine resta quello: uno swap metterebbe «args» prima di «timeout_secs»"
        );
    }

    /// Fuori dal prefisso: si lascia stare e si dice. Indovinare che
    /// `/altro/posto` volesse dire «la radice» vorrebbe dire riscrivere un
    /// percorso che qualcuno aveva messo lì apposta.
    #[test]
    fn a_workdir_outside_the_prefix_is_left_alone_and_reported() {
        let mut document = document_with_workdir("/altro/posto");

        let (moved, left) =
            relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert!(moved.is_empty());
        assert_eq!(left.len(), 1);
        assert_eq!(
            document["graph"]["steps"][0]["with"]["workdir"],
            "/altro/posto"
        );
    }

    /// **UN RINVIO NON SI RISCRIVE.** `{"$from": "/innesco/text"}` è un
    /// puntatore che si risolve a esecuzione contro l'ingresso vero: qui non
    /// c'è niente da spostare, e toccarlo vorrebbe dire inventare.
    #[test]
    fn a_workdir_that_is_a_reference_is_never_touched() {
        let mut document = serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": [{
                "id": "unico", "deps": [], "action": "external_engine",
                "max_attempts": 1, "when": null,
                "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                "with": {"workdir": {"$from": "/innesco/text"}}
            }]},
            "inputs": {}
        });

        let (moved, left) =
            relocate_workdirs(&mut document, Path::new("/vecchio/albero")).expect("ha dei passi");

        assert!(moved.is_empty() && left.is_empty());
        assert_eq!(
            document["graph"]["steps"][0]["with"]["workdir"],
            serde_json::json!({"$from": "/innesco/text"})
        );
    }

    /// **THE CURE MUST REACH WHERE THE REFUSAL POINTS.** `flow check` refuses a
    /// flow for an absolute place field in the declared inputs and names
    /// `flow relocate` as the way out; relocate walked only the steps, so on
    /// this machine it printed a report, changed nothing, exited zero, and the
    /// check stayed red on the very field it had named.
    #[test]
    fn a_place_field_in_the_declared_inputs_is_relocated_too() {
        let mut document = serde_json::json!({
            "id": "prova", "description": "d",
            "graph": {"steps": []},
            "inputs": {
                "riferimenti": {
                    "workdir": "/vecchio/albero/pagina",
                    "root_path": "/vecchio/albero/pagina",
                    "brief": "un testo che nomina /vecchio/albero e resta com'è"
                },
                "altrove": {"workdir": "/una/casa/fuori"},
                "sulla-radice": {"workdir": "/vecchio/albero"}
            }
        });

        let (moved, left) = relocate_declared_inputs(&mut document, Path::new("/vecchio/albero"));

        assert_eq!(document["inputs"]["riferimenti"]["workdir"], "pagina");
        // AN INPUT IS READ BY NAME: the field on the root becomes «.», never
        // taken away, or the flow would receive one key less than it declares.
        assert_eq!(document["inputs"]["sulla-radice"]["workdir"], ".");
        // Outside the prefix, and not a place field: neither is this command's
        // to decide.
        assert_eq!(document["inputs"]["altrove"]["workdir"], "/una/casa/fuori");
        assert_eq!(
            document["inputs"]["riferimenti"]["root_path"],
            "/vecchio/albero/pagina"
        );
        assert!(document["inputs"]["riferimenti"]["brief"]
            .as_str()
            .expect("il testo")
            .contains("/vecchio/albero"));
        assert_eq!(moved.len(), 2, "spostati: {moved:?}");
        assert_eq!(left.len(), 1, "lasciati: {left:?}");
    }
}
