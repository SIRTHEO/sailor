//! Il motore dei terminali, guidato a mano e senza la finestra.
//!
//! ```text
//! cargo run -p terminal --example open_and_dispatch -- <cartella> [riga...]
//! ```
//!
//! Senza argomenti apre un terminale nella cartella corrente, ci scrive
//! `echo ciao`, legge la risposta, e poi mostra cosa succede a una richiesta che
//! riguarda un flusso: non viene eseguita, viene smistata. Con degli argomenti
//! sottopone le righe che gli si danno, una per volta, e dice dove sono andate.
//!
//! **ESISTE PERCHÉ IL MOTORE NON È DENTRO LA FINESTRA.** È la dimostrazione che
//! un terminale di Sailor si apre, si guida e si guarda senza aprire niente —
//! e il punto in cui chi costruirà l'interfaccia può leggere l'ordine dei gesti.

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
            // Le virgolette in mezzo distinguono la riga rimandata indietro dal
            // terminale da quella che la shell ha davvero prodotto.
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
                // Il tempo di far girare il comando: qui si guarda, non si
                // misura, e un attesa fissa basta per una dimostrazione.
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
