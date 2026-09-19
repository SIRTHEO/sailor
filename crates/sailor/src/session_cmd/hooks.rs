//! Grafting Sailor's hooks into the settings files of the command lines that
//! read them, and taking them out again. Kept apart from the terminal
//! bookkeeping it shared a file with: the two only ever met in one dispatch.

use super::*;

/// Walks every command line and hands the resolved addresses to `work`, which
/// answers whether it did anything there.
///
/// **THE ADDRESSES ARE RESOLVED IN ONE PLACE.** A second walk would be a second
/// idea of where Sailor wrote, and a graft and an inverse disagreeing about it
/// is a graft that cannot be undone.
pub(super) fn each_command_line(
    request: &Request<'_>,
    catalog: &toolbox::descriptor::Catalog,
    machine: &toolbox::Machine,
    said: &mut Vec<String>,
    mut work: impl FnMut(
        &toolbox::descriptor::Descriptor,
        &std::path::Path,
        Option<&std::path::Path>,
        &mut Vec<String>,
    ) -> Result<bool, String>,
) -> Result<bool, String> {
    // **THE LIST AND THE MACHINE ARE HANDED OVER, NOT READ HERE.** A check must
    // be able to ask this about a command line nobody ships, and asking through
    // the shipped list would put a product's name inside the check.

    // `--settings` stays, and stays one file: it serves the tests and whoever
    // moved their own. With it, the descriptor says only what the events are
    // called, no longer where to write them.
    let home = machine.env.get("HOME").cloned().unwrap_or_default();
    let only = request.options.get("tool");
    let declared_file = request.options.get("settings").map(PathBuf::from);
    if declared_file.is_some() && only.is_none() {
        return Err(catalogue::say("cli.session.settings_without_tool", &[]));
    }

    if let Some(wanted) = only {
        let known: Vec<&str> = catalog
            .descriptors
            .iter()
            .map(|loaded| loaded.descriptor.id.as_str())
            .collect();
        if !known.contains(&wanted.as_str()) {
            return Err(catalogue::say(
                "cli.session.no_such_command_line",
                &[("tool", wanted), ("known", &known.join(", "))],
            ));
        }
    }

    let mut worked_anywhere = false;
    for loaded in &catalog.descriptors {
        let tool = &loaded.descriptor;
        if tool.family != "ai_cli" || tool.disabled {
            continue;
        }
        if only.is_some_and(|wanted| *wanted != tool.id) {
            continue;
        }
        let Some(hooks) = &tool.session_hooks else {
            said.push(catalogue::say(
                "cli.session.declares_no_hooks",
                &[("tool", &tool.id)],
            ));
            continue;
        };

        // It applies to the line `--tool` names, never to «the one the code
        // knows»: a fallback choosing for itself would put a product name back
        // in a condition, through another door.
        let file = match &declared_file {
            Some(path) => path.clone(),
            None => {
                let root = machine.env.get(&hooks.file.root_var).cloned();
                match hooks.file.path(root.as_deref(), &home) {
                    Some(path) => path,
                    None => {
                        said.push(catalogue::say(
                            "cli.session.no_settings_address",
                            &[("tool", &tool.id)],
                        ));
                        continue;
                    }
                }
            }
        };

        // Under `--settings` the words follow the declared file instead of
        // their own address: whoever diverts the graft diverts all of it,
        // and a test writing half into its scratch and half into the real
        // home would leave that half behind.
        let directory = hooks.words.as_ref().and_then(|words| match &declared_file {
            Some(path) => path.parent().map(|beside| {
                beside.join(
                    std::path::Path::new(&words.below_home)
                        .file_name()
                        .unwrap_or_default(),
                )
            }),
            None => words.path(machine.env.get(&words.root_var).map(String::as_str), &home),
        });

        worked_anywhere |= work(tool, &file, directory.as_deref(), said)?;
    }
    Ok(worked_anywhere)
}

/// Grafts every command line that declares how, and **names each one that does
/// not**.
///
/// The settings address used to live here, under a comment arguing it was «the
/// address of what we are grafting» and so not a coupling. It was: two other
/// lines with the same four moments got nothing, and silence reads as success.
pub(super) fn install_hooks(request: &Request<'_>) -> Result<Report, String> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::descriptor::Catalog::load(&toolbox::default_sources(&machine));
    grafting(request, &catalog, &machine)
}

/// The same graft, with the list and the machine handed over, so a check can
/// run the whole road over a command line nobody ships.
pub(super) fn grafting(
    request: &Request<'_>,
    catalog: &toolbox::descriptor::Catalog,
    machine: &toolbox::Machine,
) -> Result<Report, String> {
    let mut said = Vec::new();
    let declared_file = request.options.contains_key("settings");
    let grafted_any = each_command_line(
        request,
        catalog,
        machine,
        &mut said,
        |tool, file, words, said| {
            let missing = moments_without_an_event(tool);
            // **NO CATCH-ALL ARM.** A format that gets a variant and no arm is a
            // compile error, which is the same promise the old arm made in prose:
            // a format declared and not written must never pass in silence.
            match format_of(tool) {
                toolbox::descriptor::FileFormat::Json => said.push(grafted_into(tool, file)?),
                toolbox::descriptor::FileFormat::Toml => {
                    said.push(grafted_into_toml(tool, file, &key_of(tool))?)
                }
            }

            // **WHICH HOME, AND WHY THAT ONE.** A line whose file moves with a
            // variable has two addresses, and a graft that names only the file it
            // wrote leaves whoever reads unable to tell it went to the one their
            // sessions actually read.
            let root_var = root_var_of(tool);
            if !declared_file && !root_var.is_empty() {
                said.push(which_home(tool, &root_var, machine));
            }

            if !missing.is_empty() {
                said.push(format!(
                    "  {}",
                    catalogue::say(
                        "cli.session.moments_without_an_event",
                        &[("tool", &tool.id), ("moments", &format!("{missing:?}"))],
                    )
                ));
            }

            match words {
                Some(directory) => said.push(wrote_the_two_commands(directory)?),
                None => said.push(format!(
                    "  {}",
                    catalogue::say("cli.session.no_typed_words", &[("tool", &tool.id)])
                )),
            }
            // Every format the walk reaches is written now, so reaching a line is
            // grafting it. What is left below answers for the lines never reached.
            Ok(true)
        },
    )?;

    // The lines the walk did say are kept: they answer «which one refused».
    if !grafted_any {
        said.push(catalogue::say("cli.session.nothing_grafted", &[]));
        return Err(said.join("\n"));
    }
    Ok(Report::spoken(said.join("\n")))
}

/// Takes the graft back out of every command line it went into, and **names
/// each thing it could not take out and why**.
///
/// The owner's requirement is that a command line be left as Sailor found it.
/// A silent failure here is worse than at the graft: whoever ran this believes
/// the file is clean and has no reason to look again.
pub(super) fn uninstall_hooks(request: &Request<'_>) -> Result<Report, String> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::descriptor::Catalog::load(&toolbox::default_sources(&machine));
    taking_out(request, &catalog, &machine)
}

/// Takes our hooks out of a settings file, **by subtraction**.
///
/// It reads every event the file holds, not the ones the descriptor names
/// today: a command line that renames an event would otherwise leave ours
/// behind for ever, in the one place nobody would think to look. What may be
/// taken is settled by [`ours`], which is also what the graft asks.
pub(super) fn uninstalled(settings: &std::path::Path) -> Result<String, String> {
    let text = match std::fs::read_to_string(settings) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(catalogue::say(
                "cli.session.uninstall.no_file",
                &[("file", &settings.display().to_string())],
            ))
        }
        Err(error) => return Err(format!("{}: {error}", settings.display())),
    };
    if text.trim().is_empty() {
        return Ok(catalogue::say(
            "cli.session.uninstall.file_empty",
            &[("file", &settings.display().to_string())],
        ));
    }
    // **A FILE WE CANNOT READ IS NOT REWRITTEN**, the same rule as the graft:
    // rewriting it with our part taken out would erase the configuration of
    // whoever uses it, over a typo.
    let mut root: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("{}: not valid JSON ({error})", settings.display()))?;

    let Some(hooks) = root.get_mut("hooks").and_then(|at| at.as_object_mut()) else {
        return Ok(catalogue::say(
            "cli.session.uninstall.no_hooks",
            &[("file", &settings.display().to_string())],
        ));
    };

    let mut taken = Vec::new();
    let mut emptied = Vec::new();
    let mut unreadable = Vec::new();
    for (event, entries) in hooks.iter_mut() {
        let Some(list) = entries.as_array_mut() else {
            unreadable.push(event.clone());
            continue;
        };
        let before = list.len();
        list.retain(|entry| {
            !serde_json::to_string(entry)
                .map(|written| ours(&written))
                .unwrap_or(false)
        });
        if list.len() == before {
            continue;
        }
        taken.push(event.clone());
        if list.is_empty() {
            emptied.push(event.clone());
        }
    }

    let mut said = Vec::new();
    for event in &unreadable {
        said.push(catalogue::say(
            "cli.session.uninstall.not_an_array",
            &[("event", event), ("file", &settings.display().to_string())],
        ));
    }
    if taken.is_empty() {
        said.insert(
            0,
            catalogue::say(
                "cli.session.uninstall.nothing_of_ours",
                &[("file", &settings.display().to_string())],
            ),
        );
        return Ok(said.join("\n"));
    }

    // An event left holding an empty array is a trace of the graft too, and it
    // goes - but only where we are the ones who emptied it.
    for event in &emptied {
        hooks.remove(event);
    }
    let all_gone = hooks.is_empty();
    if let Some(object) = root.as_object_mut() {
        if all_gone {
            object.remove("hooks");
        }
        if object.is_empty() {
            said.push(catalogue::say(
                "cli.session.uninstall.empty_object_left",
                &[("file", &settings.display().to_string())],
            ));
        }
    }

    let written = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    std::fs::write(settings, format!("{written}\n"))
        .map_err(|error| format!("{}: {error}", settings.display()))?;
    said.insert(
        0,
        catalogue::say(
            "cli.session.uninstall.taken_out",
            &[
                ("file", &settings.display().to_string()),
                ("events", &taken.join(", ")),
            ],
        ),
    );
    Ok(said.join("\n"))
}

/// Grafts the moments **this** command line can report, under its own names.
/// The ones it cannot report do not enter and are named elsewhere: there is no
/// fallback here, because a fallback would be invisible to whoever reads.
pub(super) fn grafted_into(
    tool: &toolbox::descriptor::Descriptor,
    settings: &std::path::Path,
) -> Result<String, String> {
    let named = events_this_line_can_report(tool);
    if named.is_empty() {
        return Ok(nothing_to_graft(tool));
    }
    installed(settings, &named, &tool.id)
}

/// The line a graft writes, in **one copy for both formats**, naming the
/// command line it went into: a hook that does not say who it was grafted for
/// reaches us with a session and a directory and nothing else, and the terminal
/// is then tracked as somebody and announced to the others as nobody.
pub(super) fn hook_command(binary: &str, verb: &str, cli: &str) -> String {
    format!("{binary} session {verb} --cli {cli}")
}

pub(super) fn nothing_to_graft(tool: &toolbox::descriptor::Descriptor) -> String {
    catalogue::say("cli.session.nothing_to_graft", &[("tool", &tool.id)])
}

/// The same graft into a settings file written in TOML.
pub(super) fn grafted_into_toml(
    tool: &toolbox::descriptor::Descriptor,
    settings: &std::path::Path,
    under: &[String],
) -> Result<String, String> {
    let named = events_this_line_can_report(tool);
    if named.is_empty() {
        return Ok(nothing_to_graft(tool));
    }
    let binary = std::env::current_exe()
        .map_err(|error| {
            catalogue::say(
                "cli.no_telling_where_i_am",
                &[("error", &error.to_string())],
            )
        })?
        .display()
        .to_string();
    let commands: Vec<(&str, String)> = named
        .iter()
        .map(|(event, verb)| (*event, hook_command(&binary, verb, &tool.id)))
        .collect();

    // **A FILE THAT IS THERE AND WILL NOT BE READ STOPS THE GRAFT.** Treating
    // an unreadable file as an empty one appends to nothing and writes back a
    // file holding our lines alone, which is the configuration of whoever uses
    // it deleted over a permission.
    let existing = match std::fs::read_to_string(settings) {
        Ok(text) => text,
        Err(_) if !settings.exists() => String::new(),
        Err(error) => return Err(format!("{}: {error}", settings.display())),
    };
    let graft = crate::toml_graft::appended(&existing, under, &commands, MARKS)
        .map_err(|reason| format!("{}: {reason}", settings.display()))?;
    if graft.added.is_empty() {
        return Ok(already_grafted(settings));
    }
    if let Some(parent) = settings.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    std::fs::write(settings, &graft.text)
        .map_err(|error| format!("{}: {error}", settings.display()))?;
    Ok(just_grafted(settings, &graft.added.join(", ")))
}

/// The two sentences the report ends on, said the same for every format: two
/// formats wording it differently would read as two different things done.
pub(super) fn already_grafted(settings: &std::path::Path) -> String {
    let file = settings.display().to_string();
    catalogue::say("cli.session.already_grafted", &[("file", &file)])
}

pub(super) fn just_grafted(settings: &std::path::Path, events: &str) -> String {
    let file = settings.display().to_string();
    catalogue::say(
        "cli.session.grafted",
        &[("file", &file), ("events", events)],
    )
}

/// Grafts the hooks into a settings file, **by adding**.
///
/// The binary's path is the one running right now (`current_exe`): a graft
/// writing plain `sailor` would work only where that name is already on the
/// `PATH` of whoever opens the terminal, which is not something knowable here.
pub(super) fn installed(
    settings: &std::path::Path,
    events: &[(&str, &str)],
    cli: &str,
) -> Result<String, String> {
    let mut root: serde_json::Value = match std::fs::read_to_string(settings) {
        Ok(text) if text.trim().is_empty() => serde_json::json!({}),
        // **A FILE WE CANNOT READ IS NOT REWRITTEN.** Replacing it with our own
        // part alone would erase the configuration of whoever uses it, over a
        // typo.
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("{}: not valid JSON ({error})", settings.display()))?,
        Err(_) => serde_json::json!({}),
    };

    let binary = std::env::current_exe()
        .map_err(|error| {
            catalogue::say(
                "cli.no_telling_where_i_am",
                &[("error", &error.to_string())],
            )
        })?
        .display()
        .to_string();

    let hooks = root
        .as_object_mut()
        .ok_or_else(|| {
            catalogue::say(
                "cli.session.root_not_an_object",
                &[("file", &settings.display().to_string())],
            )
        })?
        .entry("hooks")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| {
            catalogue::say(
                "cli.session.hooks_not_an_object",
                &[("file", &settings.display().to_string())],
            )
        })?;

    let mut added = Vec::new();
    for (event, verb) in events {
        let command = hook_command(&binary, verb, cli);
        let list = hooks
            .entry(*event)
            .or_insert_with(|| serde_json::json!([]))
            .as_array_mut()
            .ok_or_else(|| {
                catalogue::say(
                    "cli.session.event_not_an_array",
                    &[("file", &settings.display().to_string()), ("event", event)],
                )
            })?;

        // Ours, told by the command and not by the position — **and one of
        // ours that no longer says what we would write is rewritten**, or the
        // terminals grafted before this line stay announced as nobody.
        let ours_here: Vec<usize> = list
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                serde_json::to_string(entry)
                    .map(|written| ours(&written))
                    .unwrap_or(false)
            })
            .map(|(at, _)| at)
            .collect();
        let up_to_date = ours_here.iter().any(|at| {
            serde_json::to_string(&list[*at])
                .map(|written| written.contains(&command))
                .unwrap_or(false)
        });
        if up_to_date {
            continue;
        }
        for at in ours_here.iter().rev() {
            list.remove(*at);
        }
        list.push(serde_json::json!({
            "hooks": [{"type": "command", "command": command}]
        }));
        added.push(*event);
    }

    if added.is_empty() {
        return Ok(already_grafted(settings));
    }
    if let Some(parent) = settings.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("{}: {error}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(&root).map_err(|error| error.to_string())?;
    std::fs::write(settings, format!("{text}\n"))
        .map_err(|error| format!("{}: {error}", settings.display()))?;
    Ok(just_grafted(settings, &added.join(", ")))
}
