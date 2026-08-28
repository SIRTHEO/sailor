//! Le prove del rilevamento, su una macchina finta costruita qui dentro.
//!
//! PERCHÉ NON SI PROVA SULLA MACCHINA VERA. Una prova che dice «claude c'è»
//! passa da me e cade da chiunque altro, e soprattutto non poteva venire
//! diversa: non prova il rilevamento, prova la mia installazione. Qui il
//! percorso, la casa e le variabili sono una cartella temporanea, e ogni caso —
//! il presente, l'assente, il non verificabile — si costruisce.

use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use toolbox::{descriptor::Source, Catalog, Machine, Presence, VersionReading};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Una cartella tutta nostra, che si porta via quello che ci abbiamo messo.
struct Sandbox {
    root: PathBuf,
}

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "toolbox-{name}-{}-{n}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("la cartella di prova si crea");
        Sandbox { root }
    }

    fn dir(&self, name: &str) -> PathBuf {
        let path = self.root.join(name);
        fs::create_dir_all(&path).expect("sottocartella");
        path
    }

    fn write(&self, name: &str, content: &str) -> PathBuf {
        let path = self.root.join(name);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("cartella del file");
        }
        fs::write(&path, content).expect("scrittura del file di prova");
        path
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        // Una cartella resa illeggibile di proposito va rimessa a posto, o non
        // si cancella più e la prossima esecuzione se la ritrova addosso.
        restore(&self.root);
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn restore(dir: &Path) {
    let _ = fs::set_permissions(dir, fs::Permissions::from_mode(0o755));
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.path().is_dir() {
            restore(&entry.path());
        }
    }
}

/// Un eseguibile finto che stampa quello che gli diciamo di stampare.
fn fake_binary(dir: &Path, name: &str, script: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{script}\n")).expect("scrittura dell'eseguibile finto");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("bit di esecuzione");
    path
}

fn machine(path_dirs: Vec<PathBuf>, home: &Path) -> Machine {
    let mut env = BTreeMap::new();
    env.insert("HOME".to_string(), home.to_string_lossy().into_owned());
    Machine {
        path_dirs,
        home: home.to_path_buf(),
        env,
        version_probes: true,
    }
}

fn catalog_from(path: &Path) -> Catalog {
    Catalog::load(&[Source::File(path.to_path_buf())])
}

// ── uno strumento presente, riconosciuto col suo descrittore ────────────

#[test]
fn a_present_tool_is_found_with_the_descriptor_that_named_it() {
    let sandbox = Sandbox::new("present");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "attrezzo", "echo 'attrezzo 4.2'");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "label": "L'attrezzo",
             "detect": {"command": "attrezzo"},
             "version": {"args": ["--version"]}}]"#,
    );
    let machine = machine(vec![bin_dir.clone()], &sandbox.root);
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);

    let found = &report.findings[0];
    assert_eq!(found.name, "attrezzo");
    // DA QUALE DESCRITTORE, E DA QUALE FILE: senza queste due righe l'elenco non
    // si può smentire, e un elenco che non si smentisce non si corregge.
    assert_eq!(found.descriptor_id, "attrezzo");
    assert_eq!(found.descriptor_source, descriptors.to_string_lossy());
    assert!(
        matches!(&found.presence, Presence::Present(why) if why.contains("attrezzo")),
        "{:?}",
        found.presence
    );
    assert_eq!(
        found.executable.as_deref(),
        Some(bin_dir.join("attrezzo").to_string_lossy().as_ref())
    );
    assert_eq!(
        found.version,
        VersionReading::Declared("attrezzo 4.2".to_string())
    );
}

/// Un nome che c'è ma non è eseguibile non è il binario cercato: la shell fa
/// così, e un rilevamento che dicesse il contrario manderebbe un flusso a
/// invocare un file di testo.
#[test]
fn a_file_without_the_execute_bit_is_not_the_tool() {
    let sandbox = Sandbox::new("notexec");
    let bin_dir = sandbox.dir("bin");
    fs::write(bin_dir.join("attrezzo"), "non sono un programma").unwrap();
    fs::set_permissions(bin_dir.join("attrezzo"), fs::Permissions::from_mode(0o644)).unwrap();
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let machine = machine(vec![bin_dir], &sandbox.root);
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);
    assert!(
        matches!(report.findings[0].presence, Presence::Absent(_)),
        "{:?}",
        report.findings[0].presence
    );
}

// ── uno strumento assente, riconosciuto come assente ────────────────────

#[test]
fn a_missing_tool_is_absent_not_unknown() {
    let sandbox = Sandbox::new("absent");
    let bin_dir = sandbox.dir("bin");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "mai-installato", "family": "tool",
             "detect": {"command": "mai-installato"}}]"#,
    );
    let machine = machine(vec![bin_dir], &sandbox.root);
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);

    match &report.findings[0].presence {
        Presence::Absent(reason) => assert!(reason.contains("mai-installato"), "{reason}"),
        other => panic!("una cartella leggibile e vuota è una misura: {other:?}"),
    }
    assert!(report.findings[0].executable.is_none());
    assert!(matches!(
        report.findings[0].version,
        VersionReading::NotAsked(_)
    ));
}

// ── la distinzione che rende utile l'inventario ─────────────────────────

/// LA PROVA CHE PORTA IL CRATE. Stesso descrittore, stesso strumento assente,
/// due mondi diversi: in uno le cartelle si leggono e la risposta è «non c'è»,
/// nell'altro non si leggono e la risposta è «non lo so». Se il codice
/// rispondesse `Absent` in tutti e due, questa prova diventerebbe rossa qui —
/// che è esattamente quello che deve succedere.
#[test]
fn an_unreadable_path_is_undetermined_not_absent() {
    let sandbox = Sandbox::new("blocked");
    let open = sandbox.dir("aperta");
    let closed = sandbox.dir("chiusa");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let catalog = catalog_from(&descriptors);

    let visible = toolbox::detect(&catalog, &machine(vec![open.clone()], &sandbox.root));
    assert!(
        matches!(visible.findings[0].presence, Presence::Absent(_)),
        "con la sola cartella leggibile la risposta è una misura: {:?}",
        visible.findings[0].presence
    );

    let blind = toolbox::detect(&catalog, &machine(vec![open, closed], &sandbox.root));
    match &blind.findings[0].presence {
        Presence::Undetermined(reason) => assert!(reason.contains("chiusa"), "{reason}"),
        other => panic!("una cartella che non si legge non autorizza a dire «non c'è»: {other:?}"),
    }
}

/// Nessuna cartella in cui cercare non è «non c'è»: è «non ho guardato».
#[test]
fn an_empty_search_path_is_undetermined() {
    let sandbox = Sandbox::new("nopath");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    assert!(
        matches!(report.findings[0].presence, Presence::Undetermined(_)),
        "{:?}",
        report.findings[0].presence
    );
}

/// La stessa distinzione sui percorsi, non solo sugli eseguibili: un file che
/// non c'è e un file che non si può guardare sono due risposte diverse.
#[test]
fn a_path_probe_tells_missing_from_blocked() {
    let sandbox = Sandbox::new("pathprobe");
    let closed = sandbox.dir("chiusa");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).unwrap();
    let descriptors = sandbox.write(
        "tools.json",
        &format!(
            r#"[{{"id": "assente", "family": "tool", "detect": {{"path": "{}/non-c-e.json"}}}},
                {{"id": "coperto", "family": "tool", "detect": {{"path": "{}/coperto.json"}}}}]"#,
            sandbox.root.to_string_lossy(),
            closed.to_string_lossy()
        ),
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    let assente = report.findings.iter().find(|f| f.name == "assente").unwrap();
    let coperto = report.findings.iter().find(|f| f.name == "coperto").unwrap();
    assert!(
        matches!(assente.presence, Presence::Absent(_)),
        "{:?}",
        assente.presence
    );
    assert!(
        matches!(coperto.presence, Presence::Undetermined(_)),
        "{:?}",
        coperto.presence
    );
}

// ── la versione: chiesta, non chiesta, chiesta senza risposta ───────────

/// Uno strumento c'è ma non dice la sua versione: presente, e versione «non
/// disponibile» col perché. Confonderla con l'assenza cancellerebbe dall'elenco
/// uno strumento installato — è successo davvero su questa macchina con due
/// righe di comando che si lamentano dei permessi prima di rispondere.
#[test]
fn a_present_tool_whose_version_fails_stays_present() {
    let sandbox = Sandbox::new("badversion");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "scontroso", "echo 'permesso negato' 1>&2; exit 1");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "scontroso", "family": "tool", "detect": {"command": "scontroso"},
             "version": {"args": ["--version"]}}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let found = &report.findings[0];
    assert!(found.presence.is_present(), "{:?}", found.presence);
    match &found.version {
        VersionReading::Unavailable(reason) => {
            assert!(reason.contains("permesso negato"), "{reason}")
        }
        other => panic!("una versione che non si è ottenuta non è una versione: {other:?}"),
    }
}

/// Un binario che non torna non ferma il rilevamento: il tetto lo tronca, e la
/// versione diventa «non disponibile» col motivo.
#[test]
fn a_hanging_version_probe_does_not_hang_the_detection() {
    let sandbox = Sandbox::new("hang");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "lento", "exec sleep 60");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "lento", "family": "tool", "detect": {"command": "lento"},
             "version": {"args": ["--version"], "timeout_secs": 1}}]"#,
    );
    let start = std::time::Instant::now();
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    assert!(report.findings[0].presence.is_present());
    assert!(matches!(
        report.findings[0].version,
        VersionReading::Unavailable(_)
    ));
    assert!(
        start.elapsed() < std::time::Duration::from_secs(20),
        "il tetto deve troncare davvero: {:?}",
        start.elapsed()
    );
}

/// UN AVVERTIMENTO NON È UNA VERSIONE, e distinguerli è un dato del descrittore,
/// non un ramo di codice per un binario in particolare. Il caso è vero: qui
/// `ollama --version`, col servizio non raggiungibile, stampa prima un
/// avvertimento — e senza questo campo quella riga finiva registrata come
/// versione.
#[test]
fn a_banner_before_the_version_is_skipped_by_the_descriptor() {
    let sandbox = Sandbox::new("banner");
    let bin_dir = sandbox.dir("bin");
    fake_binary(
        &bin_dir,
        "ciarliero",
        "echo 'Attenzione: nessun servizio'; echo 'ciarliero version 9.9'",
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "grezzo", "family": "tool", "detect": {"command": "ciarliero"},
             "version": {"args": ["--version"]}},
            {"id": "preciso", "family": "tool", "detect": {"command": "ciarliero"},
             "version": {"args": ["--version"], "must_contain": "version"}},
            {"id": "sbagliato", "family": "tool", "detect": {"command": "ciarliero"},
             "version": {"args": ["--version"], "must_contain": "mai-scritto"}}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let version = |name: &str| {
        report
            .findings
            .iter()
            .find(|f| f.name == name)
            .unwrap()
            .version
            .clone()
    };
    assert_eq!(
        version("grezzo"),
        VersionReading::Declared("Attenzione: nessun servizio".to_string()),
        "senza il campo si prende la prima riga: è il comportamento che il campo esiste per correggere"
    );
    assert_eq!(
        version("preciso"),
        VersionReading::Declared("ciarliero version 9.9".to_string())
    );
    // Una riga chiesta e mai stampata non è una versione: si dice, non si
    // ripiega sulla prima che capita.
    match version("sbagliato") {
        VersionReading::Unavailable(reason) => assert!(reason.contains("mai-scritto"), "{reason}"),
        other => panic!("{other:?}"),
    }
}

/// Con le esecuzioni spente non si esegue niente, e la versione lo dice invece
/// di restare vuota.
#[test]
fn with_version_probes_off_nothing_is_executed() {
    let sandbox = Sandbox::new("noprobe");
    let bin_dir = sandbox.dir("bin");
    let spia = sandbox.root.join("spia");
    fake_binary(
        &bin_dir,
        "attrezzo",
        &format!("touch '{}'; echo 1.0", spia.to_string_lossy()),
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"},
             "version": {"args": ["--version"]}}]"#,
    );
    let mut machine = machine(vec![bin_dir], &sandbox.root);
    machine.version_probes = false;
    let report = toolbox::detect(&catalog_from(&descriptors), &machine);
    assert!(report.findings[0].presence.is_present());
    assert!(matches!(
        report.findings[0].version,
        VersionReading::NotAsked(_)
    ));
    assert!(!spia.exists(), "spente, le esecuzioni non devono avvenire");
}

// ── un descrittore malformato non fa cadere il rilevamento ──────────────

#[test]
fn a_malformed_descriptor_does_not_take_the_others_down() {
    let sandbox = Sandbox::new("malformed");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "buono", "echo 'buono 1.0'");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[
            {"id": "senza-famiglia"},
            {"id": "senza-verifica", "family": "tool"},
            {"id": "campo-inventato", "family": "tool", "detect": {"command": "x"}, "boh": 1},
            {"id": "buono", "family": "tool", "detect": {"command": "buono"},
             "version": {"args": ["--version"]}}
        ]"#,
    );
    let catalog = catalog_from(&descriptors);
    assert_eq!(
        catalog.problems.len(),
        3,
        "tre righe sbagliate, tre segnalazioni: {:?}",
        catalog.problems
    );
    // La segnalazione dice DI CHI PARLA: «un descrittore è sbagliato» non si
    // corregge, «`senza-famiglia` è sbagliato» sì.
    let about: Vec<&str> = catalog.problems.iter().map(|p| p.about.as_str()).collect();
    assert!(about.contains(&"senza-famiglia"), "{about:?}");
    assert!(about.contains(&"senza-verifica"), "{about:?}");

    let report = toolbox::detect(&catalog, &machine(vec![bin_dir], &sandbox.root));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(
        report.findings[0].version,
        VersionReading::Declared("buono 1.0".to_string())
    );
    // Le segnalazioni viaggiano col rapporto: chi legge deve sapere che l'elenco
    // è parziale, o crederà che quello che manca non ci sia.
    assert_eq!(report.problems.len(), 3);
}

/// Un file intero illeggibile non porta giù gli altri file di descrittori.
#[test]
fn a_broken_file_does_not_take_the_other_files_down() {
    let sandbox = Sandbox::new("brokenfile");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "buono", "echo ok");
    let rotto = sandbox.write("rotto.json", "{ questo non è json");
    let sano = sandbox.write(
        "sano.json",
        r#"[{"id": "buono", "family": "tool", "detect": {"command": "buono"}}]"#,
    );
    let catalog = Catalog::load(&[Source::File(rotto), Source::File(sano)]);
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    let report = toolbox::detect(&catalog, &machine(vec![bin_dir], &sandbox.root));
    assert_eq!(report.findings.len(), 1);
    assert!(report.findings[0].presence.is_present());
}

// ── l'elenco è un dato: si estende, si riscrive, si spegne ──────────────

/// La promessa del progetto, provata: una riga di comando nuova si aggiunge
/// scrivendo un descrittore, e nessuna riga di questo crate la nomina.
#[test]
fn a_new_cli_is_declared_without_recompiling_anything() {
    let sandbox = Sandbox::new("newcli");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "openrouter", "echo '1.0.2'");
    let descriptors = sandbox.write(
        "mio.json",
        r#"[{"id": "openrouter-cli", "family": "ai_cli", "label": "OpenRouter CLI",
             "detect": {"command": "openrouter"},
             "version": {"args": ["--version"]}}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let found = &report.findings[0];
    assert_eq!(found.family, "ai_cli");
    assert!(found.presence.is_present(), "{:?}", found.presence);
    assert_eq!(
        found.version,
        VersionReading::Declared("1.0.2".to_string())
    );
}

/// Chi arriva dopo vince sull'`id`, e `disabled` cancella. È il modo per
/// togliere di mezzo un descrittore spedito senza ricompilare.
#[test]
fn a_user_file_overrides_and_switches_off_a_shipped_descriptor() {
    let sandbox = Sandbox::new("override");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "attrezzo", "echo 1.0");
    let spediti = sandbox.write(
        "spediti.json",
        r#"[{"id": "attrezzo", "family": "tool", "label": "quello di serie",
             "detect": {"command": "attrezzo"}},
            {"id": "da-togliere", "family": "tool", "detect": {"command": "attrezzo"}}]"#,
    );
    let miei = sandbox.write(
        "miei.json",
        r#"[{"id": "attrezzo", "family": "tool", "label": "il mio",
             "detect": {"command": "attrezzo"}},
            {"id": "da-togliere", "family": "tool", "detect": {"command": "attrezzo"},
             "disabled": true}]"#,
    );
    let catalog = Catalog::load(&[Source::File(spediti), Source::File(miei.clone())]);
    let report = toolbox::detect(&catalog, &machine(vec![bin_dir], &sandbox.root));
    assert_eq!(report.findings.len(), 1, "{:?}", report.findings);
    assert_eq!(report.findings[0].label, "il mio");
    assert_eq!(
        report.findings[0].descriptor_source,
        miei.to_string_lossy()
    );
}

/// I descrittori spediti col prodotto si caricano tutti: se uno di loro fosse
/// scritto male, questa prova lo direbbe qui invece che sulla macchina di chi
/// installa.
#[test]
fn the_shipped_descriptors_all_load() {
    let catalog = Catalog::load(&[Source::Builtin]);
    assert!(
        catalog.problems.is_empty(),
        "i descrittori spediti non si leggono: {:?}",
        catalog.problems
    );
    assert!(catalog.live().len() > 10, "{}", catalog.live().len());
    let famiglie: Vec<&str> = catalog
        .live()
        .iter()
        .map(|l| l.descriptor.family.as_str())
        .collect();
    for attesa in ["ai_cli", "mcp_server", "tool"] {
        assert!(famiglie.contains(&attesa), "manca la famiglia {attesa}");
    }
}

// ── le voci scoperte leggendo una configurazione ────────────────────────

#[test]
fn servers_declared_in_a_config_file_are_discovered_one_by_one() {
    let sandbox = Sandbox::new("enumerate");
    sandbox.write(
        "config.json",
        r#"{"mcpServers": {"socraticode": {"command": "npx"}, "context7": {"command": "npx"}}}"#,
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "server", "family": "mcp_server",
             "enumerate": {"json_keys": {"files": ["~/config.json"],
                                         "pointer": ["mcpServers"]}}}]"#,
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    let nomi: Vec<&str> = report.findings.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(nomi, vec!["context7", "socraticode"], "{nomi:?}");
    // Ogni voce sa da dove viene: il descrittore e il file che la dichiara.
    assert_eq!(report.findings[0].descriptor_id, "server");
    assert!(
        matches!(&report.findings[0].presence, Presence::Present(why) if why.contains("config.json")),
        "{:?}",
        report.findings[0].presence
    );
}

/// Un file che c'è e non dichiara niente è una misura; un file che non si legge
/// non lo è. La stessa distinzione, sull'altra forma di descrittore.
#[test]
fn an_unreadable_config_is_undetermined_while_an_empty_one_is_absent() {
    let sandbox = Sandbox::new("enumblocked");
    sandbox.write("vuoto.json", r#"{"mcpServers": {}}"#);
    let rotto = sandbox.write("rotto.json", "{ non json");
    let descriptors = sandbox.write(
        "tools.json",
        &format!(
            r#"[{{"id": "vuoto", "family": "mcp_server",
                  "enumerate": {{"json_keys": {{"files": ["~/vuoto.json"],
                                                "pointer": ["mcpServers"]}}}}}},
                {{"id": "rotto", "family": "mcp_server",
                  "enumerate": {{"json_keys": {{"files": ["{}"],
                                                "pointer": ["mcpServers"]}}}}}},
                {{"id": "inesistente", "family": "mcp_server",
                  "enumerate": {{"json_keys": {{"files": ["~/mai-scritto.json"],
                                                "pointer": ["mcpServers"]}}}}}}]"#,
            rotto.to_string_lossy()
        ),
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    let by = |name: &str| {
        report
            .findings
            .iter()
            .find(|f| f.name == name)
            .unwrap_or_else(|| panic!("manca {name}"))
            .presence
            .clone()
    };
    assert!(matches!(by("vuoto"), Presence::Absent(_)), "{:?}", by("vuoto"));
    assert!(
        matches!(by("rotto"), Presence::Undetermined(_)),
        "{:?}",
        by("rotto")
    );
    assert!(
        matches!(by("inesistente"), Presence::Absent(_)),
        "{:?}",
        by("inesistente")
    );
}

/// Un `*` nel cammino raccoglie le voci dichiarate un livello più sotto: senza,
/// tutto ciò che sta per progetto resterebbe invisibile e l'elenco direbbe zero.
#[test]
fn a_star_in_the_pointer_reaches_the_per_project_declarations() {
    let sandbox = Sandbox::new("star");
    sandbox.write(
        "config.json",
        r#"{"projects": {"/a": {"mcpServers": {"uno": {}}},
                          "/b": {"mcpServers": {"due": {}}}}}"#,
    );
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "server", "family": "mcp_server",
             "enumerate": {"json_keys": {"files": ["~/config.json"],
                                         "pointer": ["projects", "*", "mcpServers"]}}}]"#,
    );
    let report = toolbox::detect(&catalog_from(&descriptors), &machine(vec![], &sandbox.root));
    let nomi: Vec<&str> = report.findings.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(nomi, vec!["due", "uno"], "{nomi:?}");
}

// ── dove vive la configurazione ─────────────────────────────────────────

#[test]
fn the_config_paths_say_which_ones_are_really_there() {
    let sandbox = Sandbox::new("config");
    let bin_dir = sandbox.dir("bin");
    fake_binary(&bin_dir, "attrezzo", "echo 1.0");
    sandbox.write("settings.json", "{}");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "attrezzo", "family": "tool", "detect": {"command": "attrezzo"},
             "config": ["~/settings.json", "~/mai-scritto.json"]}]"#,
    );
    let report = toolbox::detect(
        &catalog_from(&descriptors),
        &machine(vec![bin_dir], &sandbox.root),
    );
    let config = &report.findings[0].config;
    assert_eq!(config.len(), 2);
    assert!(config[0].presence.is_present(), "{:?}", config[0]);
    assert!(
        matches!(config[1].presence, Presence::Absent(_)),
        "{:?}",
        config[1]
    );
}

/// Una variabile che non esiste resta scritta com'è: sostituirla col vuoto
/// costruirebbe un percorso plausibile e sbagliato, e chi legge non capirebbe
/// perché il rilevamento guarda in un posto che non ha mai nominato.
#[test]
fn an_undefined_variable_stays_visible_in_the_path() {
    let sandbox = Sandbox::new("var");
    let mut m = machine(vec![], &sandbox.root);
    m.env.insert("MIA_CASA".to_string(), "/una/casa".to_string());
    assert_eq!(m.expand("$MIA_CASA/x"), "/una/casa/x");
    assert_eq!(m.expand("${MIA_CASA}/x"), "/una/casa/x");
    assert_eq!(m.expand("$NON_ESISTE/x"), "$NON_ESISTE/x");
    assert_eq!(
        m.expand("~/y"),
        sandbox.root.join("y").to_string_lossy()
    );
}

// ── l'azione di flusso ──────────────────────────────────────────────────

#[test]
fn the_flow_action_answers_with_the_findings() {
    use flow::{Action, ActionOutcome, SharedState};
    let sandbox = Sandbox::new("action");
    let descriptors = sandbox.write(
        "tools.json",
        r#"[{"id": "mai-installato-davvero", "family": "prova",
             "detect": {"command": "mai-installato-davvero"}}]"#,
    );
    let input = serde_json::json!({
        "descriptor_paths": [descriptors.to_string_lossy()],
        "include_defaults": false,
        "version_probes": false
    });
    let mut shared = SharedState::new();
    let ActionOutcome::Went(output) = toolbox::DetectToolsAction
        .execute(&input, &mut shared)
        .expect("un rilevamento non fallisce per come è il mondo")
    else {
        panic!("un rilevamento eseguito è sempre Went")
    };
    assert_eq!(output["total"], 1);
    assert_eq!(output["present"], 0);
    assert_eq!(output["findings"][0]["descriptor_id"], "mai-installato-davvero");
    assert_eq!(output["findings"][0]["presence"]["state"], "absent");
}

/// Un ingresso che non si legge è l'unico guasto che appartiene all'azione: lo
/// ha scritto chi ha scritto il passo, e va detto a lui.
#[test]
fn the_flow_action_rejects_an_input_it_cannot_read() {
    use flow::{Action, SharedState};
    let mut shared = SharedState::new();
    let input = serde_json::json!({"famiglia": "ai_cli"});
    assert!(toolbox::DetectToolsAction.execute(&input, &mut shared).is_err());
}

#[test]
fn the_registry_finds_the_action_by_its_stable_name() {
    let mut registry = flow::ActionRegistry::default();
    toolbox::register_default(&mut registry);
    assert!(registry.get(toolbox::DETECT_TOOLS_ACTION).is_some());
}

// ── la scoperta per percorsi: i nomi, non «la cartella non è vuota» ──────

/// UN SERVIZIO PER FILE, E CHI LEGGE VUOLE I NOMI. `detect` risponde «c'è o non
/// c'è», e su una cartella di servizi del sistema operativo quella risposta non
/// serve a niente: chi deve decidere cosa migrare ha bisogno di sapere *quali*.
///
/// Il nome è il percorso intero, di proposito: due file che si chiamano uguale
/// in due cartelle diverse sono due automazioni diverse, e la fusione delle voci
/// omonime — quella che serve a un server MCP dichiarato in due posti — li
/// conterebbe per uno solo.
#[test]
fn enumerating_paths_names_every_file_that_matches() {
    let sandbox = Sandbox::new("percorsi");
    let home = sandbox.dir("casa");
    let agents = sandbox.dir("casa/agenti");
    fs::write(agents.join("uno.plist"), "<plist/>").expect("un servizio");
    fs::write(agents.join("due.plist"), "<plist/>").expect("un altro servizio");
    fs::write(agents.join("nota.txt"), "non è un servizio").expect("un file estraneo");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{
            "id": "servizi",
            "family": "automation_schedule",
            "label": "i servizi",
            "enumerate": { "paths": ["~/agenti/*.plist"] }
        }]"#,
    );

    let report = toolbox::detect(&catalog_from(&descriptors), &machine(Vec::new(), &home));

    let mut names: Vec<&str> = report.findings.iter().map(|f| f.name.as_str()).collect();
    names.sort();
    assert_eq!(
        names,
        vec![
            agents.join("due.plist").to_string_lossy().as_ref(),
            agents.join("uno.plist").to_string_lossy().as_ref(),
        ],
        "un nome per file, col percorso intero, e il file estraneo fuori"
    );
    assert!(report.findings.iter().all(|f| f.presence.is_present()));
}

/// «NON C'È NIENTE DA MIGRARE» DETTO A CHI HA VENTI SERVIZI è la bugia peggiore
/// che questo elenco possa dire, ed è quella che veniva naturale: `glob` ingoia
/// l'errore di `read_dir` e restituisce zero percorsi sia per una cartella vuota
/// sia per una che non si legge. Le due risposte sono diverse e devono restare
/// diverse — è la ragione per cui esiste questo file di prove.
#[test]
fn a_folder_that_cannot_be_read_is_not_a_folder_without_automations() {
    let sandbox = Sandbox::new("cartella-chiusa");
    let home = sandbox.dir("casa");
    let closed = sandbox.dir("casa/agenti");
    fs::write(closed.join("uno.plist"), "<plist/>").expect("un servizio che c'è davvero");
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o000)).expect("cartella chiusa");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{
            "id": "servizi",
            "family": "automation_schedule",
            "label": "i servizi",
            "enumerate": { "paths": ["~/agenti/*.plist"] }
        }]"#,
    );

    let report = toolbox::detect(&catalog_from(&descriptors), &machine(Vec::new(), &home));

    assert_eq!(report.findings.len(), 1);
    let presence = &report.findings[0].presence;
    assert!(
        matches!(presence, Presence::Undetermined(_)),
        "una cartella che non si legge non è una cartella vuota: {presence:?}"
    );
}

/// Una cartella che davvero non esiste è un'assenza misurata, e il motivo lo
/// dice: senza questa metà la prova sopra si soddisferebbe rispondendo «non ho
/// potuto guardare» sempre, che è l'altro modo di non dire niente.
#[test]
fn a_folder_that_is_not_there_is_a_measured_absence() {
    let sandbox = Sandbox::new("cartella-assente");
    let home = sandbox.dir("casa");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{
            "id": "servizi",
            "family": "automation_schedule",
            "label": "i servizi",
            "enumerate": { "paths": ["~/agenti/*.plist"] }
        }]"#,
    );

    let report = toolbox::detect(&catalog_from(&descriptors), &machine(Vec::new(), &home));

    assert_eq!(report.findings.len(), 1);
    match &report.findings[0].presence {
        Presence::Absent(reason) => assert!(
            reason.contains("non esiste"),
            "il motivo deve dire che la cartella non c'è: {reason}"
        ),
        other => panic!("una cartella che non esiste è un'assenza: {other:?}"),
    }
}

/// UN `enumerate` CHE NON DICE DOVE GUARDARE non è un elenco vuoto: è un elenco
/// scritto male, e le due si leggono uguali — «qui non c'è niente» — mentre una
/// sola delle due si ripara.
#[test]
fn an_enumerate_without_anywhere_to_look_is_a_problem() {
    let sandbox = Sandbox::new("enumerate-vuoto");
    let descriptors = sandbox.write(
        "elenco.json",
        r#"[{ "id": "vuoto", "family": "tool", "label": "niente", "enumerate": {} }]"#,
    );

    let catalog = catalog_from(&descriptors);

    assert!(catalog.live().is_empty());
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    assert!(
        catalog.problems[0].reason.contains("json_keys"),
        "{:?}",
        catalog.problems[0]
    );
}
