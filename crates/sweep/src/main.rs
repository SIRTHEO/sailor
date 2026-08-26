use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let deleting = std::env::args().any(|argument| argument == "--delete");
    let home = std::env::var("HOME").unwrap_or_else(|_| "/Users/theo".to_owned());
    let state = std::path::Path::new(&home).join(".claude/state");
    let ledger = state.join("sweep-ledger");
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let run_id = format!("marker-sweep-{}-{stamp}", std::process::id());
    match sweep::run(
        run_id,
        sweep::SweepConfig {
            state_dir: state.to_string_lossy().into_owned(),
            deleting,
        },
        ledger,
    ) {
        Ok(execution) => println!("marker-sweep: {:?}", execution.decisions.last()),
        Err(error) => {
            eprintln!("marker-sweep: {error}");
            std::process::exit(1);
        }
    }
}
