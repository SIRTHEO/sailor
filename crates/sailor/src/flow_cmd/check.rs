//! `sailor flow check`: the report on a flow before it runs.

use flow::{ActionRegistry, FlowFile, Graph};
use registry::default_registry;
use serde_json::Value;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use ui::gather::FlowSource;

use super::cap_and_schedule::WHAT_THE_CAP_DOES_NOT_PROMISE_KEY;
use super::cost::{in_units, models_asked_by, models_seen_by, what_is_priced};
use super::engines::{engine_lines_into, login_states_into};
use super::extensions::{extensions_of_this_machine_into, undeclared_extensions_named_in_text};
use super::hazards::{
    blind_steps_asking_for_a_session, deciders_that_are_not_checks, handed_without_choices,
    hardcoded_paths,
    outside_text_in_command, pointers_that_cannot_match, undelimited_commits, HardcodedPath,
};
use super::{missing_actions, one_flow, open_default_ledger};

/// **THE LINES ARE PROBED UNLESS SOMEONE SAYS NO.** A check behind a flag is a
/// check nobody asks, and fault 27 is the proof: nobody would have typed
/// `--engines` to find a defect they did not know they had. `--no-engines`
/// stays for whoever works offline or is in a hurry.
pub(super) fn check_flow(sources: &[FlowSource], name: &str, try_engines: bool) -> Result<String, String> {
    let (flow, _) = one_flow(sources, name)?;
    let tools = toolbox::Tools::current();
    let real = actions::RealDryProbe;
    // **AN UNREADABLE PROFILE STORE DOES NOT STOP THE CHECK**, for the same
    // reason it does not stop a run: a world with no profiles is looked at,
    // and the homes section stays quiet instead of stating a falsehood.
    let profiles = profiles::store_io::load_store().unwrap_or_default();
    let world = EngineWorld {
        probe: &real,
        profiles: &profiles,
    };
    let registry = default_registry(open_default_ledger(), None);
    let (mut report, unknown) = check_report(
        &flow,
        &registry,
        Some(&tools),
        if try_engines { Some(&world) } else { None },
    );
    // **THE PRICE LIST IS READ HERE AND NOT INSIDE `check_report`.** That
    // report is pure — flow, registry, detector, probe, all passed in — and
    // only the ledger knows the models a flow has used. Keeping it out leaves
    // `check_report` testable without opening one.
    let prices = actions::current_price_list();
    // **WHAT KIND OF CAP IT IS, DECIDED HERE AND NOT BELIEVED.** It needs both
    // the machine's descriptors and its price list, so it sits beside the price
    // list and outside the pure report, for the same reason.
    what_the_cap_is_into(&mut report, &flow, &tools, &prices);
    report.push_str(&what_is_priced(
        &prices,
        &models_asked_by(&flow, &tools),
        models_seen_by(&flow.id).as_ref(),
        flow.spend_cap_micros,
    ));
    // The inventory is this machine's, so it is read here for the same reason
    // as the price list: a test feeds `check_report` a scratch one instead.
    extensions_of_this_machine_into(&mut report, &flow);
    if let Some(refusal) = refusals_of(&flow, &registry).into_iter().next() {
        println!("{report}");
        return Err(refusal);
    }
    if unknown.is_empty() {
        return Ok(report);
    }
    // THE REPORT IS SEEN EVEN WHEN THE FLOW IS BROKEN. Whoever checks a flow
    // does it to understand it: answering with the error line alone would
    // force a second run of the command to see the rest.
    println!("{report}");
    Err(catalogue::say(
        "cli.flow.tools_no_descriptor_declares",
        &[("flow", &flow.id), ("tools", &unknown.join(", "))],
    ))
}

/// Every reason this flow is refused, in the order a reader meets them.
///
/// **THE LIST IS ONE, AND WHOEVER WRITES A FLOW ASKS IT TOO.** A flow saved by
/// `sailor flow edit` and refused by the next check is the fault that command
/// closes. Not here: the tools no descriptor declares, which take a detector.
pub fn refusals_of(flow: &FlowFile, registry: &ActionRegistry) -> Vec<String> {
    let mut refused = Vec::new();
    // A reference inside an executed field stops before the run: afterwards it
    // is shell text and no longer tells itself apart from what the flow wrote.
    let mounted: Vec<String> = outside_text_in_command(flow)
        .iter()
        .map(|found| format!("{} in «{}»", found.step, found.field))
        .collect();
    if !mounted.is_empty() {
        refused.push(catalogue::say(
            "cli.flow.value_mounted_into_an_executed_field",
            &[("flow", &flow.id), ("fields", &mounted.join(", "))],
        ));
    }
    // An error, not a warning: elsewhere the flow does not fail, it works in
    // the wrong place. See fault 25.
    let stuck: Vec<String> = hardcoded_paths(flow)
        .iter()
        .filter(|path| path.fatal)
        .map(|path| format!("{} in «{}» ({})", path.step, path.field, path.value))
        .collect();
    if !stuck.is_empty() {
        refused.push(catalogue::say(
            "cli.flow.absolute_path_in_a_place_field",
            &[("flow", &flow.id), ("fields", &stuck.join("; "))],
        ));
    }
    // One of the two shapes is silent: in `when` a pointer that cannot match
    // makes the step skip, and a skipped step closes green.
    let dead: Vec<String> = pointers_that_cannot_match(flow)
        .iter()
        .map(|found| {
            if found.field.is_empty() {
                format!("{} in «when» ({})", found.step, found.pointer)
            } else {
                format!("{} in «{}» ({})", found.step, found.field, found.pointer)
            }
        })
        .collect();
    if !dead.is_empty() {
        refused.push(catalogue::say(
            "cli.flow.pointer_that_cannot_match",
            &[("flow", &flow.id), ("fields", &dead.join("; "))],
        ));
    }
    // An error, not a warning: the step does not fail, it commits another
    // session's staged work under a message about something else.
    let sweeping: Vec<String> = undelimited_commits(flow)
        .iter()
        .map(|commit| format!("{} in «{}»", commit.step, commit.field))
        .collect();
    if !sweeping.is_empty() {
        refused.push(catalogue::say(
            "cli.flow.commit_without_paths",
            &[("flow", &flow.id), ("steps", &sweeping.join(", "))],
        ));
    }
    let seeing = blind_steps_asking_for_a_session(flow);
    if !seeing.is_empty() {
        refused.push(catalogue::say(
            "cli.flow.blind_and_asking_for_a_session",
            &[("flow", &flow.id), ("steps", &seeing.join(", "))],
        ));
    }
    // An error, not a warning: a run that closes on this step closes without
    // asking anybody, and the whole saving depends on what said yes.
    let deciding = deciders_that_are_not_checks(flow, registry);
    if !deciding.is_empty() {
        refused.push(catalogue::say(
            "cli.flow.decides_done_without_a_check",
            &[("flow", &flow.id), ("steps", &deciding.join(", "))],
        ));
    }
    let unasked = handed_without_choices(flow);
    if !unasked.is_empty() {
        refused.push(catalogue::say(
            "cli.flow.handed_without_choices",
            &[("flow", &flow.id), ("steps", &unasked.join(", "))],
        ));
    }
    refused
}

/// **WHY TWO OUTCOMES AND NOT ONE.** A flow asking for a tool not installed
/// here is sound — it runs elsewhere, and installing it makes it run here —
/// while a name no catalogue declares is broken on every machine. Only the
/// second is an error; the first is a warning, since a product running on
/// different machines cannot call a flow broken for not being its own.
///
/// **THE PROBE AND THE PROFILE STORE TRAVEL TOGETHER.** The two zero-cost
/// questions `flow check` asks an engine — «is the line I mount sound?» and
/// «is the home you start from logged in?» — are halves of one question. Both
/// enter from outside, so `check_report` stays pure and a test can hand it a
/// throwaway home instead of depending on the running machine's.
pub(super) struct EngineWorld<'a> {
    pub(super) probe: &'a dyn actions::EngineProbe,
    pub(super) profiles: &'a profiles::ProfileStore,
}

#[cfg(test)]
impl<'a> EngineWorld<'a> {
    /// A world where **no profile is declared**: the state of a freshly
    /// installed machine, and the right one for tests about command lines
    /// rather than homes. With no active profile the credentials section stays
    /// quiet, so those tests stay on what they test.
    pub(super) fn without_profiles(probe: &'a dyn actions::EngineProbe) -> Self {
        static NO_PROFILES: std::sync::OnceLock<profiles::ProfileStore> =
            std::sync::OnceLock::new();
        Self {
            probe,
            profiles: NO_PROFILES.get_or_init(profiles::ProfileStore::default),
        }
    }
}

pub(super) fn check_report(
    flow: &FlowFile,
    registry: &ActionRegistry,
    tools: Option<&toolbox::Tools>,
    world: Option<&EngineWorld>,
) -> (String, Vec<String>) {
    let dependency_count: usize = flow.graph.steps().iter().map(|step| step.deps.len()).sum();
    let missing = missing_actions(&flow.graph, registry);
    let mut report = format!(
        "flusso: {}\ndescrizione: {}\npassi: {}\ncicli: nessuno\ndipendenze: {}",
        flow.id,
        flow.description,
        flow.graph.steps().len(),
        dependency_count
    );
    for step in flow.graph.steps() {
        let dependencies = if step.deps.is_empty() {
            "nessuna".to_owned()
        } else {
            step.deps.join(", ")
        };
        let _ = write!(report, "\n  {} <- {}", step.id, dependencies);
        if let Some(phase) = &step.phase {
            report.push_str(&catalogue::say("cli.flow.step_phase", &[("phase", phase)]));
        }
    }
    // **THE CAP IS IN THE REPORT, AND WITH IT WHAT IT DOES NOT PROMISE.**
    // Whoever checks a flow before launching is deciding whether they can
    // afford it: an invisible cap is found out only once a run is stopped, and
    // one shown without its limits reads as a guarantee on the spend, which it
    // is not. The line is always there, cap or no cap: NO_CAP's word is itself
    // information, and a silent report leaves the reader wondering whether the
    // check looked at all.
    match flow.spend_cap_micros {
        None => report.push_str(&catalogue::say("cli.flow.no_spend_cap", &[])),
        Some(cap) => {
            let _ = write!(
                report,
                "{}{}",
                catalogue::say(
                    "cli.flow.spend_cap",
                    &[("micros", &cap.to_string()), ("units", &in_units(cap))]
                ),
                catalogue::say(WHAT_THE_CAP_DOES_NOT_PROMISE_KEY, &[])
            );
        }
    }
    // **WHOEVER CHECKS A FLOW MUST SEE WHAT CAN BE WRITTEN INTO IT.** Naming
    // only the missing actions answers «does this flow run?» and not «what can
    // I put in the next step». The list comes from the registry, never from a
    // copy written here beside it.
    let _ = write!(
        report,
        "\nazioni disponibili: {}",
        registry.names().join(", ")
    );
    if missing.is_empty() {
        report.push_str("\nazioni mancanti: nessuna");
    } else {
        let _ = write!(
            report,
            "\nazioni mancanti: {}",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        );
    }

    let wanted = tools_wanted(&flow.graph);
    let mut unknown = Vec::new();
    match tools {
        // With no detector nothing is declared: a silent report beats one
        // calling every tool unknown for want of a way to look.
        None => {}
        Some(tools) => {
            let (declared, undeclared): (Vec<String>, Vec<String>) =
                wanted.into_iter().partition(|id| tools.declares(id));
            unknown = undeclared;
            if !declared.is_empty() {
                let _ = write!(report, "\nstrumenti chiesti: {}", declared.join(", "));
            }
            if !unknown.is_empty() {
                let _ = write!(
                    report,
                    "\nstrumenti che nessun descrittore dichiara: {}",
                    unknown.join(", ")
                );
            }
            capabilities_into(&mut report, &flow.graph, tools);
            fallbacks_into(&mut report, &flow.graph, tools);
            data_pacts_into(&mut report, &flow.graph, tools);
            // With no probe the report **stays quiet** here instead of calling
            // sound lines it never looked at: the same rule as the missing
            // detector above.
            if let Some(world) = world {
                engine_lines_into(&mut report, &flow.graph, tools, world.probe);
                login_states_into(&mut report, &flow.graph, tools, world);
            }
        }
    }

    // **FIELDS THE ACTION DOES NOT KNOW, TOLD BEFORE SPENDING.** Fault 20:
    // `"prompt"` written where `"stdin"` goes started in silence, the engine
    // got a maimed line, and the error returned was its own — after the call
    // was paid for. Only what a person hand-wrote in the flow is looked at,
    // where a spare field is nobody's output.
    let stray = stray_fields(flow, registry);
    if !stray.is_empty() {
        let _ = write!(
            report,
            "\ncampi che l'azione non conosce (verranno ignorati): {}",
            stray.join("; ")
        );
    }

    // What a step's text leans on, named before the run: two steps once
    // followed skills one home had, no field said so, and the flow passed here
    // while working worse everywhere else without a word. See fault 17.
    let leaning: Vec<String> = undeclared_extensions_named_in_text(flow)
        .iter()
        .map(|named| format!("{} in «{}» ({})", named.step, named.field, named.name))
        .collect();
    if !leaning.is_empty() {
        report.push_str(&catalogue::say(
            "cli.flow.extensions_named_not_declared",
            &[("fields", &leaning.join("; "))],
        ));
    }

    // **FAULT 25, TOLD BEFORE STARTING.** An absolute `workdir` is not seen by
    // running: it is seen later, by looking at which repository got dirty.
    let (fatal, advisory): (Vec<HardcodedPath>, Vec<HardcodedPath>) = hardcoded_paths(flow)
        .into_iter()
        .partition(|path| path.fatal);
    if !fatal.is_empty() {
        let _ = write!(
            report,
            "\npercorsi assoluti in un campo di posizione: {}",
            describe_paths(&fatal)
        );
    }
    if !advisory.is_empty() {
        let _ = write!(
            report,
            "\npercorsi assoluti dentro un testo (il flusso gira, l'istruzione no): {}",
            describe_paths(&advisory)
        );
    }
    (report, unknown)
}

/// Step and field on every line: a warning missing one is unusable — «there is
/// an absolute path» does not say which of the seven steps to change.
fn describe_paths(paths: &[HardcodedPath]) -> String {
    paths
        .iter()
        .map(|path| format!("{} in «{}» ({})", path.step, path.field, path.value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// A capability one step asks of one precise engine.
///
/// The three names travel together because a warning missing one is unusable:
/// «`response_shape` is absent» does not tell the reader which step to change,
/// and in a flow asking the same of the first and third engine of a chain it
/// does not even say which of the two.
struct WantedCapability {
    step: String,
    tool: String,
    capability: String,
}

/// The capabilities steps ask for, step by step and engine by engine.
///
/// **THE CARTESIAN PRODUCT IS WANTED.** A step writing `"tool": ["claude-code",
/// "agy"]` asks that capability of both: the fallback can land on anyone of the
/// chain, so a check reading the first alone would stay quiet about the engine
/// the run ends on when the first dies — as `tools_wanted` counts chains too.
/// Every step of this flow, as far as its cap is concerned.
///
/// **A HANDED STEP DECIDES ON ITS OWN**, whatever the others say: it can start
/// calls outside the control before the call, so no arithmetic over the rest
/// covers the run. Every other step that asks an engine must be reservable —
/// the weakest engine of a chain decides, because any of them may answer.
pub(super) fn cap_facts(
    flow: &FlowFile,
    tools: &dyn actions::ToolResolver,
    prices: &models::pricing::PriceList,
) -> Vec<actions::reserve::StepFact> {
    use actions::reserve::{ceiling_for, reserve_of, why_no_ceiling, Reserve, StepFact};
    let mut facts = Vec::new();
    for step in flow.graph.steps() {
        if step.action == actions::handoff::HANDED_TO_AGENT_ACTION {
            facts.push(StepFact {
                step: step.id.clone(),
                handed_to_agent: true,
                reserve: None,
            });
            continue;
        }
        if step.action != actions::EXTERNAL_ENGINE_ACTION {
            continue;
        }
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        let declared = actions::ceiling_declared_in(with);
        let mut weakest: Option<Reserve> = None;
        for id in engines_of(with) {
            let option = tools.spend_ceiling_option(&id);
            let made = match option.as_ref().and_then(|held| ceiling_for(held, &declared)) {
                None => Reserve::Unknown(format!(
                    "of «{id}», {}",
                    why_no_ceiling(option.as_ref(), &declared)
                )),
                Some(ceiling) => reserve_of(&ceiling, &prices_for(with, &id, prices)),
            };
            let worse = match (&weakest, &made) {
                (None, _) | (Some(Reserve::Known(_)), Reserve::Unknown(_)) => true,
                (Some(Reserve::Known(held)), Reserve::Known(now)) => now > held,
                _ => false,
            };
            if worse {
                weakest = Some(made);
            }
        }
        facts.push(StepFact {
            step: step.id.clone(),
            handed_to_agent: false,
            reserve: weakest,
        });
    }
    facts
}

/// The tariffs of the model this step asks of that engine. A step naming none
/// leaves them empty, and then no ceiling in tokens prices out.
fn prices_for(
    with: &Value,
    id: &str,
    prices: &models::pricing::PriceList,
) -> models::pricing::PriceMicros {
    with.get("model")
        .and_then(|named| named.get(id))
        .and_then(Value::as_str)
        .and_then(|name| prices.find(name))
        .map(models::pricing::Price::micros)
        .unwrap_or_default()
}

/// Why a run of this flow would not start, when it would not.
///
/// Only a flow asking for a guaranteed cap is ever refused: the default is the
/// stop threshold, which almost every shipped step can offer. Whoever asks for
/// the guarantee and cannot have it is told before the run opens, rather than
/// when the bill arrives.
pub(super) fn why_the_run_would_not_start(
    flow: &FlowFile,
    tools: &dyn actions::ToolResolver,
    prices: &models::pricing::PriceList,
) -> Option<String> {
    if flow.required_cap_kind() != flow::CapKind::Guaranteed {
        return None;
    }
    let because = match flow.spend_cap_micros {
        None => catalogue::say("cli.flow.no_cap_to_guarantee", &[]),
        Some(_) => {
            let verdict = actions::reserve::verdict_on(&cap_facts(flow, tools, prices));
            if verdict.kind == flow::CapKind::Guaranteed {
                return None;
            }
            verdict.because.join("; ")
        }
    };
    Some(catalogue::say(
        "cli.flow.run_not_started_without_a_guaranteed_cap",
        &[("because", &because)],
    ))
}

/// The same question asked of this machine: its descriptors, its price list.
/// Both launchers — the command line and the window — ask it here, so a run
/// refused in one is refused in the other.
pub fn why_a_run_here_would_not_start(flow: &FlowFile) -> Option<String> {
    why_the_run_would_not_start(
        flow,
        &toolbox::Tools::current(),
        &actions::current_price_list(),
    )
}

/// Writes what kind of cap this flow declares, every reason it is not one, and
/// whether a run of it would start at all.
fn what_the_cap_is_into(
    report: &mut String,
    flow: &FlowFile,
    tools: &dyn actions::ToolResolver,
    prices: &models::pricing::PriceList,
) {
    if let Some(_cap) = flow.spend_cap_micros {
        let verdict = actions::reserve::verdict_on(&cap_facts(flow, tools, prices));
        report.push_str(&match verdict.kind {
            actions::reserve::CapKind::Guaranteed => {
                catalogue::say("cli.flow.cap_is_guaranteed", &[])
            }
            actions::reserve::CapKind::StopThreshold => catalogue::say(
                "cli.flow.cap_is_a_stop_threshold",
                &[("because", &verdict.because.join("; "))],
            ),
        });
    }
    // A flow that neither caps nor asks for a kind of cap gets no line: there
    // is nothing about money that could hold it back, and a verdict here would
    // invent a subject.
    if flow.spend_cap_micros.is_some() || flow.spend_cap_kind.is_some() {
        report.push_str(&would_the_run_start(flow, tools, prices));
    }
}

/// The line that answers the question whoever runs `flow check` is really
/// asking: would this thing start.
fn would_the_run_start(
    flow: &FlowFile,
    tools: &dyn actions::ToolResolver,
    prices: &models::pricing::PriceList,
) -> String {
    match why_the_run_would_not_start(flow, tools, prices) {
        None => catalogue::say("cli.flow.the_run_would_start", &[]),
        Some(why) => catalogue::say("cli.flow.the_run_would_not_start", &[("why", &why)]),
    }
}

fn capabilities_wanted(graph: &Graph) -> Vec<WantedCapability> {
    let mut wanted = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        let asked: Vec<String> = match with.get("needs_capabilities") {
            Some(Value::Array(names)) => names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            // A single name is written without the brackets, as everywhere.
            Some(Value::String(name)) => vec![name.clone()],
            _ => continue,
        };
        let engines = engines_of(with);
        for capability in &asked {
            for tool in &engines {
                wanted.push(WantedCapability {
                    step: step.id.clone(),
                    tool: tool.clone(),
                    capability: capability.clone(),
                });
            }
        }
    }
    wanted
}

/// Writes into the report the capabilities steps ask for, and how they stand.
///
/// **IT IS A WARNING, NOT AN ERROR.** An engine lacking a capability does not
/// break a flow: whoever cannot impose a shape on the answer asks for it in the
/// prompt and pays more tokens. Under the permanent «model independence»
/// constraint an absent capability is a declared condition, not a fault — the
/// flow still passes, and the launcher knows **before** spending instead of
/// reading it in the engine's answer.
///
/// **AND THE TWO ABSENCES GET TWO DIFFERENT SENTENCES.** «Declares it does not
/// have it» is repaired by changing engine; «nobody looked» is repaired by
/// measuring. One word for both would pass every omission off as measured —
/// exactly what the `capabilities` block exists not to do.
fn capabilities_into(report: &mut String, graph: &Graph, tools: &toolbox::Tools) {
    let mut available = Vec::new();
    let mut gaps = Vec::new();
    for wanted in capabilities_wanted(graph) {
        // A tool no descriptor declares was named above: repeating it here in
        // other words would send the reader hunting two defects where one is.
        let Some(state) = tools.capability(&wanted.tool, &wanted.capability) else {
            continue;
        };
        let line = format!(
            "{} chiede {} a {}",
            wanted.step, wanted.capability, wanted.tool
        );
        match state {
            toolbox::CapabilityState::Available => available.push(line),
            toolbox::CapabilityState::Absent => {
                gaps.push(format!("{line}, che dichiara di non averla"))
            }
            toolbox::CapabilityState::NotLookedAt => gaps.push(format!(
                "{line}, che non la dichiara — nessuno ha guardato se ce l'ha"
            )),
        }
    }
    if !available.is_empty() {
        let _ = write!(report, "\ncapacità chieste: {}", available.join("; "));
    }
    if !gaps.is_empty() {
        let _ = write!(
            report,
            "\ncapacità che il motore non dichiara (il passo funziona lo stesso, \
             pagando di più): {}",
            gaps.join("; ")
        );
    }
}

/// Writes into the report **who cannot be the fallback the chain assigns**,
/// and **which descriptors contradict each other**.
///
/// **THEY ARE ONE DEFECT SEEN FROM TWO SIDES.** A descriptor declaring a
/// capability without the line to use it (fault 32) and one not declaring how
/// it exhausts while placed mid-chain (fault 31) fail the same way: **nothing
/// breaks**. The first makes an engine look askable, the second makes a step
/// look like it has two fallbacks when it has none; what is missing in both is
/// somebody comparing two declarations, and here it happens before spending
/// rather than on the first run where the first engine dies.
///
/// **THE RULES ARE NOT WRITTEN HERE.** They live in `toolbox::Descriptor`, and
/// the tests on the shipped descriptors ask those same two functions. A copy
/// inside `flow check` would be the second rule diverging from the first —
/// fault 10 — and the diverging one is the one a person reads. It stays a
/// warning: a flow whose fallback never fires still runs and does its work
/// while the first engine answers, so it is not broken — it has fewer
/// fallbacks than it looks to have, and the launcher must know beforehand.
///
/// A step whose text is private, and the pact of every engine it names. An
/// engine nobody measured is not a maybe: the run refuses it before spending,
/// so a flow all of whose engines are refused cannot run anywhere, and saying
/// so before the run is the whole point of a check.
fn data_pacts_into(report: &mut String, graph: &Graph, tools: &toolbox::Tools) {
    use actions::ToolResolver as _;

    let mut shut = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        if !actions::private_data_asked_in(with) {
            continue;
        }
        let chain = engines_of(with);
        if chain.is_empty() {
            continue;
        }
        let refused: Vec<String> = chain
            .iter()
            .filter(|id| !tools.data_pact(id).accepts_private())
            .map(|id| {
                catalogue::say(
                    "cli.flow.private_step_engine_refused",
                    &[
                        ("step", &step.id),
                        ("engine", id),
                        ("pact", &tools.data_pact(id).to_string()),
                    ],
                )
            })
            .collect();
        if refused.len() == chain.len() {
            shut.extend(refused);
        }
    }
    if !shut.is_empty() {
        let _ = write!(
            report,
            "{}",
            catalogue::say(
                "cli.flow.private_step_no_engine_may_run",
                &[("steps", &shut.join("; "))],
            )
        );
    }
}

fn fallbacks_into(report: &mut String, graph: &Graph, tools: &toolbox::Tools) {
    let mut plugs = Vec::new();
    for step in graph.steps() {
        let Some(with) = step.with.as_ref() else {
            continue;
        };
        // The last of the chain has nobody to hand the work to: demanding an
        // exhaustion declaration of it would demand a useless measure.
        let chain = engines_of(with);
        let Some((_, before_the_last)) = chain.split_last() else {
            continue;
        };
        for tool in before_the_last {
            // A tool no descriptor declares was named above: repeating it here
            // would send the reader hunting two defects where one is.
            if !tools.declares(tool) {
                continue;
            }
            if let Some(why) = tools.cannot_be_a_fallback(tool) {
                plugs.push(format!("{} → {tool}: {why}", step.id));
            }
        }
    }
    if !plugs.is_empty() {
        let _ = write!(
            report,
            "\nmotori messi in posizione di ripiego che non possono farlo (il passo \
             muore su di loro, e i motori dopo non partono): {}",
            plugs.join("; ")
        );
    }

    // Only the descriptors this flow names: a whole contradictory catalogue is
    // not **this** flow's defect, and showing it here would send someone to
    // fix files this run never touches.
    let named = tools_wanted(graph);
    let disagreeing: Vec<String> = tools
        .contradictions()
        .into_iter()
        .filter(|found| named.contains(&found.tool))
        .map(|found| found.line())
        .collect();
    if !disagreeing.is_empty() {
        let _ = write!(
            report,
            "\ndescrittori che dicono due cose diverse sullo stesso fatto: {}",
            disagreeing.join("; ")
        );
    }
}

/// The hand-written fields the step's action does not recognise.
///
/// It looks in the two places a person writes: the step's `with` in the graph,
/// and the input declared in `inputs`. Never the input the step really gets —
/// that holds the dependencies' output, where stray fields are the norm.
fn stray_fields(flow: &FlowFile, registry: &ActionRegistry) -> Vec<String> {
    let mut found = Vec::new();
    for step in flow.graph.steps() {
        let Some(action) = registry.get(&step.action) else {
            // The action is absent: `azioni mancanti` already says so, and
            // saying it twice would send the reader hunting two defects.
            continue;
        };
        for declared in [step.with.as_ref(), flow.inputs.get(&step.id)]
            .into_iter()
            .flatten()
        {
            let stray = action.unknown_fields(declared);
            if !stray.is_empty() {
                found.push(format!("{}: {}", step.id, stray.join(", ")));
            }
        }
    }
    found
}

/// The tools a flow asks for by identifier.
///
/// It reads the `tool` field of every step, whatever the action: the field's
/// name says that is a tool identifier, not the action carrying it. A future
/// action asking for one would be checked with nobody touching this function.
///
/// **IT COUNTS THE ENGINES INSIDE A CHAIN TOO.** A step may write `"tool":
/// ["claude-code", "agy"]` instead of a single name. Reading the string alone
/// makes those steps look like they ask for nothing, and the check would close
/// green having seen half the flow's engines: fault 3 remade in the same shape.
fn tools_wanted(graph: &Graph) -> BTreeSet<String> {
    graph
        .steps()
        .iter()
        .filter_map(|step| step.with.as_ref())
        .flat_map(engines_of)
        .collect()
}

/// The engines a `with` names, in the order written: one name or a chain.
/// The reader is the action's own, so the check and the run cannot read one
/// step in two ways; the order is data, since the first is tried first.
pub(super) fn engines_of(with: &Value) -> Vec<String> {
    actions::engines_named_in(with)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::*;
    use super::*;
    use registry::{registry_in, House};

    // ── the fields the action does not know ──────────────────────────

    /// **THE TYPO, CAUGHT BEFORE IT IS PAID FOR.**
    ///
    /// A flow wrote `"prompt"` where `"stdin"` goes. The step started anyway,
    /// the engine got a maimed command line, and the error returned was its
    /// own: «Input must be provided either through stdin». A paid call for a
    /// seven-letter typo.
    #[test]
    fn a_field_the_action_does_not_know_is_named_before_the_run() {
        let inputs = r#"{"root":{"tool":"claude-code","prompt":"ciao","timeout_secs":10}}"#;
        let json = flow_json("external_engine", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(
            report.contains("campi che l'azione non conosce"),
            "il controllo deve nominarli: {report}"
        );
        assert!(
            report.contains("root: prompt"),
            "e dire in quale passo e quale campo: {report}"
        );
    }

    /// A `for_each` step is a step like the others to the check: listed with
    /// its dependencies, its action known, and its stray fields named.
    #[test]
    fn a_for_each_step_is_listed_and_its_stray_fields_are_named() {
        let json = r#"{
            "id": "prova", "description": "flusso di prova",
            "graph": {"steps": [{
                "id": "ripeti", "deps": [], "action": "for_each", "max_attempts": 1,
                "when": null, "input_schema": {"type": "any"}, "output_schema": {"type": "any"},
                "with": {"flow": "foglia", "items": [1, 2], "flusso": "foglia"}
            }]},
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("ripeti <- nessuna"), "the step is listed: {report}");
        assert!(
            report.contains("azioni mancanti: nessuna"),
            "the action is one the engine registers: {report}"
        );
        assert!(
            report.contains("ripeti: flusso"),
            "and the field it does not know is named: {report}"
        );
    }

    /// The twin: the **same** flow with the right field says nothing.
    ///
    /// Without it, a check that always complained would pass the test above
    /// and make every report unreadable.
    #[test]
    fn the_same_flow_written_right_says_nothing() {
        let inputs = r#"{"root":{"tool":"claude-code","stdin":"ciao","timeout_secs":10}}"#;
        let json = flow_json("external_engine", "[]", inputs);
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(
            !report.contains("campi che l'azione non conosce"),
            "un flusso scritto bene non deve essere accusato: {report}"
        );
    }

    /// **NO SHIPPED FLOW CARRIES AN UNKNOWN FIELD.** It measures the check
    /// itself: were it saying things at random, this would say so at once on
    /// real code instead of an invented flow.
    #[test]
    fn no_shipped_flow_carries_a_field_nobody_reads() {
        let registry = registry_in(House::empty(), None, None);
        for (name, text) in flow::system::FLOWS {
            let flow: FlowFile = serde_json::from_str(text)
                .unwrap_or_else(|why| panic!("il flusso «{name}» non si carica: {why}"));
            assert!(
                stray_fields(&flow, &registry).is_empty(),
                "«{name}» ha campi che nessuno legge: {:?}",
                stray_fields(&flow, &registry)
            );
        }
    }

    /// A catalogue the test decides, so the outcome does not depend on what is
    /// installed on whoever runs it.
    fn tools_declaring(ids: &[&str]) -> toolbox::Tools {
        let entries: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    r#"{{"id":"{id}","family":"tool","label":"{id}","detect":{{"command":"{id}"}}}}"#
                )
            })
            .collect();
        let file = std::env::temp_dir().join(format!("prova-strumenti-{}.json", ids.join("-")));
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, entries.join(","))).expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(
            catalog,
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
    }

    fn flow_wanting_tool(tool: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "root",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": "{tool}", "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// The measured defect: `flow check` closed at zero saying «azioni
    /// mancanti: nessuna» on a flow naming a tool that existed nowhere, and
    /// the fault was found only by running.
    #[test]
    fn a_tool_no_catalogue_declares_is_named_by_the_check() {
        let flow = flow_wanting_tool("questo-non-esiste-in-nessun-catalogo");
        let tools = tools_declaring(&["git"]);

        let (report, unknown) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert_eq!(unknown, vec!["questo-non-esiste-in-nessun-catalogo"]);
        assert!(
            report.contains(
                "strumenti che nessun descrittore dichiara: questo-non-esiste-in-nessun-catalogo"
            ),
            "{report}"
        );
    }

    /// The other half, and the one that makes the product adoptable: a
    /// **declared** tool is no defect, even when it is not installed on this
    /// machine. A flow written elsewhere is not a broken flow, and calling it
    /// one would make every shared flow unusable.
    #[test]
    fn a_declared_tool_is_reported_but_never_an_error() {
        let flow = flow_wanting_tool("strumento-dichiarato-mai-installato");
        let tools = tools_declaring(&["strumento-dichiarato-mai-installato"]);

        let (report, unknown) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(unknown.is_empty(), "non è un errore: {unknown:?}");
        assert!(
            report.contains("strumenti chiesti: strumento-dichiarato-mai-installato"),
            "{report}"
        );
    }

    /// A catalogue with a single engine, and the capabilities the test gives it.
    ///
    /// **THE FILE NAME CARRIES A COUNTER**, and it is not fussiness: `cargo
    /// test` runs the tests in one process, so two tests writing the same
    /// identifier would steal the file from each other — fault 21, seen once
    /// in twenty runs and always on a different test.
    fn tools_with_capabilities(id: &str, capabilities: &str) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let file = std::env::temp_dir().join(format!(
            "prova-capacita-{}-{}-{id}.json",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[{{"id":"{id}","family":"ai_cli","label":"{id}",
                    "detect":{{"command":"{id}"}},"capabilities":{capabilities}}}]}}"#
            ),
        )
        .expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(
            catalog,
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
    }

    fn flow_needing_capability(tool: &str, capability: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "root",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": "{tool}", "needs_capabilities": ["{capability}"], "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// **THE THIRD CASE THE CHECK COULD NOT TELL.** It told «the tool is not
    /// here» from «it exists in no catalogue»; a step asking an engine for
    /// something that engine cannot do passed for sound, and the defect was
    /// found by paying for the call.
    #[test]
    fn a_capability_the_engine_declares_absent_is_named_with_step_and_engine() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities("un-motore", r#"{"response_shape": false}"#);

        let (report, unknown) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            unknown.is_empty(),
            "resta un avviso, non un errore: {unknown:?}"
        );
        assert!(report.contains("root"), "nomina il passo: {report}");
        assert!(report.contains("un-motore"), "nomina il motore: {report}");
        assert!(
            report.contains("response_shape"),
            "nomina la capacità: {report}"
        );
        assert!(
            report.contains("dichiara di non averla"),
            "e dice che qualcuno ha guardato: {report}"
        );
    }

    /// **AND THE DISTINCTION REACHES THE READER.** Were the two sentences one,
    /// the `capabilities` block could have been a list of what is there, and
    /// every silence would pass for a measure. The remedy differs: above one
    /// changes engine, here one measures what one has.
    #[test]
    fn a_capability_nobody_measured_is_told_apart_from_one_declared_absent() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities("un-motore", r#"{"choose_model": true}"#);

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            report.contains("nessuno ha guardato"),
            "il descrittore tace su quella capacità: {report}"
        );
        assert!(
            !report.contains("dichiara di non averla"),
            "e tacere non è dichiarare un'assenza: {report}"
        );
    }

    /// A flow whose private step names only engines nobody measured cannot run
    /// anywhere, and the check says so before a run finds out by failing.
    #[test]
    fn a_private_step_whose_engines_may_not_take_it_is_named_by_the_check() {
        let flow = private_flow_wanting(&["muto", "taciturno"]);
        let tools = tools_with_pacts(&[("muto", "unknown"), ("taciturno", "trains")]);

        let (report, _) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            report.contains("cannot run anywhere") || report.contains("da nessuna parte"),
            "the flow cannot run anywhere and the check must say it: {report}"
        );
        assert!(
            report.contains("muto") && report.contains("taciturno"),
            "and it names every engine that may not take it: {report}"
        );
    }

    /// One engine that may take it is enough: a chain falls back, and a check
    /// that shouted at every unmeasured engine in a chain would stop being read.
    #[test]
    fn a_private_step_with_one_engine_that_may_take_it_raises_no_warning() {
        let flow = private_flow_wanting(&["muto", "misurato"]);
        let tools = tools_with_pacts(&[("muto", "unknown"), ("misurato", "does_not_train")]);

        let (report, _) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            !report.contains("cannot run anywhere") && !report.contains("da nessuna parte"),
            "one engine may take it, so the step runs: {report}"
        );
    }

    /// A step that says nothing about its text is public, and a public step
    /// goes to any engine: the check must not read silence as a demand.
    #[test]
    fn a_step_that_declares_no_data_is_not_read_as_private() {
        let flow = flow_wanting_tool("muto");
        let tools = tools_with_pacts(&[("muto", "unknown")]);

        let (report, _) =
            check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            !report.contains("cannot run anywhere") && !report.contains("da nessuna parte"),
            "silence is public: {report}"
        );
    }

    /// Engines declaring the pact each is given, and a flow whose only step is
    /// private and names the chain asked for.
    fn tools_with_pacts(pacts: &[(&str, &str)]) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let entries: Vec<String> = pacts
            .iter()
            .map(|(id, pact)| {
                format!(
                    r#"{{"id":"{id}","family":"ai_cli","label":"{id}",
                        "detect":{{"command":"{id}"}},"data_pact":"{pact}"}}"#
                )
            })
            .collect();
        let file = std::env::temp_dir().join(format!(
            "prova-patti-{}-{}.json",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::write(&file, format!(r#"{{"tools":[{}]}}"#, entries.join(","))).expect("scrivere");
        toolbox::Tools::new(
            toolbox::Catalog::load(&[toolbox::Source::File(file)]),
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
    }

    fn private_flow_wanting(chain: &[&str]) -> FlowFile {
        let tools: Vec<String> = chain.iter().map(|id| format!("\"{id}\"")).collect();
        let json = format!(
            r#"{{
                "id": "prova",
                "description": "flusso di prova",
                "graph": {{
                    "steps": [{{
                        "id": "root",
                        "deps": [],
                        "action": "external_engine",
                        "max_attempts": 1,
                        "when": null,
                        "with": {{"tool": [{}], "data": "private", "timeout_secs": 10}},
                        "input_schema": {{"type": "any"}},
                        "output_schema": {{"type": "any"}}
                    }}],
                    "skippable_dependencies": []
                }},
                "inputs": {{}}
            }}"#,
            tools.join(",")
        );
        serde_json::from_str(&json).expect("caricare il flusso")
    }

    /// A capability declared and obtainable raises no warning: a check that
    /// complains even when all is well stops being read.
    #[test]
    fn a_capability_the_engine_has_raises_no_warning() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities(
            "un-motore",
            r#"{"response_shape": {"args": ["--json-schema"], "takes_value": true}}"#,
        );

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            !report.contains("capacità che il motore non dichiara"),
            "{report}"
        );
        assert!(
            report.contains("capacità chieste: root chiede response_shape a un-motore"),
            "quello che c'è si vede lo stesso: {report}"
        );
    }

    /// **A STEP DECLARING ITS OWN CAPABILITIES HAS NO SPARE FIELD.** Without
    /// `needs_capabilities` in the engine spec, the same report would also
    /// raise the stray-fields line, and the reader would hunt a typo that is
    /// not there: fault 20 reversed, a true warning on a right field.
    #[test]
    fn declaring_needed_capabilities_is_not_a_stray_field() {
        let flow = flow_needing_capability("un-motore", "response_shape");
        let tools = tools_with_capabilities("un-motore", r#"{"response_shape": true}"#);

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), Some(&tools), None);

        assert!(
            !report.contains("campi che l'azione non conosce"),
            "{report}"
        );
    }

    // ── who cannot be the fallback the chain gives them ────────────────

    /// A catalogue of two engines, the first of which declares — or stays
    /// quiet about — how it says it cannot work.
    fn tools_where_the_first_says(exhaustion: &str) -> toolbox::Tools {
        static SERIAL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let file = std::env::temp_dir().join(format!(
            "prova-ripiego-{}-{}.json",
            std::process::id(),
            SERIAL.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
        ));
        std::fs::write(
            &file,
            format!(
                r#"{{"tools":[
                  {{"id":"primo","family":"ai_cli","label":"primo",
                    "detect":{{"command":"primo"}},
                    "ask":{{"args":["-p"],"prompt":"stdin"{exhaustion}}},
                    "capabilities":{{"ask_without_interaction":{{"args":["-p"]}}}}}},
                  {{"id":"secondo","family":"ai_cli","label":"secondo",
                    "detect":{{"command":"secondo"}},
                    "ask":{{"args":["-p"],"prompt":"stdin","unusable_when":["quota"]}},
                    "capabilities":{{"ask_without_interaction":{{"args":["-p"]}}}}}}
                ]}}"#
            ),
        )
        .expect("scrivere");
        let catalog = toolbox::Catalog::load(&[toolbox::Source::File(file)]);
        toolbox::Tools::new(
            catalog,
            toolbox::Machine::bare(std::path::PathBuf::from(toolbox::probe::NOWHERE)),
        )
    }

    /// One step with a chain of two engines.
    fn flow_with_a_chain() -> FlowFile {
        let json = r#"{
            "id": "catena",
            "description": "un passo con un ripiego",
            "graph": {
                "steps": [{
                    "id": "root", "deps": [], "action": "external_engine",
                    "max_attempts": 1, "when": null,
                    "with": {"tool": ["primo", "secondo"], "timeout_secs": 10},
                    "input_schema": {"type": "any"}, "output_schema": {"type": "any"}
                }],
                "skippable_dependencies": []
            },
            "inputs": {}
        }"#;
        serde_json::from_str(json).expect("caricare il flusso")
    }

    /// **FAULT 31, TOLD BEFORE SPENDING AND ON THE LAUNCHER'S OWN FLOWS.**
    ///
    /// A test over this tree's flows watches this tree. Whoever writes a flow
    /// of their own, with a descriptor of their own in
    /// `~/.config/sailor/tools.d/`, would remake the same defect with nothing
    /// going red anywhere: the chain would look like a fallback and have none.
    ///
    /// **THE TWO CASES SHARE ONE TEST ON PURPOSE.** The inputs differ only in
    /// whether the first engine declares its own words: two equal reports would
    /// mean the check is not looking, and a test seeking the phrase alone would
    /// stay green against a mutant that always prints it.
    #[test]
    fn a_chain_whose_first_engine_cannot_fall_back_is_named_by_the_check() {
        let flow = flow_with_a_chain();
        let registry = registry_in(House::empty(), None, None);

        let silent = tools_where_the_first_says("");
        let (about_the_silent, _) = check_report(&flow, &registry, Some(&silent), None);
        let speaking = tools_where_the_first_says(r#","unusable_when":["weekly limit"]"#);
        let (about_the_speaking, _) = check_report(&flow, &registry, Some(&speaking), None);

        assert!(
            about_the_silent.contains("motori messi in posizione di ripiego che non possono farlo")
                && about_the_silent.contains("root → primo"),
            "{about_the_silent}"
        );
        assert!(
            !about_the_speaking.contains("posizione di ripiego"),
            "chi dichiara le proprie parole non va segnalato: {about_the_speaking}"
        );
        // And the defect is the **first**'s: the last has nobody to hand the
        // work to, and demanding a measure of it would demand it for nothing.
        assert!(
            !about_the_silent.contains("root → secondo"),
            "{about_the_silent}"
        );
    }

    /// With no detector the report stays quiet about tools instead of calling
    /// them all unknown: being unable to look is not having seen a gap.
    #[test]
    fn without_a_detector_the_check_says_nothing_about_tools() {
        let flow = flow_wanting_tool("qualunque");

        let (report, unknown) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(unknown.is_empty());
        assert!(!report.contains("strument"), "{report}");
    }

    // ── what kind of cap: guaranteed, or a stop threshold ───────────────

    /// An engine that can be told the most one call may spend, and one that
    /// cannot: the two facts the verdict is made of.
    struct SomeHoldToACeiling;

    impl actions::ToolResolver for SomeHoldToACeiling {
        fn resolve(&self, id: &str) -> Result<String, String> {
            Ok(format!("/finto/{id}"))
        }
        fn spend_ceiling_option(&self, id: &str) -> Option<actions::reserve::CeilingOption> {
            (id == "motore-con-tetto").then(|| actions::reserve::CeilingOption {
                args: vec!["--max-budget-usd".to_owned()],
                unit: actions::reserve::UNIT_CURRENCY.to_owned(),
            })
        }
    }

    fn a_flow_of(steps: &str) -> FlowFile {
        let json = format!(
            r#"{{
                "id": "prova", "description": "flusso di prova",
                "graph": {{ "steps": [{steps}], "skippable_dependencies": [] }},
                "inputs": {{}}
            }}"#
        );
        let mut flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");
        flow.spend_cap_micros = Some(5_000_000);
        flow
    }

    fn a_step(id: &str, action: &str, with: &str) -> String {
        format!(
            r#"{{"id": "{id}", "deps": [], "action": "{action}", "max_attempts": 1,
                 "when": null, "with": {with},
                 "input_schema": {{"type": "any"}}, "output_schema": {{"type": "any"}}}}"#
        )
    }

    /// **A HANDED STEP TAKES THE GUARANTEE AWAY FROM THE WHOLE RUN.** The two
    /// flows differ by one step, and that step asks no engine of its own: what
    /// changes is only that it can start calls outside this control.
    ///
    /// *Mutant run*: in `cap_facts`, treat `handed_to_agent` as any other
    /// action. Both reports then say «guaranteed» and this goes red.
    #[test]
    fn a_flow_with_a_handed_step_is_not_a_guaranteed_cap() {
        let bounded = a_step(
            "chiedi",
            "external_engine",
            r#"{"tool": "motore-con-tetto", "stdin": "ciao", "timeout_secs": 10, "max_spend_micros": 600000}"#,
        );
        let handed = a_step("delega", "handed_to_agent", r#"{"mandate": "fai"}"#);
        let prices = models::pricing::PriceList::default();

        let mut alone = String::new();
        what_the_cap_is_into(
            &mut alone,
            &a_flow_of(&bounded),
            &SomeHoldToACeiling,
            &prices,
        );
        assert!(alone.contains("guaranteed cap"), "{alone}");
        assert!(!alone.contains("stop threshold"), "{alone}");

        let mut with_handed = String::new();
        what_the_cap_is_into(
            &mut with_handed,
            &a_flow_of(&format!("{bounded}, {handed}")),
            &SomeHoldToACeiling,
            &prices,
        );
        assert!(with_handed.contains("stop threshold"), "{with_handed}");
        assert!(with_handed.contains("«delega»"), "{with_handed}");
    }

    /// **AN ENGINE THAT TAKES NO CEILING IS THE OTHER HALF.** Same flow, same
    /// declared maximum, only the engine changes: one can be told the ceiling
    /// and one cannot, and the second leaves the cap a stop threshold naming
    /// the capability that is missing.
    #[test]
    fn an_engine_that_takes_no_ceiling_leaves_the_cap_a_stop_threshold() {
        let prices = models::pricing::PriceList::default();
        let of = |tool: &str| {
            let mut said = String::new();
            what_the_cap_is_into(
                &mut said,
                &a_flow_of(&a_step(
                    "chiedi",
                    "external_engine",
                    &format!(
                        r#"{{"tool": "{tool}", "stdin": "ciao", "timeout_secs": 10, "max_spend_micros": 600000}}"#
                    ),
                )),
                &SomeHoldToACeiling,
                &prices,
            );
            said
        };

        assert!(of("motore-con-tetto").contains("guaranteed cap"));
        let without = of("motore-senza-tetto");
        assert!(without.contains("stop threshold"), "{without}");
        assert!(without.contains("native_spend_cap"), "{without}");
    }

    /// **THE FLOW SAYS WHICH OF THE TWO IT WANTS, AND THE ANSWER CHANGES.**
    /// One flow, one engine that takes no ceiling: the default starts under a
    /// stop threshold, the guarantee does not start at all and says why.
    ///
    /// *Mutant run*: make `why_the_run_would_not_start` answer `None` always.
    /// The second half goes red, and so does `flow check`'s line.
    #[test]
    fn a_flow_that_requires_a_guaranteed_cap_and_cannot_have_one_does_not_start() {
        let prices = models::pricing::PriceList::default();
        let flow = a_flow_of(&a_step(
            "chiedi",
            "external_engine",
            r#"{"tool": "motore-senza-tetto", "stdin": "ciao", "timeout_secs": 10}"#,
        ));
        assert_eq!(
            why_the_run_would_not_start(&flow, &SomeHoldToACeiling, &prices),
            None,
            "the default is the stop threshold, and it starts"
        );

        let mut demanding = flow.clone();
        demanding.spend_cap_kind = Some(flow::CapKind::Guaranteed);
        let refused = why_the_run_would_not_start(&demanding, &SomeHoldToACeiling, &prices)
            .expect("a guarantee this flow cannot give");
        assert!(refused.contains("Run not started"), "{refused}");
        assert!(refused.contains("native_spend_cap"), "{refused}");

        // And `flow check` says it before anyone launches.
        let mut said = String::new();
        what_the_cap_is_into(&mut said, &demanding, &SomeHoldToACeiling, &prices);
        assert!(said.contains("would a run start: no"), "{said}");
        let mut allowed = String::new();
        what_the_cap_is_into(&mut allowed, &flow, &SomeHoldToACeiling, &prices);
        assert!(allowed.contains("would a run start: yes"), "{allowed}");
    }

    /// **THE GUARANTEE HELD, AND THE RUN STARTS.** The same demand against an
    /// engine that does take a ceiling: nothing holds the run back.
    #[test]
    fn a_guaranteed_cap_that_can_be_had_starts() {
        let mut flow = a_flow_of(&a_step(
            "chiedi",
            "external_engine",
            r#"{"tool": "motore-con-tetto", "stdin": "ciao", "timeout_secs": 10, "max_spend_micros": 600000}"#,
        ));
        flow.spend_cap_kind = Some(flow::CapKind::Guaranteed);
        assert_eq!(
            why_the_run_would_not_start(
                &flow,
                &SomeHoldToACeiling,
                &models::pricing::PriceList::default()
            ),
            None
        );
    }

    /// **A GUARANTEE OVER NOTHING IS REFUSED TOO.** A flow demanding the
    /// guaranteed kind and declaring no cap has nothing to guarantee, and
    /// starting it would leave the demand with no effect at all.
    #[test]
    fn a_guaranteed_cap_with_no_cap_declared_does_not_start() {
        let mut flow = a_flow_of(&a_step(
            "chiedi",
            "external_engine",
            r#"{"tool": "motore-con-tetto", "stdin": "ciao", "timeout_secs": 10, "max_spend_micros": 600000}"#,
        ));
        flow.spend_cap_micros = None;
        flow.spend_cap_kind = Some(flow::CapKind::Guaranteed);
        let refused = why_the_run_would_not_start(
            &flow,
            &SomeHoldToACeiling,
            &models::pricing::PriceList::default(),
        )
        .expect("there is no cap to guarantee");
        assert!(refused.contains("spend_cap_micros"), "{refused}");
    }

    /// **A CAP THE FLOW DOES NOT DECLARE GETS NO VERDICT**: there is nothing
    /// to be guaranteed or not, and a line here would invent a subject.
    #[test]
    fn a_flow_with_no_cap_is_told_nothing_about_its_kind() {
        let mut flow = a_flow_of(&a_step(
            "chiedi",
            "external_engine",
            r#"{"tool": "motore-senza-tetto", "stdin": "ciao", "timeout_secs": 10}"#,
        ));
        flow.spend_cap_micros = None;
        let mut said = String::new();
        what_the_cap_is_into(
            &mut said,
            &flow,
            &SomeHoldToACeiling,
            &models::pricing::PriceList::default(),
        );
        assert!(said.is_empty(), "{said}");
    }

    // ── the spend cap: `flow check` and `flow cap` ──────────────────────

    /// **TWO FLOWS DIFFERING ONLY BY THE CAP GET TWO DIFFERENT REPORTS.**
    ///
    /// **THE COMPARISON IS BETWEEN THE TWO REPORTS, NOT AGAINST A WORD.** A
    /// test seeking the word «cap» would stay green against a mutant always
    /// printing the same line. Here the inputs differ only by the cap, so two
    /// equal outputs mean the check is not looking at it.
    ///
    /// *Mutant run*: in `check_report`'s `Some(cap)` arm, print
    /// `"\ntetto di spesa: nessuno"` as the `None` arm does. The two reports
    /// become identical and this test goes red.
    #[test]
    fn two_flows_that_differ_only_by_the_cap_get_two_different_reports() {
        let json = flow_json("shell_check", "[]", "{}");
        let without: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");
        let mut with = without.clone();
        with.spend_cap_micros = Some(2_500_000);

        let registry = registry_in(House::empty(), None, None);
        let (said_without, _) = check_report(&without, &registry, None, None);
        let (said_with, _) = check_report(&with, &registry, None, None);

        assert_ne!(
            said_without, said_with,
            "il tetto non compare nel rapporto: {said_with}"
        );
        assert!(said_without.contains("spend cap: none"), "{said_without}");
        assert!(said_with.contains("2500000 micro"), "{said_with}");
    }

    /// **A CAP THAT IS THERE CARRIES WHAT IT DOES NOT PROMISE.**
    ///
    /// A number alone reads as a guarantee on the spend. The three real limits
    /// — the brake does not reach the engines, the first front is never braked,
    /// costless calls stay out — must sit beside the number, not in a document
    /// nobody opens while launching.
    #[test]
    fn a_cap_in_the_report_declares_what_it_does_not_promise() {
        let json = flow_json("shell_check", "[]", "{}");
        let mut flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");
        flow.spend_cap_micros = Some(1);

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("does not reach the engines"), "{report}");
        assert!(report.contains("first front"), "{report}");
        assert!(report.contains("stay out of the sum"), "{report}");
    }

    #[test]
    fn check_reports_steps_dependencies_and_every_missing_action() {
        let json = flow_json("azione_assente", "[]", "{}");
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("passi: 1"), "{report}");
        assert!(report.contains("cicli: nessuno"), "{report}");
        assert!(report.contains("dipendenze: 0"), "{report}");
        assert!(report.contains("root <- nessuna"), "{report}");
        assert!(
            report.contains("azioni mancanti: azione_assente"),
            "{report}"
        );
    }

    #[test]
    fn check_names_each_dependency_not_only_the_total() {
        let json = r#"{
            "id": "dipendenze",
            "description": "rende visibili gli archi",
            "graph": {
                "steps": [
                    {"id":"root","deps":[],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}},
                    {"id":"child","deps":["root"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}}
                ]
            },
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("dipendenze: 1"), "{report}");
        assert!(report.contains("child <- root"), "{report}");
    }

    /// The phase is for whoever reads the report, so it sits on the step's own
    /// line — and only there: a step that names none gets no label, or the
    /// reader would take the hole for a phase nobody wrote.
    #[test]
    fn check_names_the_phase_of_a_step_that_has_one_and_stays_quiet_otherwise() {
        let json = r#"{
            "id": "fasi",
            "description": "one step names its phase",
            "graph": {
                "steps": [
                    {"id":"root","deps":[],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"}},
                    {"id":"child","deps":["root"],"action":"shell_check","max_attempts":1,"when":null,"input_schema":{"type":"any"},"output_schema":{"type":"any"},"phase":"build"}
                ]
            },
            "inputs": {}
        }"#;
        let flow: FlowFile = serde_json::from_str(json).expect("the flow loads");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        let labelled = format!(
            "child <- root{}",
            catalogue::say("cli.flow.step_phase", &[("phase", "build")])
        );
        assert!(report.contains(&labelled), "{report}");
        assert!(report.contains("root <- nessuna\n"), "{report}");
    }

    #[test]
    fn both_default_actions_are_known_to_check() {
        let registry = registry_in(House::empty(), None, None);
        assert!(registry.get("external_engine").is_some());
        assert!(registry.get("shell_check").is_some());
    }

    /// **THE READER AND THE WRITER ARE THERE EVEN WITH NO LEDGER.**
    ///
    /// The mutant that fells it moves one of the two registrations inside the
    /// `if let Some(ledger)` arm: `flow check` would call an existing action
    /// missing, and would do so exactly on a freshly installed machine. That
    /// the writer then refuses to run without a ledger is another test, in
    /// `registry`: here only the name being known is measured.
    #[test]
    fn the_history_question_is_registered_even_without_a_deposit() {
        let registry = registry_in(House::empty(), None, None);
        assert!(registry.get("history_ask").is_some());
        assert!(
            registry.get("store_write").is_some(),
            "chi scrive è nominabile: senza deposito rifiuta, ma non è un'azione mancante"
        );
    }

    /// The report names the **available** actions, not only the missing ones.
    ///
    /// It falls if the list disappears or stops coming from the registry:
    /// whoever opens a flow to see what can go in it would read a stale line,
    /// or none at all.
    #[test]
    fn the_check_names_the_actions_a_flow_can_use() {
        let json = flow_json("shell_check", "[]", "{}");
        let flow: FlowFile = serde_json::from_str(&json).expect("caricare il flusso");

        let (report, _) = check_report(&flow, &registry_in(House::empty(), None, None), None, None);

        assert!(report.contains("azioni disponibili: "), "{report}");
        assert!(report.contains("history_ask"), "{report}");
        assert!(report.contains("external_engine"), "{report}");
    }

    /// The trigger node and the detector are actions like the others: a flow
    /// naming them is checked with nobody registering them by hand.
    #[test]
    fn the_trigger_and_the_detector_are_known_to_check() {
        let registry = registry_in(House::empty(), None, None);
        assert!(registry.get("trigger").is_some());
        assert!(registry.get("detect_tools").is_some());
    }

    /// **THE ENGINE REGISTERED HERE CAN RESOLVE A TOOL.** The mutant that fells
    /// this test removes the line that substitutes it: the step would answer
    /// «this engine has no way to resolve it» again, and a flow naming tools
    /// instead of binaries would stop starting. The identifier looked up does
    /// not exist on purpose: what counts is *who* complains, not that the tool
    /// is there.
    #[test]
    fn the_registered_engine_knows_how_to_resolve_a_tool_id() {
        let registry = registry_in(House::empty(), None, None);
        let engine = registry
            .get("external_engine")
            .expect("il motore è registrato");
        let input = serde_json::json!({
            "tool": "nessuno-strumento-si-chiama-cosi",
            "timeout_secs": 1
        });

        let error = engine
            .execute(&input, &flow::SharedState::new())
            .expect_err("quell'identificativo non esiste");

        assert_eq!(error.class, "tool_unavailable", "{}", error.said);
    }
}
