//! La parte con disco, ambiente e `orca` del gancio che arma il successore.
//!
//! Il giudizio puro sta in `guards::successor`; qui c'è ciò che deve toccare la
//! macchina: leggere la testa di un file per il frontmatter, contare le sessioni
//! vive, chiedere a Orca i pannelli dell'albero.
//!
//! FAIL-OPEN OVUNQUE. Un gancio che rompe la scrittura di una consegna è peggio
//! del problema che risolve: ogni errore diventa «non lo so», e un «non lo so»
//! non frena.

use guards::successor::{count_agents, is_handoff_doc, mandate};
use std::fs;
use std::io::Read;

/// I primi 400 byte del file, come li legge il Python.
///
/// La lunghezza è la stessa di proposito: il frontmatter sta in testa, e un
/// limite diverso farebbe divergere le due implementazioni su un file che
/// dichiara `type: project` al byte 401 — improbabile, ma il confronto lo
/// vedrebbe e nessuno saprebbe perché.
fn head(path: &str) -> Option<String> {
    let mut f = fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 400];
    let n = f.read(&mut buf).ok()?;
    buf.truncate(n);
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// È un documento di consegna? Legge il file solo quando serve davvero.
pub fn is_doc(path: &str) -> bool {
    is_handoff_doc(path, head(path).as_deref())
}

/// Quanti pannelli con un agente ci sono in questo albero. `None` se ignoto.
pub fn panes_here(root: &str) -> Option<usize> {
    let out = std::process::Command::new("orca")
        .args(["terminal", "list", "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    Some(count_agents(&v, root))
}

/// Le sessioni Claude vive adesso. `None` se non si è potuto sapere.
///
/// Si chiede al binario invece di contare i processi: un `ps | grep claude`
/// conta anche i subagent e i wrapper della shell, e sovrastima di molto.
pub fn live_sessions() -> Option<usize> {
    let out = std::process::Command::new("claude")
        .args(["agents", "--json"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).ok()?;
    v.as_array().map(|a| a.len())
}

/// Risponde alle domande che lo strumento di equivalenza pone al Python.
///
/// Non è un gancio: è il punto d'aggancio del confronto. Senza un modo di
/// interrogare il Rust dall'esterno, il porting si proverebbe solo sui casi
/// scritti a mano — e sono proprio quelli a non trovare i difetti.
pub fn probe(verb: &str, a: &str, b: &str) -> i32 {
    match verb {
        "doc" => println!("{}", if is_doc(a) { "True" } else { "False" }),
        "mandate" => print!("{}", mandate(a)),
        "fingerprint" => println!("{}", guards::successor::armed_fingerprint(a, b)),
        "hours" => println!(
            "{}",
            match a.parse::<u32>() {
                Ok(h) if guards::successor::within_hours(h) => "True",
                _ => "False",
            }
        ),
        // I due conteggi che parlano con la macchina. Esposti qui perché il
        // confronto li eserciti contro il Python sullo stato reale: sono le due
        // risposte che i tetti usano, e provarle solo a tavolino vorrebbe dire
        // provare la soglia e non la misura da cui dipende.
        "panes" => println!(
            "{}",
            panes_here(a).map(|n| n.to_string()).unwrap_or("-1".into())
        ),
        "live" => println!(
            "{}",
            live_sessions().map(|n| n.to_string()).unwrap_or("-1".into())
        ),
        "agents" => {
            let mut raw = String::new();
            if std::io::stdin().read_to_string(&mut raw).is_err() {
                println!("0");
                return 0;
            }
            let n = serde_json::from_str::<serde_json::Value>(&raw)
                .map(|v| count_agents(&v, a))
                .unwrap_or(0);
            println!("{n}");
        }
        _ => {
            eprintln!("verbo sconosciuto: {verb}");
            return 1;
        }
    }
    0
}
