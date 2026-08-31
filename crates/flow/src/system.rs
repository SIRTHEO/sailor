//! Da dove vengono i flussi, e quali il prodotto si porta dietro.
//!
//! **PERCHÉ STA NEL CRATE DEL FLUSSO E NON IN QUELLO DELLA FINESTRA.** Fino al
//! 29/08/2026 la risposta a «dove sono i flussi» viveva in `ui::gather`, cioè
//! dentro chi disegna la finestra. Finché a chiederlo era solo la finestra la
//! cosa reggeva; non regge più adesso che un *passo di flusso* deve poter
//! chiedere «quali flussi vede questa macchina» per dire quali strumenti
//! servono. La stessa domanda posta da due posti diversi con due risposte
//! diverse è il difetto che `crates/flow/src/file.rs` racconta di aver già
//! pagato una volta sul formato del file. Qui c'è una risposta sola; chi la
//! vuole la importa.
//!
//! **I FLUSSI DI SISTEMA SONO INCORPORATI NEL BINARIO, NON CERCATI SU DISCO.**
//! È lo stesso schema di `toolbox::descriptor::BUILTIN`, e per la stessa
//! ragione: un binario appena installato — o copiato su un'altra macchina —
//! deve rispondere senza che nessuno abbia copiato una cartella. Non c'è un
//! percorso di installazione da indovinare, quindi non c'è un percorso di
//! installazione che possa essere sbagliato.
//!
//! **E SI SOVRASCRIVONO PER NOME, non si spengono.** Chi vuole un flusso di
//! sistema diverso ne scrive uno con lo stesso nome in casa propria o nel
//! proprio progetto, e quello vince. Il modo di cambiare un flusso spedito è
//! scrivere un flusso, che è il modo di fare qualunque cosa in Sailor.

use crate::FlowFile;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Che cosa si scrive alla voce «dove» per la sorgente di sistema.
///
/// NON È UN PERCORSO, E NON DEVE SEMBRARLO. I flussi spediti non stanno in una
/// cartella: stanno dentro il binario. Chi mostra le sorgenti mostra anche
/// questa riga, e una cartella plausibile ma inesistente manderebbe qualcuno a
/// cercare dei file lì dentro — o a crearceli, dove nessuno li leggerebbe.
pub const PLACE: &str = "(spediti col prodotto)";

/// Come si chiama la sorgente di sistema per chi legge.
pub const BUILTIN_ORIGIN: &str = "di sistema";

/// I flussi che il prodotto si porta dietro: nome del flusso e testo del file.
///
/// Il nome è quello che si vedrebbe sul disco senza `.flow.json`, e una prova
/// controlla che coincida con l'`id` dichiarato dentro: due nomi per la stessa
/// cosa vorrebbero dire che «esegui questo» e «sovrascrivi questo» parlano di
/// due flussi diversi.
pub const FLOWS: &[(&str, &str)] = &[
    (
        "strumenti-di-questa-macchina",
        include_str!("../system/strumenti-di-questa-macchina.flow.json"),
    ),
    (
        "migrazione-a-sailor",
        include_str!("../system/migrazione-a-sailor.flow.json"),
    ),
];

/// Un posto dove si cercano i flussi, con il nome che chi guarda ne vede.
///
/// `dir` per la sorgente di sistema vale [`PLACE`]: non è una cartella, ed è
/// l'unico modo di tenere un tipo solo per tutte le sorgenti senza far credere
/// che i flussi spediti si possano modificare aprendo un file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowSource {
    pub origin: &'static str,
    pub dir: PathBuf,
}

impl FlowSource {
    /// La sorgente dei flussi spediti col prodotto.
    pub fn builtin() -> FlowSource {
        FlowSource {
            origin: BUILTIN_ORIGIN,
            dir: PathBuf::from(PLACE),
        }
    }

    /// Vero se questa sorgente è quella incorporata nel binario.
    pub fn is_builtin(&self) -> bool {
        is_place(&self.dir)
    }
}

/// Vero se questo «dove» nomina i flussi incorporati invece di una cartella.
pub fn is_place(dir: &Path) -> bool {
    dir == Path::new(PLACE)
}

/// I flussi caricati da un registro: quelli validi e quelli rifiutati col
/// motivo. È la stessa forma per il disco e per l'incorporato, perché chi legge
/// non deve trattarli in modo diverso.
pub type FlowRegistry = BTreeMap<String, Result<FlowFile, String>>;

/// Tutti i posti in cui si cercano i flussi, nell'ordine in cui si guardano —
/// dal meno specifico al più specifico.
///
/// **PERCHÉ LA SORGENTE DI SISTEMA C'È SEMPRE, ANCHE CON `SAILOR_FLOWS`.** Chi
/// dichiara quella variabile sta dicendo dove stanno *i suoi* flussi, e questa
/// funzione lo rispetta: la casa e il progetto spariscono. I flussi spediti no,
/// perché non sono in nessuna cartella e non c'è niente da sostituire — sono la
/// dotazione del binario, come i descrittori degli strumenti, dove
/// `SAILOR_TOOL_DESCRIPTORS` aggiunge e non toglie mai `Source::Builtin`. Chi ne
/// vuole uno diverso ne scrive uno con lo stesso nome nella cartella che ha
/// dichiarato: vince quello, e l'origine dice che è successo.
///
/// **L'ORDINE È L'UNICA REGOLA DI PRECEDENZA**: a parità di nome vince chi viene
/// dopo, quindi `di sistema` < `tuoi` < `del progetto`.
pub fn sources(
    home_flows: &Path,
    working: Option<&Path>,
    declared: Option<&Path>,
) -> Vec<FlowSource> {
    let mut sources = vec![FlowSource::builtin()];
    if let Some(declared) = declared.filter(|path| !path.as_os_str().is_empty()) {
        sources.push(FlowSource {
            origin: "dichiarati",
            dir: declared.to_path_buf(),
        });
        return sources;
    }
    sources.push(FlowSource {
        origin: "tuoi",
        dir: home_flows.to_path_buf(),
    });
    if let Some(project) = working.and_then(|working| project_flows_from(working, home_flows)) {
        sources.push(FlowSource {
            origin: "del progetto",
            dir: project,
        });
    }
    sources
}

/// La cartella dei flussi del progetto, cercata risalendo.
///
/// **SI RISALE, E NON È UN LUSSO.** Un programma non viene quasi mai avviato
/// dalla radice del progetto: la finestra di Sailor parte da `desktop/src-tauri`,
/// un editor parte da dove ha l'ultimo file aperto, un terminale da dove
/// l'utente si trovava. Guardare solo la cartella corrente vuol dire non
/// trovare niente quasi sempre — misurato il 29/08/2026: la finestra avviata
/// per lavorare su Sailor non vedeva i quattro flussi di Sailor.
///
/// **DEVE CONTENERE UN FLUSSO, non solo chiamarsi `flows`**: una cartella vuota
/// con quel nome fermerebbe la salita prima di arrivare a quella vera, e chi
/// guarda vedrebbe un elenco vuoto invece dei propri flussi.
///
/// La casa dell'utente non conta come progetto: è già una sorgente, e contarla
/// due volte mostrerebbe ogni flusso in doppia copia.
pub fn project_flows_from(working: &Path, home_flows: &Path) -> Option<PathBuf> {
    let mut here = Some(working);
    while let Some(directory) = here {
        let candidate = directory.join("flows");
        if candidate != home_flows && holds_a_flow(&candidate) {
            return Some(candidate);
        }
        here = directory.parent();
    }
    None
}

fn holds_a_flow(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(".flow.json"))
    })
}

/// Il registro di una sorgente, qualunque essa sia.
///
/// La sorgente di sistema non ha una cartella da leggere: si riconosce dal suo
/// «dove» e si serve dal binario. Passa da qui e non da un ramo di chi chiama
/// perché **chi mostra le sorgenti conta le voci di ciascuna** — la finestra lo
/// fa — e un ramo dimenticato là fuori direbbe «di sistema: 0 flussi» accanto a
/// flussi di sistema che stanno girando.
pub fn registry_of(source: &FlowSource) -> FlowRegistry {
    if source.is_builtin() {
        builtin_registry()
    } else {
        load_registry(&source.dir)
    }
}

/// I flussi spediti col prodotto, letti dal binario.
///
/// Un flusso spedito che non si legge resta nel registro col suo motivo, come
/// uno rotto sul disco: farlo sparire in silenzio significherebbe che una
/// versione del prodotto perde un flusso senza che nessuno se ne accorga. La
/// prova qui sotto fa in modo che non possa succedere a chi installa — cade
/// prima, da noi.
pub fn builtin_registry() -> FlowRegistry {
    let mut registry = FlowRegistry::new();
    for (name, text) in FLOWS {
        let entry = serde_json::from_str::<FlowFile>(text)
            .map_err(|error| format!("il flusso spedito «{name}» non è valido: {error}"));
        registry.insert((*name).to_owned(), entry);
    }
    registry
}

/// Legge i flussi dichiarativi in una cartella (formato `{ id, description,
/// graph, inputs }`).
///
/// In precedenza i file non leggibili venivano saltati in silenzio con la
/// motivazione che "la pagina non deve rompersi perché un file è a metà
/// scritto". Quella scelta era sbagliata: un file a metà scritto è uno stato
/// transitorio di pochi millisecondi, mentre un file rotto è permanente, e
/// trattarli allo stesso modo fa sparire il secondo per sempre. Chi guarda la
/// finestra vede un elenco corto senza sapere che è corto.
///
/// Ora ogni file `*.flow.json` o `*.json` viene incluso nel registro: se è
/// valido viene caricato, se è illeggibile o malformato viene registrato con il
/// motivo del rifiuto, così la finestra può mostrarlo marcato.
pub fn load_registry(dir: &Path) -> FlowRegistry {
    let mut registry = FlowRegistry::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return registry;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy())
            .unwrap_or_default();
        let is_flow_json = file_name.ends_with(".flow.json");
        let is_json = path.extension().and_then(|ext| ext.to_str()) == Some("json");
        if !is_flow_json && !is_json {
            continue;
        }
        let name = file_name
            .strip_suffix(".flow.json")
            .or_else(|| file_name.strip_suffix(".json"))
            .unwrap_or(&file_name)
            .to_owned();
        if name.is_empty() {
            continue;
        }
        let text = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(error) => {
                registry.insert(
                    name,
                    Err(format!("non riesco a leggere {}: {error}", path.display())),
                );
                continue;
            }
        };
        match serde_json::from_str::<FlowFile>(&text) {
            Ok(flow) => {
                registry.insert(name, Ok(flow));
            }
            Err(error) => {
                registry.insert(
                    name,
                    Err(format!("{} non è un flusso valido: {error}", path.display())),
                );
            }
        }
    }
    registry
}

/// I flussi di tutte le sorgenti, ciascuno con l'origine da cui viene.
///
/// **A PARITÀ DI NOME VINCE L'ULTIMA SORGENTE**, cioè la più specifica: è la
/// stessa regola dei descrittori degli strumenti, e la ragione è la stessa —
/// chi lavora su un progetto si aspetta che il flusso del progetto sia quello
/// che gira. La sostituzione non è silenziosa: l'origine resta visibile su ogni
/// riga, quindi chi vede un flusso comportarsi diversamente dal previsto può
/// capire da dove è venuto senza cercare.
pub fn load_all(sources: &[FlowSource]) -> Vec<(String, &'static str, Result<FlowFile, String>)> {
    let mut found: Vec<(String, &'static str, Result<FlowFile, String>)> = Vec::new();
    for source in sources {
        for (name, entry) in registry_of(source) {
            match found.iter_mut().find(|(existing, _, _)| existing == &name) {
                Some(slot) => *slot = (name, source.origin, entry),
                None => found.push((name, source.origin, entry)),
            }
        }
    }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

// ── scrivere un flusso, e cancellarlo ────────────────────────────────────
//
// **PERCHÉ QUESTE FUNZIONI SONO ARRIVATE QUI IL 31/08/2026.** Stavano in
// `desktop/src-tauri/src/flows.rs`, cioè nel guscio della finestra, che è
// **fuori dal workspace Rust**: la riga di comando non le poteva chiamare, e
// `sailor flow cap` — che deve riscrivere un `.flow.json` — avrebbe dovuto
// riscriverle. Sarebbero diventate due autori dello stesso file, con due idee
// diverse su cosa sia un nome sicuro e su come si sostituisce un file senza
// farlo vedere a metà. È il guasto 10, e questo modulo è già il posto che sa
// dove stanno i flussi e con quale precedenza: chi sa dove stanno è chi li
// scrive. Stesso trasloco già fatto per `registry::record_flow_run`.
//
// **COSA È RIMASTO AL GUSCIO, E NON PER DIMENTICANZA.** Il controllo che le
// azioni nominate esistano: la lista delle azioni la conoscono `actions`,
// `trigger` e `registry`, che dipendono tutti da questo crate. Farla entrare qui
// sarebbe un ciclo — e sarebbe anche sbagliato: quali azioni esistano dipende da
// chi compone il programma, non dal formato del flusso.

/// Scrive un flusso nella cartella dei flussi.
///
/// **PRENDE UN `FlowFile` GIÀ COSTRUITO, NON DEL JSON.** Chi arriva da un
/// `serde_json::Value` — la tela della finestra — lo deserializza da sé, e così
/// l'errore che vede è quello della validazione del grafo, con le parole di
/// `Graph::validate`. Chi invece ha già il flusso in mano — `sailor flow cap`,
/// che l'ha appena letto per cambiargli un campo — non deve rifare il giro da
/// JSON per riottenere ciò che ha già.
pub fn save_in(flows_dir: &Path, flow: &FlowFile) -> Result<(), String> {
    let id = safe_flow_id(&flow.id)?;
    fs::create_dir_all(flows_dir)
        .map_err(|error| format!("non riesco a preparare la cartella dei flussi: {error}"))?;
    let file_name = format!("{id}.flow.json");
    reject_a_name_that_collides_only_by_case(flows_dir, &file_name)?;
    let target = flows_dir.join(&file_name);
    let mut text = serde_json::to_string_pretty(flow)
        .map_err(|error| format!("non riesco a comporre il flusso in JSON: {error}"))?;
    // Un file di testo finisce con un a-capo: senza, `git diff` lo dichiara su
    // ogni flusso riscritto, e la riga che qualcuno aggiungerà a mano comparirà
    // attaccata all'ultima.
    text.push('\n');
    write_atomically(&target, text.as_bytes())
}

/// Cancella un flusso dalla cartella dei flussi.
pub fn delete_in(flows_dir: &Path, name: &str) -> Result<(), String> {
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
/// silenzio — chi guarda deve vedere che il nome è stato rifiutato.
pub fn safe_flow_id(id: &str) -> Result<&str, String> {
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
/// deve fare chi ha una persona davanti, non un file system.
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
    target.with_file_name(format!(".{file_name}.tmp-{}-{unique}", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "sailor-sistema-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("cartella di prova");
        dir
    }

    fn put_flow(dir: &Path, name: &str) {
        fs::create_dir_all(dir).expect("cartella");
        fs::write(dir.join(format!("{name}.flow.json")), "{}").expect("flusso");
    }

    /// LA PROVA CHE DEVE CADERE DA NOI E NON DA CHI INSTALLA. Un flusso spedito
    /// è dentro il binario: se è malformato, nessun utente può ripararlo — può
    /// solo scriverne uno suo con lo stesso nome, senza sapere perché serva.
    #[test]
    fn every_shipped_flow_loads() {
        let registry = builtin_registry();
        assert_eq!(registry.len(), FLOWS.len(), "nessun nome ripetuto");
        for (name, entry) in &registry {
            assert!(entry.is_ok(), "il flusso spedito «{name}»: {entry:?}");
        }
    }

    /// IL NOME DEL FILE È IL NOME DEL FLUSSO. Chi sovrascrive un flusso di
    /// sistema scrive un file con quel nome; chi lo esegue lo chiama per `id`.
    /// Se i due divergessero, «l'ho sovrascritto» e «l'ho eseguito» parlerebbero
    /// di due flussi diversi senza che nessuno lo veda.
    #[test]
    fn the_shipped_name_is_the_declared_id() {
        for (name, entry) in builtin_registry() {
            let flow = entry.expect("flusso valido");
            assert_eq!(flow.id, name, "nome del file e id dichiarato");
        }
    }

    /// La sorgente di sistema è la meno specifica: chi scrive un flusso in casa
    /// propria con lo stesso nome deve vincere, o «personalizzabile» è una
    /// parola senza meccanismo dietro.
    #[test]
    fn the_system_source_is_the_least_specific() {
        let places = sources(Path::new("/casa/flows"), None, None);
        assert_eq!(places[0], FlowSource::builtin());
        assert_eq!(places[0].origin, "di sistema");
        assert_eq!(places.last().expect("almeno una").origin, "tuoi");
    }

    /// `SAILOR_FLOWS` toglie di mezzo la casa e il progetto, non la dotazione
    /// del binario: quella non sta in nessuna cartella, quindi non c'è nessuna
    /// cartella da sostituire.
    #[test]
    fn a_declared_folder_replaces_the_disk_but_not_the_binary() {
        let places = sources(
            Path::new("/casa/flows"),
            None,
            Some(Path::new("/qui/i/flussi")),
        );
        let origins: Vec<&str> = places.iter().map(|p| p.origin).collect();
        assert_eq!(origins, vec!["di sistema", "dichiarati"]);
    }

    /// Un flusso di sistema si sostituisce scrivendone uno con lo stesso nome,
    /// e l'origine lo dice: senza quella riga chi ha modificato il proprio non
    /// saprebbe se sta guardando il suo o quello spedito.
    #[test]
    fn a_home_flow_overrides_a_system_flow_of_the_same_name() {
        let base = scratch("sovrascrittura");
        let home_flows = base.join("casa").join("flows");
        let shipped = FLOWS[0].0;
        put_flow(&home_flows, shipped);

        let all = load_all(&sources(&home_flows, None, None));

        let (_, origin, _) = all
            .iter()
            .find(|(name, _, _)| name == shipped)
            .expect("il flusso c'è");
        assert_eq!(*origin, "tuoi", "vince quello dell'utente");
        assert_eq!(
            all.iter().filter(|(name, _, _)| name == shipped).count(),
            1,
            "una riga sola, non due copie"
        );
        // **SOSTITUISCE, NON SI AGGIUNGE.** Senza questa riga la prova resta
        // verde anche se la sorgente di sistema sparisce del tutto — misurato
        // il 29/08/2026 togliendola apposta — perché un flusso solo che vince
        // su niente si legge uguale a un flusso che ne sovrascrive un altro.
        assert_eq!(
            all.len(),
            FLOWS.len(),
            "gli altri flussi spediti restano: {all:?}"
        );

        let _ = fs::remove_dir_all(&base);
    }

    /// Su una macchina appena installata — nessuna cartella, niente copiato —
    /// i flussi di sistema ci sono lo stesso.
    #[test]
    fn a_fresh_machine_still_has_the_system_flows() {
        let nowhere = std::env::temp_dir().join("sailor-casa-che-non-esiste-mai");
        let all = load_all(&sources(&nowhere.join("flows"), None, None));
        assert_eq!(all.len(), FLOWS.len());
        assert!(all.iter().all(|(_, origin, _)| *origin == "di sistema"));
    }

    /// Il difetto misurato il 29/08/2026: la finestra partiva da
    /// `desktop/src-tauri` e non vedeva i flussi del progetto, due cartelle più
    /// in su.
    #[test]
    fn the_project_flows_are_found_from_a_subfolder() {
        let root = scratch("risalita");
        put_flow(&root.join("flows"), "uno");
        let deep = root.join("desktop").join("src-tauri");
        fs::create_dir_all(&deep).expect("sottocartella");

        let found = project_flows_from(&deep, Path::new("/casa/altrove/flows"));

        assert_eq!(found, Some(root.join("flows")));
        let _ = fs::remove_dir_all(&root);
    }

    /// Una cartella che si chiama `flows` ma è vuota non deve fermare la
    /// salita: chi guarda vedrebbe un elenco vuoto al posto dei propri flussi.
    #[test]
    fn an_empty_flows_folder_does_not_stop_the_climb() {
        let root = scratch("vuota");
        put_flow(&root.join("flows"), "vero");
        let middle = root.join("dentro");
        fs::create_dir_all(middle.join("flows")).expect("cartella vuota che si chiama flows");

        let found = project_flows_from(&middle, Path::new("/casa/altrove/flows"));

        assert_eq!(found, Some(root.join("flows")));
        let _ = fs::remove_dir_all(&root);
    }

    /// La casa non è un progetto: contarla due volte mostrerebbe ogni flusso in
    /// doppia copia, e chi guarda non saprebbe quale delle due gira.
    #[test]
    fn the_home_is_never_also_the_project() {
        let home = scratch("casa");
        let home_flows = home.join("flows");
        put_flow(&home_flows, "mio");

        assert_eq!(project_flows_from(&home, &home_flows), None);
        let _ = fs::remove_dir_all(&home);
    }

    /// Contare le voci della sorgente di sistema deve dare il numero dei flussi
    /// spediti, non zero: chi mostra «dove ho guardato e cosa ho trovato» passa
    /// da qui, e uno zero lì dentro è indistinguibile da un guasto.
    #[test]
    fn counting_the_builtin_source_does_not_count_a_folder() {
        assert_eq!(registry_of(&FlowSource::builtin()).len(), FLOWS.len());
    }

    // ── scrivere un flusso ──────────────────────────────────────────────
    //
    // Queste prove erano nel guscio della finestra, che sta **fuori dal
    // workspace**: `cargo test --workspace` non le eseguiva. Sono arrivate qui
    // col codice che provano, e da oggi girano insieme a tutte le altre.

    /// Un flusso completo: due passi, una dipendenza, una pianificazione, degli
    /// ingressi. Serve alla prova d'identità — un flusso povero non potrebbe
    /// perdere niente nel giro.
    fn a_full_flow(id: &str) -> FlowFile {
        let text = format!(
            r#"{{
                "id": "{id}",
                "description": "due passi, una ricorrenza e degli ingressi",
                "graph": {{
                    "steps": [
                        {{
                            "id": "primo", "deps": [], "action": "shell_check",
                            "max_attempts": 1, "when": null,
                            "input_schema": {{"type": "any"}},
                            "output_schema": {{"type": "any"}}
                        }},
                        {{
                            "id": "secondo", "deps": ["primo"], "action": "shell_check",
                            "max_attempts": 3, "when": null,
                            "with": {{"command": "true"}},
                            "input_schema": {{"type": "any"}},
                            "output_schema": {{"type": "any"}}
                        }}
                    ]
                }},
                "inputs": {{ "primo": {{ "command": "true", "timeout_secs": 5 }} }},
                "schedule": {{
                    "recurrence": {{ "kind": "daily_at", "hour": 3, "minute": 30 }},
                    "weight": "heavy",
                    "perimeter": ["/una/cartella"]
                }}
            }}"#
        );
        serde_json::from_str(&text).expect("il flusso di prova è valido")
    }

    fn read_back(dir: &Path, id: &str) -> FlowFile {
        let text = fs::read_to_string(dir.join(format!("{id}.flow.json")))
            .expect("il file scritto si rilegge");
        serde_json::from_str(&text).expect("e si deserializza")
    }

    fn entries(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .expect("cartella leggibile")
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect()
    }

    /// **METTERE UN TETTO NON DEVE PERDERE NIENT'ALTRO.**
    ///
    /// È il rischio vero di `sailor flow cap`: si legge un flusso, gli si cambia
    /// un campo, lo si riscrive — e nel giro sparisce la pianificazione, o un
    /// `with` di un passo, e nessuno se ne accorge finché quel flusso non manca
    /// all'appuntamento notturno.
    ///
    /// **IL CONFRONTO È SUL `FlowFile`, NON SUL TESTO.** Confrontare il testo
    /// direbbe rosso a un `serde_json::to_string_pretty` che cambia
    /// l'indentazione, cioè a una differenza che non è una perdita.
    ///
    /// **IL MUTANTE CHE CONTA**: un campo che il giro perde. Marcando
    /// `FlowFile::schedule` con `#[serde(skip_serializing)]` — che è ciò che
    /// succede a chi aggiunge un campo e non aggiorna la scrittura — il flusso
    /// riletto torna senza pianificazione e questa prova diventa rossa.
    #[test]
    fn setting_the_cap_leaves_the_rest_of_the_flow_identical() {
        let dir = scratch("tetto-e-identita");
        let before = a_full_flow("con-tetto");
        save_in(&dir, &before).expect("prima scrittura");

        let mut with_cap = read_back(&dir, "con-tetto");
        with_cap.spend_cap_micros = Some(250_000);
        save_in(&dir, &with_cap).expect("riscrittura col tetto");
        let after = read_back(&dir, "con-tetto");

        assert_eq!(
            after.spend_cap_micros,
            Some(250_000),
            "il tetto è quello che si è messo"
        );
        // E tutto il resto è quello di prima, campo per campo: se un giorno
        // `FlowFile` cresce, questo confronto cresce con lui senza che nessuno
        // debba ricordarsene.
        let mut without_the_cap = after.clone();
        without_the_cap.spend_cap_micros = None;
        assert_eq!(
            without_the_cap, before,
            "il giro ha cambiato qualcosa oltre al tetto"
        );

        let _ = fs::remove_dir_all(&dir);
    }

    /// Togliere il tetto lo riporta a `None`, che non è `Some(0)`: il primo è
    /// «nessuno ha messo un limite», il secondo è «non deve spendere niente».
    #[test]
    fn clearing_the_cap_writes_no_cap_instead_of_a_zero() {
        let dir = scratch("tetto-tolto");
        let mut flow = a_full_flow("senza-tetto");
        flow.spend_cap_micros = Some(500);
        save_in(&dir, &flow).expect("scrittura col tetto");

        flow.spend_cap_micros = None;
        save_in(&dir, &flow).expect("riscrittura senza");

        assert_eq!(read_back(&dir, "senza-tetto").spend_cap_micros, None);
        let _ = fs::remove_dir_all(&dir);
    }

    /// **UN FILE DI TESTO FINISCE CON UN A-CAPO.**
    ///
    /// Senza, `git diff` scrive «\ No newline at end of file» su ogni flusso che
    /// passa di qui, e la riga successiva che qualcuno aggiungerà a mano
    /// comparirà attaccata all'ultima. Costa un carattere e si vede subito su
    /// ogni flusso riscritto da `sailor flow cap`.
    #[test]
    fn a_written_flow_ends_with_a_newline() {
        let dir = scratch("a-capo");
        save_in(&dir, &a_full_flow("finito-bene")).expect("scrittura");

        let text = fs::read_to_string(dir.join("finito-bene.flow.json")).expect("rileggere");

        assert!(text.ends_with('\n'), "il file non finisce con un a-capo");
        let _ = fs::remove_dir_all(&dir);
    }

    /// **UN CAMPO CHE NON C'ERA NON DEVE COMPARIRE COME `null`.**
    ///
    /// Riscrivere un flusso per cambiargli il tetto non deve aggiungergli righe
    /// che nessuno ha scritto: `"schedule": null` e `"spend_cap_micros": null`
    /// non dicono niente che l'assenza non dica già, e riempiono di rumore il
    /// diff di chi rilegge il proprio flusso dopo il comando. Assente e `null`
    /// si rileggono uguali — lo prova `clearing_the_cap_writes_no_cap`.
    #[test]
    fn a_field_that_was_absent_does_not_come_back_as_null() {
        let dir = scratch("niente-null");
        let mut bare = a_full_flow("nudo");
        bare.schedule = None;
        bare.spend_cap_micros = None;
        save_in(&dir, &bare).expect("scrittura");

        let text = fs::read_to_string(dir.join("nudo.flow.json")).expect("rileggere");

        assert!(!text.contains("schedule"), "{text}");
        assert!(!text.contains("spend_cap_micros"), "{text}");
        assert_eq!(read_back(&dir, "nudo"), bare, "e si rilegge identico");
        let _ = fs::remove_dir_all(&dir);
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: senza il controllo su `..` questo
    /// id scriverebbe fuori dalla cartella dei flussi, nel suo genitore.
    #[test]
    fn a_flow_id_that_climbs_out_of_the_directory_is_refused() {
        let dir = scratch("evasione");
        // Il bersaglio dell'evasione sta fuori dalla cartella usa-e-getta: va
        // ripulito prima e dopo, o un mutante che la lascia passare sporca
        // `$TMPDIR` per i giri successivi invece di farsi vedere qui.
        let escaped = dir
            .parent()
            .expect("la prova ha un genitore")
            .join("evaso.flow.json");
        let _ = fs::remove_file(&escaped);

        let error = save_in(&dir, &a_full_flow("../evaso")).expect_err("id con .. rifiutato");

        assert!(error.contains("percorso"), "{error}");
        assert!(entries(&dir).is_empty(), "la cartella resta vuota");
        assert!(!escaped.exists(), "e niente è uscito dalla cartella");
        let _ = fs::remove_file(&escaped);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_flow_id_with_a_path_separator_is_refused() {
        let dir = scratch("separatore");
        let error =
            save_in(&dir, &a_full_flow("sotto/cartella")).expect_err("id con / rifiutato");
        assert!(error.contains("percorso"), "{error}");
        assert!(entries(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_empty_flow_id_is_refused_and_writes_nothing() {
        let dir = scratch("id-vuoto");
        let error = save_in(&dir, &a_full_flow("")).expect_err("id vuoto rifiutato");
        assert!(error.contains("vuoto"), "{error}");
        assert!(entries(&dir).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    /// IL FILE SYSTEM NON DICE CHE SONO LO STESSO FILE. Su APFS come lo
    /// installa macOS, salvare «mioflusso» sopra un «MioFlusso» esistente
    /// sostituisce il contenuto senza un errore e lascia il nome vecchio.
    #[test]
    fn a_name_that_differs_only_by_case_is_refused() {
        let dir = scratch("maiuscole");
        save_in(&dir, &a_full_flow("MioFlusso")).expect("il primo si scrive");

        let error = save_in(&dir, &a_full_flow("mioflusso")).expect_err("il secondo è rifiutato");

        assert!(error.contains("MioFlusso"), "{error}");
        // E quello che c'era resta intero: il rifiuto non deve aver toccato
        // niente, che è il motivo per cui esiste.
        assert_eq!(read_back(&dir, "MioFlusso").id, "MioFlusso");
        assert_eq!(entries(&dir).len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    /// LA MISURA CHE POTEVA VENIRE DIVERSA: la seconda scrittura porta una
    /// descrizione diversa. Un mutante che salti `fs::rename` lascerebbe la
    /// prima sul disco.
    #[test]
    fn a_second_write_replaces_the_content_instead_of_leaving_it() {
        let dir = scratch("sostituzione");
        save_in(&dir, &a_full_flow("stesso-id")).expect("prima scrittura");
        let mut second = a_full_flow("stesso-id");
        second.description = "seconda versione, diversa dalla prima".to_owned();
        save_in(&dir, &second).expect("seconda scrittura");

        assert_eq!(
            read_back(&dir, "stesso-id").description,
            "seconda versione, diversa dalla prima"
        );
        assert_eq!(entries(&dir).len(), 1, "e nessun file temporaneo rimasto");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_removes_the_flow_and_says_so_when_there_is_nothing_to_remove() {
        let dir = scratch("cancellazione");
        save_in(&dir, &a_full_flow("da-cancellare")).expect("scrittura");
        delete_in(&dir, "da-cancellare").expect("cancellazione");
        assert!(entries(&dir).is_empty());

        let error = delete_in(&dir, "mai-esistito").expect_err("un assente non si cancella");
        assert!(error.contains("non esiste"), "{error}");
        let _ = fs::remove_dir_all(&dir);
    }
}
