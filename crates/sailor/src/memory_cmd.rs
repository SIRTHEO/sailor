//! `sailor memory page`: the page of memories as the one file every command
//! line reads at its start, written under Sailor's home or printed.

pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor memory page [--print]",
    says_key: "cli.memory.page_says",
}];

pub fn run(args: &[String]) -> i32 {
    let print = match args {
        [page] if page == "page" => false,
        [page, flag] if page == "page" && flag == "--print" => true,
        _ => {
            eprintln!("sailor memory: {}", catalogue::say("cli.memory.wants_page", &[("usage", USAGE[0].form)]));
            return 2;
        }
    };
    let ledger = match ledger::Ledger::open(ui::gather::default_ledger_dir()) {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("sailor memory: {error}");
            return 1;
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let memories = match actions::memory::remembered(&ledger, now) {
        Ok(memories) => memories,
        Err(error) => {
            eprintln!("sailor memory: {error}");
            return 1;
        }
    };
    if print {
        println!("{}", actions::memory::page(&memories));
        return 0;
    }
    let Some(home) = ledger::sailor_home() else {
        eprintln!("sailor memory: {}", catalogue::say("cli.memory.no_home", &[]));
        return 1;
    };
    match actions::memory::write_page(&ledger, &home) {
        Ok(path) => {
            println!(
                "{}",
                catalogue::say(
                    "cli.memory.written",
                    &[("count", &memories.len().to_string()), ("path", &path.display().to_string())],
                )
            );
            0
        }
        Err(error) => {
            eprintln!("sailor memory: {}", catalogue::say(&format!("run.failure.{}", error.class), &[]));
            eprintln!("   {}", error.said);
            1
        }
    }
}
