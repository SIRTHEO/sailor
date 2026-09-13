use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use ledger::Ledger;
use serde::{Deserialize, Serialize};
use ui::gather::default_ledger_dir;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StripRun {
    pub run_id: String,
    pub entity: String,
    pub state: String,
    pub step: Option<String>,
    pub elapsed_secs: i64,
    pub cap_micros: Option<i64>,
    pub spend_micros: i64,
    pub device: Option<String>,
    pub holder_gone: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StripAccount {
    pub name: String,
    pub cli_id: String,
    pub monogram: String,
    pub unavailable: bool,
    pub spent_fraction: Option<f64>,
    pub resets_at: Option<String>,
    pub read_at: Option<i64>,
    pub unit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StripState {
    pub runs: Vec<StripRun>,
    pub accounts: Vec<StripAccount>,
    pub ended_today: usize,
}

type CachedAccounts = Option<(i64, Vec<StripAccount>)>;

static ACCOUNT_READINGS: OnceLock<Mutex<CachedAccounts>> = OnceLock::new();

pub(crate) fn rank_run(state: &str, holder_gone: bool) -> u8 {
    if state == "waiting" {
        0
    } else if holder_gone || state == "holder_gone" {
        2
    } else {
        1
    }
}

pub(crate) fn sort_strip_runs(runs: &mut [StripRun]) {
    runs.sort_by(|a, b| {
        let rank_a = rank_run(&a.state, a.holder_gone);
        let rank_b = rank_run(&b.state, b.holder_gone);
        if rank_a != rank_b {
            return rank_a.cmp(&rank_b);
        }
        b.elapsed_secs.cmp(&a.elapsed_secs)
    });
}

pub(crate) fn compute_monograms(profiles: &[profiles::Profile]) -> Vec<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    profiles
        .iter()
        .map(|p| {
            let count = counts.entry(p.cli_id.clone()).or_insert(0);
            *count += 1;
            let prefix = match p.cli_id.as_str() {
                "claude" => "CL",
                "codex" => "CX",
                "antigravity" => "AG",
                "gemini" => "GM",
                other => {
                    if other.len() >= 2 {
                        &other[..2]
                    } else {
                        other
                    }
                }
            };
            format!("{}{}", prefix.to_uppercase(), count)
        })
        .collect()
}

fn device_of(worktree: Option<&str>, open_terminals: &[(String, String)]) -> Option<String> {
    let wt = worktree?;
    let matching: Vec<&str> = open_terminals
        .iter()
        .filter(|(_, t_wt)| t_wt == wt)
        .map(|(tty, _)| tty.as_str())
        .collect();
    if matching.len() == 1 {
        Some(matching[0].to_owned())
    } else {
        None
    }
}

fn cap_of(ledger: &Ledger, run_id: &str) -> Option<i64> {
    match ledger.flow_of_run(run_id) {
        Ok(ledger::RunFlow::Recorded(defs)) => defs.first().and_then(|def| {
            serde_json::from_str::<flow::FlowFile>(&def.body)
                .ok()
                .and_then(|f| f.spend_cap_micros)
        }),
        _ => None,
    }
}

fn check_holder(pid: Option<u32>) -> bool {
    matches!(pid, Some(pid) if !ledger::pid_is_alive(pid))
}

pub(crate) fn collect_strip_state(now: i64, since: Option<i64>) -> Result<StripState, String> {
    let ledger_dir = default_ledger_dir();
    let mut runs = Vec::new();
    let mut ended_today = 0;

    let open_terminals: Vec<(String, String)> = match sessions::Sessions::default_path() {
        Ok(path) => match sessions::Sessions::open(path) {
            Ok(store) => match store.terminals() {
                Ok(term_rows) => term_rows
                    .into_iter()
                    .filter(|r| r.closed_at.is_none() && r.detached_at.is_none())
                    .map(|r| (r.tty, r.worktree))
                    .collect(),
                Err(_) => Vec::new(),
            },
            Err(_) => Vec::new(),
        },
        Err(_) => Vec::new(),
    };

    if ledger_dir.exists() {
        if let Ok(ledger) = Ledger::open(&ledger_dir) {
            let mut seen_runs = std::collections::HashSet::new();

            if let Ok(waiting) = ledger.waiting_runs() {
                for wr in waiting {
                    if !seen_runs.insert(wr.run_id.clone()) {
                        continue;
                    }
                    let header = ledger.run_header(&wr.run_id).ok().flatten();
                    let worktree = header.as_ref().and_then(|h| h.worktree.as_deref());
                    let spend_micros = header.as_ref().map(|h| h.total_cost_micros).unwrap_or(0);
                    let cap_micros = cap_of(&ledger, &wr.run_id);
                    let device = device_of(worktree, &open_terminals);
                    let step = if let Ok(steps) = ledger.steps(&wr.run_id) {
                        crate::handoff::handed_of(&steps)
                            .first()
                            .map(|h| h.step_id.clone())
                    } else {
                        None
                    };
                    let elapsed_secs = (now - wr.waiting_since).max(0);

                    runs.push(StripRun {
                        run_id: wr.run_id,
                        entity: wr.entity,
                        state: "waiting".to_owned(),
                        step,
                        elapsed_secs,
                        cap_micros,
                        spend_micros,
                        device,
                        holder_gone: false,
                    });
                }
            }

            if let Ok(unfinished) = ledger.unfinished_runs() {
                for ur in unfinished {
                    if !seen_runs.insert(ur.run_id.clone()) {
                        continue;
                    }
                    let header = ledger.run_header(&ur.run_id).ok().flatten();
                    let worktree = header.as_ref().and_then(|h| h.worktree.as_deref());
                    let spend_micros = header.as_ref().map(|h| h.total_cost_micros).unwrap_or(0);
                    let cap_micros = cap_of(&ledger, &ur.run_id);
                    let device = device_of(worktree, &open_terminals);

                    let (step, holder_gone, step_started_at) = if let Ok(steps) = ledger.steps(&ur.run_id) {
                        let open_step = steps.into_iter().find(|s| s.outcome.is_none());
                        if let Some(s) = open_step {
                            let gone = check_holder(s.held_by_pid);
                            (Some(s.step_id), gone, s.started_at)
                        } else {
                            (None, false, ur.oldest_started_at)
                        }
                    } else {
                        (None, false, ur.oldest_started_at)
                    };
                    let elapsed_secs = (now - step_started_at).max(0);
                    let state = if holder_gone {
                        "holder_gone".to_owned()
                    } else {
                        "working".to_owned()
                    };

                    runs.push(StripRun {
                        run_id: ur.run_id,
                        entity: ur.entity,
                        state,
                        step,
                        elapsed_secs,
                        cap_micros,
                        spend_micros,
                        device,
                        holder_gone,
                    });
                }
            }

            if let Ok(not_yet_runs) = ledger.runs_to_ask_again() {
                for nyr in not_yet_runs {
                    if !seen_runs.insert(nyr.run_id.clone()) {
                        continue;
                    }
                    let header = ledger.run_header(&nyr.run_id).ok().flatten();
                    let worktree = header.as_ref().and_then(|h| h.worktree.as_deref());
                    let spend_micros = header.as_ref().map(|h| h.total_cost_micros).unwrap_or(0);
                    let cap_micros = cap_of(&ledger, &nyr.run_id);
                    let device = device_of(worktree, &open_terminals);
                    let elapsed_secs = (now - nyr.waiting_since).max(0);

                    runs.push(StripRun {
                        run_id: nyr.run_id,
                        entity: nyr.entity,
                        state: "not_yet".to_owned(),
                        step: None,
                        elapsed_secs,
                        cap_micros,
                        spend_micros,
                        device,
                        holder_gone: false,
                    });
                }
            }

            sort_strip_runs(&mut runs);

            let midnight = since.unwrap_or_else(|| now - (now % 86400));
            ended_today = ledger
                .browse(
                    &format!("SELECT COUNT(*) FROM runs WHERE ended_at IS NOT NULL AND ended_at >= {midnight}"),
                    1,
                )
                .ok()
                .and_then(|ans| ans.rows.first().and_then(|r| r.first().and_then(|c| c.as_i64())))
                .unwrap_or(0) as usize;
        }
    }

    let accounts = collect_accounts(now);

    Ok(StripState {
        runs,
        accounts,
        ended_today,
    })
}

fn collect_accounts(now: i64) -> Vec<StripAccount> {
    let cache = ACCOUNT_READINGS.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((read_at, accounts)) = &*guard {
            if now - read_at < 60 {
                return accounts.clone();
            }
        }
    }

    let store = profiles::store_io::load_store().unwrap_or_default();
    let monograms = compute_monograms(&store.profiles);

    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let readings = sailor::remaining_cmd::per_profile(&catalog, &machine, now);

    let mut accounts = Vec::new();
    for (profile, monogram) in store.profiles.into_iter().zip(monograms) {
        let reading = readings.iter().find(|r| {
            r.engine.ends_with(&format!(" · {}", profile.name)) || r.engine == profile.name
        });

        let (unavailable, spent_fraction, resets_at, read_at, unit) = match reading {
            Some(r) => match &r.result {
                Ok(windows) if !windows.is_empty() => {
                    let w = &windows[0];
                    (
                        false,
                        Some(w.used_fraction),
                        w.resets_at.clone(),
                        Some(w.observed_at),
                        Some(w.unit.clone()),
                    )
                }
                _ => (true, None, None, None, None),
            },
            None => (true, None, None, None, None),
        };

        accounts.push(StripAccount {
            name: profile.name,
            cli_id: profile.cli_id,
            monogram,
            unavailable,
            spent_fraction,
            resets_at,
            read_at,
            unit,
        });
    }

    if let Ok(mut guard) = cache.lock() {
        *guard = Some((now, accounts.clone()));
    }
    accounts
}

#[tauri::command]
pub(crate) fn strip(since: Option<i64>) -> Result<StripState, String> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |s| s.as_secs() as i64);
    collect_strip_state(now, since)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_runs_ranked_waiting_then_working_then_holder_gone() {
        let mut runs = vec![
            StripRun {
                run_id: "run-holder-gone".to_string(),
                entity: "deploy".to_string(),
                state: "holder_gone".to_string(),
                step: Some("upload".to_string()),
                elapsed_secs: 50,
                cap_micros: None,
                spend_micros: 0,
                device: None,
                holder_gone: true,
            },
            StripRun {
                run_id: "run-working".to_string(),
                entity: "test-flow".to_string(),
                state: "working".to_string(),
                step: Some("compile".to_string()),
                elapsed_secs: 30,
                cap_micros: Some(1_000_000),
                spend_micros: 200_000,
                device: Some("ttys001".to_string()),
                holder_gone: false,
            },
            StripRun {
                run_id: "run-waiting".to_string(),
                entity: "review".to_string(),
                state: "waiting".to_string(),
                step: Some("approve".to_string()),
                elapsed_secs: 10,
                cap_micros: None,
                spend_micros: 50_000,
                device: Some("ttys002".to_string()),
                holder_gone: false,
            },
        ];

        sort_strip_runs(&mut runs);

        assert_eq!(runs[0].run_id, "run-waiting", "parked on a person must rank first");
        assert_eq!(runs[1].run_id, "run-working", "at work must rank second");
        assert_eq!(runs[2].run_id, "run-holder-gone", "holder gone must rank last");
    }

    #[test]
    fn strip_runs_same_rank_sorted_by_elapsed_descending() {
        let mut runs = vec![
            StripRun {
                run_id: "run-newer".to_string(),
                entity: "flow-a".to_string(),
                state: "working".to_string(),
                step: None,
                elapsed_secs: 15,
                cap_micros: None,
                spend_micros: 0,
                device: None,
                holder_gone: false,
            },
            StripRun {
                run_id: "run-older".to_string(),
                entity: "flow-b".to_string(),
                state: "working".to_string(),
                step: None,
                elapsed_secs: 120,
                cap_micros: None,
                spend_micros: 0,
                device: None,
                holder_gone: false,
            },
        ];

        sort_strip_runs(&mut runs);

        assert_eq!(runs[0].run_id, "run-older", "longer running run should be first");
        assert_eq!(runs[1].run_id, "run-newer");
    }
}
