//! Il guscio del binario, e nient'altro.
//!
//! **TUTTO IL RESTO STA IN `lib.rs` DAL 01/09/2026.** La tabella dei comandi,
//! l'instradamento e le prove del dispatch erano qui, dentro un crate che
//! produce solo un eseguibile: nessun altro programma poteva leggerli. Quando
//! la finestra ha dovuto mostrare i comandi di Sailor, le strade erano due —
//! ricopiarli in TypeScript, o esporli. Ricopiarli è il guasto 10, che in
//! questo repo si è già ripresentato cinque volte, l'ultima lo stesso giorno
//! sul vocabolario delle azioni. Quindi qui resta solo la chiamata a
//! `std::process::exit`, che è l'unica cosa che una prova non può eseguire.
//!
//! Il perché del crate, l'elenco dei comandi e le prove dell'instradamento
//! stanno in `lib.rs`, che è dove sono andati: qui non si ricopiano.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(sailor::dispatch(&args));
}
