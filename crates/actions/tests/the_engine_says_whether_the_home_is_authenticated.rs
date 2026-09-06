//! What knows whether a home is authenticated is **the engine**, and the
//! descriptor declares in which words it says so.
//!
//! **WHY IT IS PROVED ON HAND-WRITTEN OUTPUTS, NEVER BY CALLING THE COMMAND.** A
//! test launching `codex login status` would report the state of the machine
//! running it, not whether the recognition works: green on an authenticated
//! laptop, red on the same code on a freshly installed machine. That is, **it
//! could not come out differently for the reason it claims**. The outputs here
//! are the real ones, measured on this machine and copied once; the judgement is
//! a pure function, and the tests are the same everywhere.
//!
//! **THE EXIT CODE STAYS OUT, DELIBERATELY.** On this machine both measured
//! engines *do* distinguish by outcome — `codex login status` exits 1 logged out
//! and 0 logged in, and `claude auth status` does the same — but that is a fact
//! about those two, not a rule: `judge_login_status` never receives the outcome,
//! so no future engine can have itself declared authenticated by a zero meaning
//! something else. Same choice as `judge_dry_run`, where the outcome really lied.

use actions::{judge_login_status, LoginRecipe, LoginVerdict};
use models::usage::Pointer;

/// How `codex` declares its own home, as measured.
fn codex_recipe() -> LoginRecipe {
    LoginRecipe {
        args: vec!["login".to_owned(), "status".to_owned()],
        answer: None,
        logged_in_when: vec!["logged in using".to_owned()],
        logged_out_when: vec!["not logged in".to_owned()],
    }
}

/// How `claude` declares it: a boolean field inside a JSON envelope.
fn claude_recipe() -> LoginRecipe {
    LoginRecipe {
        args: vec!["auth".to_owned(), "status".to_owned()],
        answer: Some(Pointer::Path(vec!["loggedIn".to_owned()])),
        logged_in_when: vec!["true".to_owned()],
        logged_out_when: vec!["false".to_owned()],
    }
}

/// The real output of `CODEX_HOME=<directory with no auth.json> codex login
/// status`: nothing on stdout, the answer on **stderr**. Exit 1.
const CODEX_SAYS_NO: &str = "Not logged in";

/// The real output of `codex login status` in the authenticated home. Exit 0.
const CODEX_SAYS_YES: &str = "Logged in using ChatGPT";

/// The real output of `CLAUDE_CONFIG_DIR=<empty directory> claude auth status`.
/// The fields identifying the owner have no place in a test: the two keys
/// carrying the answer are the real ones, to the letter.
const CLAUDE_SAYS_NO: &str = r#"{
  "loggedIn": false,
  "authMethod": "none",
  "apiProvider": "firstParty",
  "analyticsDisabled": false,
  "projectsDirectory": "/una/casa/vuota/projects"
}"#;

const CLAUDE_SAYS_YES: &str = r#"{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "apiProvider": "firstParty",
  "analyticsDisabled": false,
  "projectsDirectory": "/una/casa/vera/projects",
  "subscriptionType": "max"
}"#;

/// **THE ANSWER IS ON STDERR, AND WATCHING ONE PIPE WOULD LOSE IT.**
/// `codex login status` writes nothing on stdout: a judgement reading only there
/// would never find either form and would always say «nobody looked» — failing
/// in silence exactly like the defect this block closes.
#[test]
fn codex_says_it_in_prose_and_both_answers_are_recognised() {
    assert!(
        matches!(
            judge_login_status(&codex_recipe(), "", CODEX_SAYS_NO),
            LoginVerdict::LoggedOut { .. }
        ),
        "una casa senza credenziali deve risultare non autenticata"
    );
    assert!(
        matches!(
            judge_login_status(&codex_recipe(), "", CODEX_SAYS_YES),
            LoginVerdict::LoggedIn { .. }
        ),
        "una casa autenticata deve risultare autenticata"
    );
}

/// Inside a sandbox `codex` prefixes a line of its own — «WARNING: proceeding,
/// even though we could not create PATH aliases» — with nothing to do with
/// credentials. Recognition looks for **the declared words** anywhere in what
/// the engine said, so noise in front does not move it.
#[test]
fn a_warning_line_before_the_answer_does_not_change_the_verdict() {
    let noisy = format!(
        "WARNING: proceeding, even though we could not create PATH aliases: \
         Operation not permitted (os error 1)\n{CODEX_SAYS_NO}"
    );
    assert!(matches!(
        judge_login_status(&codex_recipe(), "", &noisy),
        LoginVerdict::LoggedOut { .. }
    ));
}

/// **THE POINTER IS `usage`'s, NOT A SECOND MECHANISM.** `claude` does not
/// answer in prose: it puts the answer in a boolean field of a JSON envelope.
/// The descriptor says where with the same key path it declares the tokens
/// with, and the words sought are the two values that field can take.
#[test]
fn claude_says_it_in_json_and_the_pointer_reaches_the_boolean() {
    assert!(
        matches!(
            judge_login_status(&claude_recipe(), CLAUDE_SAYS_NO, ""),
            LoginVerdict::LoggedOut { .. }
        ),
        "`\"loggedIn\": false` è un no"
    );
    assert!(
        matches!(
            judge_login_status(&claude_recipe(), CLAUDE_SAYS_YES, ""),
            LoginVerdict::LoggedIn { .. }
        ),
        "`\"loggedIn\": true` è un sì"
    );
}

/// **THE NO IS READ BEFORE THE YES, AND THE ORIGINAL DEFECT IS HERE.** «Not
/// logged in» *contains* «logged in»: looking for the yes words first would find
/// an empty home authenticated, exactly the silence this work closes. The
/// defences are two — the declared words are the measured, longer ones, and the
/// reading order is fixed — because the first depends on whoever writes a
/// descriptor and the second does not.
#[test]
fn the_negative_answer_wins_even_when_it_contains_the_positive_words() {
    let sloppy = LoginRecipe {
        args: vec!["login".to_owned(), "status".to_owned()],
        // The short words a careless descriptor would write.
        logged_in_when: vec!["logged in".to_owned()],
        logged_out_when: vec!["not logged in".to_owned()],
        answer: None,
    };
    assert!(
        matches!(
            judge_login_status(&sloppy, "", CODEX_SAYS_NO),
            LoginVerdict::LoggedOut { .. }
        ),
        "il sì è stato riconosciuto dentro un no: è il difetto originale rimesso"
    );
}

/// **EMPTY MEANS «NOBODY LOOKED», NEVER «IT IS AUTHENTICATED».** It is the line
/// already written for `refuses_without_prompt`, and it holds here word for
/// word: an engine whose descriptor declares no way of asking is not an
/// authenticated engine — it is an engine nobody questioned.
#[test]
fn a_descriptor_that_declares_nothing_never_says_authenticated() {
    let silent = LoginRecipe {
        args: vec!["login".to_owned(), "status".to_owned()],
        answer: None,
        logged_in_when: Vec::new(),
        logged_out_when: Vec::new(),
    };
    assert!(matches!(
        judge_login_status(&silent, "", CODEX_SAYS_YES),
        LoginVerdict::NotDeclared
    ));

    // Half a declaration is not enough either: saying how the yes is recognised
    // and staying silent on the no distinguishes nothing, and the direction it
    // would err in is the reassuring one.
    let half = LoginRecipe {
        args: vec!["login".to_owned(), "status".to_owned()],
        answer: None,
        logged_in_when: vec!["logged in using".to_owned()],
        logged_out_when: Vec::new(),
    };
    assert!(matches!(
        judge_login_status(&half, "", CODEX_SAYS_NO),
        LoginVerdict::NotDeclared
    ));
}

/// An answer resembling neither declared form is not a yes. The engine said
/// something — an update, a network error — and the reader must see its words,
/// not an invented verdict.
#[test]
fn an_answer_neither_form_recognises_is_not_authenticated() {
    let said = "error: could not reach api.openai.com";
    match judge_login_status(&codex_recipe(), "", said) {
        LoginVerdict::Unrecognised { said: words } => assert!(
            words.contains("api.openai.com"),
            "le parole del motore sono la diagnosi: {words}"
        ),
        other => panic!("una risposta che non si riconosce non è un verdetto: {other:?}"),
    }
}

/// **WHAT IS SHOWN IS THE ANSWER, NOT THE WHOLE ENVELOPE, AND THAT IS NO
/// MATTER OF STYLE.**
///
/// The real answer of `claude auth status` carries **the owner's email address,
/// their organisation's identifier, that organisation's name and the
/// subscription type**. That text lands in a `sailor profiles list` row and in a
/// `sailor flow check` report — two outputs pasted into a handover, sent to a
/// colleague, poured into a log. A diagnosis must not carry whoever uses it.
///
/// Where a pointer exists, the value it isolated **is** the answer — «false» is
/// not less precise than the envelope holding it, it is more precise — so that
/// is shown. Where none exists, the subject is already everything the engine
/// said, and `codex` answers a line of prose naming nobody.
#[test]
fn what_gets_shown_is_the_answer_not_the_envelope_around_it() {
    let owner = r#"{
  "loggedIn": true,
  "authMethod": "claude.ai",
  "email": "qualcuno@example.invalid",
  "orgId": "5ae89c2b-0000-0000-0000-000000000000",
  "orgName": "l'organizzazione di qualcuno",
  "subscriptionType": "max"
}"#;
    let LoginVerdict::LoggedIn { said } = judge_login_status(&claude_recipe(), owner, "") else {
        panic!("l'involucro dice di sì");
    };
    for private in [
        "qualcuno@example.invalid",
        "5ae89c2b",
        "l'organizzazione di qualcuno",
        "max",
    ] {
        assert!(
            !said.contains(private),
            "«{private}» è finito in un testo che si stampa e si incolla: {said}"
        );
    }
    assert_eq!(
        said, "true",
        "la risposta è il valore che il puntatore isola"
    );

    // With no pointer there is nothing to isolate, and the engine's prose stays
    // the whole diagnosis: `codex`'s case, where the sentence names nobody.
    let LoginVerdict::LoggedOut { said } = judge_login_status(&codex_recipe(), "", CODEX_SAYS_NO)
    else {
        panic!("la prosa dice di no");
    };
    assert_eq!(said, CODEX_SAYS_NO);
}

/// A pointer that finds nothing — the envelope is not JSON, or the key is
/// missing — leaves the answer unknown. That is the right way to err: an
/// imprecise descriptor worsens the diagnosis, it does not invent a yes.
#[test]
fn a_pointer_that_finds_nothing_never_says_authenticated() {
    let said = "Logged in using ChatGPT";
    assert!(
        matches!(
            judge_login_status(&claude_recipe(), said, ""),
            LoginVerdict::Unrecognised { .. }
        ),
        "un cammino di chiavi su un'uscita in prosa non trova niente, e non è un sì"
    );
}
