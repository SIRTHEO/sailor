//! `sailor search <words>`: everything Sailor keeps that mentions them — the
//! flows of every source, the ledger's recent runs, steps, events and store,
//! and the fault register kept beside the ledger.

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
            let faults_store = ledger_dir.join(faults::FAULTS_FILE);
            let hits = actions::search::search_the_ledger_and_the_faults(&ledger, Some(&faults_store), query)?;
            lines.push(catalogue::say(
                "cli.search.ledger_found",
                &[("count", &hits.len().to_string()), ("query", query)],
            ));
            for hit in &hits {
                lines.push(format!(
                    "  {}{}{}\n      {}",
                    hit.id,
                    memory_named(&ledger, hit),
                    note_named(&ledger, hit),
                    hit.excerpt.replace('\n', " ")
                ));
            }
        }
        Err(error) => lines.push(catalogue::say(
            "cli.search.ledger_closed",
            &[("error", &error.to_string())],
        )),
    }
    Ok(lines.join("\n"))
}

/// A memory among the hits, named for a person: its label, and the tree it
/// holds in by the tree's short name. Nothing for any other kind of hit.
fn memory_named(ledger: &Ledger, hit: &ledger::search::Hit) -> String {
    let Some(memory) = actions::search::memory_behind(ledger, hit) else {
        return String::new();
    };
    let said = match &memory.tree {
        Some(tree) => catalogue::say(
            "cli.search.memory_in_tree",
            &[("label", &memory.label), ("tree", actions::memory::tree_name(tree))],
        ),
        None => catalogue::say("cli.search.memory_everywhere", &[("label", &memory.label)]),
    };
    format!(" · {said}")
}

/// A note among the hits, named for a person: its title beside the slug the
/// id already carries. Nothing for any other kind of hit.
fn note_named(ledger: &Ledger, hit: &ledger::search::Hit) -> String {
    let Some(note) = actions::search::note_behind(ledger, hit) else {
        return String::new();
    };
    format!(
        " · {}",
        catalogue::say(
            "cli.search.note_titled",
            &[("title", &note.title), ("slug", &note.slug)],
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A MEMORY FOUND IS NAMED WITH ITS TREE**, by the short name, beside its
    /// label; one that holds in every tree says so instead.
    #[test]
    fn a_memory_found_is_named_with_its_tree() {
        let dir = std::env::temp_dir().join(format!("sailor-search-cmd-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("a ledger");
        let memory = |label: &str, value: &str, tree: Option<&str>| actions::memory::Memory {
            kind: "project".to_owned(),
            label: label.to_owned(),
            value: value.to_owned(),
            provenance: "test".to_owned(),
            modified: 1,
            valid_from: 1,
            valid_until: None,
            tree: tree.map(str::to_owned),
        };
        actions::memory::remember(&ledger, memory("the trunk", "a quokka sits on sorgenti", Some("/trees/a-checkout")))
            .expect("kept");
        actions::memory::remember(&ledger, memory("the home", "a quokka sits under state", None)).expect("kept");
        drop(ledger);

        let text = report(&[], &dir, "quokka").expect("a report");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(
            text.contains(&catalogue::say("cli.search.memory_in_tree", &[("label", "the trunk"), ("tree", "a-checkout")])),
            "{text}"
        );
        assert!(
            text.contains(&catalogue::say("cli.search.memory_everywhere", &[("label", "the home")])),
            "{text}"
        );
    }

    /// **A WORD ONLY A NOTE SAYS IS FOUND**, and the line names the slug it is
    /// held under. A working note kept out of the repository has to stay
    /// findable, or keeping it here is worse than leaving the file on disk.
    #[test]
    fn a_word_only_in_a_notes_body_is_found_and_the_slug_is_named() {
        let dir = std::env::temp_dir().join(format!("sailor-search-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let ledger = Ledger::open(&dir).expect("a ledger");
        actions::notes::import(
            &ledger,
            actions::notes::Note {
                slug: "an-evening-log".to_owned(),
                title: "An evening log".to_owned(),
                text: "# An evening log\n\nthe wombat took the last turn\n".to_owned(),
                tree: None,
                imported_at: 7,
                removed_at: None,
            },
        )
        .expect("taken in");
        drop(ledger);

        let text = report(&[], &dir, "wombat").expect("a report");
        let _ = std::fs::remove_dir_all(&dir);

        assert!(text.contains("store:notes/an-evening-log"), "{text}");
        assert!(
            text.contains(&catalogue::say(
                "cli.search.note_titled",
                &[("title", "An evening log"), ("slug", "an-evening-log")],
            )),
            "{text}"
        );
    }
}
