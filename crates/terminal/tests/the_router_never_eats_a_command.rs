//! Lo smistamento non si mangia un comando vero.
//!
//! **QUESTA È LA PROVA CHE CONTA.** Un terminale che ogni tanto non esegue
//! quello che scrivi è peggio di uno che non smista affatto: diventa
//! imprevedibile, e l'imprevedibilità si paga su tutte le righe che verranno
//! dopo. Quindi le prove qui sotto non chiedono «lo smistamento funziona?» ma
//! «lo smistamento riesce a **non** funzionare quando deve?».
//!
//! **IL MONDO È DICHIARATO, NON EREDITATO.** `is_command` è un tratto proprio
//! perché una prova possa dire quali binari esistono. Con la macchina vera,
//! «la guardia ferma `git`» sarebbe verde su questa casa e rossa su una casa
//! senza `git`, e la batteria racconterebbe della macchina invece che del
//! codice. Una sola prova, in fondo, guarda la macchina vera: quella che
//! controlla che le regole spedite si carichino.
//!
//! **IL MUTANTE.** Togliendo la chiamata alla guardia da `Router::route`, le
//! prove `a_real_command_is_never_routed_*` cadono tutte. È stato verificato
//! prima di dichiarare finito.

use std::sync::Arc;
use terminal::{Catalog, CommandLookup, Passed, Routed, Router};

/// Un mondo dichiarato: queste parole sono eseguibili, le altre no.
struct World(Vec<&'static str>);

impl CommandLookup for World {
    fn is_command(&self, word: &str) -> bool {
        self.0.contains(&word)
    }
}

/// Un mondo in cui niente è eseguibile: serve a mostrare che la guardia non è
/// l'unica difesa, e che senza di lei le altre non bastano.
struct EmptyWorld;

impl CommandLookup for EmptyWorld {
    fn is_command(&self, _word: &str) -> bool {
        false
    }
}

/// Regole apposta aggressive: riconoscono qualunque riga contenga una parola
/// comunissima. Se lo smistamento fosse fragile, cadrebbe qui.
const GREEDY_RULES: &str = r#"[
  {"id": "marked", "flow": "un-flusso", "when": {"kind": "starts_with", "text": "? "},
   "explicit": true, "strip_match": true},
  {"id": "greedy", "flow": "un-flusso", "when": {"kind": "contains_all", "words": ["status"]}},
  {"id": "greedier", "flow": "un-flusso", "when": {"kind": "contains_all", "words": ["ls"]}}
]"#;

fn router_with(rules: &str, world: Arc<dyn CommandLookup>) -> Router {
    let mut catalog = Catalog::default();
    catalog.absorb("prova", rules);
    assert!(
        catalog.problems.is_empty(),
        "le regole della prova non si caricano: {:?}",
        catalog.problems
    );
    Router::new(&catalog, world)
}

fn greedy_router() -> Router {
    router_with(
        GREEDY_RULES,
        Arc::new(World(vec!["git", "ls", "cargo", "sailor"])),
    )
}

/// **UN COMANDO VERO NON VIENE MAI SMISTATO**, nemmeno con una regola che lo
/// riconosce per intero.
#[test]
fn a_real_command_is_never_routed_when_its_first_word_is_runnable() {
    let router = greedy_router();
    for line in [
        "git status",
        "ls",
        "cargo test -p terminal",
        "sailor flow list",
    ] {
        match router.route(line) {
            Routed::Command { why, .. } => assert!(
                matches!(why, Passed::RunnableFirstWord(_)),
                "«{line}» è passata per il motivo sbagliato: {why:?}"
            ),
            other => panic!("«{line}» è stata mangiata dallo smistamento: {other:?}"),
        }
    }
}

/// Le parole che una shell esegue senza cercare nessun binario. In questo mondo
/// dichiarato **niente** è eseguibile: se la guardia contasse solo sul percorso,
/// `cd /tmp` finirebbe a un flusso.
#[test]
fn a_real_command_is_never_routed_when_it_is_a_shell_word() {
    let router = router_with(GREEDY_RULES, Arc::new(EmptyWorld));
    for line in ["cd /tmp", "export STATUS=1", "exit", "source ~/.zshrc"] {
        match router.route(line) {
            Routed::Command { .. } => {}
            other => panic!("«{line}» è stata mangiata dallo smistamento: {other:?}"),
        }
    }
}

/// La sintassi di shell passa anche quando la prima parola non è un binario
/// noto: una riga con una pipe dentro non è una frase.
#[test]
fn a_real_command_is_never_routed_when_it_has_shell_syntax() {
    let router = router_with(GREEDY_RULES, Arc::new(EmptyWorld));
    for line in [
        "qualcosa | wc -l",
        "qualcosa > status.txt",
        "qualcosa && altro",
        "echo $(qualcosa) status",
        "qualcosa; ls",
    ] {
        match router.route(line) {
            Routed::Command { .. } => {}
            other => panic!("«{line}» è stata mangiata dallo smistamento: {other:?}"),
        }
    }
}

/// Un percorso è un comando anche se il binario non è nel percorso di ricerca:
/// `./ls` non si cerca, si esegue dov'è.
#[test]
fn a_real_command_is_never_routed_when_it_names_a_file() {
    let router = router_with(GREEDY_RULES, Arc::new(EmptyWorld));
    for line in [
        "./status.sh",
        "/usr/bin/ls",
        "~/bin/status",
        "../tools/ls -la",
    ] {
        match router.route(line) {
            Routed::Command { why, .. } => {
                assert!(matches!(why, Passed::PathLike(_)), "«{line}»: {why:?}")
            }
            other => panic!("«{line}» è stata mangiata dallo smistamento: {other:?}"),
        }
    }
}

/// **NEL DUBBIO, PASSA.** Un percorso di ricerca che non si è potuto leggere
/// tutto deve valere come «forse c'è»: il tratto lo dichiara, e questa prova lo
/// misura su un mondo che dice sempre «non so» rispondendo `true`.
#[test]
fn a_doubt_about_the_machine_becomes_a_command_not_a_request() {
    struct AlwaysUnsure;
    impl CommandLookup for AlwaysUnsure {
        fn is_command(&self, _word: &str) -> bool {
            true
        }
    }
    let router = router_with(GREEDY_RULES, Arc::new(AlwaysUnsure));
    match router.route("trova tutto quello che riguarda status") {
        Routed::Command { .. } => {}
        other => panic!("un dubbio è diventato una richiesta: {other:?}"),
    }
}

/// **UNA RICHIESTA VIENE SMISTATA**, ed è il caso per cui tutto questo esiste.
/// Senza questa prova le altre si soddisfarebbero passando sempre.
#[test]
fn a_request_that_is_not_a_command_goes_to_the_flow() {
    let router = greedy_router();
    match router.route("controlla lo status di tutto") {
        Routed::Flow { route, flow, text } => {
            assert_eq!(route, "greedy");
            assert_eq!(flow, "un-flusso");
            assert_eq!(text, "controlla lo status di tutto");
        }
        other => panic!("una richiesta è rimasta un comando: {other:?}"),
    }
}

/// Il marcatore esplicito scavalca la guardia: è l'unico modo per mandare a un
/// flusso una frase che comincia con un binario vero.
#[test]
fn a_marked_line_goes_to_the_flow_even_if_it_looks_like_a_command() {
    let router = greedy_router();
    match router.route("? git status mi dice cose strane, indaga") {
        Routed::Flow { route, text, .. } => {
            assert_eq!(route, "marked");
            assert_eq!(
                text, "git status mi dice cose strane, indaga",
                "il marcatore non deve arrivare al flusso"
            );
        }
        other => panic!("la riga marcata non è stata smistata: {other:?}"),
    }
}

/// **IL DESCRITTORE NON PUÒ SPEGNERE LA GUARDIA.** Una regola esplicita il cui
/// marcatore potrebbe iniziare un comando non si carica: è il confine che
/// impedisce ai dati di prendersi il potere che il codice non gli dà.
#[test]
fn an_explicit_rule_whose_marker_could_start_a_command_is_refused() {
    let mut catalog = Catalog::default();
    catalog.absorb(
        "prova",
        r#"[{"id": "pericolosa", "flow": "un-flusso",
             "when": {"kind": "starts_with", "text": "git "}, "explicit": true}]"#,
    );
    assert!(catalog.live().is_empty(), "{:?}", catalog.live());
    assert_eq!(catalog.problems.len(), 1);
    assert!(
        catalog.problems[0].reason.contains("guardia"),
        "{}",
        catalog.problems[0].reason
    );
}

/// Una regola esplicita per parole non si carica: `explicit` è un permesso per i
/// marcatori, e una regola per parole non è un marcatore.
#[test]
fn an_explicit_rule_that_is_not_a_marker_is_refused() {
    let mut catalog = Catalog::default();
    catalog.absorb(
        "prova",
        r#"[{"id": "confusa", "flow": "un-flusso",
             "when": {"kind": "contains_all", "words": ["sailor"]}, "explicit": true}]"#,
    );
    assert!(catalog.live().is_empty());
    assert_eq!(catalog.problems.len(), 1);
}

/// La seconda difesa delle regole per parole: sotto il numero di parole
/// dichiarato non scattano.
#[test]
fn a_short_line_does_not_reach_a_rule_that_asks_for_a_long_one() {
    let router = router_with(
        r#"[{"id": "lunga", "flow": "un-flusso",
             "when": {"kind": "contains_all", "words": ["sailor"]}, "minimum_words": 4}]"#,
        Arc::new(EmptyWorld),
    );
    match router.route("sailor adesso") {
        Routed::Command { why, .. } => assert_eq!(why, Passed::NoRuleMatched),
        other => panic!("due parole non sono una richiesta: {other:?}"),
    }
    match router.route("cosa manca ancora a sailor") {
        Routed::Flow { route, .. } => assert_eq!(route, "lunga"),
        other => panic!("cinque parole lo sono: {other:?}"),
    }
}

/// Le parole si confrontano intere: `statusbar` non contiene la parola
/// `status`. Senza questo, una regola per parole diventerebbe una regola per
/// pezzi di parola, e nessuno che la scrive se lo aspetta.
#[test]
fn a_word_rule_matches_words_and_not_pieces_of_them() {
    let router = router_with(
        r#"[{"id": "parola", "flow": "un-flusso",
             "when": {"kind": "contains_all", "words": ["status"]}}]"#,
        Arc::new(EmptyWorld),
    );
    match router.route("guarda la statusbar in alto") {
        Routed::Command { why, .. } => assert_eq!(why, Passed::NoRuleMatched),
        other => panic!("«statusbar» non è «status»: {other:?}"),
    }
}

/// **UN TERMINALE SENZA REGOLE È UN TERMINALE NORMALE.** Il valore predefinito
/// dello smistamento è non smistare: è ciò che rende sicuro spegnerlo.
#[test]
fn with_no_rules_everything_is_a_command() {
    let router = Router::without_routes(Arc::new(EmptyWorld));
    for line in [
        "git status",
        "trova i residui di configurazione",
        "? qualsiasi cosa",
    ] {
        assert!(
            matches!(router.route(line), Routed::Command { .. }),
            "senza regole non si smista niente: «{line}»"
        );
    }
}

/// Una riga vuota non è una richiesta e non è un comando: passa, e il terminale
/// ne fa quello che ne fa una shell — una riga vuota.
#[test]
fn an_empty_line_is_not_a_request() {
    let router = greedy_router();
    match router.route("   ") {
        Routed::Command { why, .. } => assert_eq!(why, Passed::Empty),
        other => panic!("{other:?}"),
    }
}

/// Le regole spedite col prodotto si caricano, e nominano un flusso che esiste
/// in questo repo. È l'unica prova qui che guarda il mondo vero.
#[test]
fn the_shipped_rules_all_load_and_name_a_flow_that_exists() {
    let catalog = Catalog::load(&[terminal::Source::Builtin]);
    assert!(
        catalog.problems.is_empty(),
        "le regole spedite non si leggono: {:?}",
        catalog.problems
    );
    assert!(!catalog.live().is_empty(), "l'elenco spedito è vuoto");

    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(std::path::Path::parent)
        .expect("il crate sta due livelli sotto la radice")
        .to_path_buf();
    for loaded in catalog.live() {
        let file = root
            .join("flows")
            .join(format!("{}.flow.json", loaded.route.flow));
        assert!(
            file.exists(),
            "la regola «{}» manda al flusso «{}», che non esiste in flows/",
            loaded.route.id,
            loaded.route.flow
        );
    }
}

/// Lo stesso `id` scritto due volte: vince l'ultimo caricato, ed è così che un
/// utente riscrive una regola spedita senza cancellarla.
#[test]
fn the_last_rule_with_an_id_wins() {
    let mut catalog = Catalog::default();
    catalog.absorb(
        "spedito",
        r#"[{"id": "x", "flow": "primo", "when": {"kind": "starts_with", "text": "? "}}]"#,
    );
    catalog.absorb(
        "mio",
        r#"[{"id": "x", "flow": "secondo", "when": {"kind": "starts_with", "text": "? "}}]"#,
    );
    assert_eq!(catalog.live().len(), 1);
    assert_eq!(catalog.live()[0].route.flow, "secondo");
    assert_eq!(catalog.live()[0].source, "mio");
}

/// Una riga sbagliata non cancella quelle buone: senza questa regola un elenco
/// parziale sembrerebbe vuoto, che è peggio.
#[test]
fn a_broken_rule_does_not_take_the_good_ones_with_it() {
    let mut catalog = Catalog::default();
    catalog.absorb(
        "prova",
        r#"[{"id": "buona", "flow": "un-flusso", "when": {"kind": "starts_with", "text": "? "}},
            {"id": "rotta", "flow": "un-flusso", "when": {"kind": "inventata"}}]"#,
    );
    assert_eq!(catalog.live().len(), 1);
    assert_eq!(catalog.problems.len(), 1);
    assert_eq!(catalog.problems[0].about, "rotta");
}

/// Una regola spenta sparisce dall'elenco vivo: è così che si toglie una regola
/// spedita senza toccare il binario.
#[test]
fn a_disabled_rule_disappears() {
    let mut catalog = Catalog::default();
    catalog.absorb(
        "spedito",
        r#"[{"id": "x", "flow": "f", "when": {"kind": "starts_with", "text": "? "}}]"#,
    );
    catalog.absorb(
        "mio",
        r#"[{"id": "x", "flow": "f", "when": {"kind": "starts_with", "text": "? "},
             "disabled": true}]"#,
    );
    assert!(catalog.live().is_empty());
    assert!(catalog.known().is_empty());
}
