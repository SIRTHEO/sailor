//! `sailor role set <NAME> <tool>... [--replace]` and `sailor role list`: a
//! person writes the row a flow's role resolves through, instead of a test
//! being the only thing that ever could.

use actions::roles::SetRole;
use flow::ActionError;
use ledger::Ledger;

pub const USAGE: &[crate::Form] = &[
    crate::Form {
        form: "sailor role set <NAME> <tool>... [--replace]",
        says_key: "cli.role.form.set",
    },
    crate::Form {
        form: "sailor role list",
        says_key: "cli.role.form.list",
    },
];

/// What the words on the line ask for.
#[derive(Debug, PartialEq)]
enum Ask {
    Set {
        name: String,
        tools: Vec<String>,
        replace: bool,
    },
    List,
}

pub fn run(args: &[String]) -> i32 {
    let ask = match asked(args) {
        Ok(ask) => ask,
        Err(said) => {
            eprintln!("sailor role: {said}");
            return 2;
        }
    };
    let ledger = match Ledger::open(ui::gather::default_ledger_dir()) {
        Ok(ledger) => ledger,
        Err(error) => {
            eprintln!("sailor role: {error}");
            return 1;
        }
    };
    let tools = toolbox::Tools::current();
    match applied(&ledger, ask, &|id| tools.declares(id), now()) {
        Ok(said) => {
            println!("{said}");
            0
        }
        Err(error) => {
            eprintln!(
                "sailor role: {}",
                catalogue::say(&format!("run.failure.{}", error.class), &[])
            );
            eprintln!("   {}", error.said);
            1
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or_default()
}

fn wrong_form() -> String {
    catalogue::say(
        "cli.role.wants_a_form",
        &[("usage", &crate::forms_as_lines(USAGE).join("\n       "))],
    )
}

/// The ask the words describe, or the reason they are not one. An option
/// nobody offers is refused by name rather than read as a tool: a mistyped
/// `--replce` must not become a role that resolves to an engine called that.
fn asked(args: &[String]) -> Result<Ask, String> {
    let (verb, rest) = args.split_first().ok_or_else(wrong_form)?;
    match verb.as_str() {
        "list" if rest.is_empty() => Ok(Ask::List),
        "set" => {
            let mut replace = false;
            let mut loose = Vec::new();
            for word in rest {
                match word.as_str() {
                    "--replace" => replace = true,
                    option if option.starts_with("--") => {
                        return Err(format!(
                            "{}\n{}",
                            catalogue::say("cli.unknown_option", &[("option", option)]),
                            wrong_form()
                        ));
                    }
                    _ => loose.push(word.clone()),
                }
            }
            match loose.split_first() {
                Some((name, tools)) if !tools.is_empty() => Ok(Ask::Set {
                    name: name.clone(),
                    tools: tools.to_vec(),
                    replace,
                }),
                _ => Err(wrong_form()),
            }
        }
        _ => Err(wrong_form()),
    }
}

/// The ask carried out, and what to say about it. The machine's own list of
/// declared tools arrives as `declares`, so what is written is only ever what
/// this machine could run.
fn applied(
    ledger: &Ledger,
    ask: Ask,
    declares: &dyn Fn(&str) -> bool,
    at: i64,
) -> Result<String, ActionError> {
    match ask {
        Ask::Set { name, tools, replace } => {
            let set = actions::roles::set(
                ledger,
                &SetRole { name: name.clone(), tools, replace },
                declares,
                at,
            )?;
            let now = set.tools.join(", ");
            Ok(match set.replaced {
                Some(before) => catalogue::say(
                    "cli.role.replaced",
                    &[("role", &name), ("tools", &now), ("before", &before.join(", "))],
                ),
                None => catalogue::say("cli.role.set", &[("role", &name), ("tools", &now)]),
            })
        }
        Ask::List => {
            let roles = actions::roles::list(ledger)?;
            if roles.is_empty() {
                return Ok(catalogue::say("cli.role.none", &[]));
            }
            Ok(roles
                .iter()
                .map(|(name, tools)| format!("{name}: {}", tools.join(", ")))
                .collect::<Vec<_>>()
                .join("\n"))
        }
    }
}

#[cfg(test)]
mod tests;
