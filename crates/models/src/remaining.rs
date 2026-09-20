//! How much of **a person's** quota is left, read instead of asked.
//!
//! **WHY IT EXISTS.** An agent cannot count what its harness spends for it: in
//! the A/B it declared 33 turns out of 75 real ones. The cure is not to ask
//! better — it is to **read**, spending nothing to do it.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// An OAuth usage channel, as a descriptor declares it: **THIS MODULE KNOWS
/// NO PROVIDER.** A versioned channel can stop answering with nothing here
/// changing, so a missing reading is a reading that is not there — never an
/// error and never a zero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OauthUsageChannel {
    /// The engine this reading is about: the descriptor's `id`.
    pub engine: String,
    pub credentials: PathBuf,
    /// The keys down to the token inside the credentials file.
    pub token_pointer: Vec<String>,
    pub url: String,
    /// Whole header lines, `name: value`.
    pub headers: Vec<String>,
    /// A command that prints the credentials, when they are not in the file.
    pub held_by: Vec<String>,
    /// The keys down to the access token's own expiry, unix milliseconds.
    /// Empty when the provider's credentials carry none: fault 169 is then
    /// not distinguishable from a real refusal, and `Refused` stays plain.
    pub access_expires_pointer: Vec<String>,
    /// The keys down to the refresh token's own expiry, unix milliseconds.
    pub refresh_expires_pointer: Vec<String>,
    /// The words this provider uses for its windows.
    pub shape: WindowWords,
}

/// What a provider calls the parts of its answer. The default below is one
/// provider's, and only because it was the first measured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowWords {
    /// The keys down to the object holding the windows; empty is the root.
    pub windows_at: Vec<String>,
    pub used: String,
    /// Whether `used` is out of a hundred or out of one.
    pub used_in_percent: bool,
    pub resets: String,
    /// Whether `resets` is an instant written out or a count of seconds.
    pub resets_in_seconds: bool,
    /// The key inside a window holding how long that window lasts, in seconds.
    /// Empty where the provider sends no such field.
    pub lasts: String,
    /// How long a window lasts, by the provider's own name for it, where the
    /// provider never sends the length: measured once, written down, and not
    /// guessed from the name at every reading.
    pub lasts_by_name: BTreeMap<String, u64>,
    /// The refusal kinds that mean **the credential itself is no good**; every
    /// other refusal is a «not now», and **EMPTY IS THE SAFE DEFAULT**.
    pub dead_when: Vec<String>,
}

impl Default for WindowWords {
    fn default() -> Self {
        WindowWords {
            windows_at: Vec::new(),
            used: "utilization".to_owned(),
            used_in_percent: true,
            resets: "resets_at".to_owned(),
            resets_in_seconds: false,
            lasts: String::new(),
            lasts_by_name: BTreeMap::new(),
            dead_when: Vec::new(),
        }
    }
}

/// How much of a quota window is already gone, and when that window resets.
///
/// **NOT THE COST OF A RUN, AND CONFUSING THEM IS WORSE THAN NOT HAVING IT.**
/// It is the person's quota over every session — the terminal beside this one,
/// the editor, yesterday's job in the same window.
#[derive(Debug, Clone, PartialEq)]
pub struct Remaining {
    /// Whose quota this is: the engine descriptor's `id`.
    pub engine: String,
    /// Which window: `five_hour`, `seven_day`, or a name this version does not
    /// know. **Not a closed set**, and it must not become one: the provider
    /// added windows while this file was being written.
    pub unit: String,
    /// How much is already spent, from `0.0` to `1.0`. **A FRACTION, NOT A
    /// PERCENTAGE**: the provider answers `50.0` for "half". It sits beside
    /// other ratios, and two lookalike units get summed by mistake once.
    pub used_fraction: f64,
    /// When the window restarts, in the shape the provider says it.
    ///
    /// **IT STAYS TEXT, AND THAT IS NOT LAZINESS** — fault 14: an instant
    /// derived from a rarely seen shape is invented data wearing the face of a
    /// measure. Convert it when something waits for that hour.
    pub resets_at: Option<String>,
    /// How long this window lasts, in seconds, where anything says so.
    ///
    /// **THE NAME OF A WINDOW DOES NOT SAY WHAT IT IS**: `primary_window`
    /// names an order, and was measured at a week on one account and thirty
    /// days on another. Only this says which window a person is looking at.
    pub lasts_seconds: Option<u64>,
    /// When we looked. A quota ages: a value without the instant it was read
    /// at cannot be told apart from yesterday's.
    pub observed_at: i64,
}

/// Which window a reading is about, from how long it lasts. **THE BANDS ARE
/// WIDE ON PURPOSE**, and a third length never enters one of the two: it
/// keeps its seconds instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HowLong {
    Session,
    Week,
    /// A length that is neither, kept as it was read.
    Other(u64),
    NotSaid,
}

const A_MINUTE: u64 = 60;
const AN_HOUR: u64 = 60 * A_MINUTE;
const A_DAY: u64 = 24 * AN_HOUR;

fn plural(out: &mut fmt::Formatter<'_>, count: u64, unit: &str) -> fmt::Result {
    write!(out, "{count} {unit}{}", if count == 1 { "" } else { "s" })
}

impl fmt::Display for HowLong {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HowLong::Session => out.write_str("session"),
            HowLong::Week => out.write_str("week"),
            HowLong::Other(secs) => match *secs {
                secs if secs >= A_DAY => plural(out, (secs + A_DAY / 2) / A_DAY, "day"),
                secs if secs >= AN_HOUR => plural(out, (secs + AN_HOUR / 2) / AN_HOUR, "hour"),
                secs => plural(out, secs.div_ceil(A_MINUTE), "minute"),
            },
            HowLong::NotSaid => out.write_str("window"),
        }
    }
}

impl HowLong {
    /// Which window a length is, and never which window a name says it is.
    pub fn of(lasts_seconds: Option<u64>) -> HowLong {
        const HALF_A_DAY: u64 = 12 * 60 * 60;
        const SIX_DAYS: u64 = 6 * 24 * 60 * 60;
        const EIGHT_DAYS: u64 = 8 * 24 * 60 * 60;
        match lasts_seconds {
            None => HowLong::NotSaid,
            Some(secs) if secs <= HALF_A_DAY => HowLong::Session,
            Some(secs) if (SIX_DAYS..=EIGHT_DAYS).contains(&secs) => HowLong::Week,
            Some(secs) => HowLong::Other(secs),
        }
    }
}

impl Remaining {
    pub fn how_long(&self) -> HowLong {
        HowLong::of(self.lasts_seconds)
    }
}

/// Why a reading is not there.
///
/// **NONE OF THESE SHAPES CARRIES THE TOKEN**: a generic `Display` echoing a
/// command line or a response body is how a secret ends up in a log, and a log
/// never gives it back.
#[derive(Clone, PartialEq, Eq)]
pub enum RemainingError {
    /// The credentials file is not there: that engine is not authenticated
    /// here.
    NoCredentials(PathBuf),
    /// The file is there and cannot be read, or is not JSON.
    CredentialsUnreadable(String),
    /// The file is JSON and carries no token key. This version of that engine
    /// keeps its credentials somewhere else.
    NoToken,
    /// `curl` did not start, or did not answer.
    Unreachable(String),
    /// It answered and said no, in its own words; only `message` is copied,
    /// never the token. **ONLY `dead_when` KINDS LAND HERE.**
    Refused(String),
    /// It answered and asked to be asked later: rate limited, busy, briefly
    /// down. **A COUNTER THAT REFUSES TO COUNT IS NOT A CLOSED DOOR.**
    NotNow(String),
    /// It refused, and the same credentials say why on their own terms: the
    /// short-lived access token had already passed its own `expiresAt` when
    /// asked, while the refresh token beside it had not (fault 169). A `claude`
    /// run renews the access token on its own; this reader never does. This is
    /// evidence, not a verdict — the refresh token could still be rejected for
    /// a reason these timestamps cannot show.
    RefusedWithAnUnexpiredRefreshToken { said: String, refresh_expires_at: i64 },
    /// It answered something that is not the expected JSON: the channel is
    /// beta, and this is how it will break.
    NotUnderstood,
}

impl RemainingError {
    /// Whether this refusal is about the credential rather than the moment.
    pub fn credential_is_dead(&self) -> bool {
        matches!(self, RemainingError::Refused(_))
    }

    /// Whether the provider was reached at all: a channel that stopped at the
    /// doorstep is worth less than a refusal, and must not outrank one.
    pub fn provider_answered(&self) -> bool {
        matches!(
            self,
            RemainingError::Refused(_)
                | RemainingError::NotNow(_)
                | RemainingError::RefusedWithAnUnexpiredRefreshToken { .. }
        )
    }
}

impl fmt::Debug for RemainingError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, out)
    }
}

impl fmt::Display for RemainingError {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RemainingError::NoCredentials(path) => {
                write!(out, "no credentials in {}", path.display())
            }
            RemainingError::CredentialsUnreadable(why) => {
                write!(out, "the credentials cannot be read: {why}")
            }
            RemainingError::NoToken => {
                write!(out, "the credentials carry no token key")
            }
            RemainingError::Unreachable(why) => write!(out, "the channel does not answer: {why}"),
            RemainingError::Refused(said) => write!(out, "the engine refused: {said}"),
            RemainingError::NotNow(said) => {
                write!(out, "the engine asked to be asked later: {said}")
            }
            RemainingError::RefusedWithAnUnexpiredRefreshToken { said, refresh_expires_at } => write!(
                out,
                "the engine refused: {said} — but the refresh token beside it is not due to \
                 expire until {refresh_expires_at} (unix ms): a live run of the client would \
                 likely renew the access token on its own, this is not necessarily a sign the \
                 account needs a fresh login"
            ),
            RemainingError::NotUnderstood => write!(
                out,
                "the answer is not in the expected shape: the channel is beta and \
                 versioned, and can change without warning"
            ),
        }
    }
}

/// The access token, in a shape that **cannot be printed by accident**. A
/// `String` ends up in a `{:?}` or a leftover `dbg!`, neither of which looks
/// like printing a secret: so `Debug` is hand-written, there is no `Display`,
/// and the only reader is the `curl` stdin configuration.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl fmt::Debug for Token {
    fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
        out.write_str("Token(hidden)")
    }
}

impl Token {
    /// The token inside an engine's credentials file, under the keys the
    /// descriptor points at. A missing key is [`RemainingError::NoToken`] and
    /// not a panic: that file belongs to somebody else.
    pub fn from_credentials_at(text: &str, pointer: &[String]) -> Result<Token, RemainingError> {
        let parsed: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| RemainingError::CredentialsUnreadable(error.to_string()))?;
        let mut at = &parsed;
        for key in pointer {
            at = at.get(key).ok_or(RemainingError::NoToken)?;
        }
        at.as_str()
            .filter(|token| !token.is_empty())
            .map(|token| Token(token.to_owned()))
            .ok_or(RemainingError::NoToken)
    }

    /// The configuration `curl` reads **from its own stdin**.
    ///
    /// **THE TOKEN NEVER TRAVELS IN AN ARGUMENT, AND THAT IS THE POINT OF THIS
    /// FUNCTION.** `curl -H "Authorization: Bearer …"` puts the secret in the
    /// process's command line, and anyone on the machine reads that with `ps`.
    /// With `-K -` it travels on a pipe that exists only between these two.
    fn curl_config(&self, url: &str, headers: &[String]) -> String {
        let mut config = format!(
            "url = \"{url}\"\nheader = \"Authorization: Bearer {}\"\n",
            self.0
        );
        for header in headers {
            config.push_str(&format!("header = \"{header}\"\n"));
        }
        config.push_str("silent\nshow-error\nmax-time = 30\n");
        config
    }
}

/// The quota windows inside a usage answer.
///
/// **IT KNOWS NO WINDOW NAME, AND MUST NOT.** It takes every top-level key
/// whose value is an object holding a numeric `utilization`. One real answer
/// carried fourteen: two full, one at zero under a name in no documentation,
/// eleven null. A `match` would have lost the third the day it appeared.
pub fn from_oauth_usage(
    body: &str,
    engine: &str,
    observed_at: i64,
    words: &WindowWords,
) -> Result<Vec<Remaining>, RemainingError> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| RemainingError::NotUnderstood)?;
    let whole = parsed.as_object().ok_or(RemainingError::NotUnderstood)?;

    // **A REFUSAL IS RECOGNISED BEFORE THE WINDOWS ARE COUNTED**: this
    // provider's refusal is valid JSON with no `utilization` anywhere, so
    // scanning it for quotas returns an empty list — "no consumption on record".
    // **Look at `error.message`, not the envelope.** A revocation carries a
    // top-level `"type": "error"`, a rate limit does not: matching the envelope
    // let the rate limit through as that empty list, to an automated poller.
    if let Some(error) = whole.get("error") {
        if let Some(said) = error.get("message").and_then(serde_json::Value::as_str) {
            let kind = error.get("type").and_then(serde_json::Value::as_str).unwrap_or_default();
            return Err(if words.dead_when.iter().any(|fatal| fatal == kind) {
                RemainingError::Refused(said.to_owned())
            } else {
                RemainingError::NotNow(said.to_owned())
            });
        }
    }

    // **THE WINDOWS ARE WHERE THE DESCRIPTOR SAYS.** Read at the root, windows
    // nested under a name of their own answer «nothing spent» for a full one.
    let mut here = &parsed;
    for key in &words.windows_at {
        here = here.get(key).ok_or(RemainingError::NotUnderstood)?;
    }
    let windows = here.as_object().ok_or(RemainingError::NotUnderstood)?;

    let mut found = Vec::new();
    for (unit, window) in windows {
        let Some(fields) = window.as_object() else {
            continue;
        };
        let Some(percent) = fields
            .get(&words.used)
            .and_then(serde_json::Value::as_f64)
        else {
            // **WHAT IS NOT A MEASURE DOES NOT BECOME A ZERO.** A null window,
            // one whose `utilization` is null, one without the field at all:
            // each leaves the list instead of entering it at zero. A zero among
            // quotas reads "you have everything free" — reassuring, and wrong.
            continue;
        };
        found.push(Remaining {
            engine: engine.to_owned(),
            unit: unit.clone(),
            // The provider says `50.0` for half. See the note on
            // `used_fraction`: the unit changes here, once, in one place.
            used_fraction: if words.used_in_percent { percent / 100.0 } else { percent },
            resets_at: fields.get(&words.resets).and_then(|when| {
                if words.resets_in_seconds {
                    when.as_i64().map(crate::fuel::rfc3339_of_unix_secs)
                } else {
                    when.as_str().map(str::to_owned)
                }
            }),
            lasts_seconds: fields
                .get(&words.lasts)
                .and_then(serde_json::Value::as_u64)
                .or_else(|| words.lasts_by_name.get(unit).copied()),
            observed_at,
        });
    }
    Ok(found)
}

/// Really reads an engine's quota through the channel its descriptor declares.
/// **READ-ONLY, AND FREE**: it invokes no engine and consumes nothing, which
/// is why it can sit in a check that runs often. A number from here written
/// next to a step would be a measure with the right face and the wrong meaning
/// (fault 37); its place is next to "can I launch another one?".
pub fn read_oauth_usage(
    channel: &OauthUsageChannel,
    observed_at: i64,
) -> Result<Vec<Remaining>, RemainingError> {
    let text = match channel.held_by.split_first() {
        Some((program, arguments)) => held_by(program, arguments)?,
        None => {
            let path: &Path = &channel.credentials;
            if !path.exists() {
                return Err(RemainingError::NoCredentials(path.to_path_buf()));
            }
            std::fs::read_to_string(path)
                .map_err(|error| RemainingError::CredentialsUnreadable(error.to_string()))?
        }
    };
    let token = Token::from_credentials_at(&text, &channel.token_pointer)?;
    let body = ask_curl(&token.curl_config(&channel.url, &channel.headers))?;
    read_against_the_credentials(
        from_oauth_usage(&body, &channel.engine, observed_at, &channel.shape),
        &text,
        channel,
    )
}

/// The reading, with a refusal weighed against the credentials it was made
/// with. **THE REFUSAL IS MINTED BY THE BODY, NOT BY `curl`**: `ask_curl`
/// answers `Unreachable` or `NotUnderstood` and never `Refused`, because the
/// provider says no with an HTTP status and a JSON body that `curl` reports as
/// success. Weighing the error `curl` hands back therefore weighs nothing.
fn read_against_the_credentials(
    read: Result<Vec<Remaining>, RemainingError>,
    text: &str,
    channel: &OauthUsageChannel,
) -> Result<Vec<Remaining>, RemainingError> {
    read.map_err(|why| match why {
        RemainingError::Refused(said) => refusal_against_expiry(said, text, channel),
        other => other,
    })
}

/// A `Refused` read against the same credentials text, once — never a second
/// keychain read, which a renewal landing between the two could make
/// disagree with itself. `RefusedWithAnUnexpiredRefreshToken` only where both
/// pointers are declared and both parse: an absent or malformed timestamp
/// leaves the plain refusal exactly as the provider said it.
fn refusal_against_expiry(said: String, text: &str, channel: &OauthUsageChannel) -> RemainingError {
    if channel.access_expires_pointer.is_empty() || channel.refresh_expires_pointer.is_empty() {
        return RemainingError::Refused(said);
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_millis() as i64)
        .unwrap_or(0);
    let access_expired = epoch_millis_at(text, &channel.access_expires_pointer).is_some_and(|at| at <= now);
    let refresh_expires_at = epoch_millis_at(text, &channel.refresh_expires_pointer);
    match (access_expired, refresh_expires_at) {
        (true, Some(refresh_expires_at)) if refresh_expires_at > now => {
            RemainingError::RefusedWithAnUnexpiredRefreshToken { said, refresh_expires_at }
        }
        _ => RemainingError::Refused(said),
    }
}

/// A unix-milliseconds number at a JSON pointer inside already-read
/// credentials text; `None` for anything that is not exactly that, never a
/// guess of zero.
fn epoch_millis_at(text: &str, pointer: &[String]) -> Option<i64> {
    let parsed: serde_json::Value = serde_json::from_str(text).ok()?;
    let mut at = &parsed;
    for key in pointer {
        at = at.get(key)?;
    }
    at.as_i64()
}

/// What the keeper of secrets prints, or why it would not.
///
/// **IT IS ASKED, NEVER SEARCHED FOR.** The command comes whole from the
/// descriptor with the home already put in, so this knows how to run a line
/// and nothing about who keeps what.
fn held_by(program: &str, arguments: &[String]) -> Result<String, RemainingError> {
    let run = std::process::Command::new(program)
        .args(arguments)
        .output()
        .map_err(|error| RemainingError::CredentialsUnreadable(format!("{program}: {error}")))?;
    if !run.status.success() {
        let said = String::from_utf8_lossy(&run.stderr);
        return Err(RemainingError::CredentialsUnreadable(format!(
            "«{program} {}» answered nothing: {}",
            arguments.join(" "),
            said.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&run.stdout).into_owned())
}

/// `curl` as a process, with the configuration on its stdin.
///
/// Same road as [`crate::fetch`] — a process instead of an HTTP library, so as
/// not to drag in a crate the rest of the workspace does not have. No test
/// covers this half: a test that goes on the network is red when the line
/// drops, not when the code is wrong.
fn ask_curl(config: &str) -> Result<String, RemainingError> {
    use std::io::Write;

    let mut child = Command::new("curl")
        .arg("--config")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| RemainingError::Unreachable(error.to_string()))?;
    child
        .stdin
        .as_mut()
        .ok_or_else(|| RemainingError::Unreachable("curl did not open its stdin".to_owned()))?
        .write_all(config.as_bytes())
        .map_err(|error| RemainingError::Unreachable(error.to_string()))?;
    let done = child
        .wait_with_output()
        .map_err(|error| RemainingError::Unreachable(error.to_string()))?;
    if !done.status.success() {
        // **`curl`'s STDERR IS REPORTED, THE CONFIGURATION IS NOT.** The first
        // has never seen the token; the second contains it.
        return Err(RemainingError::Unreachable(
            String::from_utf8_lossy(&done.stderr).trim().to_owned(),
        ));
    }
    String::from_utf8(done.stdout).map_err(|_| RemainingError::NotUnderstood)
}

/// The pure half: reading a body, tested on a hand-written sample. The gesture
/// that goes on the network has no tests, and [`ask_curl`] says why.
#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/oauth-usage-sample.json");
    const ENGINE: &str = "claude-code";
    const URL: &str = "https://api.anthropic.com/api/oauth/usage";
    const BETA: &str = "anthropic-beta: oauth-2025-04-20";

    /// The channel of the engine the sample was recorded from, as a
    /// descriptor would declare it; the tests never carry a provider's name
    /// anywhere else.
    fn pointer() -> Vec<String> {
        vec!["claudeAiOauth".to_owned(), "accessToken".to_owned()]
    }

    fn parse(body: &str, observed_at: i64) -> Result<Vec<Remaining>, RemainingError> {
        from_oauth_usage(body, ENGINE, observed_at, &WindowWords::default())
    }

    fn parse_for_a_descriptor_that_names_its_fatal_kinds(
        body: &str,
    ) -> Result<Vec<Remaining>, RemainingError> {
        let words = WindowWords {
            dead_when: vec!["authentication_error".to_owned(), "permission_error".to_owned()],
            ..WindowWords::default()
        };
        from_oauth_usage(body, ENGINE, 0, &words)
    }

    /// **A PROVIDER THAT NESTS ITS WINDOWS IS NOT ONE WITHOUT QUOTA.** Read at
    /// the root it yields the empty list, for a window that is full.
    #[test]
    fn the_windows_are_read_where_the_descriptor_says_and_by_its_own_words() {
        let body = r#"{"plan": "a-plan", "rate_limit": {
            "primary_window": {"used_percent": 100, "reset_at": 1789130917},
            "secondary_window": {"used_percent": 65, "reset_at": 1789546933}}}"#;
        let words = WindowWords {
            windows_at: vec!["rate_limit".to_owned()],
            used: "used_percent".to_owned(),
            used_in_percent: true,
            resets: "reset_at".to_owned(),
            resets_in_seconds: true,
            lasts: "limit_window_seconds".to_owned(),
            ..WindowWords::default()
        };

        assert_eq!(
            from_oauth_usage(body, ENGINE, 7, &WindowWords::default()),
            Ok(Vec::new()),
            "read by another provider's words it looks empty, which is the danger"
        );

        let found = from_oauth_usage(body, ENGINE, 7, &words).expect("its own words");

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].unit, "primary_window");
        assert_eq!(
            found[0].how_long(),
            HowLong::NotSaid,
            "«primary» names an order, and this body never said a length"
        );
        assert_eq!(found[0].used_fraction, 1.0);
        assert_eq!(found[0].resets_at.as_deref(), Some("2026-09-11T12:48:37Z"));
        assert_eq!(found[1].used_fraction, 0.65);
    }

    /// **THE NAME OF A WINDOW IS NOT ITS LENGTH.** This provider numbers its
    /// windows instead of naming them, and the same word covered five hours on
    /// one account and nineteen days on another. The length it sends beside
    /// them is the only thing that tells a person which window they are in.
    #[test]
    fn a_numbered_window_is_told_apart_by_the_length_beside_it() {
        let body = r#"{"rate_limit": {
            "primary_window": {"used_percent": 100, "limit_window_seconds": 18000},
            "secondary_window": {"used_percent": 65, "limit_window_seconds": 604800}}}"#;
        let words = WindowWords {
            windows_at: vec!["rate_limit".to_owned()],
            used: "used_percent".to_owned(),
            lasts: "limit_window_seconds".to_owned(),
            ..WindowWords::default()
        };

        let found = from_oauth_usage(body, ENGINE, 7, &words).expect("its own words");

        assert_eq!(found[0].how_long(), HowLong::Session);
        assert_eq!(found[1].how_long(), HowLong::Week);
        assert_eq!(found[0].how_long().to_string(), "session");
    }

    /// A provider that names its windows sends no length: the descriptor
    /// carries the one that was measured, so both providers answer alike.
    #[test]
    fn a_named_window_takes_the_length_the_descriptor_measured() {
        let words = WindowWords {
            lasts_by_name: BTreeMap::from([
                ("five_hour".to_owned(), 18_000),
                ("seven_day".to_owned(), 604_800),
            ]),
            ..WindowWords::default()
        };
        let found = from_oauth_usage(SAMPLE, ENGINE, 1_000, &words).expect("the sample parses");

        assert_eq!(
            window(&found, "five_hour").expect("the sitting").how_long(),
            HowLong::Session
        );
        assert_eq!(
            window(&found, "seven_day").expect("the week").how_long(),
            HowLong::Week
        );
    }

    /// **A THIRD LENGTH IS NOT ROUNDED INTO ONE OF THE TWO.** A real account
    /// answered with a window nineteen days long; called a week it would send
    /// somebody back to a door that stays shut for twelve more days.
    #[test]
    fn a_length_that_is_neither_is_kept_whole() {
        let body = r#"{"rate_limit": {"primary_window":
            {"used_percent": 100, "limit_window_seconds": 1641600}}}"#;
        let words = WindowWords {
            windows_at: vec!["rate_limit".to_owned()],
            used: "used_percent".to_owned(),
            lasts: "limit_window_seconds".to_owned(),
            ..WindowWords::default()
        };

        let found = from_oauth_usage(body, ENGINE, 7, &words).expect("its own words");

        assert_eq!(found[0].how_long(), HowLong::Other(1_641_600));
        assert_eq!(found[0].how_long().to_string(), "19 days");
    }

    /// **A LENGTH IS SAID IN THE UNITS A PERSON USES**, or a table holding
    /// «session», «week» and `2592000s` cannot be read across.
    #[test]
    fn a_length_neither_shape_covers_is_still_said_in_words() {
        let words = |secs| HowLong::of(Some(secs)).to_string();
        assert_eq!(words(2_592_000), "30 days");
        assert_eq!(words(24 * 60 * 60), "1 day");
        assert_eq!(words(14 * 60 * 60), "14 hours");
        // Half a day or shorter is a session: these come through the shape.
        assert_eq!(HowLong::Other(60 * 60).to_string(), "1 hour");
        assert_eq!(HowLong::Other(90).to_string(), "2 minutes");
        assert_eq!(HowLong::Other(60).to_string(), "1 minute");
    }

    /// A window nothing measured says so, and does not borrow a neighbour's
    /// name: `nimbus_quill` sits beside the two that are known.
    #[test]
    fn a_window_nobody_measured_stays_unnamed() {
        let words = WindowWords {
            lasts_by_name: BTreeMap::from([("five_hour".to_owned(), 18_000)]),
            ..WindowWords::default()
        };
        let found = from_oauth_usage(SAMPLE, ENGINE, 1_000, &words).expect("the sample parses");

        assert_eq!(
            window(&found, "five_hour").expect("the sitting").how_long(),
            HowLong::Session
        );
        for entry in found.iter().filter(|entry| entry.unit != "five_hour") {
            assert_eq!(
                entry.how_long(),
                HowLong::NotSaid,
                "{} borrowed a length",
                entry.unit
            );
        }
    }

    fn token_of(text: &str) -> Result<Token, RemainingError> {
        Token::from_credentials_at(text, &pointer())
    }

    fn window<'a>(found: &'a [Remaining], unit: &str) -> Option<&'a Remaining> {
        found.iter().find(|entry| entry.unit == unit)
    }

    /// **THE TWO FULL WINDOWS ARE READ, AND THE PERCENTAGE BECOMES A
    /// FRACTION.** The provider's `50.0` is half a window: carried through
    /// as-is it would claim fifty times the quota spent, a number that means
    /// nothing in any unit.
    #[test]
    fn the_two_full_windows_are_read_as_fractions() {
        let found = parse(SAMPLE, 1_000).expect("the sample parses");

        let five_hour = window(&found, "five_hour").expect("the five-hour window is there");
        assert_eq!(
            five_hour.used_fraction, 0.5,
            "50.0 per cent is half a window"
        );
        assert_eq!(five_hour.engine, ENGINE);
        assert_eq!(
            five_hour.resets_at.as_deref(),
            Some("2026-09-01T03:29:59.801054+00:00")
        );
        assert_eq!(
            five_hour.observed_at, 1_000,
            "a quota with no instant ages in silence"
        );

        assert_eq!(
            window(&found, "seven_day")
                .expect("and the seven-day one")
                .used_fraction,
            0.32
        );
    }

    /// **A WINDOW THIS VERSION NEVER HEARD OF IS REPORTED ANYWAY.** A list of
    /// names written into the code would lose it, and a lost quota is red
    /// nowhere.
    #[test]
    fn a_window_this_version_never_heard_of_is_reported_anyway() {
        let found = parse(SAMPLE, 0).expect("the sample parses");
        let unknown = window(&found, "nimbus_quill").expect("the unknown window is there too");
        assert_eq!(unknown.used_fraction, 0.075);
    }

    /// **WHAT IS NOT A MEASURE NEVER ENTERS AS A ZERO.** Four different shapes
    /// of "there is no number here", and all four must drop out instead of
    /// saying "you have everything free".
    #[test]
    fn what_is_not_a_measure_never_becomes_a_zero() {
        let found = parse(SAMPLE, 0).expect("the sample parses");
        let units: Vec<&str> = found.iter().map(|entry| entry.unit.as_str()).collect();

        for absent in ["seven_day_opus", "extra_usage", "spend", "limits"] {
            assert!(
                !units.contains(&absent),
                "«{absent}» declares no consumption: it must not appear among the quotas. Found: {units:?}"
            );
        }
        assert_eq!(
            units.len(),
            4,
            "only the four with a numeric `utilization`: {units:?}"
        );
    }

    /// A real window with no reset instant stays in the list: consumption is
    /// known even when the restart is not.
    #[test]
    fn a_window_without_a_reset_keeps_its_measure() {
        let found = parse(SAMPLE, 0).expect("the sample parses");
        let no_reset = window(&found, "no_reset").expect("it is there");
        assert_eq!(no_reset.used_fraction, 0.0);
        assert_eq!(no_reset.resets_at, None, "never an invented instant");
    }

    /// **A REFUSAL IS NOT AN ANSWER WITH ZERO WINDOWS**, and this module had
    /// the defect. The body is the real one: the token on disk was rotated
    /// under the reader's feet, the endpoint answered 401 with valid JSON, and
    /// the reader reported "no quota window declared". Asked about consumption
    /// it said none was on record, and whoever reads that before launching
    /// something reads a green light.
    #[test]
    fn a_refusal_is_a_refusal_and_never_an_empty_measure() {
        let refused = r#"{"type":"error","error":{"type":"authentication_error",
            "message":"OAuth access token has been revoked."},"request_id":null}"#;

        let said = parse_for_a_descriptor_that_names_its_fatal_kinds(refused)
            .expect_err("it is a refusal, not a measure");
        assert_eq!(
            said,
            RemainingError::Refused("OAuth access token has been revoked.".to_owned()),
            "the provider's own words carry through: they say what to do, namely authenticate again"
        );
        assert!(said.credential_is_dead(), "this one is about the credential");
    }

    /// **A COUNTER THAT REFUSES TO COUNT IS NOT A DOOR THAT REFUSES TO OPEN**:
    /// three working accounts read `shut` all day for the meter's rate limit.
    #[test]
    fn a_rate_limit_is_a_not_now_and_never_a_dead_credential() {
        let limited = r#"{"error":{"type":"rate_limit_error",
            "message":"Rate limited. Please try again later."}}"#;

        let said = parse_for_a_descriptor_that_names_its_fatal_kinds(limited)
            .expect_err("it is still not a measure");
        assert_eq!(
            said,
            RemainingError::NotNow("Rate limited. Please try again later.".to_owned())
        );
        assert!(!said.credential_is_dead(), "the account was never asked about");
    }

    /// **AN ACCESS TOKEN PAST ITS HOUR IS NOT A SIGNED-OUT ACCOUNT.** The
    /// provider answered, so this outranks a channel that stopped at a missing
    /// file; the credential is not the thing that died, so the panel must not
    /// offer a fresh login over it.
    #[test]
    fn an_unexpired_refresh_token_answers_both_questions_apart() {
        let enriched = RemainingError::RefusedWithAnUnexpiredRefreshToken {
            said: "token expired".to_owned(),
            refresh_expires_at: 1,
        };
        assert!(enriched.provider_answered(), "the provider is the one who said no");
        assert!(
            !enriched.credential_is_dead(),
            "a live refresh token beside it is why this variant exists at all"
        );
    }

    /// **A KIND NOBODY NAMED CANNOT KILL AN ACCOUNT**: the channel is beta.
    #[test]
    fn a_refusal_of_an_unnamed_kind_is_a_not_now() {
        let odd = r#"{"error":{"type":"a_kind_from_next_year","message":"no"}}"#;
        assert!(!parse_for_a_descriptor_that_names_its_fatal_kinds(odd)
            .expect_err("still a refusal")
            .credential_is_dead());
    }

    /// **THE PROVIDER REFUSES IN MORE THAN ONE SHAPE, AND BOTH WERE SEEN
    /// TWENTY MINUTES APART.** The first carries a top-level `type`, this one
    /// does not — only `{"error": {…}}`. A check written on the first let the
    /// second through **as an empty list**, i.e. "no consumption on record",
    /// and the second is the answer to whoever asked too often. Recognition
    /// sits on what the two share — an `error` holding a `message`.
    #[test]
    fn a_refusal_without_the_outer_type_is_still_a_refusal() {
        let limited = r#"{"error":{"type":"rate_limit_error",
            "message":"Rate limited. Please try again later."}}"#;

        assert_eq!(
            parse(limited, 0),
            Err(RemainingError::NotNow(
                "Rate limited. Please try again later.".to_owned()
            )),
            "the reader must learn it was refused, not that it consumed nothing"
        );
    }

    /// **AND A REAL ANSWER WITH NO WINDOWS STAYS A REAL ANSWER.** Without this
    /// half one could just call every empty list a refusal, and the two cases
    /// would become indistinguishable on the other side.
    #[test]
    fn a_usage_answer_with_every_window_null_is_not_a_refusal() {
        let empty = r#"{"five_hour":null,"seven_day":null,"member_dashboard_available":false}"#;
        assert_eq!(parse(empty, 0), Ok(vec![]));
    }

    /// The channel is beta: the way it will break is by answering something
    /// else.
    #[test]
    fn a_body_that_is_not_the_expected_shape_is_a_declared_failure() {
        assert_eq!(
            parse("<html>502</html>", 0),
            Err(RemainingError::NotUnderstood)
        );
        assert_eq!(
            parse("[1, 2, 3]", 0),
            Err(RemainingError::NotUnderstood)
        );
    }

    // ── the token ────────────────────────────────────────────────────────

    /// Text shaped like the real file, with a recognisable fake secret inside:
    /// if it shows up anywhere, it is seen at once.
    const A_SECRET: &str = "this-must-not-show-up-anywhere";

    fn credentials_with(token: &str) -> String {
        format!(r#"{{"mcpOAuth": {{}}, "claudeAiOauth": {{"accessToken": "{token}"}}}}"#)
    }

    #[test]
    fn the_token_is_taken_from_the_key_the_file_really_uses() {
        assert!(token_of(&credentials_with(A_SECRET)).is_ok());
    }

    /// **THE TOKEN IS NOT PRINTABLE, AND THIS IS THE TEST THAT HOLDS IT.** A
    /// `#[derive(Debug)]` in place of the hand-written one puts the defect
    /// back and turns this line red. It is the only way to test an absence:
    /// test the gesture that would violate it.
    #[test]
    fn no_way_of_printing_a_token_shows_it() {
        let token = token_of(&credentials_with(A_SECRET)).expect("it is there");
        let printed = format!("{token:?}");
        assert!(
            !printed.contains(A_SECRET),
            "the token ended up in a printout: {printed}"
        );
        assert_eq!(printed, "Token(hidden)");
    }

    /// **AND NO ERROR MESSAGE CARRIES IT EITHER.** An error is written in a
    /// hurry and ends up in a log, where it stays.
    #[test]
    fn no_failure_message_carries_the_token() {
        let broken = format!("{{\"claudeAiOauth\": {{\"accessToken\": \"{A_SECRET}\"}}");
        let refused = token_of(&broken).expect_err("the JSON is truncated");
        let said = format!("{refused} / {refused:?}");
        assert!(
            !said.contains(A_SECRET),
            "the token ended up in the error: {said}"
        );

        let no_key = token_of(r#"{"claudeAiOauth": {}}"#).expect_err("missing");
        assert_eq!(no_key, RemainingError::NoToken);
    }

    /// The configuration going to `curl`'s stdin carries the token — it must —
    /// and no **argument** does. That is the difference between a secret on a
    /// pipe and a secret readable with `ps`.
    #[test]
    fn the_secret_travels_on_the_pipe_and_never_in_an_argument() {
        let token = token_of(&credentials_with(A_SECRET)).expect("it is there");
        let config = token.curl_config(URL, &[BETA.to_owned()]);
        assert!(
            config.contains(A_SECRET),
            "without the token the request is not authenticated"
        );
        assert!(config.contains(URL));
        assert!(
            config.contains(BETA),
            "the channel is versioned: the version is declared"
        );
    }

    /// **THE POINTER IS THE DESCRIPTOR'S, NOT THIS MODULE'S.** The same file
    /// read under another provider's keys yields no token, and a token under
    /// other keys is found when the pointer says so.
    #[test]
    fn the_token_is_found_where_the_pointer_says_and_nowhere_else() {
        let text = credentials_with(A_SECRET);
        let elsewhere = vec!["somebodyElse".to_owned(), "token".to_owned()];
        assert_eq!(Token::from_credentials_at(&text, &elsewhere), Err(RemainingError::NoToken));
        let flat = format!(r#"{{"token": "{A_SECRET}"}}"#);
        assert!(Token::from_credentials_at(&flat, &["token".to_owned()]).is_ok());
    }

    /// An engine not authenticated here is not a fault: it is a reading that
    /// is not there, and whoever wanted it carries on without.
    #[test]
    fn a_machine_without_those_credentials_says_so_instead_of_failing_loudly() {
        let channel = OauthUsageChannel {
            engine: ENGINE.to_owned(),
            credentials: PathBuf::from("/this/home/does/not/exist/.credentials.json"),
            token_pointer: pointer(),
            url: URL.to_owned(),
            headers: vec![BETA.to_owned()],
            held_by: Vec::new(),
            access_expires_pointer: Vec::new(),
            refresh_expires_pointer: Vec::new(),
            shape: WindowWords::default(),
        };
        let refused = read_oauth_usage(&channel, 0).expect_err("there is nothing to read");
        assert!(matches!(refused, RemainingError::NoCredentials(_)));
    }

    /// A keeper that will not answer is a refusal that names the line it ran,
    /// never a reading of nothing: the file beside it does not stand in.
    #[test]
    fn a_keeper_that_refuses_names_the_line_that_was_run() {
        let channel = OauthUsageChannel {
            engine: ENGINE.to_owned(),
            credentials: PathBuf::from("/this/home/does/not/exist/.credentials.json"),
            token_pointer: pointer(),
            url: URL.to_owned(),
            headers: vec![BETA.to_owned()],
            held_by: vec!["false".to_owned(), "--for".to_owned(), "a-home".to_owned()],
            access_expires_pointer: Vec::new(),
            refresh_expires_pointer: Vec::new(),
            shape: WindowWords::default(),
        };

        let refused = read_oauth_usage(&channel, 0).expect_err("the keeper says no");

        let said = refused.to_string();
        assert!(said.contains("false --for a-home"), "{said}");
        assert!(!said.contains(".credentials.json"), "the file is not the story: {said}");
    }

    // ── fault 169: an expired access token is not a signed-out account ──

    /// **THE SEAM, NOT THE TWO HALVES.** Both halves were right and nothing
    /// joined them: `curl` reports the provider's «no» as a success, so the
    /// refusal is minted by the body, and weighing the error `curl` returns
    /// weighed a shape that never arrives. A live account whose access token
    /// had passed its hour read `shut`, with a login line it did not need.
    #[test]
    fn a_body_refusing_a_stale_access_token_is_weighed_against_the_credentials() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as i64)
            .unwrap_or(0);
        let text = credentials_at(now - 3_600_000, now + 30 * 86_400_000);
        let mut channel = channel_with_expiry_pointers(access_pointer(), refresh_pointer());
        channel.shape.dead_when = vec!["authentication_error".to_owned()];
        let refused = r#"{"error":{"type":"authentication_error","message":"OAuth token expired"}}"#;

        let read = from_oauth_usage(refused, ENGINE, 0, &channel.shape);
        assert!(
            matches!(read, Err(RemainingError::Refused(_))),
            "the body is what mints the refusal: {read:?}"
        );

        let weighed = read_against_the_credentials(read, &text, &channel);
        assert!(
            matches!(weighed, Err(RemainingError::RefusedWithAnUnexpiredRefreshToken { .. })),
            "and a live refresh token beside it must survive the seam: {weighed:?}"
        );
        assert!(
            !weighed.unwrap_err().credential_is_dead(),
            "so the account is not offered a login it does not need"
        );
    }

    fn credentials_at(access_expires_at: i64, refresh_expires_at: i64) -> String {
        format!(
            r#"{{"claudeAiOauth": {{"accessToken": "{A_SECRET}", "expiresAt": {access_expires_at}, "refreshTokenExpiresAt": {refresh_expires_at}}}}}"#
        )
    }

    fn access_pointer() -> Vec<String> {
        vec!["claudeAiOauth".to_owned(), "expiresAt".to_owned()]
    }

    fn refresh_pointer() -> Vec<String> {
        vec!["claudeAiOauth".to_owned(), "refreshTokenExpiresAt".to_owned()]
    }

    fn channel_with_expiry_pointers(access: Vec<String>, refresh: Vec<String>) -> OauthUsageChannel {
        OauthUsageChannel {
            engine: ENGINE.to_owned(),
            credentials: PathBuf::from("/unused"),
            token_pointer: pointer(),
            url: URL.to_owned(),
            headers: vec![BETA.to_owned()],
            held_by: Vec::new(),
            access_expires_pointer: access,
            refresh_expires_pointer: refresh,
            shape: WindowWords::default(),
        }
    }

    /// **Mutant run**: return `RemainingError::Refused(said)` unconditionally
    /// from `refusal_against_expiry` and this goes red — the enriched variant
    /// never appears.
    #[test]
    fn an_expired_access_token_with_a_live_refresh_token_is_told_apart_from_a_real_refusal() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as i64)
            .unwrap_or(0);
        let text = credentials_at(now - 3_600_000, now + 30 * 86_400_000);
        let channel = channel_with_expiry_pointers(access_pointer(), refresh_pointer());

        let enriched = refusal_against_expiry("token expired".to_owned(), &text, &channel);

        match &enriched {
            RemainingError::RefusedWithAnUnexpiredRefreshToken { said, refresh_expires_at } => {
                assert_eq!(said, "token expired");
                assert_eq!(*refresh_expires_at, now + 30 * 86_400_000);
            }
            other => panic!("expected the enriched variant, got {other}"),
        }
        assert!(
            enriched.to_string().contains("not necessarily a sign the account needs a fresh login"),
            "{enriched}"
        );
    }

    /// Both tokens expired: this is a real refusal, told plainly.
    #[test]
    fn both_tokens_expired_stays_a_plain_refusal() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_millis() as i64)
            .unwrap_or(0);
        let text = credentials_at(now - 3_600_000, now - 60_000);
        let channel = channel_with_expiry_pointers(access_pointer(), refresh_pointer());

        let plain = refusal_against_expiry("token expired".to_owned(), &text, &channel);

        assert_eq!(plain, RemainingError::Refused("token expired".to_owned()));
    }

    /// A provider whose descriptor declares neither pointer is unaffected:
    /// today's behaviour, unchanged.
    #[test]
    fn a_provider_with_no_expiry_pointers_declared_stays_a_plain_refusal() {
        let channel = channel_with_expiry_pointers(Vec::new(), Vec::new());
        let plain = refusal_against_expiry("token expired".to_owned(), "{}", &channel);
        assert_eq!(plain, RemainingError::Refused("token expired".to_owned()));
    }
}
