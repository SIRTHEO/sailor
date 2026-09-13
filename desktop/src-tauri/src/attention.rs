//! The attention queue: what wants a person right now, and where.
//!
//! One single query answering: does anything want me, and where?
//! Combines handed steps, capped runs, unreachable engine quotas, and dead terminals.
//! Ranks: unreadable (0), handed (1), cap_reached (2), engine_unreachable (3), terminal_dead (4).
//! Links to a terminal if exactly one matches the run's worktree; otherwise links to the run.

use ledger::Ledger;
use serde::{Deserialize, Serialize};
use ui::gather::default_ledger_dir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AttentionLink {
    Tty { tty: String },
    Run { run_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttentionRow {
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tty: Option<String>,
    pub status_word: String,
    pub reason: String,
    pub since: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<AttentionLink>,
}

/// A run links to a terminal only when exactly one open terminal matches its worktree.
pub(crate) fn link_for_run(
    run_id: &str,
    worktree: Option<&str>,
    open_terminals: &[(String, String)],
) -> AttentionLink {
    if let Some(wt) = worktree {
        let matching: Vec<&str> = open_terminals
            .iter()
            .filter(|(_, t_wt)| t_wt == wt)
            .map(|(tty, _)| tty.as_str())
            .collect();
        if matching.len() == 1 {
            return AttentionLink::Tty {
                tty: matching[0].to_owned(),
            };
        }
    }
    AttentionLink::Run {
        run_id: run_id.to_owned(),
    }
}

fn kind_rank(kind: &str) -> u8 {
    match kind {
        "unreadable" => 0,
        "handed" => 1,
        "cap_reached" => 2,
        "engine_unreachable" => 3,
        "terminal_dead" => 4,
        _ => 5,
    }
}

pub(crate) fn rank_attention_rows(rows: &mut [AttentionRow]) {
    rows.sort_by(|a, b| {
        let rank_a = kind_rank(&a.kind);
        let rank_b = kind_rank(&b.kind);
        if rank_a != rank_b {
            return rank_a.cmp(&rank_b);
        }
        match (a.since, b.since) {
            (Some(s_a), Some(s_b)) => s_a.cmp(&s_b),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => std::cmp::Ordering::Equal,
        }
    });
}

/// The ledger, opened for the attention queue alone: a directory that is not
/// there is named as such, never conflated with `Ledger::open`'s own error for
/// a directory that exists but will not open (permissions, a corrupt file).
fn open_ledger_for_attention(dir: &std::path::Path) -> Result<Ledger, String> {
    if !dir.exists() {
        return Err(catalogue::say("window.attention.unreadable_missing", &[]));
    }
    Ledger::open(dir).map_err(|error| error.to_string())
}

/// One row saying the store could not be read, in place of everything a
/// readable one would have answered — never silence, and never invented.
fn unreadable_row(dir: &std::path::Path, why: &str) -> AttentionRow {
    AttentionRow {
        kind: "unreadable".to_owned(),
        run_id: None,
        step_id: None,
        tty: None,
        status_word: catalogue::say("window.attention.unreadable_status", &[]),
        reason: catalogue::say(
            "window.attention.unreadable_reason",
            &[("path", &dir.display().to_string()), ("why", why)],
        ),
        since: None,
        link: None,
    }
}

/// Everything the ledger itself answers for the queue: handed steps waiting
/// on a person, and runs that reached their cap. A store that will not open
/// answers with exactly one row of its own, and nothing invented beside it.
fn ledger_rows(dir: &std::path::Path, open_terminals: &[(String, String)]) -> Vec<AttentionRow> {
    let ledger = match open_ledger_for_attention(dir) {
        Ok(ledger) => ledger,
        Err(why) => return vec![unreadable_row(dir, &why)],
    };

    let mut rows = Vec::new();

    if let Ok(waiting_runs) = ledger.waiting_runs() {
        for waiting_run in waiting_runs {
            let worktree = ledger
                .run_header(&waiting_run.run_id)
                .ok()
                .flatten()
                .and_then(|h| h.worktree);
            if let Ok(steps) = ledger.steps(&waiting_run.run_id) {
                let handed_steps = crate::handoff::handed_of(&steps);
                for handed in handed_steps {
                    let reason = if !handed.mandate.trim().is_empty() {
                        handed.mandate.trim().lines().next().unwrap_or("").to_owned()
                    } else {
                        "reason unknown".to_owned()
                    };
                    let reason = if reason.is_empty() {
                        "reason unknown".to_owned()
                    } else {
                        reason
                    };
                    let link = link_for_run(&waiting_run.run_id, worktree.as_deref(), open_terminals);
                    rows.push(AttentionRow {
                        kind: "handed".to_owned(),
                        run_id: Some(waiting_run.run_id.clone()),
                        step_id: Some(handed.step_id),
                        tty: None,
                        status_word: "waiting on you".to_owned(),
                        reason,
                        since: Some(handed.since),
                        link: Some(link),
                    });
                }
            }
        }
    }

    if let Ok(answer) = ledger.browse(
        "SELECT run_id, entity, started_at, ended_at, worktree, stop_reason, error FROM runs WHERE status = 'cap_reached' ORDER BY COALESCE(ended_at, started_at) DESC",
        50,
    ) {
        for cells in answer.rows {
            if let Some(run_id) = cells.first().and_then(|v| v.as_str()) {
                let started_at = cells.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
                let ended_at = cells.get(3).and_then(|v| v.as_i64());
                let worktree = cells.get(4).and_then(|v| v.as_str());
                let stop_reason = cells.get(5).and_then(|v| v.as_str());
                let error = cells.get(6).and_then(|v| v.as_str());

                let reason = stop_reason
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| error.filter(|e| !e.trim().is_empty()))
                    .unwrap_or("reason unknown")
                    .to_owned();

                let since = ended_at.or(Some(started_at));
                let link = link_for_run(run_id, worktree, open_terminals);

                rows.push(AttentionRow {
                    kind: "cap_reached".to_owned(),
                    run_id: Some(run_id.to_owned()),
                    step_id: None,
                    tty: None,
                    status_word: "at its cap".to_owned(),
                    reason,
                    since,
                    link: Some(link),
                });
            }
        }
    }

    rows
}

pub(crate) fn collect_attention_queue() -> Result<Vec<AttentionRow>, String> {
    let mut rows = Vec::new();

    let (open_terminals, abandoned) = match sessions::Sessions::default_path() {
        Ok(path) => match sessions::Sessions::open(path) {
            Ok(store) => match store.terminals() {
                Ok(term_rows) => {
                    let open: Vec<(String, String)> = term_rows
                        .iter()
                        .filter(|r| r.closed_at.is_none() && r.detached_at.is_none())
                        .map(|r| (r.tty.clone(), r.worktree.clone()))
                        .collect();
                    let abandoned = sessions::census::Census::of(&sessions::census::LocalMachine).abandoned(&term_rows);
                    (open, Some(abandoned))
                }
                Err(_) => (Vec::new(), None),
            },
            Err(_) => (Vec::new(), None),
        },
        Err(_) => (Vec::new(), None),
    };

    if let Some(sessions::census::Abandoned::Seen { ttys }) = abandoned {
        for tty in ttys {
            rows.push(AttentionRow {
                kind: "terminal_dead".to_owned(),
                run_id: None,
                step_id: None,
                tty: Some(tty.clone()),
                status_word: "stopped".to_owned(),
                reason: "killed without closing: nothing runs there any more".to_owned(),
                since: None,
                link: Some(AttentionLink::Tty { tty }),
            });
        }
    }

    rows.extend(ledger_rows(&default_ledger_dir(), &open_terminals));

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let readings = sailor::remaining_cmd::per_profile(&catalog, &machine, now);
    for reading in readings {
        if let Err(why) = reading.result {
            let reason = if !why.trim().is_empty() {
                format!("{}: {}", reading.engine, why)
            } else {
                "reason unknown".to_owned()
            };
            rows.push(AttentionRow {
                kind: "engine_unreachable".to_owned(),
                run_id: None,
                step_id: None,
                tty: None,
                status_word: "unreachable".to_owned(),
                reason,
                since: None,
                link: None,
            });
        }
    }

    rank_attention_rows(&mut rows);
    Ok(rows)
}

#[tauri::command]
pub(crate) fn attention_queue() -> Result<Vec<AttentionRow>, String> {
    collect_attention_queue()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A DIRECTORY THAT IS NOT THERE IS A SENTENCE, NOT AN EMPTY LIST.**
    /// Fault: `if ledger_dir.exists() { ... }` with no else arm made a queue
    /// that could not be read look exactly like a queue with nothing in it —
    /// a person watching it lose its one waiting row this way would think the
    /// task had gone away, not that the store had.
    #[test]
    fn a_missing_directory_answers_with_one_unreadable_row_and_invents_nothing_else() {
        let dir = std::env::temp_dir().join(format!(
            "sailor-attention-missing-{}-{}",
            std::process::id(),
            line!()
        ));
        let _ = std::fs::remove_dir_all(&dir);

        let rows = ledger_rows(&dir, &[]);

        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].kind, "unreadable");
        assert!(rows[0].reason.contains(&dir.display().to_string()), "{}", rows[0].reason);
        assert!(!dir.exists(), "reading the queue must not create the directory it found missing");
    }

    #[test]
    fn two_terminals_on_one_worktree_link_is_run() {
        let open_terminals = vec![
            ("ttys001".to_string(), "/work/project".to_string()),
            ("ttys002".to_string(), "/work/project".to_string()),
        ];
        let link = link_for_run("run-42", Some("/work/project"), &open_terminals);
        assert_eq!(link, AttentionLink::Run { run_id: "run-42".to_string() });
    }

    #[test]
    fn one_terminal_on_worktree_link_is_tty() {
        let open_terminals = vec![
            ("ttys001".to_string(), "/work/project".to_string()),
            ("ttys002".to_string(), "/work/other".to_string()),
        ];
        let link = link_for_run("run-42", Some("/work/project"), &open_terminals);
        assert_eq!(link, AttentionLink::Tty { tty: "ttys001".to_string() });
    }

    #[test]
    fn ranking_order_and_fallback_reason() {
        let mut rows = vec![
            AttentionRow {
                kind: "terminal_dead".to_string(),
                run_id: None,
                step_id: None,
                tty: Some("ttys001".to_string()),
                status_word: "stopped".to_string(),
                reason: "killed without closing".to_string(),
                since: None,
                link: Some(AttentionLink::Tty { tty: "ttys001".to_string() }),
            },
            AttentionRow {
                kind: "handed".to_string(),
                run_id: Some("r-2".to_string()),
                step_id: Some("s-2".to_string()),
                tty: None,
                status_word: "waiting on you".to_string(),
                reason: "reason unknown".to_string(),
                since: Some(200),
                link: Some(AttentionLink::Run { run_id: "r-2".to_string() }),
            },
            AttentionRow {
                kind: "handed".to_string(),
                run_id: Some("r-1".to_string()),
                step_id: Some("s-1".to_string()),
                tty: None,
                status_word: "waiting on you".to_string(),
                reason: "first line".to_string(),
                since: Some(100),
                link: Some(AttentionLink::Run { run_id: "r-1".to_string() }),
            },
            AttentionRow {
                kind: "cap_reached".to_string(),
                run_id: Some("r-cap".to_string()),
                step_id: None,
                tty: None,
                status_word: "at its cap".to_string(),
                reason: "cap reached".to_string(),
                since: Some(50),
                link: Some(AttentionLink::Run { run_id: "r-cap".to_string() }),
            },
            AttentionRow {
                kind: "engine_unreachable".to_string(),
                run_id: None,
                step_id: None,
                tty: None,
                status_word: "unreachable".to_string(),
                reason: "offline".to_string(),
                since: None,
                link: None,
            },
        ];

        rank_attention_rows(&mut rows);

        let kinds: Vec<&str> = rows.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, vec!["handed", "handed", "cap_reached", "engine_unreachable", "terminal_dead"]);
        assert_eq!(rows[0].run_id.as_deref(), Some("r-1"));
        assert_eq!(rows[1].run_id.as_deref(), Some("r-2"));
        assert_eq!(rows[1].reason, "reason unknown");
    }
}
