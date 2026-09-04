//! How much of **a person's** quota is left, read instead of asked.
//!
//! **WHY IT EXISTS.** An agent cannot count what its harness spends for it: in
//! the A/B it declared 33 turns out of 75 real ones. The cure is not to ask
//! better — it is to **read**, spending nothing to do it.

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
    /// When we looked. A quota ages: a value without the instant it was read
    /// at cannot be told apart from yesterday's.
    pub observed_at: i64,
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
    /// It answered, and said no. It carries the provider's own words, which
    /// say **what to do** — "the token has been revoked" is cured by
    /// authenticating again, and no sentence written here would say it better.
    /// **It never carries the token**: only the `message` field is copied,
    /// never the request.
    Refused(String),
    /// It answered something that is not the expected JSON: the channel is
    /// beta, and this is how it will break.
    NotUnderstood,
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
) -> Result<Vec<Remaining>, RemainingError> {
    let parsed: serde_json::Value =
        serde_json::from_str(body).map_err(|_| RemainingError::NotUnderstood)?;
    let windows = parsed.as_object().ok_or(RemainingError::NotUnderstood)?;

    // **A REFUSAL IS RECOGNISED BEFORE THE WINDOWS ARE COUNTED**: this
    // provider's refusal is valid JSON with no `utilization` anywhere, so
    // scanning it for quotas returns an empty list — "no consumption on record".
    // **Look at `error.message`, not the envelope.** A revocation carries a
    // top-level `"type": "error"`, a rate limit does not: matching the envelope
    // let the rate limit through as that empty list, to an automated poller.
    if let Some(said) = windows
        .get("error")
        .and_then(|error| error.get("message"))
        .and_then(serde_json::Value::as_str)
    {
        return Err(RemainingError::Refused(said.to_owned()));
    }

    let mut found = Vec::new();
    for (unit, window) in windows {
        let Some(fields) = window.as_object() else {
            continue;
        };
        let Some(percent) = fields
            .get("utilization")
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
            used_fraction: percent / 100.0,
            resets_at: fields
                .get("resets_at")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned),
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
    let path: &Path = &channel.credentials;
    if !path.exists() {
        return Err(RemainingError::NoCredentials(path.to_path_buf()));
    }
    let text = std::fs::read_to_string(path)
        .map_err(|error| RemainingError::CredentialsUnreadable(error.to_string()))?;
    let token = Token::from_credentials_at(&text, &channel.token_pointer)?;
    let body = ask_curl(&token.curl_config(&channel.url, &channel.headers))?;
    from_oauth_usage(&body, &channel.engine, observed_at)
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
        from_oauth_usage(body, ENGINE, observed_at)
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

        let said = parse(refused, 0).expect_err("it is a refusal, not a measure");
        assert_eq!(
            said,
            RemainingError::Refused("OAuth access token has been revoked.".to_owned()),
            "the provider's own words carry through: they say what to do, namely authenticate again"
        );
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
            Err(RemainingError::Refused(
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
        };
        let refused = read_oauth_usage(&channel, 0).expect_err("there is nothing to read");
        assert!(matches!(refused, RemainingError::NoCredentials(_)));
    }
}
