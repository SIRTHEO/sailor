//! Il registro del gate, provato dove `HOME` si può sostituire.
//!
//! PERCHÉ STA QUI E NON ACCANTO AGLI ALTRI CASI. `journal::record` scrive sotto
//! `$HOME`, che è una variabile del **processo**: sostituirla dentro la batteria
//! di `guards` la sostituirebbe anche per i casi che girano in parallelo — tre
//! moduli la leggono per risolvere una tilde o trovare lo stato — e un lucchetto
//! per file dà l'aspetto dell'isolamento senza la sostanza. Un file di prova è
//! un binario a sé: qui dentro non gira nient'altro, e il registro vero non
//! riceve nemmeno una riga.
//!
//! COSA PROTEGGE. Senza queste righe il gate lavora e non lo si può smentire:
//! ogni misura futura sulla sua efficacia leggerebbe zero e concluderebbe «non
//! serve». Il passaggio si registra quanto il blocco, o non esce nessun tasso.

use guards::socraticode_gate::{record, Verdict};
use hook_io::Decision;

fn verdict(decision: Decision, reason: &'static str, count: Option<i64>) -> Verdict {
    Verdict {
        decision,
        reason,
        path: Some("/repo/src/index.ts".to_string()),
        count,
    }
}

#[test]
fn the_journal_gets_one_line_for_each_case_of_the_gates_own_business() {
    let home = hook_io::testing::test_dir("socraticode-gate-registro");
    std::env::set_var("HOME", &home);
    let journal = home.join(".claude").join("state").join("ganci.jsonl");

    record(
        &verdict(Decision::Block("…".to_string()), "ricerca-concettuale", None),
        "Grep",
        "sessione1",
    );
    record(
        &verdict(Decision::Pass, "sotto-quota-impact", Some(7)),
        "Edit",
        "sessione1",
    );
    // Fuori perimetro: il registro conta i casi DI COMPETENZA, e sporcarlo con
    // ogni chiamata a strumento renderebbe il denominatore inutile.
    record(&verdict(Decision::Pass, "", None), "Read", "sessione1");

    let text = std::fs::read_to_string(&journal).expect("il registro deve esistere");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "una riga per caso di competenza, nessuna per il resto: {lines:?}"
    );
    assert!(lines[0].contains(r#""decisione":"blocca""#), "{}", lines[0]);
    assert!(
        lines[0].contains(r#""motivo":"ricerca-concettuale""#),
        "{}",
        lines[0]
    );
    assert!(lines[0].contains(r#""strumento":"Grep""#), "{}", lines[0]);
    // Il passaggio si registra come il blocco, e il conteggio è un NUMERO: chi
    // legge il registro somma quel campo.
    assert!(lines[1].contains(r#""decisione":"passa""#), "{}", lines[1]);
    assert!(lines[1].contains(r#""conteggio":7"#), "{}", lines[1]);
    assert!(
        lines[1].contains(r#""percorso":"/repo/src/index.ts""#),
        "{}",
        lines[1]
    );
}
