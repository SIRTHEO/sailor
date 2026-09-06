//! The routing never eats a real command.
//!
//! **THIS IS THE PROOF THAT COUNTS.** A terminal that now and then does not run
//! what you type is worse than one that never routes at all: it becomes
//! unpredictable, and unpredictability is paid for on every line that comes
//! after. So the proofs below do not ask "does the routing work?" but "can the
//! routing manage **not** to work when it must not?".
//!
//! **THE WORLD IS DECLARED, NOT INHERITED.** `is_command` is a trait precisely
//! so a proof can say which binaries exist. With the real machine, "the guard
//! stops `git`" would be green on this house and red on a house without `git`,
//! and the suite would tell of the machine instead of the code. One proof only,
//! at the end, looks at the real machine: the one checking that the shipped
//! rules load.
//!
//! **THE MUTANT.** Take the call to the guard out of `Router::route` and the
//! `a_real_command_is_never_routed_*` proofs all fall. Verified before
//! declaring this finished.

use std::sync::Arc;
use terminal::{Catalog, CommandLookup, Passed, Routed, Router};

/// A declared world: these words are executable, the others are not.
struct World(Vec<&'static str>);

impl CommandLookup for World {
    fn is_command(&self, word: &str) -> bool {
        self.0.contains(&word)
    }
}

/// A world where nothing is executable: it shows the guard is not the only
/// defence, and that without it the others are not enough.
struct EmptyWorld;

impl CommandLookup for EmptyWorld {
    fn is_command(&self, _word: &str) -> bool {
        false
    }
}

/// Deliberately aggressive rules: they match any line holding a very common
/// word. Were the routing fragile, it would fall here.
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

/// **A REAL COMMAND IS NEVER ROUTED**, not even under a rule that matches it
/// whole.
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

/// The words a shell runs without looking for any binary. In this declared
/// world **nothing** is executable: were the guard to rely on the path alone,
/// `cd /tmp` would end up at a flow.
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

/// Shell syntax passes even when the first word is no known binary: a line
/// with a pipe in it is not a sentence.
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

/// A path is a command even when the binary is not on the search path: `./ls`
/// is not looked for, it is run where it is.
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

/// **IN DOUBT, LET IT THROUGH.** A search path that could not be read whole
/// must count as "it might be there": the trait declares it, and this proof
/// measures it on a world always saying "I do not know" by answering `true`.
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

/// **A REQUEST IS ROUTED**, which is the case all this exists for. Without this
/// proof the others would be satisfied by always letting everything through.
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

/// The explicit marker steps over the guard: it is the only way to send a flow
/// a sentence beginning with a real binary.
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

/// **THE DESCRIPTOR CANNOT SWITCH THE GUARD OFF.** An explicit rule whose
/// marker could begin a command does not load: it is the boundary stopping data
/// from taking power the code never gave it.
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

/// An explicit rule by words does not load: `explicit` is a leave for markers,
/// and a rule by words is no marker.
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

/// The second defence of rules by words: below the declared word count they do
/// not fire.
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

/// Words are compared whole: `statusbar` does not hold the word `status`.
/// Without this a rule by words would become a rule by pieces of word, and
/// nobody writing one expects that.
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

/// **A TERMINAL WITH NO RULES IS AN ORDINARY TERMINAL.** The routing's default
/// is not to route: that is what makes switching it off safe.
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

/// An empty line is neither a request nor a command: it passes, and the
/// terminal makes of it what a shell makes of it — an empty line.
#[test]
fn an_empty_line_is_not_a_request() {
    let router = greedy_router();
    match router.route("   ") {
        Routed::Command { why, .. } => assert_eq!(why, Passed::Empty),
        other => panic!("{other:?}"),
    }
}

/// The rules shipped with the product load, and name a flow that exists in this
/// repository. It is the only proof here that looks at the real world.
#[test]
fn the_shipped_rules_all_load_and_name_a_flow_that_exists() {
    let catalog = Catalog::load(&[terminal::Source::Builtin]);
    assert!(
        catalog.problems.is_empty(),
        "le regole spedite non si leggono: {:?}",
        catalog.problems
    );
    assert!(!catalog.live().is_empty(), "l'elenco spedito è vuoto");

    // **A SHIPPED RULE MAY NAME ONLY A SHIPPED FLOW.** The two travel together
    // inside the binary, so they must match there: this proof used to look for
    // the flow in the repository's `flows/`, where it sat because it was
    // **ours**. Green here and false everywhere else — the same shape as fault
    // 41, two lists that must match and nobody comparing them.
    for loaded in catalog.live() {
        assert!(
            flow::system::FLOWS
                .iter()
                .any(|(name, _)| *name == loaded.route.flow),
            "la regola «{}» manda al flusso «{}», che il prodotto non spedisce: \
             su una macchina che non è la nostra non porta da nessuna parte",
            loaded.route.id,
            loaded.route.flow
        );
    }
}

/// The same `id` written twice: the last loaded wins, and that is how a user
/// rewrites a shipped rule without deleting it.
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

/// One bad line does not erase the good ones: without this rule a partial list
/// would look empty, which is worse.
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

/// A rule switched off vanishes from the live list: that is how a shipped rule
/// is taken away without touching the binary.
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
