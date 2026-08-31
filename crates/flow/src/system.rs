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
use std::path::{Path, PathBuf};

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

/// Le stesse sorgenti, lette dall'ambiente di questo processo.
///
/// **ESISTE PERCHÉ QUESTE SEI RIGHE STAVANO PER NASCERE UNA SECONDA VOLTA.**
/// La prima copia è `ui::gather::flow_sources`; la seconda serviva a chi
/// costruisce il registro delle azioni, perché il passo `subflow` deve cercare
/// il flusso che chiama esattamente dove lo cerca `sailor flow run` — o due
/// macchine eseguono flussi diversi con lo stesso nome senza dirlo. Due copie
/// di una regola di precedenza è il difetto che `file.rs` e `registry` hanno
/// già pagato ciascuno una volta: la regola sta qui, e chi la vuole la importa.
///
/// La casa resta un argomento: chi la conosce è `ledger::sailor_home`, e questo
/// crate non dipende da `ledger` — è la direzione che tiene in piedi tutto il
/// resto.
pub fn sources_from_env(home_flows: &Path) -> Vec<FlowSource> {
    let declared = std::env::var_os("SAILOR_FLOWS").map(PathBuf::from);
    let working = std::env::current_dir().ok();
    sources(home_flows, working.as_deref(), declared.as_deref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

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
}
