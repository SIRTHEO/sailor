//! Il payload dà quello che ha, e quello che non ha non ferma niente.
//!
//! **CHI ARRIVA NON DEVE SAPERE COSA CI SERVE.** Un gancio manda il proprio
//! JSON, che è scritto da qualcun altro e cambierà senza avvisare. Se un campo
//! in più lo facesse rifiutare, ogni versione nuova del programma che scrive
//! quel JSON spegnerebbe il tracciamento — è il guasto 8, che parlava dei
//! descrittori e vale identico qui. Se un campo mancante lo facesse fallire,
//! non si potrebbe tracciare niente che non sia esattamente un gancio.

use sessions::{anchor_from, Census, Payload};

fn nothing_seen() -> Census {
    Census::NoTerminal
}

#[test]
fn the_four_fields_are_read_when_they_are_there() {
    let payload = Payload::parse(
        r#"{"session_id":"abc","transcript_path":"/tmp/abc.jsonl",
            "cwd":"/somewhere","hook_event_name":"PreToolUse"}"#,
    )
    .expect("è JSON");
    assert_eq!(payload.session_id.as_deref(), Some("abc"));
    assert_eq!(payload.transcript_path.as_deref(), Some("/tmp/abc.jsonl"));
    assert_eq!(payload.cwd.as_deref(), Some("/somewhere"));
    assert_eq!(payload.hook_event_name.as_deref(), Some("PreToolUse"));
}

/// Un campo che questa versione non conosce è un campo in più, non un payload
/// rotto.
#[test]
fn a_field_we_do_not_know_is_not_a_reason_to_refuse_the_rest() {
    let payload = Payload::parse(r#"{"session_id":"abc","something_new":{"deep":[1,2]}}"#)
        .expect("un campo ignoto non fa rifiutare il payload");
    assert_eq!(payload.session_id.as_deref(), Some("abc"));
}

/// Niente su standard input è il caso di chi invoca il comando a mano: ha
/// comunque un tty e una cartella.
#[test]
fn nothing_at_all_is_an_empty_payload_and_not_an_error() {
    assert_eq!(Payload::parse("").expect("vuoto"), Payload::default());
    assert_eq!(Payload::parse("   \n ").expect("solo spazi"), Payload::default());
}

/// Un testo che non è JSON invece si dice: è un gancio scritto male, e tacere
/// lo lascerebbe scritto male per sempre.
#[test]
fn something_that_is_not_json_is_said_out_loud() {
    let complaint = Payload::parse("non sono json").expect_err("non è JSON");
    assert!(complaint.contains("JSON"), "{complaint}");
}

/// L'albero cade sulla cartella corrente quando il payload non lo dice, e il
/// capostipite resta ignoto quando il censimento non lo sa: **ignoto non è
/// vuoto**.
#[test]
fn the_anchor_falls_back_without_inventing_anything() {
    let payload = Payload::default();
    let anchor = anchor_from(&payload, "ttys004".to_owned(), &nothing_seen());
    assert_eq!(anchor.tty, "ttys004");
    assert!(!anchor.worktree.is_empty());
    assert_eq!(
        anchor.ancestor, None,
        "un capostipite che non si è potuto leggere resta None, non una stringa vuota"
    );

    let declared = Payload::parse(r#"{"cwd":"/declared"}"#).expect("è JSON");
    assert_eq!(
        anchor_from(&declared, "ttys004".to_owned(), &nothing_seen()).worktree,
        "/declared"
    );
}

/// Il nome corto e quello lungo dello stesso terminale sono la stessa chiave.
#[test]
fn the_long_name_of_a_terminal_and_the_short_one_are_one_key() {
    assert_eq!(sessions::tty::short_name("/dev/ttys004"), "ttys004");
    assert_eq!(sessions::tty::short_name("ttys004"), "ttys004");
}

/// L'etichetta di chi ha disegnato la finestra: il nome dell'applicazione
/// ospite, non l'involucro che sta più in fondo al percorso.
#[test]
fn the_label_names_the_application_and_not_the_wrapper() {
    assert_eq!(
        sessions::census::label_for(
            "/Applications/Whatever.app/Contents/Frameworks/Whatever Helper.app/Contents/MacOS/Whatever Helper"
        ),
        "Whatever"
    );
    assert_eq!(sessions::census::label_for("/bin/zsh"), "zsh");
    assert_eq!(sessions::census::label_for("caffeinate"), "caffeinate");
}
