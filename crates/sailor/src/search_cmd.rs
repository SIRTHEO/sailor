//! `sailor search <words>`: everything Sailor keeps that mentions them — the
//! flows of every source, and the ledger's recent runs, steps and store.

use flow::system::FlowSource;
use ledger::Ledger;

pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor search <words>",
    says_key: "cli.search.says",
}];

pub fn run(args: &[String]) -> i32 {
    if args.is_empty() {
        eprintln!("sailor search: {}", catalogue::say("cli.search.no_words", &[("usage", USAGE[0].form)]));
        return 2;
    }
    let query = args.join(" ");
    match report(&ui::gather::flow_sources(), &ui::gather::default_ledger_dir(), &query) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(message) => {
            eprintln!("sailor search: {message}");
            1
        }
    }
}

fn report(sources: &[FlowSource], ledger_dir: &std::path::Path, query: &str) -> Result<String, String> {
    let mut lines = Vec::new();
    let known = flow::system::load_all(sources);
    let flows = actions::search::rank_flows(&known, query)?;
    lines.push(catalogue::say(
        "cli.search.flows_found",
        &[("count", &flows.len().to_string()), ("query", query)],
    ));
    for hit in &flows {
        lines.push(format!(
            "  {} · {}\n      {}",
            hit["flow"].as_str().unwrap_or_default(),
            hit["origin"].as_str().unwrap_or_default(),
            hit["excerpt"].as_str().unwrap_or_default().replace('\n', " ")
        ));
    }
    match Ledger::open(ledger_dir) {
        Ok(ledger) => {
            let hits = ledger
                .search(query, actions::search::RECENT_RUNS, actions::search::RECENT_STEPS)
                .map_err(|error| error.to_string())?;
            lines.push(catalogue::say(
                "cli.search.ledger_found",
                &[("count", &hits.len().to_string()), ("query", query)],
            ));
            for hit in &hits {
                lines.push(format!("  {}\n      {}", hit.id, hit.excerpt.replace('\n', " ")));
            }
        }
        Err(error) => lines.push(catalogue::say(
            "cli.search.ledger_closed",
            &[("error", &error.to_string())],
        )),
    }
    Ok(lines.join("\n"))
}
