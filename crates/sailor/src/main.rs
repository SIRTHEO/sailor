//! The binary's shell, and nothing else.
//!
//! **EVERYTHING ELSE LIVES IN `lib.rs`.** The command table, the routing and
//! its tests sat in a crate that only builds an executable, so no other program
//! could read them; recopying them into the window is fault 10, already seen
//! five times in this repo. Only `std::process::exit` stays: a test cannot run it.

fn main() {
    let args: Vec<String> = std::env::args().collect();
    std::process::exit(sailor::dispatch(&args));
}
