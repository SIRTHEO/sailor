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
            eprintln!("«{root}» is not a workspace: {error}");
            std::process::exit(64);
        }
    };
    println!(
        "workspace: {} ({})",
        workspace.name,
        workspace.root.display()
    );

    let lines: Vec<String> = args.collect();
    let lines = if lines.is_empty() {
        vec![
            // The quotes in the middle tell the line echoed back by the
            // terminal apart from the one the shell really produced.
            "echo ci\"a\"o".to_string(),
            "? find the leftovers of the configuration".to_string(),
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
            eprintln!("the terminal did not open: {error}");
            std::process::exit(70);
        }
    };
    println!(
        "terminal «{}» open, process {}\n",
        terminal.id(),
        terminal.process_id()
    );

    for line in &lines {
        let before = seen.text().len();
        match terminal.submit(line) {
            Ok(Routed::Command { why, .. }) => {
                println!("«{line}» → TERMINAL (why: {why:?})");
                // Time enough to run the command: here one watches, one does
                // not measure, and a fixed wait is enough for a demonstration.
                std::thread::sleep(Duration::from_millis(800));
                let text = seen.text();
                for row in text[before.min(text.len())..].lines() {
                    println!("    | {row}");
                }
            }
            Ok(Routed::Flow { route, flow, text }) => {
                println!("«{line}» → FLOW «{flow}» (rule «{route}»)");
                println!("    delivered: {text}");
                println!(
                    "    from here on it is up to whoever composes the program: this text\n\
                     \x20   becomes the `text` of the manual trigger, and the run starts there."
                );
            }
            Err(error) => println!("«{line}» → the terminal does not answer: {error}"),
        }
        println!();
    }

    println!("open terminals:");
    for row in terminals.list() {
        println!(
            "  {} in {} (alive: {}, process {})",
            row.id, row.workspace_root, row.alive, row.process_id
        );
    }
    terminals.close_all();
}
