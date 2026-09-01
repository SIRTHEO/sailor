//! Chi sa se una casa è autenticata è **il motore**, e il descrittore dichiara
//! con quali parole lo dice.
//!
//! **PERCHÉ SI PROVA SU USCITE SCRITTE A MANO, E MAI CHIAMANDO IL COMANDO.** Una
//! prova che lanciasse `codex login status` direbbe come sta messa la macchina di
//! chi la esegue, non se il riconoscimento funziona: verde su un portatile
//! autenticato, rossa sullo stesso codice su una macchina appena installata.
//! Cioè **non potrebbe venire diversa per la ragione che dichiara**. Qui le
//! uscite sono quelle vere, misurate il 01/09/2026 su questa macchina e copiate
//! una volta sola; il giudizio è una funzione pura, e le prove sono le stesse
//! ovunque.
//!
//! **IL CODICE D'USCITA NON ENTRA, ED È DELIBERATO.** Su questa macchina i due
//! motori misurati *distinguono* con l'esito — `codex login status` esce 1 non
//! autenticato e 0 autenticato, e `claude auth status` fa lo stesso — ma è un
//! fatto di quei due, non una regola: `judge_login_status` non riceve l'esito,
//! così nessun motore futuro può farsi dichiarare autenticato da uno zero che
//! vuol dire tutt'altro. È la stessa scelta di `judge_dry_run`, e lì l'esito
//! mentiva davvero.

use actions::{judge_login_status, LoginRecipe, LoginVerdict};
use models::usage::Pointer;

/// Come `codex` dichiara la propria casa, misurato il 01/09/2026.
fn codex_recipe() -> LoginRecipe {
    LoginRecipe {
        args: vec!["login".to_owned(), "status".to_owned()],
        answer: None,
        logged_in_when: vec!["logged in using".to_owned()],
        logged_out_when: vec!["not logged in".to_owned()],
    }
}

/// Come `claude` la dichiara: un campo booleano dentro un involucro JSON.
fn claude_recipe() -> LoginRecipe {
    LoginRecipe {
        args: vec!["auth".to_owned(), "status".to_owned()],
        answer: Some(Pointer::Path(vec!["loggedIn".to_owned()])),
        logged_in_when: vec!["true".to_owned()],
        logged_out_when: vec!["false".to_owned()],
    }
}

/// L'uscita vera di `CODEX_HOME=<cartella senza auth.json> codex login status`:
/// niente su stdout, la risposta su **stderr**. Uscita 1.
const CODEX_SAYS_NO: &str = "Not logged in";

/// L'uscita vera di `codex login status` nella casa autenticata. Uscita 0.
const CODEX_SAYS_YES: &str = "Logged in using ChatGPT";

/// L'uscita vera di `CLAUDE_CONFIG_DIR=<cartella vuota> claude auth status`.
/// I campi che identificano il proprietario non stanno in una prova: le due
/// chiavi che portano la risposta sono quelle vere, alla lettera.
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

/// **LA RISPOSTA STA SU STDERR, E GUARDARE UNA PIPA SOLA LA PERDEREBBE.**
/// `codex login status` non scrive niente su stdout: un giudizio che leggesse
/// solo di là non troverebbe mai nessuna delle due forme e direbbe sempre
/// «nessuno ha guardato», cioè fallirebbe in silenzio proprio come il difetto
/// che questo blocco chiude.
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

/// Sotto perimetro `codex` premette una riga sua — «WARNING: proceeding, even
/// though we could not create PATH aliases» — che non c'entra con le
/// credenziali. Il riconoscimento cerca **le parole dichiarate** dentro tutto
/// ciò che il motore ha detto, quindi il rumore davanti non lo sposta.
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

/// **IL PUNTATORE È QUELLO DI `usage`, NON UN SECONDO MECCANISMO.** `claude` non
/// risponde in prosa: mette la risposta in un campo booleano di un involucro
/// JSON. Il descrittore dice dove sta con lo stesso cammino di chiavi con cui
/// dichiara dove stanno i token, e le parole cercate sono i due valori che quel
/// campo può prendere.
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

/// **IL NO SI LEGGE PRIMA DEL SÌ, E QUI STA IL DIFETTO ORIGINALE.** «Not logged
/// in» *contiene* «logged in»: chi cercasse prima le parole del sì troverebbe
/// una casa vuota autenticata, che è esattamente il silenzio che questo lavoro
/// chiude. Le due difese sono due — le parole dichiarate sono quelle misurate e
/// più lunghe, e l'ordine di lettura è vincolato — perché la prima dipende da chi
/// scrive un descrittore e la seconda no.
#[test]
fn the_negative_answer_wins_even_when_it_contains_the_positive_words() {
    let sloppy = LoginRecipe {
        args: vec!["login".to_owned(), "status".to_owned()],
        // Le parole corte che un descrittore distratto scriverebbe.
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

/// **VUOTO VUOL DIRE «NESSUNO HA GUARDATO», MAI «È AUTENTICATO».** È la frase
/// già scritta per `refuses_without_prompt`, e vale qui identica: un motore il
/// cui descrittore non dichiara come si chiede non è un motore autenticato — è
/// un motore che nessuno ha interrogato.
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

    // Anche mezza dichiarazione non basta: chi dice come si riconosce il sì e
    // tace sul no non può distinguere niente, e il verso in cui sbaglierebbe è
    // quello che tranquillizza.
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

/// Una risposta che non somiglia a nessuna delle due forme dichiarate non è un
/// sì. Il motore ha risposto qualcosa — un aggiornamento, un errore di rete —
/// e chi legge deve vedere le sue parole, non un verdetto inventato.
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

/// Un puntatore che non trova niente — l'involucro non è JSON, o la chiave non
/// c'è — lascia la risposta sconosciuta. È il modo giusto di sbagliare: un
/// descrittore impreciso peggiora la diagnosi, e non inventa un sì.
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
