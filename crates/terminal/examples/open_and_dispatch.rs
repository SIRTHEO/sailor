//! The terminal engine, driven by hand and without the window.
//!
//! ```text
//! cargo run -p terminal --example open_and_dispatch -- <directory> [line...]
//! ```
//!
//! With no arguments it opens a terminal in the current directory, writes
//! `echo ciao` into it, reads the answer, and then shows what happens to a
//! request about a flow: it is not executed, it is routed. With arguments it
//! submits the lines it is given, one at a time, and says where they went.
//!
//! **IT EXISTS BECAUSE THE ENGINE IS NOT INSIDE THE WINDOW.** It is the proof
//! that a Sailor terminal can be opened, driven and watched without opening
//! anything — and where whoever builds the interface reads the gesture order.

use std::sync::Arc;
use std::time::Duration;
use terminal::{Buffer, Opening, Output, Routed, Terminals, Workspace};

fn main() {
    let mut args = std::env::args().skip(1);
    let root = args.next().unwrap_or_else(|| ".".to_string());
    let workspace = match Workspace::open(&root) {
        Ok(workspace) => workspace,
        Err(error) => {
            eprintln!("«{root}» non è uno spazio di lavoro: {error}");
            std::process::exit(64);
        }
    };
    println!(
        "spazio di lavoro: {} ({})",
        workspace.name,
        workspace.root.display()
    );

    let lines: Vec<String> = args.collect();
    let lines = if lines.is_empty() {
        vec![
            // The quotes in the middle tell the line echoed back by the
            // terminal apart from the one the shell really produced.
            "echo ci\"a\"o".to_string(),
            "? trova i residui di configurazione".to_string(),
        ]
    } else {
        lines
    };

    let terminals = Terminals::current();
    let seen = Arc::new(Buffer::new());
    let terminal = match terminals.open(workspace, &Opening::default(), |_| {
        Arc::clone(&seen) as Arc<dyn Output>
    }) {
        Ok(terminal) => terminal,
        Err(error) => {
            eprintln!("il terminale non si è aperto: {error}");
            std::process::exit(70);
        }
    };
    println!(
        "terminale «{}» aperto, processo {}\n",
        terminal.id(),
        terminal.process_id()
    );

    for line in &lines {
        let before = seen.text().len();
        match terminal.submit(line) {
            Ok(Routed::Command { why, .. }) => {
                println!("«{line}» → TERMINALE (perché: {why:?})");
                // Time enough to run the command: here one watches, one does
                // not measure, and a fixed wait is enough for a demonstration.
                std::thread::sleep(Duration::from_millis(800));
                let text = seen.text();
                for row in text[before.min(text.len())..].lines() {
                    println!("    | {row}");
                }
            }
            Ok(Routed::Flow { route, flow, text }) => {
                println!("«{line}» → FLUSSO «{flow}» (regola «{route}»)");
                println!("    consegna: {text}");
                println!(
                    "    da qui in poi tocca a chi compone il programma: questo testo\n\
                     \x20   diventa il `text` dell'innesco manuale, e la corsa parte da lì."
                );
            }
            Err(error) => println!("«{line}» → il terminale non risponde: {error}"),
        }
        println!();
    }

    println!("terminali aperti:");
    for row in terminals.list() {
        println!(
            "  {} in {} (vivo: {}, processo {})",
            row.id, row.workspace_root, row.alive, row.process_id
        );
    }
    terminals.close_all();
}
