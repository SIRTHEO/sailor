//! `sailor accounts`: which accounts Sailor can reach, what each one spent, and
//! which of them has nothing left to give. It reads the store and the profile
//! homes and calls no engine, so asking costs nothing.

use crate::profiles_cmd::{overview, Access, ProfileView};
use ledger::{AccountStanding, Ledger};

/// How far back the spending is summed unless the line says otherwise.
const A_WEEK_OF_HOURS: i64 = 168;

/// How long a quota stays out once an engine says it ran out. Claude's window
/// is five hours; the others are not published, so this is the one figure that
/// is known rather than a figure per engine that would be invented.
const A_QUOTA_COMES_BACK_IN: i64 = 5 * 3_600;

pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor accounts [--hours <n>] [--json]",
    says_key: "",
}];

pub fn run(args: &[String]) -> i32 {
    match dispatch(args) {
        Ok(message) => {
            println!("{message}");
            0
        }
        Err(message) => {
            eprintln!("sailor accounts: {message}");
            1
        }
    }
}

/// What an account is, once access and spending are read together.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// Reachable, and the engine has not refused for want of quota.
    Ready,
    /// The engine said it ran out, recently enough that it may still be out.
    RanOut,
    /// Nobody can use it: not signed in, or answering as somebody else.
    Shut,
    /// It exists and nothing here can tell: an absence, not a refusal.
    Unknown,
}

impl Standing {
    fn key(self) -> &'static str {
        match self {
            Standing::Ready => "ready",
            Standing::RanOut => "ran_out",
            Standing::Shut => "shut",
            Standing::Unknown => "unknown",
        }
    }

    /// The worst of the lot, which is what a single mark has to show.
    fn worse(self, other: Standing) -> Standing {
        let rank = |standing: Standing| match standing {
            Standing::Ready => 0,
            Standing::Unknown => 1,
            Standing::RanOut => 2,
            Standing::Shut => 3,
        };
        if rank(other) > rank(self) {
            other
        } else {
            self
        }
    }
}

/// One account as both surfaces show it: what it is, and what it did.
#[derive(Debug, Clone)]
pub struct AccountView {
    pub cli: String,
    pub profile: Option<String>,
    pub active: bool,
    pub standing: Standing,
    pub said: String,
    pub calls: u64,
    pub spent_micros: i64,
    pub tokens: u64,
    pub last_call_at: i64,
    pub ran_out_at: Option<i64>,
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let (hours, as_json) = how_it_was_asked(args)?;
    let since = machine::now() - hours * 3_600;
    let directory = ledger::default_directory()
        .ok_or_else(|| catalogue::say("cli.accounts.no_home_no_store", &[]))?;
    let ledger = Ledger::open(&directory).map_err(|error| error.to_string())?;
    let spending = ledger
        .accounts_standing(since)
        .map_err(|error| error.to_string())?;
    let declared = overview(None)?;
    let views = joined(&declared, &spending, machine::now());
    Ok(if as_json {
        as_one_object(&views, machine::now())
    } else {
        report(&views, hours)
    })
}

fn how_it_was_asked(args: &[String]) -> Result<(i64, bool), String> {
    let mut hours = A_WEEK_OF_HOURS;
    let mut as_json = false;
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--json" => as_json = true,
            "--hours" => {
                let said = rest.next().ok_or_else(usage_line)?;
                hours = said
                    .parse()
                    .map_err(|_| catalogue::say("cli.accounts.hours_is_a_number", &[("value", said)]))?;
            }
            _ => return Err(usage_line()),
        }
    }
    Ok((hours, as_json))
}

fn usage_line() -> String {
    format!("{} {}", catalogue::say("cli.usage_heading", &[]), USAGE[0].form)
}

/// **PUBLIC SO A TEST AND THE WINDOW READ THE SAME JOIN.** An account the store
/// spent on but no profile declares is still shown: a call that cost money and
/// belongs to nobody is the one worth seeing.
pub fn joined(
    declared: &[ProfileView],
    spending: &[AccountStanding],
    now: i64,
) -> Vec<AccountView> {
    let mut views: Vec<AccountView> = declared
        .iter()
        .map(|profile| {
            let spent = spending.iter().find(|standing| {
                standing.cli == profile.cli_id
                    && standing.profile.as_deref() == Some(profile.name.as_str())
            });
            one_view(
                profile.cli_id.clone(),
                Some(profile.name.clone()),
                profile.active,
                standing_of(profile.access, spent, now),
                profile.said.clone(),
                spent,
            )
        })
        .collect();
    for standing in spending {
        let declared_already = views.iter().any(|view| {
            view.cli == standing.cli && view.profile.as_deref() == standing.profile.as_deref()
        });
        if !declared_already {
            views.push(one_view(
                standing.cli.clone(),
                standing.profile.clone(),
                false,
                if ran_out_recently(Some(standing), now) {
                    Standing::RanOut
                } else {
                    Standing::Unknown
                },
                catalogue::say("cli.accounts.no_profile_declares_it", &[]),
                Some(standing),
            ));
        }
    }
    views.sort_by(|left, right| {
        right
            .spent_micros
            .cmp(&left.spent_micros)
            .then(right.calls.cmp(&left.calls))
            .then(left.cli.cmp(&right.cli))
            .then(left.profile.cmp(&right.profile))
    });
    views
}

fn one_view(
    cli: String,
    profile: Option<String>,
    active: bool,
    standing: Standing,
    said: String,
    spent: Option<&AccountStanding>,
) -> AccountView {
    AccountView {
        cli,
        profile,
        active,
        standing,
        said,
        calls: spent.map(|found| found.calls).unwrap_or_default(),
        spent_micros: spent.map(|found| found.spent_micros).unwrap_or_default(),
        tokens: spent.map(|found| found.tokens).unwrap_or_default(),
        last_call_at: spent.map(|found| found.last_call_at).unwrap_or_default(),
        ran_out_at: spent.and_then(|found| found.ran_out_at),
    }
}

/// **A SHUT DOOR OUTRANKS AN EMPTY TANK.** An account nobody can sign into will
/// not come back on its own in five hours, so it is the worse of the two.
fn standing_of(access: Access, spent: Option<&AccountStanding>, now: i64) -> Standing {
    match access {
        Access::No | Access::Mismatched => Standing::Shut,
        Access::NotKnown => Standing::Unknown,
        Access::Yes | Access::Unverified | Access::HomeDoesNotMove => {
            if ran_out_recently(spent, now) {
                Standing::RanOut
            } else {
                Standing::Ready
            }
        }
    }
}

fn ran_out_recently(spent: Option<&AccountStanding>, now: i64) -> bool {
    spent
        .and_then(|found| found.ran_out_at)
        .is_some_and(|at| now - at < A_QUOTA_COMES_BACK_IN)
}

/// The worst standing of the accounts in use: what one mark has to show.
///
/// **A PROFILE ON THE SHELF IS NOT A PROBLEM.** One declared, never called and
/// not in force held the mark at its worst for good, and a mark that is always
/// at its worst is one nobody reads.
pub fn worst_of(views: &[AccountView]) -> Standing {
    views
        .iter()
        .filter(|view| view.active || view.calls > 0)
        .fold(Standing::Ready, |worst, view| worst.worse(view.standing))
}

/// One mark per standing, so a glance is enough.
fn mark_of(standing: Standing) -> &'static str {
    match standing {
        Standing::Ready => "●",
        Standing::RanOut => "◐",
        Standing::Shut => "○",
        Standing::Unknown => "◌",
    }
}

fn dollars(micros: i64) -> String {
    format!("{:.2}", micros as f64 / 1_000_000.0)
}

/// **PUBLIC SO A TEST READS THE WORDS THE PERSON READS.**
pub fn report(views: &[AccountView], hours: i64) -> String {
    if views.is_empty() {
        return catalogue::say("cli.accounts.none_declared", &[]);
    }
    let mut lines = vec![catalogue::say(
        "cli.accounts.heading",
        &[
            ("count", &views.len().to_string()),
            ("hours", &hours.to_string()),
        ],
    )];
    for view in views {
        lines.push(catalogue::say(
            "cli.accounts.one_account",
            &[
                ("mark", mark_of(view.standing)),
                ("cli", &view.cli),
                (
                    "profile",
                    view.profile
                        .as_deref()
                        .unwrap_or(&catalogue::say("cli.accounts.from_the_terminal", &[])),
                ),
                ("standing", &catalogue::say(&format!("cli.accounts.standing.{}", view.standing.key()), &[])),
                ("spent", &dollars(view.spent_micros)),
                ("calls", &view.calls.to_string()),
            ],
        ));
        if view.standing != Standing::Ready && !view.said.is_empty() {
            lines.push(format!("      {}", view.said));
        }
    }
    lines.join("\n")
}

/// The same reading as one JSON object, for a surface that draws rather than
/// prints. **THE KEYS ARE NOT PROSE**: a mark that had to parse a translated
/// word would read every account as shut the day the line is translated.
pub fn as_one_object(views: &[AccountView], now: i64) -> String {
    let accounts: Vec<serde_json::Value> = views
        .iter()
        .map(|view| {
            serde_json::json!({
                "cli": view.cli,
                "profile": view.profile,
                "active": view.active,
                "standing": view.standing.key(),
                "said": view.said,
                "calls": view.calls,
                "spent": dollars(view.spent_micros),
                "tokens": view.tokens,
                "last_call_ago_s": ago(view.last_call_at, now),
                "ran_out_ago_s": view.ran_out_at.map(|at| now - at),
            })
        })
        .collect();
    serde_json::json!({
        "worst": worst_of(views).key(),
        "read_at": now,
        "accounts": accounts,
    })
    .to_string()
}

fn ago(at: i64, now: i64) -> Option<i64> {
    (at > 0).then(|| (now - at).max(0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const NOW: i64 = 1_000_000;

    fn declared(cli: &str, name: &str, access: Access) -> ProfileView {
        ProfileView {
            cli_id: cli.to_owned(),
            name: name.to_owned(),
            home_dir: PathBuf::from("/nowhere"),
            active: true,
            access,
            said: "as the engine put it".to_owned(),
        }
    }

    fn spent(cli: &str, name: Option<&str>, micros: i64, ran_out_at: Option<i64>) -> AccountStanding {
        AccountStanding {
            cli: cli.to_owned(),
            profile: name.map(str::to_owned),
            home: None,
            calls: 3,
            spent_micros: micros,
            tokens: 100,
            last_call_at: NOW - 60,
            ran_out_at,
        }
    }

    #[test]
    fn a_declared_account_is_joined_to_what_the_store_saw_it_spend() {
        let views = joined(
            &[declared("codex", "someone@example.test", Access::Yes)],
            &[spent("codex", Some("someone@example.test"), 2_500_000, None)],
            NOW,
        );
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].spent_micros, 2_500_000);
        assert_eq!(views[0].standing, Standing::Ready);
    }

    /// **A CALL THAT COST MONEY AND BELONGS TO NOBODY IS THE ONE WORTH SEEING.**
    #[test]
    fn an_account_that_answered_and_no_profile_declares_is_still_shown() {
        let views = joined(&[], &[spent("antigravity", None, 5_770_000, None)], NOW);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].cli, "antigravity");
        assert_eq!(views[0].standing, Standing::Unknown);
    }

    #[test]
    fn a_quota_that_ran_out_inside_the_window_reads_as_ran_out_and_outside_it_does_not() {
        let inside = joined(
            &[declared("codex", "who@example.test", Access::Yes)],
            &[spent("codex", Some("who@example.test"), 0, Some(NOW - 60))],
            NOW,
        );
        assert_eq!(inside[0].standing, Standing::RanOut);
        let outside = joined(
            &[declared("codex", "who@example.test", Access::Yes)],
            &[spent(
                "codex",
                Some("who@example.test"),
                0,
                Some(NOW - A_QUOTA_COMES_BACK_IN - 1),
            )],
            NOW,
        );
        assert_eq!(outside[0].standing, Standing::Ready);
    }

    /// **A SHUT DOOR DOES NOT OPEN IN FIVE HOURS.**
    #[test]
    fn a_door_nobody_can_open_outranks_an_empty_tank() {
        let views = joined(
            &[declared("claude", "who@example.test", Access::No)],
            &[spent("claude", Some("who@example.test"), 0, Some(NOW - 60))],
            NOW,
        );
        assert_eq!(views[0].standing, Standing::Shut);
        assert_eq!(worst_of(&views), Standing::Shut);
    }

    #[test]
    fn a_home_that_answers_as_somebody_else_is_shut_and_an_unreadable_one_is_not() {
        for (access, expected) in [
            (Access::Mismatched, Standing::Shut),
            (Access::NotKnown, Standing::Unknown),
            (Access::Unverified, Standing::Ready),
            (Access::HomeDoesNotMove, Standing::Ready),
        ] {
            let views = joined(&[declared("codex", "who@example.test", access)], &[], NOW);
            assert_eq!(views[0].standing, expected, "on {access:?}");
        }
    }

    #[test]
    fn the_worst_of_the_lot_is_what_one_mark_shows() {
        assert_eq!(worst_of(&[]), Standing::Ready);
        let views = joined(
            &[
                declared("claude", "fine@example.test", Access::Yes),
                declared("codex", "empty@example.test", Access::Yes),
            ],
            &[spent("codex", Some("empty@example.test"), 0, Some(NOW - 60))],
            NOW,
        );
        assert_eq!(worst_of(&views), Standing::RanOut);
    }

    #[test]
    fn the_biggest_spender_is_named_first() {
        let views = joined(
            &[
                declared("claude", "small@example.test", Access::Yes),
                declared("codex", "big@example.test", Access::Yes),
            ],
            &[
                spent("claude", Some("small@example.test"), 1, None),
                spent("codex", Some("big@example.test"), 9_000_000, None),
            ],
            NOW,
        );
        assert_eq!(views[0].profile.as_deref(), Some("big@example.test"));
    }

    /// **THE KEYS ARE NOT PROSE.** A surface that parsed a translated word
    /// would read every account as shut the day the line is translated.
    #[test]
    fn the_json_carries_keys_a_translation_cannot_move() {
        let views = joined(
            &[declared("codex", "who@example.test", Access::Yes)],
            &[spent("codex", Some("who@example.test"), 1_230_000, Some(NOW - 60))],
            NOW,
        );
        let said = as_one_object(&views, NOW);
        let read: serde_json::Value = serde_json::from_str(&said).expect("valid json");
        assert_eq!(read["worst"], "ran_out");
        assert_eq!(read["accounts"][0]["standing"], "ran_out");
        assert_eq!(read["accounts"][0]["spent"], "1.23");
        assert_eq!(read["accounts"][0]["ran_out_ago_s"], 60);
    }

    #[test]
    fn an_account_that_never_answered_says_so_rather_than_claiming_an_instant() {
        let views = joined(&[declared("claude", "idle@example.test", Access::Yes)], &[], NOW);
        let read: serde_json::Value =
            serde_json::from_str(&as_one_object(&views, NOW)).expect("valid json");
        assert!(read["accounts"][0]["last_call_ago_s"].is_null());
    }

    #[test]
    fn an_empty_store_and_no_profile_says_so_rather_than_printing_a_heading() {
        let said = report(&[], 24);
        assert!(!said.contains('\n'), "an empty reading printed a list: {said}");
    }

    #[test]
    fn the_line_is_read_for_the_window_and_the_shape() {
        assert_eq!(how_it_was_asked(&[]).expect("no argument"), (A_WEEK_OF_HOURS, false));
        assert_eq!(
            how_it_was_asked(&["--json".to_owned()]).expect("json"),
            (A_WEEK_OF_HOURS, true)
        );
        assert_eq!(
            how_it_was_asked(&["--hours".to_owned(), "24".to_owned()]).expect("hours"),
            (24, false)
        );
        assert!(how_it_was_asked(&["--hours".to_owned()]).is_err());
        assert!(how_it_was_asked(&["--hours".to_owned(), "many".to_owned()]).is_err());
        assert!(how_it_was_asked(&["--whatever".to_owned()]).is_err());
    }

    /// **A MARK ALWAYS AT ITS WORST IS ONE NOBODY READS.**
    #[test]
    fn a_profile_declared_never_called_and_not_in_force_does_not_hold_the_mark() {
        let mut shelved = declared("claude", "shelf@example.test", Access::No);
        shelved.active = false;
        let views = joined(&[shelved], &[], NOW);
        assert_eq!(views[0].standing, Standing::Shut, "it is still shown as shut");
        assert_eq!(worst_of(&views), Standing::Ready, "the shelf held the mark");
    }

    #[test]
    fn the_same_profile_once_it_answers_does_hold_the_mark() {
        let mut shelved = declared("claude", "shelf@example.test", Access::No);
        shelved.active = false;
        let views = joined(
            &[shelved],
            &[spent("claude", Some("shelf@example.test"), 0, None)],
            NOW,
        );
        assert_eq!(worst_of(&views), Standing::Shut);
    }
}
