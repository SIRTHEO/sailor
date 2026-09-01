//! La finestra e il motore hanno **una lista di azioni sola**, e adesso
//! qualcuno la misura.
//!
//! **IL GUASTO 10, NELLA SUA ULTIMA COPIA RIMASTA.** «La stessa lista di
//! componenti scritta in due punti del programma»: la cura scritta accanto al
//! guasto è «una sola fonte, e le altre la chiedono invece di ricopiarla». Sul
//! lato Rust è già stata applicata — `crates/registry` esiste apposta, e il suo
//! commento in testa lo racconta. Sul lato della finestra no: `ACTION_KIND` in
//! `desktop/src/flow.ts` è una seconda lista, scritta a mano, che nessuno
//! confronta con la prima.
//!
//! **L'ANCORA STA FUORI DA TUTTE E DUE LE COPIE.** I nomi del motore non si
//! ricopiano qui: si prendono **eseguendo** `registry::default_registry` e
//! chiedendogli `names()`. Confrontare due elenchi scritti a mano li lascia
//! sbagliare insieme — è già successo in questo repo, e una prova che lo
//! facesse resterebbe verde col difetto rimesso.
//!
//! **PERCHÉ QUESTA PROVA STA QUI E NON IN `desktop/`.** `desktop/src-tauri`
//! dichiara un `[workspace]` vuoto: sta fuori dal workspace Rust di proposito,
//! e nessun `cargo test --workspace` lo compila. Una prova scritta là dentro
//! non diventa rossa per il gate, qualunque cosa affermi — che è lo stesso
//! difetto della regola che nessun controllo interroga. `crates/sailor` invece
//! è un membro del workspace, e il gate del flusso di sviluppo
//! (`flows/sviluppa-sailor.flow.json`, passo `prove`) lo esegue.
//!
//! **IL PERIMETRO, DICHIARATO — QUESTA PROVA VERDE NON CHIUDE IL GUASTO 10.**
//! Confronta la finestra col registro **del motore**. Ne esiste una **terza**
//! copia, `action_registry()` in `desktop/src-tauri/src/flows.rs`, che si
//! costruisce il registro a mano ed è quella a cui `save_flow` chiede se un
//! flusso si può salvare: `actions::register_default` ne porta quattro, quindi
//! un nodo `handed_to_agent`, `detect_tools`, `subflow`, `history_ask` o
//! `work_claim` si disegna con la famiglia giusta e viene **respinto al
//! salvataggio**. Questa prova non lo vede, e non è un suo difetto: è il suo
//! confine, che va scritto perché un controllo verde non si prenda per una
//! promessa più larga di quella che fa. Il giorno che `action_registry()`
//! smette di costruirsi il registro da sé, il confine cade e il guasto 10 si
//! chiude. Trovato da un giudice che non aveva scritto questo lavoro.
//!
//! **IL PREZZO, DICHIARATO.** Legge testo da un file `.ts`: non ne compila
//! l'albero sintattico, e una mappa scritta in una forma diversa le sfugge.
//! Per questo ogni caso pretende di aver letto qualcosa prima di giudicare:
//! una lettura fallita deve essere rossa, non silenziosamente verde. È la
//! stessa scelta — e lo stesso motivo — di
//! `the_shell_builds_no_request_of_its_own.rs`, che sta qui accanto.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

fn repository_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("il crate sta in <radice>/crates/sailor")
        .to_path_buf()
}

fn window_vocabulary() -> String {
    let path = repository_root().join("desktop/src/flow.ts");
    std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()))
}

/// Un contatore, e non il solo orologio: viene dal guasto 21. `cargo test`
/// manda le prove sullo stesso processo e l'orologio di macOS non ha la
/// risoluzione del nanosecondo, quindi due cartelle nate nello stesso istante
/// si rubavano il posto a vicenda.
static NEXT_SCRATCH: AtomicU32 = AtomicU32::new(0);

fn scratch_dir(label: &str) -> PathBuf {
    let serial = NEXT_SCRATCH.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "sailor-action-list-{}-{serial}-{label}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// I nomi che il motore registra **davvero**, chiesti al registro invece che
/// ricopiati.
///
/// **IL DEPOSITO SERVE.** Sei azioni si registrano solo quando c'è — quelle che
/// scrivono e leggono il deposito — e la finestra ha ragione a saperle
/// disegnare comunque: un flusso che le nomina esiste e gira.
///
/// `default_registry` legge la macchina per costruire il risolutore degli
/// strumenti, ma i **nomi** delle azioni non dipendono da cosa c'è installato:
/// sono le righe di `registry::default_registry`, e non il contenuto di
/// `~/.config/sailor/tools.d`. Questa prova non è quindi una di quelle del
/// guasto 5.
fn engine_action_names(label: &str) -> BTreeSet<String> {
    let dir = scratch_dir(label);
    let ledger = ledger::Ledger::open(&dir).expect("un deposito di prova si apre");
    registry::default_registry(Some(ledger), None)
        .names()
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Le coppie `chiave: valore` di un oggetto letterale di `flow.ts`.
///
/// Salta le righe di commento — la mappa ne porta, e devono poterne portare —
/// e si ferma alla graffa che chiude. Le virgolette si tolgono da tutti e due i
/// lati perché TypeScript le mette sulle chiavi solo quando servono.
fn object_entries(source: &str, marker: &str) -> Vec<(String, String)> {
    let from = source
        .find(marker)
        .unwrap_or_else(|| panic!("«{marker}» non si trova in desktop/src/flow.ts"));
    let body = &source[from..];
    let open = body
        .find('{')
        .unwrap_or_else(|| panic!("«{marker}» non apre nessun oggetto"));

    let mut entries = Vec::new();
    for line in body[open + 1..].lines() {
        let line = line.trim();
        if line.starts_with('}') {
            break;
        }
        if line.starts_with("//") || line.starts_with('*') || line.starts_with("/*") {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim().trim_matches('"').to_owned();
        let value = value
            .trim()
            .trim_end_matches(',')
            .trim()
            .trim_matches('"')
            .to_owned();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        entries.push((key, value));
    }
    entries
}

/// I nomi di azione che la finestra conosce: le chiavi di `ACTION_KIND`.
fn window_action_names(source: &str) -> BTreeSet<String> {
    object_entries(source, "const ACTION_KIND")
        .into_iter()
        .map(|(action, _kind)| action)
        .collect()
}

/// **NESSUN NOME INVENTATO NELLA FINESTRA.** Un nome che il motore non registra
/// non è un bottone che non fa niente: è un nodo che si disegna, si sposta e si
/// collega, e poi **non si salva** — «il flusso usa azioni che il motore non
/// conosce». Il difetto non compare mai finché qualcuno non preme quel tasto.
#[test]
fn the_window_names_no_action_the_engine_does_not_register() {
    let source = window_vocabulary();
    let named = window_action_names(&source);
    assert!(
        named.len() > 4,
        "il vocabolario della finestra non è stato letto: {} nomi trovati. \
         Una lettura fallita non deve passare per un confronto riuscito",
        named.len()
    );

    let registered = engine_action_names("inventate");
    let invented: Vec<&String> = named.difference(&registered).collect();
    assert!(
        invented.is_empty(),
        "la finestra nomina {} azioni che il motore non registra: {:?}. \
         Un nodo con uno di questi nomi si disegna e poi non si salva",
        invented.len(),
        invented
    );
}

/// **E L'ALTRO VERSO, CHE È COME IL DIFETTO PEGGIORE SI NASCONDEVA.** `kindOf`
/// ripiega su «verifica» per un nome che non conosce: un'azione del motore
/// senza famiglia non fa apparire nessun errore, fa solo disegnare il nodo
/// sbagliato. Un solo verso lascerebbe questa prova verde mentre metà del
/// vocabolario manca.
#[test]
fn every_engine_action_has_a_family_in_the_window() {
    let source = window_vocabulary();
    let named = window_action_names(&source);
    assert!(
        named.len() > 4,
        "il vocabolario della finestra non è stato letto: {} nomi trovati",
        named.len()
    );

    let registered = engine_action_names("senza-famiglia");
    let orphans: Vec<&String> = registered.difference(&named).collect();
    assert!(
        orphans.is_empty(),
        "il motore registra {} azioni a cui la finestra non dà una famiglia: {:?}. \
         Un nodo di questi si disegna come «verifica», in silenzio",
        orphans.len(),
        orphans
    );
}

/// **CIÒ CHE LA CASSETTA CREA DEVE ESISTERE.** `DEFAULT_ACTION_FOR_KIND` è il
/// nome con cui nasce un passo premuto nella cassetta: è la lista che si tocca
/// per prima, e i suoi valori sono nomi di azione come gli altri.
#[test]
fn every_action_the_palette_creates_is_registered() {
    let source = window_vocabulary();
    let created: BTreeSet<String> = object_entries(&source, "const DEFAULT_ACTION_FOR_KIND")
        .into_iter()
        .map(|(_kind, action)| action)
        .collect();
    assert!(
        created.len() > 2,
        "la cassetta dei passi non è stata letta: {} azioni trovate",
        created.len()
    );

    let registered = engine_action_names("cassetta");
    let invented: Vec<&String> = created.difference(&registered).collect();
    assert!(
        invented.is_empty(),
        "la cassetta crea {} passi con un'azione che il motore non registra: {:?}",
        invented.len(),
        invented
    );
}
