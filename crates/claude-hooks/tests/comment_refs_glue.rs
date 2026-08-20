//! La colla del gancio `comment-refs`, provata sul binario vero.
//!
//! `judge()` è provata da undici casi dentro `guards`, ma il ramo che legge il
//! payload, sceglie la fase dall'evento, interroga il disco per l'esenzione e
//! traduce la decisione in stdout/uscita non era provato da niente: è lo stesso
//! buco che il 20/08/2026 valeva per gli altri quattro ganci in servizio.
//!
//! Il caso che conta è il secondo: `written_text()` è condivisa con
//! `code-language`, e la correzione che le ha insegnato a confrontare
//! `new_string` con `old_string` arriva qui per eredità. Fino a oggi
//! quell'eredità era dedotta, non misurata — due tentativi dal vivo non avevano
//! innescato il gancio, perché in zsh `echo` traduce `\n` e il JSON arrivava
//! rotto.

use std::io::Write;
use std::process::{Command, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_claude-hooks");

struct Outcome {
    code: i32,
    stdout: String,
    stderr: String,
}

/// Lancia il gancio con un payload sullo standard input.
///
/// L'ambiente si ripulisce dalla valvola: una sessione che avesse
/// `COMMENT_REFS=off` esportata renderebbe verdi per finta tutti i casi che
/// pretendono un rifiuto.
fn run(payload: &str, valve: Option<&str>) -> Outcome {
    let mut cmd = Command::new(BIN);
    cmd.arg("comment-refs")
        .env_remove("COMMENT_REFS")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(v) = valve {
        cmd.env("COMMENT_REFS", v);
    }
    let mut child = cmd.spawn().expect("the hook binary did not start");
    child
        .stdin
        .as_mut()
        .expect("no stdin")
        .write_all(payload.as_bytes())
        .expect("could not write the payload");
    let out = child.wait_with_output().expect("the hook did not finish");
    Outcome {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
    }
}

fn denies(o: &Outcome) -> bool {
    o.stdout.contains("\"permissionDecision\": \"deny\"")
}

fn edit(path: &str, old: &str, new: &str) -> String {
    serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Edit",
        "tool_input": { "file_path": path, "old_string": old, "new_string": new },
    })
    .to_string()
}

const TS: &str = "/x/src/a.ts";
const REF: &str = "// vedi .claude/docs/piano.md";

#[test]
fn a_reference_this_edit_introduces_is_denied() {
    let o = run(&edit(TS, "const a = 1;", &format!("{REF}\nconst a = 2;")), None);
    assert!(denies(&o), "stdout: {}", o.stdout);
    assert!(o.stdout.contains("un albero ignorato da git"));
    assert_eq!(o.code, 0, "in the pre phase the denial travels in stdout");
}

#[test]
fn a_reference_copied_from_old_string_is_not_denied() {
    // Il contesto ricopiato attorno alla riga toccata non è testo scritto ora.
    let o = run(
        &edit(
            TS,
            &format!("{REF}\nconst a = 1;"),
            &format!("{REF}\nconst a = 2;"),
        ),
        None,
    );
    assert!(!denies(&o), "stdout: {}", o.stdout);
    assert_eq!(o.code, 0);
}

#[test]
fn only_old_string_separates_the_two_verdicts() {
    // Il differenziale a variabile unica: lo stesso `new_string`, due
    // `old_string`. Se la colla smettesse di guardare `old_string` — la
    // mutazione che questi casi devono saper cogliere — i due esiti
    // diventerebbero uguali e questo caso andrebbe rosso. Provarlo così, invece
    // di indebolire il gate per un attimo, evita di lasciare in servizio un
    // controllo mutilato se la prova si interrompe a metà.
    let new = format!("{REF}\nconst a = 2;");
    let introduced = run(&edit(TS, "const a = 1;", &new), None);
    let copied = run(&edit(TS, &format!("{REF}\nconst a = 1;"), &new), None);
    assert!(denies(&introduced), "stdout: {}", introduced.stdout);
    assert!(!denies(&copied), "stdout: {}", copied.stdout);
}

#[test]
fn a_write_is_judged_on_its_whole_content() {
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "Write",
        "tool_input": { "file_path": TS, "content": format!("{REF}\nconst a = 1;") },
    })
    .to_string();
    let o = run(&payload, None);
    assert!(denies(&o), "stdout: {}", o.stdout);
}

#[test]
fn a_multiedit_is_judged_edit_by_edit() {
    let payload = serde_json::json!({
        "hook_event_name": "PreToolUse",
        "tool_name": "MultiEdit",
        "tool_input": {
            "file_path": TS,
            "edits": [
                { "old_string": format!("{REF}\nconst a = 1;"),
                  "new_string": format!("{REF}\nconst a = 2;") },
                { "old_string": "const b = 1;",
                  "new_string": "// Plans.md FLD-8\nconst b = 2;" },
            ],
        },
    })
    .to_string();
    let o = run(&payload, None);
    assert!(denies(&o), "stdout: {}", o.stdout);
    assert!(
        o.stdout.contains("un piano o un rapporto"),
        "only the second edit introduces a reference: {}",
        o.stdout
    );
}

#[test]
fn the_post_phase_blocks_with_exit_two_and_speaks_on_stderr() {
    // In `post` il file è già scritto: negare non serve più, e il canale che
    // l'assistente legge è l'uscita 2 con il messaggio su stderr.
    let payload = serde_json::json!({
        "hook_event_name": "PostToolUse",
        "tool_name": "Edit",
        "tool_input": { "file_path": TS, "old_string": "const a = 1;",
                        "new_string": format!("{REF}\nconst a = 2;") },
    })
    .to_string();
    let o = run(&payload, None);
    assert_eq!(o.code, 2, "stdout: {} stderr: {}", o.stdout, o.stderr);
    assert!(o.stderr.contains("un albero ignorato da git"));
    assert!(!denies(&o), "the post phase must not emit a deny");
}

#[test]
fn the_valve_lets_everything_through() {
    let o = run(
        &edit(TS, "const a = 1;", &format!("{REF}\nconst a = 2;")),
        Some("off"),
    );
    assert!(!denies(&o), "stdout: {}", o.stdout);
    assert_eq!(o.code, 0);
}

#[test]
fn a_payload_without_a_path_is_left_alone() {
    let o = run(
        &edit("", "const a = 1;", &format!("{REF}\nconst a = 2;")),
        None,
    );
    assert!(!denies(&o), "stdout: {}", o.stdout);
}

#[test]
fn a_file_that_declares_itself_the_test_bench_is_exempt() {
    // L'esenzione si legge dal disco, non dal payload: è l'unico pezzo della
    // colla che tocca il filesystem. Anche qui una variabile sola — stesso
    // percorso, stesso payload, cambia solo cosa c'è scritto nel file.
    let dir = std::env::temp_dir().join(format!("comment-refs-glue-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("could not create the temp dir");
    let file = dir.join("bench.ts");
    let payload = edit(
        file.to_str().unwrap(),
        "const a = 1;",
        &format!("{REF}\nconst a = 2;"),
    );

    std::fs::write(&file, "const a = 1;\n").expect("could not write the file");
    let plain = run(&payload, None);

    std::fs::write(&file, "// comment-refs: banco di prova\nconst a = 1;\n")
        .expect("could not write the file");
    let bench = run(&payload, None);

    let _ = std::fs::remove_dir_all(&dir);
    assert!(denies(&plain), "stdout: {}", plain.stdout);
    assert!(!denies(&bench), "stdout: {}", bench.stdout);
}

#[test]
fn a_non_code_extension_is_left_alone() {
    let o = run(
        &edit("/x/docs/nota.md", "riga", &format!("{REF}\nriga")),
        None,
    );
    assert!(!denies(&o), "stdout: {}", o.stdout);
}

#[test]
fn broken_json_is_let_through_not_crashed() {
    let o = run("{ non e json", None);
    assert_eq!(o.code, 0);
    assert!(!denies(&o));
}
