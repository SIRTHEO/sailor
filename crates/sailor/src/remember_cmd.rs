//! `sailor remember <type> <label> <value…>`: a person writes a memory by hand,
//! through the same door a flow uses.

pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor remember <user|feedback|project|reference> <label> <value...>",
    says_key: "cli.remember.says",
}];

pub fn run(args: &[String]) -> i32 {
    let [kind, label, value @ ..] = args else {
        eprintln!("sailor remember: {}", catalogue::say("cli.remember.wants_three", &[("usage", USAGE[0].form)]));
        return 2;
    };
    if value.is_empty() {
        eprintln!("sailor remember: {}", catalogue::say("cli.remember.wants_three", &[("usage", USAGE[0].form)]));
        return 2;
    }
    let ledger = match ledger::Ledger::open(ui::gather::default_ledger_dir()) {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("sailor remember: {error}");
            return 1;
        }
    };
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default();
    let memory = actions::memory::Memory {
        kind: kind.clone(),
        label: label.clone(),
        value: value.join(" "),
        provenance: "a person, by hand".to_owned(),
        modified: at,
        valid_from: at,
        valid_until: None,
    };
    match actions::memory::remember(&ledger, memory) {
        Ok(kept) => {
            println!(
                "{}",
                catalogue::say(
                    "cli.remember.kept",
                    &[("label", &kept.label), ("type", &kept.kind)],
                )
            );
            0
        }
        Err(error) => {
            eprintln!(
                "sailor remember: {}",
                catalogue::say(&format!("run.failure.{}", error.class), &[])
            );
            eprintln!("   {}", error.said);
            1
        }
    }
}
