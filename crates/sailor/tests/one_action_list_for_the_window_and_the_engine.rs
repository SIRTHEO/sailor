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
//! **IL CONFINE CHE STAVA SCRITTO QUI È CADUTO IL 01/09/2026, E LA RIGA CHE LO
//! DICHIARAVA ADESSO SAREBBE FALSA.** Diceva: questa prova confronta la
//! finestra col registro **del motore**, ma ne esiste una **terza** copia —
//! `action_registry()` in `desktop/src-tauri/src/flows.rs` — che si costruisce
//! il registro a mano con `actions::register_default`, ed è quella a cui
//! `save_flow` chiede se un flusso si può salvare: un nodo `handed_to_agent`,
//! `detect_tools`, `subflow`, `history_ask` o `work_claim` si disegnava con la
//! famiglia giusta e veniva **respinto al salvataggio**. Il confine era
//! scritto perché un controllo verde non si prendesse per una promessa più
//! larga di quella che fa, e l'aveva trovato un giudice che non aveva scritto
//! quel lavoro.
//!
//! **ADESSO `action_registry_with` CHIAMA `registry::default_registry`**, e la
//! terza copia non c'è più: la riparazione è arrivata da un altro ramo della
//! stessa giornata, fusa la sera. Il guasto 10 si chiude qui.
//!
//! **E LA CHIUSURA SI SORVEGLIA DA QUESTO LATO, NON DA QUELLO.** La prova che
//! confronta i due registri sta in `desktop/src-tauri`, che dichiara un
//! `[workspace]` vuoto: nessun `cargo test --workspace` la compila, quindi non
//! diventa rossa per nessuno. `the_window_shell_asks_the_engine_for_its_action_list`
//! qui sotto legge quel sorgente dal gate — è la stessa disciplina con cui
//! questo file legge `flow.ts` — così chi rimettesse tre righe scelte a mano
//! trova un rosso invece del silenzio.
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

/// The names the engine really registers, asked of the registry rather than
/// copied. **THE STORE MATTERS**: six actions register only when there is one,
/// and the window is right to draw them all the same. The house is this test's
/// own, so nothing of the machine's home or tools decides the list.
fn engine_action_names(label: &str) -> BTreeSet<String> {
    let dir = scratch_dir(label);
    let ledger = ledger::Ledger::open(&dir).expect("un deposito di prova si apre");
    registry::registry_in(registry::House::under(&dir), Some(ledger), None)
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

/// **IL GUSCIO CHIEDE LA LISTA AL MOTORE, E NON SE NE COSTRUISCE UNA.**
///
/// È la terza copia del registro, quella che `save_flow` interroga per dire se
/// un flusso si può salvare. Fino al 01/09/2026 si costruiva da sé con tre
/// righe scelte a mano e rifiutava cinque azioni che dal terminale girano;
/// adesso delega a `registry::default_registry`.
///
/// **STA QUI E NON IN `desktop/src-tauri` PERCHÉ LÀ NON DIVENTEREBBE ROSSA.**
/// Quel crate dichiara un `[workspace]` vuoto: `cargo test --workspace` non lo
/// compila, e una prova che nessun gate esegue afferma senza controllare. Il
/// prezzo è dichiarato — legge testo, non compila l'albero sintattico — quindi
/// il caso pretende di aver letto qualcosa prima di giudicare, come le prove
/// qui sopra fanno con `flow.ts`.
///
/// Il mutante che la fa cadere: rimettere `actions::register_default(&mut r)`
/// dentro `action_registry_with`.
#[test]
fn the_window_shell_asks_the_engine_for_its_action_list() {
    let path = repository_root().join("desktop/src-tauri/src/flows.rs");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("leggere {}: {error}", path.display()));

    let from = source
        .find("fn action_registry_with(")
        .expect("il guscio costruisce il registro in `action_registry_with`");
    let body = &source[from..];
    let end = body.find("\n}").expect("la funzione si chiude");
    let body = &body[..end];

    assert!(
        body.contains("registry::default_registry"),
        "il guscio si costruisce il registro da sé invece di chiederlo al \
         motore: è la terza copia del guasto 10, e la finestra tornerebbe a \
         rifiutare al salvataggio azioni che dal terminale girano.\n{body}"
    );
    assert!(
        !body.contains("register_default(&mut"),
        "il guscio registra azioni a mano dentro `action_registry_with`: \
         qualunque riga scelta lì è una lista in più da tenere allineata.\n{body}"
    );
}
