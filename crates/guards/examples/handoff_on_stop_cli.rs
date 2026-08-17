//! Il giudizio dello Stop, interrogabile da fuori: serve al confronto col Python.
//!
//! PERCHÉ UN `example` E NON UN SOTTOCOMANDO. Il gancio vivo è ancora il Python:
//! il porto copre il giudizio, non l'involucro — `arm_successor` e `farewell`
//! aprono e chiudono schede vere, e adottarne metà darebbe un presidio che
//! decide bene e non fa niente. Finché quel pezzo manca, questo binario esiste
//! solo per far parlare la parte portata, e non è registrato in nessun gancio.
//!
//! Legge un oggetto JSON da stdin e si comporta come il gancio: uscita 2 col
//! messaggio su stderr quando non ci si deve fermare, 0 e silenzio negli altri
//! casi. La variante decisa va su stdout, che il gancio vero non usa: così il
//! confronto può distinguere `Pass` da `Settle` da `Surrender`, che per
//! l'originale sono tutti e tre «uscita 0».
//!
//!     echo '{"transcript_path":"…","handoff_valid":false}' \
//!       | cargo run -q --example handoff_on_stop_cli

use guards::handoff::{context_used_from_lines, thresholds_from_lines};
use guards::handoff_on_stop::{decide, Decision, Facts, RESTART_CAP_DEFAULT};
use guards::restart::count_lines;
use std::io::Read;

fn main() {
    let mut raw = String::new();
    if std::io::stdin().read_to_string(&mut raw).is_err() {
        std::process::exit(0);
    }
    let data: serde_json::Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => std::process::exit(0),
    };
    let flag = |k: &str| data.get(k).and_then(|v| v.as_bool()).unwrap_or(false);
    let num = |k: &str, d: u32| data.get(k).and_then(|v| v.as_u64()).unwrap_or(d as u64) as u32;

    let path = data
        .get("transcript_path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    // La misura si estrae qui con le funzioni già portate, invece di arrivare
    // dal caso: così il confronto copre anche l'estrazione, non solo il verdetto.
    let text = std::fs::read_to_string(path).unwrap_or_default();
    let lines: Vec<&str> = text.lines().collect();
    let thresholds = thresholds_from_lines(&lines);
    let used = context_used_from_lines(&lines);
    let restarts = count_lines(lines.iter().copied()).restarts;

    let facts = Facts {
        stop_hook_active: flag("stop_hook_active"),
        has_transcript: !path.is_empty(),
        thresholds: &thresholds,
        used,
        handoff_valid: flag("handoff_valid"),
        restarts,
        restart_cap: num("restart_cap", RESTART_CAP_DEFAULT),
        stop_blocks_so_far: num("stop_blocks_so_far", 0),
    };

    match decide(&facts) {
        Decision::Pass => println!("Pass"),
        Decision::Settle => println!("Settle"),
        Decision::Surrender => println!("Surrender"),
        Decision::Block(m) => {
            println!("Block");
            eprint!("{m}");
            std::process::exit(2);
        }
    }
}
