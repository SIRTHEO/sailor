//! What every registered action does, in the words somebody picks between two
//! neighbours by: what it takes, what it answers, what it refuses, what it
//! costs. Whether it can be redone and whether a run may be closed on it are
//! not repeated here — they are read off the registry, where the action
//! already declares them.

use flow::{ActionRegistry, StepSpecies};
use serde_json::{json, Value};

/// One action's declared surface. Only `refuses` and `costs` may be empty.
pub struct Surface {
    pub action: &'static str,
    pub does: &'static str,
    pub takes: &'static str,
    pub answers: &'static str,
    pub refuses: &'static str,
    pub costs: &'static str,
}

pub const SURFACES: &[Surface] = &[
    Surface {
        action: "action_list",
        does: "lists every action this machine can run, each with what it takes and answers",
        takes: "nothing",
        answers: "`actions`, the bare names, and `surface`, one declaration per action",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "apply_patch",
        does: "writes a unified diff onto the working tree, the one place a proposal becomes a \
               change on disk",
        takes: "`patch`, the diff text; `workdir`, the tree it lands in",
        answers: "which files it touched",
        refuses: "a path outside what the assent list allows, and a denial compiled in that no \
                  run can widen for itself",
        costs: "",
    },
    Surface {
        action: "detect_tools",
        does: "finds which command lines are installed here, by descriptor rather than by guess",
        takes: "`descriptor_paths`, `builtin_catalogs`, `family`, `version_probes`, `workdir`, \
                all optional",
        answers: "one entry per tool found, with where it is and what it declares",
        refuses: "",
        costs: "`version_probes` starts every tool once to read its version",
    },
    Surface {
        action: "dormant_steps",
        does: "names the steps a flow declares whose `when` has never once let the action run",
        takes: "nothing: it reads the ledger, not the flow files",
        answers: "three lists kept apart — never run, always skipped, reached",
        refuses: "answering at all with no ledger to read",
        costs: "walks every recorded run of every flow",
    },
    Surface {
        action: "empty_terminal",
        does: "clears the session in a live terminal by typing that command line's own reset",
        takes: "`tty`, the terminal; `cli`, which command line is in it",
        answers: "that the session came back empty",
        refuses: "a terminal Sailor does not follow; answers «not yet» while that terminal is \
                  still busy",
        costs: "it destroys the context of whoever is in there",
    },
    Surface {
        action: "external_engine",
        does: "runs one command line — an engine, or any binary — on the text this step hands it",
        takes: "`tool`, an identifier or a chain to try in order, or `bin`; `args`, `env`, \
                `stdin`, `workdir`, `model` per engine, `data`, `kind`, `blind`, `tree`, \
                `accept`, `answer_shape`",
        answers: "what the engine said, pruned to `answer_shape` where one is declared, with \
                  what the call cost",
        refuses: "a `data: private` step on an engine whose pact is `trains` or `unknown`, and a \
                  prompt that does not carry the shape it demands",
        costs: "the action that buys tokens: a chain pays for each engine it tries, and the \
                spend cap of the run is what stops it",
    },
    Surface {
        action: "fault_list",
        does: "the faults the register holds, the open ones unless everything is asked for",
        takes: "`everything`, `at_most`",
        answers: "the faults, and how many are open",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "fault_next",
        does: "the oldest fault still open, whole, so a step can make a mandate of it without a \
               second reading",
        takes: "nothing",
        answers: "the fault, and `open` as the boolean a step's `when` reads",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "fault_record",
        does: "files a new fault in the register",
        takes: "the draft: what happened, where, and how it was found",
        answers: "the number it was filed under",
        refuses: "a draft that does not parse, before the register is opened, so a malformed \
                  step is wrong on every machine",
        costs: "",
    },
    Surface {
        action: "flow_draft",
        does: "keeps a flow somebody wrote only if it stands: the graph validates and every \
               action named is registered",
        takes: "`flow`, as an object or as its JSON text",
        answers: "the id it was saved under, how many steps, and the path",
        refuses: "a step naming an action the registry does not hold, and a flow with nowhere \
                  to be written",
        costs: "",
    },
    Surface {
        action: "flow_search",
        does: "ranks the flows this machine sees by the words asked, searching the whole file \
               down to each step's prompt",
        takes: "`query`",
        answers: "the flows that match, best first",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "for_each",
        does: "runs one flow once per element of a list, each as a child run",
        takes: "`flow`, `items`, `inputs`",
        answers: "the children's outputs in the elements' order, whatever order they finished in",
        refuses: "what `subflow` refuses, for the same reasons",
        costs: "as many child runs as there are elements, as wide as the executor's front and no \
                wider",
    },
    Surface {
        action: "handed_to_agent",
        does: "describes the work and leaves it to the agent already alive in the terminal, \
               starting no process of its own",
        takes: "`mandate`, `holder`, `handoff_timeout_secs`, `options`, `needs_extensions`, \
                `same_holder_ok`",
        answers: "what the agent answered, and which option it chose",
        refuses: "an answer from a holder other than the one waited for, unless `same_holder_ok`",
        costs: "it waits: the step stays open until somebody answers or the timeout runs out",
    },
    Surface {
        action: "history_ask",
        does: "asks the ledger how it went, in named questions rather than in SQL",
        takes: "`ask`, one of `step_failures`, `failure_classes`, `last_run`, `open_runs`, \
                `self_care_lines`, `step_duration`, with `flow`, `step_id`, `within_last_runs`",
        answers: "the rows of that question; `said` bounded, and never a step's input or output",
        refuses: "a question not on that list, and any projection of its own",
        costs: "",
    },
    Surface {
        action: "ledger_search",
        does: "searches this machine's runs, store, events and fault register for the words asked",
        takes: "`query`",
        answers: "what matched, best first, each with where it came from",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "mandate_deposit",
        does: "deposits what a terminal was working on, so whoever takes it over is handed it \
               instead of a file of their own",
        takes: "`tree`, `tty`, `session`, `engine`, `model`, `tokens`, `transcript`, `reread`, \
                `alongside`, and `work`",
        answers: "that it was deposited",
        refuses: "a `work` with a field left blank while the author is still there to fill it",
        costs: "reads the tree with git; types nothing and resets no session",
    },
    Surface {
        action: "mandate_resume",
        does: "hands the deposited mandate to the session taking that terminal over",
        takes: "`tree`, `tty`, `session`",
        answers: "the mandate, whole",
        refuses: "answers «not yet» where nothing is deposited for that terminal",
        costs: "",
    },
    Surface {
        action: "mandate_waiting",
        does: "whether a mandate is waiting for this terminal, asked before anybody commits to \
               taking it",
        takes: "`tty`",
        answers: "whose it is, and when it was left",
        refuses: "answers «not yet» where nothing is waiting",
        costs: "",
    },
    Surface {
        action: "mcp_ask",
        does: "questions a tool on an MCP server and keeps the answer",
        takes: "`server` with its command, args, env and cwd; `server_tool`, `arguments`, \
                `project_root`, `checks`, `timeout_secs`, `accept`",
        answers: "what the tool answered, with the checks that ran first",
        refuses: "a call whose preflight checks did not pass and were not waived with a written \
                  reason",
        costs: "starts the server as a process and can reach a paid service; `timeout_secs` is \
                the only bound on it",
    },
    Surface {
        action: "mcp_ready",
        does: "whether that server answers at all, asked before a step spends anything on it",
        takes: "the same `server`, `server_tool`, `project_root` and `checks` as `mcp_ask`",
        answers: "ready or not, and which check said so",
        refuses: "",
        costs: "starts the server once",
    },
    Surface {
        action: "measure_session",
        does: "how full a command line's context is, read from what that command line already \
               writes about itself",
        takes: "`transcript`, `rollout`, `bytes`, `warn`, `oblige`",
        answers: "below, over, or that it does not know — and the third is never read as below",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "measure_terminal",
        does: "how full the session inside a live terminal Sailor follows is",
        takes: "`tty`, `ceiling`",
        answers: "what it read, and whether the ceiling is passed",
        refuses: "a terminal Sailor does not follow",
        costs: "",
    },
    Surface {
        action: "memory_list",
        does: "the memories in force, the superseded and the expired ones left out",
        takes: "nothing",
        answers: "the memories, and the page they render into",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "memory_query",
        does: "asks the workspace graph for what an earlier flow deposited in it",
        takes: "`workspace_id`, and `kind` or `node_id` to narrow it",
        answers: "the nodes in force and the edges between them",
        refuses: "crossing into another workspace unless a step declared the edge",
        costs: "",
    },
    Surface {
        action: "memory_replace",
        does: "rewrites the whole memory set in one gesture, saying what is kept and what is \
               dropped",
        takes: "`keep`, `drop`, `provenance`",
        answers: "how many were kept and how many dropped",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "memory_write",
        does: "deposits a node or a typed edge in the workspace graph flows learn into",
        takes: "a node — `workspace_id`, `node_id`, `kind`, `title`, `summary`, `status`, \
                `written_by` — or an edge, `from`, `to`, `kind`",
        answers: "the key and the revision it was written under; the old revision is superseded, \
                  never erased",
        refuses: "",
        costs: "the read-then-write is not atomic: without `work_claim` first, two writers on \
                one node lose one of the two writes",
    },
    Surface {
        action: "remember",
        does: "keeps one typed, labelled fact with its provenance, superseded by a later write \
               under the same label and never deleted",
        takes: "`kind`, `label`, `value`, `provenance`, `valid_until`",
        answers: "the label it was kept under",
        refuses: "a value carrying a secret, before the first byte lands",
        costs: "",
    },
    Surface {
        action: "shell_check",
        does: "runs one command line of this machine under a deadline and reads its verdict",
        takes: "`command`, `env`, `workdir`, `timeout_secs`, `accept`, `answer_shape`",
        answers: "the verdict, and what the command said where a shape is declared",
        refuses: "a failure the step did not list in `accept`: strictness is the default",
        costs: "reaches no engine and buys nothing, so it takes no room in a front's spending",
    },
    Surface {
        action: "store_list",
        does: "every record of one collection, oldest first",
        takes: "`collection`, `after`",
        answers: "the records",
        refuses: "reading with no store",
        costs: "",
    },
    Surface {
        action: "store_read",
        does: "reads one remembered record back by collection and key",
        takes: "`collection`, `key`",
        answers: "the record, or that there is none",
        refuses: "reading with no store",
        costs: "",
    },
    Surface {
        action: "store_write",
        does: "remembers a fact between one run and the next, in a collection the flow's author \
               names",
        takes: "`collection`, `key`, `value`, `written_by`, `written_at`",
        answers: "the key it was written under",
        refuses: "running with no store: a step that writes into nothing stops instead",
        costs: "",
    },
    Surface {
        action: "subflow",
        does: "runs another flow as a child run of this one",
        takes: "`flow`, `inputs`",
        answers: "the child's outputs, with the run they were recorded under",
        refuses: "a flow that calls itself directly or round a ring, a nesting past the declared \
                  depth, and running with no ledger, which would leave a child nobody can trace",
        costs: "a whole flow: its cap is the tighter of its own and what is left of this run's",
    },
    Surface {
        action: "terminal_survey",
        does: "the terminals Sailor saw opened, which is not the same list as who announced \
               themselves",
        takes: "`worktree`, `at`",
        answers: "working and gone; `closed` said goodbye, `detached` is alive and asked not to \
                  be followed",
        refuses: "saying that anybody is dead",
        costs: "",
    },
    Surface {
        action: "tool_needs",
        does: "what the flows on this machine ask for that is not installed here",
        takes: "`flows_dirs`, `include_default_sources`, `findings`",
        answers: "present, missing and unknown apart, with how many flows and steps it could read",
        refuses: "counting a flow it could not parse as a flow that asks for nothing",
        costs: "",
    },
    Surface {
        action: "topic_drift",
        does: "whether a mandate still belongs to the workspace it arrived in, by keyword \
               overlap on the graph and no embedding",
        takes: "`workspace_id`, `text`",
        answers: "whether a match elsewhere beats the match at home",
        refuses: "moving anything: the step that reads it decides",
        costs: "",
    },
    Surface {
        action: "trigger",
        does: "where the work comes from: reads the declared source and hands the signal to the \
               steps below",
        takes: "`source`, a descriptor's id; `text`, `who`, `where` for a source that carries \
                its own delivery",
        answers: "the signal — who sent it, from where, and the text",
        refuses: "a source no descriptor declares",
        costs: "executes nothing and touches nothing in the world",
    },
    Surface {
        action: "type_into_terminal",
        does: "types one line into a live terminal Sailor follows",
        takes: "`tty`, `line`",
        answers: "that the line was typed",
        refuses: "a terminal Sailor does not follow",
        costs: "somebody else's session sees what it types",
    },
    Surface {
        action: "unused_actions",
        does: "which registered actions no flow on this machine ever names",
        takes: "nothing",
        answers: "the unnamed actions, and how many flows it could read",
        refuses: "reading a step behind a `when` that never fires as unnamed — that is \
                  `dormant_steps`' question",
        costs: "",
    },
    Surface {
        action: "wait_free",
        does: "waits for the session in a terminal to be free instead of typing into one still \
               working",
        takes: "`tty`, `cli`",
        answers: "that it is free",
        refuses: "answers «not yet» while it is busy, and the run stops there until somebody \
                  resumes it",
        costs: "",
    },
    Surface {
        action: "work_claim",
        does: "an agent says it is here, on which repository and which paths, for a lease that \
               expires",
        takes: "`agent`, `repository`, `workdir`, `branch`, `paths`, `doing`, `lease_seconds`, \
                `pid`, `refuse_when_shared`",
        answers: "the claim as it now stands",
        refuses: "with `refuse_when_shared`, paths another agent already holds",
        costs: "the claim expires unless it is renewed: a process killed in its sleep cannot \
                fake one",
    },
    Surface {
        action: "work_release",
        does: "an agent lets go of what it claimed",
        takes: "`agent`, `pid`",
        answers: "what was released",
        refuses: "",
        costs: "",
    },
    Surface {
        action: "work_survey",
        does: "who else is working, and on what",
        takes: "`repository`, `at`",
        answers: "the claims still in force; an expired announcement is not one",
        refuses: "",
        costs: "",
    },
];

pub fn surface_of(action: &str) -> Option<&'static Surface> {
    SURFACES.iter().find(|surface| surface.action == action)
}

/// A surface with the two things the action itself declares beside it.
pub struct Declared {
    pub action: String,
    pub surface: Option<&'static Surface>,
    pub redo: &'static str,
    pub decides_done: bool,
}

fn redo_of(species: StepSpecies) -> &'static str {
    match species {
        StepSpecies::Repeatable => "runs again on its own",
        StepSpecies::Compensable => "undoes its effect, then runs again",
        StepSpecies::HandToHuman => "stops for a person",
    }
}

/// One name's declaration. A name the registry does not hold yet — the two the
/// list registers after reading it — says nothing about redoing rather than
/// guessing: an absent field is honest, a wrong one is not.
pub fn declared_for(action: &str, registry: &ActionRegistry) -> Declared {
    let held = registry.get(action);
    Declared {
        action: action.to_owned(),
        surface: surface_of(action),
        redo: held.map(|held| redo_of(held.species())).unwrap_or_default(),
        decides_done: held.is_some_and(|held| held.is_a_check()),
    }
}

/// Every action the registry holds, in its order, each with what it declares.
pub fn declared_in(registry: &ActionRegistry) -> Vec<Declared> {
    registry
        .names()
        .into_iter()
        .map(|action| declared_for(action, registry))
        .collect()
}

impl Declared {
    pub fn as_json(&self) -> Value {
        let mut said = json!({ "action": self.action });
        if let Some(fields) = said.as_object_mut() {
            if !self.redo.is_empty() {
                fields.insert("redo".to_owned(), json!(self.redo));
                fields.insert("decides_done".to_owned(), json!(self.decides_done));
            }
        }
        if let (Some(surface), Some(fields)) = (self.surface, said.as_object_mut()) {
            fields.insert("does".to_owned(), json!(surface.does));
            fields.insert("takes".to_owned(), json!(surface.takes));
            fields.insert("answers".to_owned(), json!(surface.answers));
            if !surface.refuses.is_empty() {
                fields.insert("refuses".to_owned(), json!(surface.refuses));
            }
            if !surface.costs.is_empty() {
                fields.insert("costs".to_owned(), json!(surface.costs));
            }
        }
        said
    }

    /// The same thing for eyes rather than for a parser.
    pub fn as_lines(&self) -> String {
        let Some(surface) = self.surface else {
            return format!("{}\n  no surface declared", self.action);
        };
        let mut lines = vec![
            format!("{}  {}", self.action, surface.does),
            format!("  takes    {}", surface.takes),
            format!("  answers  {}", surface.answers),
        ];
        if !surface.refuses.is_empty() {
            lines.push(format!("  refuses  {}", surface.refuses));
        }
        if !surface.costs.is_empty() {
            lines.push(format!("  costs    {}", surface.costs));
        }
        if !self.redo.is_empty() {
            lines.push(format!("  redone   {}", self.redo));
        }
        if self.decides_done {
            lines.push("  verdict  a run may be closed on it".to_owned());
        }
        lines.join("\n")
    }
}

/// Every surface one after the other, for whoever reads rather than parses.
pub fn as_lines(registry: &ActionRegistry) -> String {
    declared_in(registry)
        .iter()
        .map(Declared::as_lines)
        .collect::<Vec<String>>()
        .join("\n\n")
}

/// Every surface as the value `action_list` hands back.
pub fn as_json(registry: &ActionRegistry) -> Vec<Value> {
    declared_in(registry)
        .iter()
        .map(Declared::as_json)
        .collect()
}
