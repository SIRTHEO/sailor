//! A flow never grows what it sends in silence: every flow the repository
//! carries declares what one run of it costs, bound to the version it was
//! measured on.
//!
//! **WHAT THIS JUDGE CANNOT DO, SAID FIRST.** It reads no state of this
//! machine, so it never sees a token. It watches the prose a flow sends.

use sailor::flow_cmd::seeds::{flows_in, read_seeds, words_it_sends, Seed, SEED_FILE};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// How far a seed may sit from the flow it describes. **Zero.** A seed is a
/// number in a file, and a file merges: a merge keeping the older side would
/// take a re-measured seed back off with no conflict and no signal.
const HOW_STALE_A_SEED_MAY_BE: usize = 0;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("the crate sits two levels under the root")
        .to_path_buf()
}

/// What each flow of the tree sends today, beside the seed that declares it.
/// Absent from one side or the other is the interesting case, so both sides
/// are kept and neither is dropped for want of the other.
struct Measured {
    sends: BTreeMap<String, usize>,
    seeds: BTreeMap<String, Seed>,
}

fn measure(root: &Path) -> Measured {
    let seeds = read_seeds(root).expect("the seeds parse");
    let mut sends = BTreeMap::new();
    for (name, path) in flows_in(root) {
        let text = std::fs::read_to_string(&path).expect("a flow file that reads");
        // A flow that does not load is somebody else's judge: here it weighs
        // nothing rather than being counted as a flow that sends no prose.
        if let Ok(sent) = words_it_sends(&text) {
            sends.insert(name, sent);
        }
    }
    Measured {
        sends,
        seeds: seeds.flows,
    }
}

fn how_to_repair(flow: &str, sends: usize) -> String {
    format!(
        "Run «sailor flow seeds» to learn what the ledger saw a run of it cost, then write \
         the row in {SEED_FILE}: \"{flow}\": {{ \"tokens_a_run\": …, \"runs_measured\": …, \
         \"words_it_sends\": {sends} }}"
    )
}

/// A flow nobody priced is the hole this whole file exists for: a new flow
/// added with no row would otherwise ship with nothing watching its bill.
#[test]
fn a_flow_with_no_seed_is_a_flow_nobody_priced() {
    let measured = measure(&root());
    let orphans: Vec<String> = measured
        .sends
        .iter()
        .filter(|(flow, _)| !measured.seeds.contains_key(*flow))
        .map(|(flow, sends)| format!("\n  {flow}: {}", how_to_repair(flow, *sends)))
        .collect();
    assert!(
        orphans.is_empty(),
        "{} flows carry no seed in {SEED_FILE}, so nothing watches what they spend:{}",
        orphans.len(),
        orphans.concat()
    );
}

/// The other end of the same hole: a row for a flow that is gone declares a
/// cost nobody can go back to, and hides the next flow that takes its name.
#[test]
fn a_seed_for_a_flow_that_is_gone_is_a_seed_nobody_re_measured() {
    let measured = measure(&root());
    let stale: Vec<String> = measured
        .seeds
        .keys()
        .filter(|flow| !measured.sends.contains_key(*flow))
        .map(|flow| format!("\n  {flow}"))
        .collect();
    assert!(
        stale.is_empty(),
        "{SEED_FILE} declares seeds for {} flows this tree no longer carries. Take the row \
         out in the commit that removes the flow:{}",
        stale.len(),
        stale.concat()
    );
}

/// **WHAT A STEP ADDED TO A SHIPPED FLOW LOOKS LIKE FROM HERE.** The prose the
/// flow hands to engines grows; the seed does not follow by itself; this goes
/// red naming the flow and the number to write.
#[test]
fn no_flow_sends_more_to_an_engine_than_its_seed_was_measured_on() {
    let measured = measure(&root());
    for (flow, seed) in &measured.seeds {
        let Some(sends) = measured.sends.get(flow) else {
            continue;
        };
        assert!(
            *sends <= seed.words_it_sends,
            "«{flow}» now hands {sends} characters to engines, and its seed was measured on \
             {}: {} more went in. What one run costs was measured on the smaller flow and no \
             longer describes this one. {}",
            seed.words_it_sends,
            sends - seed.words_it_sends,
            how_to_repair(flow, *sends)
        );
    }
}

/// The other side of the ratchet: a seed above the flow lets the next step
/// added slip in underneath it, so the seed follows the flow down as strictly
/// as it holds it up.
#[test]
fn a_seed_that_no_longer_describes_the_flow_is_a_seed_nobody_re_measured() {
    let measured = measure(&root());
    for (flow, seed) in &measured.seeds {
        let Some(sends) = measured.sends.get(flow) else {
            continue;
        };
        assert!(
            seed.words_it_sends <= sends + HOW_STALE_A_SEED_MAY_BE,
            "the seed of «{flow}» was measured on {} characters of prose, the flow now hands \
             {sends}: {} apart. Either a merge took a re-measure back off, or somebody \
             shortened the flow without re-measuring. {}",
            seed.words_it_sends,
            seed.words_it_sends - sends,
            how_to_repair(flow, *sends)
        );
    }
}

/// A number and the runs it came from travel together or neither is worth
/// anything: a count with no runs behind it is invented, and runs with no
/// count are a measurement thrown away.
#[test]
fn a_seed_says_how_many_runs_it_was_read_off() {
    let measured = measure(&root());
    for (flow, seed) in &measured.seeds {
        assert_eq!(
            seed.runs_measured == 0,
            seed.tokens_a_run == 0,
            "«{flow}» declares {} tokens a run off {} costed runs. Zero runs is an honest \
             «nobody has measured it here»; a number off zero runs is invented, and zero \
             tokens off real runs throws the measurement away. {}",
            seed.tokens_a_run,
            seed.runs_measured,
            how_to_repair(flow, seed.words_it_sends)
        );
    }
}

/// **WHOEVER MEASURES GETS MEASURED.** A counter that stopped seeing would
/// send every flow to zero, and the tests above would only ask whether the
/// seeds had followed it down.
#[test]
fn the_counter_can_still_see_what_a_flow_sends() {
    let measured = measure(&root());
    assert!(
        !measured.sends.is_empty(),
        "no flow found in the tree: the counter is not looking"
    );
    assert!(
        measured.sends.values().any(|sends| *sends > 0),
        "every flow sends nothing: the counter is not looking"
    );
    // Today's rows, for whoever re-measures: `cargo test -p sailor --test
    // a_flow_never_grows_what_it_sends_in_silence -- --nocapture`.
    for (flow, sends) in &measured.sends {
        println!("today: {flow} hands {sends} characters to engines");
    }
}
