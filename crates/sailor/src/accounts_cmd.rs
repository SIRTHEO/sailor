//! `sailor accounts`: which accounts Sailor can reach, what each one spent, and
//! which of them has nothing left to give. It reads the store and the profile
//! homes and calls no engine, so asking costs nothing.

use crate::profiles_cmd::{overview, Access, ProfileView};
use ledger::{AccountStanding, Ledger};
use std::collections::BTreeMap;

/// How far back the spending is summed unless the line says otherwise.
const A_WEEK_OF_HOURS: i64 = 168;

/// How long a quota stays out once an engine says it ran out. Claude's window
/// is five hours; the others are not published, so this is the one figure that
/// is known rather than a figure per engine that would be invented.
const A_QUOTA_COMES_BACK_IN: i64 = 5 * 3_600;

pub const USAGE: &[crate::Form] = &[crate::Form {
    form: "sailor accounts [--hours <n>] [--json] [--quota]",
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
    pub quota: Option<QuotaSeen>,
    /// What this account did in its own home over the window, read off the
    /// engine's own records. `None` where nobody measured that engine.
    pub worked: Option<models::work::Worked>,
    /// The line a person runs to cure this row, where the engine declares one.
    pub repair: Option<String>,
}

/// One window of an account's allowance.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowLeft {
    pub unit: String,
    /// Spent, from `0.0` to `1.0`, in the shape `models::remaining` keeps it.
    pub used_fraction: f64,
    pub resets_at: Option<String>,
}

/// What asking an account for its allowance answered.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QuotaSeen {
    pub windows: Vec<WindowLeft>,
    pub refused: Option<String>,
    /// **WHETHER THE PROVIDER ITSELF ANSWERED NO.** A home with no credentials
    /// in it is nobody having looked; a token the provider calls revoked is a
    /// dead account, and only the second may contradict «authenticated».
    /// A meter that answers «too often» is neither, and carries `false` here.
    pub credential_is_dead: bool,
    /// Whether the provider was reached at all: a channel that stopped at a
    /// missing file never learned anything about this account.
    pub provider_answered: bool,
}

impl QuotaSeen {
    pub fn ran_out(&self) -> bool {
        self.windows.iter().any(|window| window.used_fraction >= 1.0)
    }

    pub fn fullest(&self) -> Option<&WindowLeft> {
        self.windows
            .iter()
            .max_by(|one, two| one.used_fraction.total_cmp(&two.used_fraction))
    }
}

/// **THE PROFILE IS THE KEY, NOT THE ENGINE.** One account answers under
/// several descriptors — a repair descriptor points at a home with no
/// credentials in it — so a reading that could be taken wins over one that
/// could not, whatever asked for it.
pub fn quota_by_profile(readings: &[toolbox::quota::Reading]) -> BTreeMap<String, QuotaSeen> {
    let mut found: BTreeMap<String, QuotaSeen> = BTreeMap::new();
    for reading in readings {
        let Some((_, profile)) = reading.engine.split_once(" · ") else {
            continue;
        };
        let seen = match &reading.result {
            Ok(windows) => QuotaSeen {
                windows: windows
                    .iter()
                    .map(|window| WindowLeft {
                        unit: window.unit.clone(),
                        used_fraction: window.used_fraction,
                        resets_at: window.resets_at.clone(),
                    })
                    .collect(),
                refused: None,
                credential_is_dead: false,
                provider_answered: true,
            },
            Err(refusal) => QuotaSeen {
                windows: Vec::new(),
                refused: Some(refusal.said.clone()),
                credential_is_dead: refusal.credential_is_dead,
                provider_answered: refusal.provider_answered,
            },
        };
        if found.get(profile).is_none_or(|held| worth(&seen) > worth(held)) {
            found.insert(profile.to_owned(), seen);
        }
    }
    found
}

/// How much one reading of an account is worth against another of the same
/// account. **THE ORDER OF THE DESCRIPTORS MUST NOT DECIDE**: a measure beats
/// any refusal, a refusal the provider itself gave beats one from a channel
/// that stopped at a missing file, and among the provider's own a dead
/// credential is the news.
fn worth(seen: &QuotaSeen) -> u8 {
    if !seen.windows.is_empty() {
        return 4;
    }
    match (seen.provider_answered, seen.credential_is_dead) {
        (true, true) => 3,
        (true, false) => 2,
        _ => 1,
    }
}

fn dispatch(args: &[String]) -> Result<String, String> {
    let (hours, as_json, with_quota) = how_it_was_asked(args)?;
    let since = machine::now() - hours * 3_600;
    let directory = ledger::default_directory()
        .ok_or_else(|| catalogue::say("cli.accounts.no_home_no_store", &[]))?;
    let ledger = Ledger::open(&directory).map_err(|error| error.to_string())?;
    let spending = ledger
        .accounts_standing(since)
        .map_err(|error| error.to_string())?;
    let mut declared = overview(None)?;
    declared.extend(crate::profiles_cmd::engines_own_homes(&declared));
    let mut views = joined(
        &declared,
        &spending,
        &asked_for_allowances(with_quota),
        &what_they_worked(since),
        machine::now(),
    );
    name_the_repairs(&mut views, &declared);
    Ok(if as_json {
        as_one_object(&views, machine::now())
    } else {
        report(&views, hours)
    })
}

/// **ASKING COSTS A ROUND TRIP PER ACCOUNT**, to the provider and to the
/// keychain, so it is asked for and never assumed. The windows are five hours
/// and seven days wide: nothing here wants asking every minute.
fn asked_for_allowances(with_quota: bool) -> BTreeMap<String, QuotaSeen> {
    if !with_quota {
        return BTreeMap::new();
    }
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    quota_by_profile(&crate::remaining_cmd::per_profile(
        &catalog,
        &machine,
        machine::now(),
    ))
}

/// The gesture that cures each row that needs one. **IT IS NAMED, NOT MADE**:
/// the login is interactive by declaration, and a panel that signs somebody in
/// by itself re-authorises whoever's browser is already open.
fn name_the_repairs(views: &mut [AccountView], declared: &[ProfileView]) {
    for view in views.iter_mut().filter(|view| view.standing == Standing::Shut) {
        let Some(profile) = declared
            .iter()
            .find(|profile| profile.cli_id == view.cli && Some(&profile.name) == view.profile.as_ref())
        else {
            continue;
        };
        view.repair = crate::profiles_cmd::how_to_sign_in(&profile.cli_id, &profile.home_dir);
    }
}

/// **READ WHATEVER IS ASKED, BECAUSE IT COSTS NO ROUND TRIP.** These records
/// are files under the homes; only the ones touched inside the window are
/// opened, so the whole panel is read without asking anybody anything.
fn what_they_worked(since: i64) -> BTreeMap<(String, String), models::work::Worked> {
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    crate::remaining_cmd::work_per_profile(&catalog, since)
}

fn how_it_was_asked(args: &[String]) -> Result<(i64, bool, bool), String> {
    let mut hours = A_WEEK_OF_HOURS;
    let mut as_json = false;
    let mut with_quota = false;
    let mut rest = args.iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--json" => as_json = true,
            "--quota" => with_quota = true,
            "--hours" => {
                let said = rest.next().ok_or_else(usage_line)?;
                hours = said
                    .parse()
                    .map_err(|_| catalogue::say("cli.accounts.hours_is_a_number", &[("value", said)]))?;
            }
            _ => return Err(usage_line()),
        }
    }
    Ok((hours, as_json, with_quota))
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
    quota: &BTreeMap<String, QuotaSeen>,
    worked: &BTreeMap<(String, String), models::work::Worked>,
    now: i64,
) -> Vec<AccountView> {
    let mut views: Vec<AccountView> = declared
        .iter()
        .map(|profile| {
            let spent = spending.iter().find(|standing| {
                standing.cli == profile.cli_id
                    && standing.profile.as_deref() == Some(profile.name.as_str())
            });
            let left = quota.get(&profile.name);
            let mut view = one_view(
                profile.cli_id.clone(),
                Some(profile.name.clone()),
                profile.active,
                standing_of(profile.access, spent, left, now),
                profile.said.clone(),
                spent,
            );
            view.quota = left.cloned();
            view.worked = worked
                .get(&(profile.cli_id.clone(), profile.name.clone()))
                .cloned();
            view
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
        // **THE PANEL IS SORTED BY WHAT WAS DONE, NOT BY WHAT WAS BILLED.**
        // The account a person works in all day bills nothing here, and it sat
        // at the bottom under seven that did nothing.
        tokens_worked(right)
            .cmp(&tokens_worked(left))
            .then(right.spent_micros.cmp(&left.spent_micros))
            .then(right.calls.cmp(&left.calls))
            .then(left.cli.cmp(&right.cli))
            .then(left.profile.cmp(&right.profile))
    });
    views
}

fn tokens_worked(view: &AccountView) -> u64 {
    view.worked.as_ref().map(|did| did.tokens().all()).unwrap_or_default()
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
        quota: None,
        worked: None,
        repair: None,
    }
}

/// **A SHUT DOOR OUTRANKS AN EMPTY TANK.** An account nobody can sign into will
/// not come back on its own in five hours, so it is the worse of the two.
fn standing_of(
    access: Access,
    spent: Option<&AccountStanding>,
    quota: Option<&QuotaSeen>,
    now: i64,
) -> Standing {
    match access {
        Access::No | Access::Mismatched => Standing::Shut,
        Access::NotKnown if quota.is_some_and(QuotaSeen::ran_out) => Standing::RanOut,
        Access::NotKnown => Standing::Unknown,
        Access::Yes | Access::Unverified | Access::HomeDoesNotMove => {
            // **THE ALLOWANCE OUTRANKS THE FLAG.** «authenticated» is read from
            // a file on disk; a provider calling the token revoked has actually
            // been asked, and it is the one telling the truth. Only that: a
            // meter refusing to be read this minute leaves the standing alone.
            if quota.is_some_and(|left| left.credential_is_dead) {
                return Standing::Shut;
            }
            if quota.is_some_and(QuotaSeen::ran_out) || ran_out_recently(spent, now) {
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

/// A token count as a person reads it at a glance: 155M, not 155_012_233.
fn in_short(tokens: u64) -> String {
    match tokens {
        0..=9_999 => tokens.to_string(),
        10_000..=999_999 => format!("{}k", tokens / 1_000),
        _ => format!("{:.1}M", tokens as f64 / 1_000_000.0),
    }
}

/// What the work would have cost at list price; `None` where a model in it
/// carries no price. **NEVER A FIGURE SHORT OF A MODEL**, and never a sentence
/// where a sum is spoken: the words for the missing price are a line of their
/// own, or the currency and «at list price» end up wrapped around them.
fn worth_of(did: &models::work::Worked) -> Option<String> {
    models::work::cost_micros(did, &models::pricing::shipped()).map(dollars)
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
        for window in view.quota.iter().flat_map(|left| &left.windows) {
            let used = format!("{:.0}", window.used_fraction * 100.0);
            lines.push(match window.resets_at.as_deref() {
                Some(resets) => catalogue::say(
                    "cli.accounts.one_window",
                    &[("unit", &window.unit), ("used", &used), ("resets", resets)],
                ),
                None => catalogue::say(
                    "cli.accounts.one_window_no_reset",
                    &[("unit", &window.unit), ("used", &used)],
                ),
            });
        }
        if let Some(did) = view.worked.as_ref().filter(|did| did.calls > 0) {
            let mut said = vec![
                ("calls", did.calls.to_string()),
                ("sessions", did.sessions.to_string()),
                ("tokens", in_short(did.tokens().all())),
            ];
            let key = match worth_of(did) {
                Some(worth) => {
                    said.push(("worth", worth));
                    "cli.accounts.what_it_worked"
                }
                None => "cli.accounts.what_it_worked_unpriced",
            };
            let said: Vec<(&str, &str)> =
                said.iter().map(|(name, value)| (*name, value.as_str())).collect();
            lines.push(catalogue::say(key, &said));
        }
        if view.standing != Standing::Ready && !view.said.is_empty() {
            lines.push(format!("      {}", view.said));
        }
        if let Some(refused) = view.quota.as_ref().and_then(|left| left.refused.as_deref()) {
            lines.push(format!("      {refused}"));
        }
        if let Some(repair) = view.repair.as_deref() {
            lines.push(catalogue::say("cli.accounts.to_cure_it", &[("line", repair)]));
        }
    }
    lines.push(catalogue::say("cli.accounts.what_the_spend_counts", &[]));
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
                "windows": view.quota.as_ref().map(|left| {
                    left.windows
                        .iter()
                        .map(|window| {
                            serde_json::json!({
                                "unit": window.unit,
                                "used_percent": (window.used_fraction * 1000.0).round() / 10.0,
                                "resets_at": window.resets_at,
                            })
                        })
                        .collect::<Vec<_>>()
                }),
                "quota_said": view.quota.as_ref().and_then(|left| left.refused.clone()),
                "repair": view.repair,
                "worked": view.worked.as_ref().map(|did| serde_json::json!({
                    "calls": did.calls,
                    "sessions": did.sessions,
                    "input_tokens": did.tokens().input,
                    "output_tokens": did.tokens().output,
                    "cache_read_tokens": did.tokens().cache_read,
                    "cache_write_tokens": did.tokens().cache_write + did.tokens().cache_write_long,
                    "at_list_price": models::work::cost_micros(did, &models::pricing::shipped())
                        .map(dollars),
                    "latest_call_at": did.latest,
                    "by_model": did.by_model.iter().map(|(model, tokens)| serde_json::json!({
                        "model": model,
                        "tokens": tokens.all(),
                    })).collect::<Vec<_>>(),
                })),
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

    fn worked(calls: u64, model: &str, input: u64) -> models::work::Worked {
        models::work::Worked {
            calls,
            sessions: 1,
            by_model: [(
                model.to_owned(),
                models::work::Tokens {
                    input,
                    ..models::work::Tokens::default()
                },
            )]
            .into_iter()
            .collect(),
            latest: "2026-09-19T12:00:00Z".to_owned(),
        }
    }

    /// **THE FAULT THIS TEST HOLDS SHUT.** Nothing a person types in a terminal
    /// passes through the store, so the account they work in all day billed
    /// $0.00 and sat below seven that did nothing at all.
    #[test]
    fn the_account_that_did_the_work_is_read_first_and_says_what_it_did() {
        let views = joined(
            &[
                declared("claude", "idle@example.test", Access::Yes),
                declared("claude", "busy@example.test", Access::Yes),
            ],
            &[spent("claude", Some("idle@example.test"), 9_000_000, None)],
            &nothing_left(),
            &[(("claude".to_owned(), "busy@example.test".to_owned()), worked(900, "claude-opus-5", 1_000_000))]
                .into_iter()
                .collect(),
            NOW,
        );
        assert_eq!(
            views[0].profile.as_deref(),
            Some("busy@example.test"),
            "the one that worked comes first, though it billed nothing"
        );
        let said = report(&views, 5);
        assert!(said.contains("900"), "the calls are shown: {said}");
        assert!(said.contains("$5.00"), "and what they weigh at list price: {said}");
    }

    /// One address signed in on two command lines is two rows, and each shows
    /// only the work done on its own command line.
    #[test]
    fn one_address_on_two_command_lines_is_credited_once_each() {
        let views = joined(
            &[
                declared("claude", "same@example.test", Access::Yes),
                declared("codex", "same@example.test", Access::Yes),
            ],
            &[],
            &nothing_left(),
            &[
                (("claude".to_owned(), "same@example.test".to_owned()), worked(7, "a-model", 10)),
                (("codex".to_owned(), "same@example.test".to_owned()), worked(3, "a-model", 10)),
            ]
            .into_iter()
            .collect(),
            NOW,
        );
        let calls_of = |cli: &str| {
            views
                .iter()
                .find(|view| view.cli == cli)
                .and_then(|view| view.worked.as_ref())
                .map(|did| did.calls)
        };
        assert_eq!(calls_of("claude"), Some(7));
        assert_eq!(calls_of("codex"), Some(3));
    }

    /// **A SENTENCE IS NOT A SUM.** Where no price covers a model, the words
    /// saying so once stood in the slot the currency and «at list price» are
    /// written around, and the line read «$no price for one of these models at
    /// list price» to whoever ran the command.
    #[test]
    fn work_nobody_priced_says_so_instead_of_wearing_a_currency_sign() {
        let views = joined(
            &[declared("claude", "who@example.test", Access::Yes)],
            &[],
            &nothing_left(),
            &[(("claude".to_owned(), "who@example.test".to_owned()), worked(4, "a-model-nobody-priced", 2_000))]
                .into_iter()
                .collect(),
            NOW,
        );
        let said = report(&views, 5);
        assert!(said.contains("no price for one of these models"), "{said}");
        assert!(!said.contains("$no price"), "a sum is never spoken: {said}");
        assert!(!said.contains("models at list price"), "nor is it priced: {said}");
    }

    fn nothing_worked() -> BTreeMap<(String, String), models::work::Worked> {
        BTreeMap::new()
    }

    fn nothing_left() -> BTreeMap<String, QuotaSeen> {
        BTreeMap::new()
    }

    fn allowance(profile: &str, seen: QuotaSeen) -> BTreeMap<String, QuotaSeen> {
        BTreeMap::from([(profile.to_owned(), seen)])
    }

    fn window(unit: &str, used_fraction: f64) -> WindowLeft {
        WindowLeft {
            unit: unit.to_owned(),
            used_fraction,
            resets_at: Some("2026-09-19T22:57:45Z".to_owned()),
        }
    }

    #[test]
    fn a_declared_account_is_joined_to_what_the_store_saw_it_spend() {
        let views = joined(
            &[declared("codex", "someone@example.test", Access::Yes)],
            &[spent("codex", Some("someone@example.test"), 2_500_000, None)],
            &nothing_left(),
            &nothing_worked(),
            NOW,
        );
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].spent_micros, 2_500_000);
        assert_eq!(views[0].standing, Standing::Ready);
    }

    /// **A CALL THAT COST MONEY AND BELONGS TO NOBODY IS THE ONE WORTH SEEING.**
    #[test]
    fn an_account_that_answered_and_no_profile_declares_is_still_shown() {
        let views = joined(&[], &[spent("antigravity", None, 5_770_000, None)], &nothing_left(), &nothing_worked(), NOW);
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].cli, "antigravity");
        assert_eq!(views[0].standing, Standing::Unknown);
    }

    #[test]
    fn a_quota_that_ran_out_inside_the_window_reads_as_ran_out_and_outside_it_does_not() {
        let inside = joined(
            &[declared("codex", "who@example.test", Access::Yes)],
            &[spent("codex", Some("who@example.test"), 0, Some(NOW - 60))],
            &nothing_left(),
            &nothing_worked(),
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
            &nothing_left(),
            &nothing_worked(),
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
            &nothing_left(),
            &nothing_worked(),
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
            let views = joined(
                &[declared("codex", "who@example.test", access)],
                &[],
                &nothing_left(),
                &nothing_worked(),
                NOW,
            );
            assert_eq!(views[0].standing, expected, "on {access:?}");
        }
    }

    /// **THE PROVIDER OUTRANKS THE FLAG.** «authenticated» is a file on disk;
    /// a token the provider itself calls revoked is a dead account, and the dot
    /// showed two of those as ready.
    #[test]
    fn a_token_the_provider_calls_revoked_is_shut_however_authenticated_it_reads() {
        let refused = QuotaSeen {
            refused: Some("the engine refused: OAuth access token has been revoked.".to_owned()),
            credential_is_dead: true,
            ..Default::default()
        };
        let views = joined(
            &[declared("claude", "who@example.test", Access::Yes)],
            &[],
            &allowance("who@example.test", refused),
            &nothing_worked(),
            NOW,
        );
        assert_eq!(views[0].standing, Standing::Shut);
    }

    /// **A METER THAT REFUSES TO BE READ IS NOT A CLOSED ACCOUNT**, and it
    /// refuses exactly when the person is working: three Claude accounts read
    /// `shut` — two with a terminal open on them that minute — because the
    /// provider limits how often its own usage endpoint may be asked.
    #[test]
    fn a_meter_that_asks_to_be_asked_later_leaves_the_account_where_it_was() {
        let later = QuotaSeen {
            refused: Some(
                "the engine asked to be asked later: Rate limited. Please try again later."
                    .to_owned(),
            ),
            credential_is_dead: false,
            ..Default::default()
        };
        let views = joined(
            &[declared("claude", "who@example.test", Access::Yes)],
            &[],
            &allowance("who@example.test", later),
            &nothing_worked(),
            NOW,
        );
        assert_eq!(views[0].standing, Standing::Ready);
        assert_eq!(worst_of(&views), Standing::Ready, "and the mark stays quiet");
        assert!(
            views[0].quota.as_ref().and_then(|left| left.refused.as_deref()).is_some(),
            "the words still show: the reading is missing, and that is worth saying"
        );
    }

    /// A home with no credentials in it is nobody having looked, and it must
    /// not turn a working account into a dead one.
    #[test]
    fn a_home_nobody_looked_in_does_not_contradict_an_account_that_works() {
        let unread = QuotaSeen {
            refused: Some("no credentials in /somewhere/.credentials.json".to_owned()),
            credential_is_dead: false,
            ..Default::default()
        };
        let views = joined(
            &[declared("claude", "who@example.test", Access::Yes)],
            &[],
            &allowance("who@example.test", unread),
            &nothing_worked(),
            NOW,
        );
        assert_eq!(views[0].standing, Standing::Ready);
    }

    #[test]
    fn an_allowance_spent_to_the_last_reads_as_ran_out_without_the_store_saying_so() {
        let full = QuotaSeen {
            windows: vec![window("primary_window", 1.0)],
            ..Default::default()
        };
        let views = joined(
            &[declared("codex", "who@example.test", Access::Yes)],
            &[],
            &allowance("who@example.test", full),
            &nothing_worked(),
            NOW,
        );
        assert_eq!(views[0].standing, Standing::RanOut);
        assert_eq!(worst_of(&views), Standing::RanOut);
    }

    #[test]
    fn the_fullest_window_is_the_one_that_speaks_for_the_account() {
        let seen = QuotaSeen {
            windows: vec![window("five_hour", 0.1), window("seven_day", 0.34)],
            ..Default::default()
        };
        assert_eq!(seen.fullest().expect("a window").unit, "seven_day");
        assert!(!seen.ran_out());
    }

    /// **ONE ACCOUNT ANSWERS UNDER SEVERAL DESCRIPTORS.** A repair descriptor
    /// points at a home with no credentials in it, and that refusal must not
    /// bury the reading that was actually taken.
    #[test]
    fn a_reading_that_could_be_taken_beats_one_that_could_not_for_the_same_account() {
        use models::remaining::Remaining;
        let taken = toolbox::quota::Reading {
            engine: "claude-code · who@example.test".to_owned(),
            result: Ok(vec![Remaining {
                engine: "claude-code".to_owned(),
                unit: "five_hour".to_owned(),
                used_fraction: 0.1,
                resets_at: None,
                observed_at: NOW,
            }]),
        };
        let unread = toolbox::quota::Reading {
            engine: "claude-code-repair · who@example.test".to_owned(),
            result: Err(toolbox::quota::Refusal {
                said: "no credentials in /somewhere".to_owned(),
                credential_is_dead: false,
                provider_answered: false,
            }),
        };
        for order in [
            vec![taken.clone(), unread.clone()],
            vec![unread.clone(), taken.clone()],
        ] {
            let found = quota_by_profile(&order);
            let seen = found.get("who@example.test").expect("the account");
            assert_eq!(seen.windows.len(), 1, "the reading was buried");
            assert_eq!(seen.refused, None);
        }
    }

    /// **THE SENTENCE SHOWN MUST COME FROM WHOEVER ACTUALLY ASKED.** The same
    /// account is read through two descriptors: one reaches the provider and is
    /// told «rate limited», the other stops at a file that was never where this
    /// engine keeps its credential. Shown the second, a person goes looking for
    /// a missing file that explains nothing.
    #[test]
    fn a_refusal_from_the_provider_beats_one_from_a_channel_that_never_asked() {
        let asked = toolbox::quota::Reading {
            engine: "claude-code · who@example.test".to_owned(),
            result: Err(toolbox::quota::Refusal {
                said: "the engine asked to be asked later: Rate limited.".to_owned(),
                credential_is_dead: false,
                provider_answered: true,
            }),
        };
        let stopped = toolbox::quota::Reading {
            engine: "claude-code-repair · who@example.test".to_owned(),
            result: Err(toolbox::quota::Refusal {
                said: "no credentials in /somewhere/.credentials.json".to_owned(),
                credential_is_dead: false,
                provider_answered: false,
            }),
        };
        for order in [
            vec![asked.clone(), stopped.clone()],
            vec![stopped.clone(), asked.clone()],
        ] {
            let found = quota_by_profile(&order);
            let seen = found.get("who@example.test").expect("the account");
            assert_eq!(
                seen.refused.as_deref(),
                Some("the engine asked to be asked later: Rate limited."),
                "the order of the descriptors decided, and it must not"
            );
        }
    }

    #[test]
    fn the_windows_an_account_has_left_are_printed_where_a_person_reads_them() {
        let mut view = one_view(
            "codex".to_owned(),
            Some("who@example.test".to_owned()),
            true,
            Standing::RanOut,
            String::new(),
            None,
        );
        view.quota = Some(QuotaSeen {
            windows: vec![window("primary_window", 1.0)],
            ..Default::default()
        });
        let said = report(&[view], 24);
        for held in ["primary_window", "100", "2026-09-19"] {
            assert!(said.contains(held), "«{held}» is missing: {said}");
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
            &nothing_left(),
            &nothing_worked(),
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
            &nothing_left(),
            &nothing_worked(),
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
            &nothing_left(),
            &nothing_worked(),
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
        let views = joined(
            &[declared("claude", "idle@example.test", Access::Yes)],
            &[],
            &nothing_left(),
            &nothing_worked(),
            NOW,
        );
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
        assert_eq!(
            how_it_was_asked(&[]).expect("no argument"),
            (A_WEEK_OF_HOURS, false, false)
        );
        assert_eq!(
            how_it_was_asked(&["--json".to_owned()]).expect("json"),
            (A_WEEK_OF_HOURS, true, false)
        );
        assert_eq!(
            how_it_was_asked(&["--quota".to_owned()]).expect("quota"),
            (A_WEEK_OF_HOURS, false, true)
        );
        assert_eq!(
            how_it_was_asked(&["--hours".to_owned(), "24".to_owned()]).expect("hours"),
            (24, false, false)
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
        let views = joined(&[shelved], &[], &nothing_left(), &nothing_worked(), NOW);
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
            &nothing_left(),
            &nothing_worked(),
            NOW,
        );
        assert_eq!(worst_of(&views), Standing::Shut);
    }
}
