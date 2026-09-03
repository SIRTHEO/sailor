//! The exact token and context count, from an engine's answer.
//!
//! `None` is a legitimate answer — a number that engine does not say — while
//! `0` never is for a missing field: nothing is invented here.

use crate::catalog::Model;

/// The tokens measured for a single call. `total_tokens` can be known even
/// when `prompt_tokens`/`completion_tokens` are not (Codex says only the total
/// on its output).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// The cost in USD, when the engine declares it itself (OpenRouter does on
    /// every answer, zero included on free models).
    pub cost_usd: Option<f64>,
}

impl TokenUsage {
    /// From the JSON body of an OpenRouter `chat/completions` answer. A field
    /// that is missing, or is not a number, stays `None` rather than failing
    /// the whole parse: an error body (429, invalid key, …) must be able to
    /// give back an empty `TokenUsage` instead of a panic.
    pub fn from_openrouter_body(body: &str) -> TokenUsage {
        let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {
            return TokenUsage::default();
        };
        let usage = parsed.get("usage");
        let field_u64 = |key: &str| usage.and_then(|u| u.get(key)).and_then(|v| v.as_u64());
        TokenUsage {
            prompt_tokens: field_u64("prompt_tokens"),
            completion_tokens: field_u64("completion_tokens"),
            total_tokens: field_u64("total_tokens"),
            cost_usd: usage.and_then(|u| u.get("cost")).and_then(|v| v.as_f64()),
        }
    }

    /// From the text output of `codex exec`: reuses `parse_codex_tokens`
    /// instead of rewriting the parsing (`"tokens used"` followed by the number
    /// with a dot as thousands separator). Codex separates neither prompt from
    /// completion nor declares a cost: those stay `None`.
    pub fn from_codex_output(output: &str) -> TokenUsage {
        let raw = parse_codex_tokens(output);
        TokenUsage {
            total_tokens: raw.parse().ok(),
            ..TokenUsage::default()
        }
    }
}

/// The full picture of a call: what went in, what came out, what is left of
/// the model's window, what it cost.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ContextAccounting {
    pub usage: TokenUsage,
    /// The window of the model used: `None` when the model is not in the
    /// catalog (e.g. Codex, which is not an OpenRouter model).
    pub context_length: Option<u64>,
    /// `context_length - total_tokens`. `None` when either addend is missing —
    /// a remainder computed on an unknown total would be an invented number
    /// wearing the face of a measure.
    pub remaining: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl ContextAccounting {
    /// Combines the measured usage with the catalog model that served it, when
    /// that model is known. If `usage.cost_usd` is already declared by the
    /// engine (OpenRouter always does) that one is used; otherwise the cost
    /// comes from the model's price list, when there is one.
    pub fn compute(usage: TokenUsage, model: Option<&Model>) -> ContextAccounting {
        let context_length = model.and_then(|m| m.context_length);
        let remaining = match (context_length, usage.total_tokens) {
            (Some(ctx), Some(total)) => Some(ctx.saturating_sub(total)),
            _ => None,
        };
        let cost_usd = usage.cost_usd.or_else(|| compute_cost(&usage, model));
        ContextAccounting {
            usage,
            context_length,
            remaining,
            cost_usd,
        }
    }
}

/// The cost from the model's price list, when the engine has not said it
/// already. It needs both halves (prompt and completion, price and count):
/// with one missing the cost stays unknown.
fn compute_cost(usage: &TokenUsage, model: Option<&Model>) -> Option<f64> {
    let model = model?;
    let prompt_tokens = usage.prompt_tokens? as f64;
    let completion_tokens = usage.completion_tokens? as f64;
    let price_in = model.price_per_million_input?;
    let price_out = model.price_per_million_output?;
    Some((prompt_tokens * price_in + completion_tokens * price_out) / 1_000_000.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, Modality};

    // A real body, captured on nvidia/nemotron-3-super-120b-a12b:free (key
    // read from a file, never printed; no trace of it left here).
    const OPENROUTER_OK: &str = r#"{"id":"gen-1787833447-8R8Z6Ce3NQrwbjknBeMw","usage":{"prompt_tokens":27,"completion_tokens":20,"total_tokens":47,"cost":0,"is_byok":false}}"#;

    #[test]
    fn reads_all_three_token_counts_and_the_cost() {
        let usage = TokenUsage::from_openrouter_body(OPENROUTER_OK);
        assert_eq!(usage.prompt_tokens, Some(27));
        assert_eq!(usage.completion_tokens, Some(20));
        assert_eq!(usage.total_tokens, Some(47));
        assert_eq!(usage.cost_usd, Some(0.0));
    }

    #[test]
    fn a_429_body_has_no_usage_at_all_not_a_panic() {
        let body = r#"{"error":{"code":429,"message":"limit"}}"#;
        let usage = TokenUsage::from_openrouter_body(body);
        assert_eq!(usage, TokenUsage::default());
    }

    #[test]
    fn garbage_input_gives_an_empty_usage_not_a_panic() {
        let usage = TokenUsage::from_openrouter_body("not json");
        assert_eq!(usage, TokenUsage::default());
    }

    #[test]
    fn codex_output_reuses_the_shared_parser() {
        let output = "some noise\ntokens used\n13.910\nmore noise";
        let usage = TokenUsage::from_codex_output(output);
        assert_eq!(usage.total_tokens, Some(13910));
        assert_eq!(usage.prompt_tokens, None);
        assert_eq!(usage.completion_tokens, None);
        assert_eq!(usage.cost_usd, None);
    }

    #[test]
    fn codex_output_without_the_marker_is_unknown_not_zero() {
        let usage = TokenUsage::from_codex_output("no useful line here");
        assert_eq!(usage.total_tokens, None);
    }

    fn sample_model(id: &str) -> Model {
        let catalog =
            Catalog::parse(include_str!("../tests/fixtures/catalog-sample.json")).unwrap();
        catalog.find(id).unwrap().clone()
    }

    #[test]
    fn computes_remaining_context_against_the_model_window() {
        let model = sample_model("nvidia/nemotron-3-super-120b-a12b:free"); // 262144
        let usage = TokenUsage {
            total_tokens: Some(47),
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.context_length, Some(262144));
        assert_eq!(acc.remaining, Some(262144 - 47));
    }

    #[test]
    fn remaining_is_none_when_the_model_is_unknown_codex_case() {
        let usage = TokenUsage {
            total_tokens: Some(13910),
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, None);
        assert_eq!(acc.context_length, None);
        assert_eq!(
            acc.remaining, None,
            "a remainder on an unknown window would be an invented number"
        );
    }

    #[test]
    fn remaining_is_none_when_total_tokens_is_unknown() {
        let model = sample_model("nvidia/nemotron-3-super-120b-a12b:free");
        let acc = ContextAccounting::compute(TokenUsage::default(), Some(&model));
        assert_eq!(acc.remaining, None);
    }

    #[test]
    fn prefers_the_engines_own_declared_cost_over_a_computed_one() {
        let model = sample_model("qwen/qwen3.8-flash");
        let usage = TokenUsage {
            prompt_tokens: Some(100),
            completion_tokens: Some(100),
            cost_usd: Some(0.4242), // deliberately unlike the computed value, to prove it wins
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.cost_usd, Some(0.4242));
    }

    #[test]
    fn computes_cost_from_the_price_list_when_the_engine_says_nothing() {
        let model = sample_model("qwen/qwen3.8-flash"); // 0.15 / 0.47 USD per million
        let usage = TokenUsage {
            prompt_tokens: Some(1_000_000),
            completion_tokens: Some(1_000_000),
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, Some(&model));
        let cost = acc.cost_usd.unwrap();
        assert!(
            (cost - 0.62).abs() < 1e-9,
            "expected 0.15+0.47=0.62, got {cost}"
        );
    }

    #[test]
    fn cost_is_unknown_when_token_counts_are_only_partial() {
        let model = sample_model("qwen/qwen3.8-flash");
        let usage = TokenUsage {
            prompt_tokens: Some(10),
            ..TokenUsage::default()
        }; // completion missing
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.cost_usd, None);
    }

    #[test]
    fn accepts_modality_check_is_reused_correctly() {
        let model = sample_model("thinkingmachines/inkling:free");
        assert!(model.accepts(Modality::Audio));
    }
}

/// The tokens `codex exec` declares in its text output.
///
/// It came from `notte`, which is no longer in the repo — so every `crates/notte`
/// path still named in `config.rs` and `fetch.rs` points at a crate that is not
/// there. It lives here because reading what an engine says it consumed is this
/// crate's trade.
pub fn parse_codex_tokens(output: &str) -> String {
    let mut lines = output.lines();
    while let Some(line) = lines.next() {
        if line.trim() == "tokens used" {
            if let Some(num_line) = lines.next() {
                let cleaned: String = num_line.trim().chars().filter(|c| *c != '.').collect();
                if !cleaned.is_empty() && cleaned.chars().all(|c| c.is_ascii_digit()) {
                    return cleaned;
                }
            }
        }
    }
    "?".to_string()
}

// ── consumption read the way a descriptor declares it ─────────────────────

/// The shape in which an engine states its own consumption.
///
/// **IT IS DATA, NOT A BRANCH PER PROVIDER.** `from_openrouter_body` and
/// `from_codex_output` above are hand-written readers for two known formats;
/// a third for every new engine is the road model-independence forbids. Here
/// the descriptor says the shape, so a new engine is measured with a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    /// The output is a JSON envelope, and pointers are key paths.
    #[default]
    Json,
    /// The output is text, and pointers are regular expressions with one
    /// capture group.
    Text,
}

/// Where a value sits inside what the engine said.
///
/// The two forms are not interchangeable: a key path holds only on a JSON
/// body, a regular expression only on text. A pointer of the wrong shape finds
/// nothing and leaves the value **unknown** — the right way to be wrong, since
/// no invented number takes its place.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pointer {
    /// A key path inside the envelope: `["usage", "input_tokens"]`.
    Path(Vec<String>),
    /// A regular expression with the number (or the text) in the first group.
    Pattern(String),
    /// **The name of the first key** of the object sitting at this path. Some
    /// engines use the model name as a *key* rather than a field:
    /// `{"modelUsage": {"claude-opus-5[1m]": {...}}}`. Without this form the
    /// name is unreachable, and without the name no price-list entry can be
    /// found: the cost would stay unknown even with a perfect list. Only the
    /// first key is taken, and `read_name` says why.
    FirstKey(Vec<String>),
}

/// What a descriptor declares it can read from its own engine's output. Every
/// pointer is optional: an engine that says only the total declares only
/// `total_tokens`, and the rest stay unknown.
/// Which pipe the engine states its usage on. Ollama prints its counts on
/// stderr while the answer goes to stdout; reading the wrong one finds nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Heard {
    #[default]
    Stdout,
    Stderr,
    Both,
}

impl Heard {
    /// The text to read the usage from, given both pipes.
    pub fn text(self, stdout: &str, stderr: &str) -> String {
        match self {
            Heard::Stdout => stdout.to_owned(),
            Heard::Stderr => stderr.to_owned(),
            Heard::Both => format!("{stdout}\n{stderr}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Declared {
    pub read: Shape,
    pub from: Heard,
    pub input_tokens: Option<Pointer>,
    pub output_tokens: Option<Pointer>,
    /// Input tokens **read from the cache**, which have a price per million
    /// all of their own — often an order of magnitude below. Summing them with
    /// the others makes the measure false exactly where it counts.
    pub cached_tokens: Option<Pointer>,
    /// Input tokens **written to the cache** — not the same as reading it:
    /// writing costs **more** than plain input, reading much less.
    ///
    /// **IT HAS A FIELD OF ITS OWN.** On a measured call cache writes were
    /// **96%** of the declared cost; counting them as plain input, or not at
    /// all, got the sum wrong by 24 times — always the reassuring way, down.
    pub cache_write_tokens: Option<Pointer>,
    /// Tokens written to a **long-lived** cache, which has a higher price of
    /// its own. Whoever does not tell them apart leaves this empty and pays
    /// the short-write price on everything — wrongly, but declaredly.
    pub cache_write_long_tokens: Option<Pointer>,
    pub total_tokens: Option<Pointer>,
    /// **HOW MANY TURNS THE CALL TOOK**, i.e. how many times the model came
    /// back to speak inside a single invocation.
    ///
    /// A four-step flow takes 2.07x the turns of one session doing the same
    /// work and reads only 8% more per turn: the bill is decided by how often
    /// each step goes over the context again, not by what each one carries.
    pub turns: Option<Pointer>,
    /// The cost the engine declares itself. Recorded as a **comparison**,
    /// never in place of the sum made on the local price list.
    pub cost: Option<Pointer>,
    /// Where the engine names the model that really served the call. It is the
    /// only honest link between a command line and a price-list entry.
    pub model: Option<Pointer>,
    /// Where the answer text sits inside the envelope, because **THE STEP'S
    /// OUTPUT MUST NOT CHANGE.** Asking an engine for `--output-format json` so
    /// it states its tokens wraps the answer too, and without this pointer a
    /// downstream step gets the envelope instead of the text: a flow with
    /// `allow_extra: false` on its own shape would go red **for a measurement
    /// it never asked for**.
    pub answer: Option<Pointer>,
}

impl Declared {
    /// True when it declares no pointer at all: the block is there, but it
    /// says nowhere to look for anything.
    pub fn is_empty(&self) -> bool {
        self.input_tokens.is_none()
            && self.output_tokens.is_none()
            && self.cached_tokens.is_none()
            && self.cache_write_tokens.is_none()
            && self.cache_write_long_tokens.is_none()
            && self.total_tokens.is_none()
            && self.cost.is_none()
            && self.model.is_none()
            && self.answer.is_none()
    }
}

/// The consumption read from a single output, plus what it takes to tie it to
/// a price list and leave the step's own output intact.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Reading {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cached_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub cache_write_long_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// How many turns the call took, when the engine declares it.
    pub turns: Option<u64>,
    /// The cost declared by the engine, in its own unit (USD as a rule).
    pub declared_cost: Option<f64>,
    /// The model the engine says it used.
    pub model: Option<String>,
    /// The answer text pulled out of the envelope, when the descriptor says
    /// where it sits.
    pub answer: Option<String>,
}

/// Reads the consumption from an engine's output the way its descriptor
/// declares. A pure function: text in, values out, no provider name.
///
/// Every field that is not found stays `None`. No path, in any shape, gives
/// back `0` for a missing value.
pub fn read_declared(said: &str, declared: &Declared) -> Reading {
    match declared.read {
        Shape::Json => read_from_json(said, declared),
        Shape::Text => read_from_text(said, declared),
    }
}

/// One text, read where the pointer says: a key path on JSON, a regular
/// expression on text — **THE POINTER SAYS THE SHAPE AND IS NOT ASKED TWICE**,
/// since asking again would allow an incoherent answer, and that incoherence
/// gives no error, only an unknown value with no visible reason. It reads what
/// an engine says about itself outside consumption: first the id of the session
/// it opened, the only way to resume one for engines that mint that id.
pub fn read_text(said: &str, pointer: &Pointer) -> Option<String> {
    match pointer {
        Pointer::Pattern(pattern) => first_group(said, pattern),
        path => {
            let body = serde_json::from_str::<serde_json::Value>(said.trim()).ok()?;
            read_name(&body, Some(path))
        }
    }
}

/// Like [`read_text`], but it also yields a boolean or a number.
///
/// For consumption a pointer landing on a boolean is a wrong pointer and
/// `as_text` says "nothing" on purpose; for a yes/no question it is the other
/// way round — `claude auth status` answers in `"loggedIn": true`. Merging the
/// two readers would silently take that defence away from consumption.
pub fn read_scalar(said: &str, pointer: &Pointer) -> Option<String> {
    match pointer {
        Pointer::Pattern(pattern) => first_group(said, pattern),
        Pointer::FirstKey(_) => read_text(said, pointer),
        Pointer::Path(_) => {
            let body = serde_json::from_str::<serde_json::Value>(said.trim()).ok()?;
            let value = walk(&body, Some(pointer))?;
            match value {
                serde_json::Value::String(text) => Some(text.clone()),
                serde_json::Value::Bool(yes) => Some(yes.to_string()),
                serde_json::Value::Number(number) => Some(number.to_string()),
                // An object or a list is not an answer: whoever wrote the
                // pointer aimed at the container instead of the value.
                _ => None,
            }
        }
    }
}

fn read_from_json(said: &str, declared: &Declared) -> Reading {
    // An unreadable envelope is not a fault: it is an engine that answered in
    // plain text where JSON was expected — quota exhausted, network error, a
    // warning on the first line. Everything stays unknown, and the call is
    // recorded all the same.
    let Ok(body) = serde_json::from_str::<serde_json::Value>(said.trim()) else {
        return Reading::default();
    };
    let number = |pointer: &Option<Pointer>| walk(&body, pointer.as_ref()).and_then(as_tokens);
    Reading {
        input_tokens: number(&declared.input_tokens),
        output_tokens: number(&declared.output_tokens),
        cached_tokens: number(&declared.cached_tokens),
        cache_write_tokens: number(&declared.cache_write_tokens),
        cache_write_long_tokens: number(&declared.cache_write_long_tokens),
        total_tokens: number(&declared.total_tokens),
        turns: number(&declared.turns),
        declared_cost: walk(&body, declared.cost.as_ref()).and_then(as_money),
        model: read_name(&body, declared.model.as_ref()),
        answer: walk(&body, declared.answer.as_ref()).and_then(as_text),
    }
}

/// A text that can sit in a field **or be the name of a key**.
///
/// **THE FIRST KEY AND NO MORE.** More than one key means the call crossed more
/// than one model, and nothing here can split the consumption between them. The
/// first is the one the engine wrote first — `serde_json` preserves insertion
/// order, the only thing that makes "first" well defined — and nothing is guessed.
fn read_name(body: &serde_json::Value, pointer: Option<&Pointer>) -> Option<String> {
    match pointer? {
        Pointer::FirstKey(keys) => {
            let mut here = body;
            for key in keys {
                here = here.get(key)?;
            }
            here.as_object()?.keys().next().cloned()
        }
        other => walk(body, Some(other)).and_then(as_text),
    }
}

fn read_from_text(said: &str, declared: &Declared) -> Reading {
    let capture = |pointer: &Option<Pointer>| match pointer.as_ref() {
        Some(Pointer::Pattern(pattern)) => first_group(said, pattern),
        // A key path on a text output: whoever wrote the descriptor got the
        // shape wrong, and the value stays unknown.
        _ => None,
    };
    Reading {
        input_tokens: capture(&declared.input_tokens).and_then(|text| digits(&text)),
        output_tokens: capture(&declared.output_tokens).and_then(|text| digits(&text)),
        cached_tokens: capture(&declared.cached_tokens).and_then(|text| digits(&text)),
        cache_write_tokens: capture(&declared.cache_write_tokens).and_then(|text| digits(&text)),
        cache_write_long_tokens: capture(&declared.cache_write_long_tokens)
            .and_then(|text| digits(&text)),
        total_tokens: capture(&declared.total_tokens).and_then(|text| digits(&text)),
        turns: capture(&declared.turns).and_then(|text| digits(&text)),
        declared_cost: capture(&declared.cost).and_then(|text| text.trim().parse().ok()),
        model: capture(&declared.model),
        answer: capture(&declared.answer),
    }
}

/// Walks a key path. An empty path means the whole body: the way of saying
/// "the answer is everything it said".
fn walk<'a>(
    body: &'a serde_json::Value,
    pointer: Option<&Pointer>,
) -> Option<&'a serde_json::Value> {
    let Pointer::Path(keys) = pointer? else {
        // A regular expression declared on a JSON read: wrong shape, unknown
        // value.
        return None;
    };
    let mut here = body;
    for key in keys {
        here = here.get(key)?;
    }
    Some(here)
}

/// A token count. A string is accepted too, because an engine writing numbers
/// above 2^53 sends them as text so as not to lose them.
fn as_tokens(value: &serde_json::Value) -> Option<u64> {
    value.as_u64().or_else(|| value.as_str().and_then(digits))
}

fn as_money(value: &serde_json::Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
}

/// The text of a value. A number or a boolean does **not** become text here: a
/// pointer that lands on something which is not text is a wrong pointer, and
/// giving back its printed form would hide the mistake in a plausible answer.
fn as_text(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(str::to_owned)
}

/// The first capture group, when the expression is valid and matches.
///
/// A badly written expression makes nothing fail: that value stays unknown, as
/// if the engine had not said it. A wrong descriptor worsens the measure, it
/// does not break the call it was measuring.
fn first_group(said: &str, pattern: &str) -> Option<String> {
    let regex = regex::Regex::new(pattern).ok()?;
    let captures = regex.captures(said)?;
    captures.get(1).map(|group| group.as_str().to_owned())
}

/// An integer written with thousands separators. `13.910` and `13,910` are
/// both thirteen thousand nine hundred and ten: which separator an engine uses
/// depends on the locale of the machine it runs on, and is not something a
/// descriptor author should have to guess. Text that is not all digits stays
/// `None` — never a fallback zero.
fn digits(text: &str) -> Option<u64> {
    let cleaned: String = text
        .trim()
        .chars()
        .filter(|c| !matches!(c, '.' | ',' | '_' | ' ' | '\u{202f}' | '\u{a0}'))
        .collect();
    if cleaned.is_empty() || !cleaned.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    cleaned.parse().ok()
}

#[cfg(test)]
mod declared_tests {
    use super::*;

    fn path(keys: &[&str]) -> Option<Pointer> {
        Some(Pointer::Path(
            keys.iter().map(|k| (*k).to_owned()).collect(),
        ))
    }

    /// An envelope shaped the way a command-line engine produces one when
    /// asked for `--output-format json`. **Not measured on any real engine**:
    /// it is the shape the reader is tested against, and its fidelity to one
    /// particular provider is not what this test asserts.
    const WRAPPED: &str = r#"{
      "result": "the real answer",
      "model": "claude-sonnet-5",
      "total_cost_usd": 0.0421,
      "usage": {
        "input_tokens": 1200,
        "output_tokens": 340,
        "cache_read_input_tokens": 98000
      }
    }"#;

    fn wrapped_declaration() -> Declared {
        Declared {
            read: Shape::Json,
            from: Heard::Stdout,
            input_tokens: path(&["usage", "input_tokens"]),
            output_tokens: path(&["usage", "output_tokens"]),
            cached_tokens: path(&["usage", "cache_read_input_tokens"]),
            cache_write_tokens: None,
            cache_write_long_tokens: None,
            total_tokens: None,
            turns: None,
            cost: path(&["total_cost_usd"]),
            model: path(&["model"]),
            answer: path(&["result"]),
        }
    }

    #[test]
    fn reads_every_declared_pointer_out_of_a_json_envelope() {
        let reading = read_declared(WRAPPED, &wrapped_declaration());
        assert_eq!(reading.input_tokens, Some(1200));
        assert_eq!(reading.output_tokens, Some(340));
        assert_eq!(reading.cached_tokens, Some(98_000));
        assert_eq!(reading.declared_cost, Some(0.0421));
        assert_eq!(reading.model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(reading.answer.as_deref(), Some("the real answer"));
    }

    /// THE ARM THAT COUNTS for criterion 3: the cache has a pointer of its own
    /// and does not end up summed into the input. If the reader confused them,
    /// these two numbers would be equal.
    #[test]
    fn cache_tokens_never_end_up_inside_the_input_count() {
        let reading = read_declared(WRAPPED, &wrapped_declaration());
        assert_eq!(reading.input_tokens, Some(1200));
        assert_ne!(reading.input_tokens, reading.cached_tokens);
    }

    #[test]
    fn a_pointer_that_finds_nothing_leaves_the_value_unknown_never_zero() {
        let declared = Declared {
            read: Shape::Json,
            input_tokens: path(&["usage", "does_not_exist"]),
            output_tokens: path(&["not", "this"]),
            ..Declared::default()
        };
        let reading = read_declared(WRAPPED, &declared);
        assert_eq!(reading.input_tokens, None);
        assert_eq!(reading.output_tokens, None);
    }

    /// An engine that answered in plain text where JSON was expected — quota
    /// exhausted, a warning on the first line — is not a fault of the reader.
    #[test]
    fn plain_text_where_json_was_expected_is_all_unknown_not_a_panic() {
        let reading = read_declared("You've hit your weekly limit", &wrapped_declaration());
        assert_eq!(reading, Reading::default());
    }

    #[test]
    fn a_number_written_as_a_string_is_still_a_count() {
        let declared = Declared {
            read: Shape::Json,
            input_tokens: path(&["n"]),
            ..Declared::default()
        };
        assert_eq!(
            read_declared(r#"{"n":"9007199254740993"}"#, &declared).input_tokens,
            Some(9_007_199_254_740_993)
        );
    }

    /// A pointer landing on a number does not become the text of that number:
    /// giving it back would hide a wrong descriptor inside a plausible answer.
    #[test]
    fn a_pointer_landing_on_a_number_is_not_read_as_text() {
        let declared = Declared {
            read: Shape::Json,
            model: path(&["usage", "input_tokens"]),
            ..Declared::default()
        };
        assert_eq!(read_declared(WRAPPED, &declared).model, None);
    }

    // ── the text shape ───────────────────────────────────────────────

    /// The output of `codex exec` as written in the `parse_codex_tokens` test
    /// above: the number sits on the line after the marker, with dots as
    /// thousands separators.
    const CODEX_LIKE: &str = "some noise\ntokens used\n13.910\nmore noise";

    #[test]
    fn reads_a_count_from_plain_text_with_a_declared_pattern() {
        let declared = Declared {
            read: Shape::Text,
            total_tokens: Some(Pointer::Pattern(r"tokens used\s*\n\s*([\d.,]+)".to_owned())),
            ..Declared::default()
        };
        let reading = read_declared(CODEX_LIKE, &declared);
        assert_eq!(reading.total_tokens, Some(13_910));
        assert_eq!(
            reading.input_tokens, None,
            "codex does not separate the two sides"
        );
        assert_eq!(reading.output_tokens, None);
    }

    #[test]
    fn a_pattern_that_does_not_match_leaves_it_unknown() {
        let declared = Declared {
            read: Shape::Text,
            total_tokens: Some(Pointer::Pattern(r"tokens used\s*\n\s*([\d.,]+)".to_owned())),
            ..Declared::default()
        };
        assert_eq!(
            read_declared("no useful line", &declared).total_tokens,
            None
        );
    }

    /// A descriptor with a badly written regular expression worsens the
    /// measure, it does not break the call it was measuring.
    #[test]
    fn a_broken_pattern_is_unknown_not_a_panic() {
        let declared = Declared {
            read: Shape::Text,
            total_tokens: Some(Pointer::Pattern("([unclosed".to_owned())),
            ..Declared::default()
        };
        assert_eq!(read_declared(CODEX_LIKE, &declared).total_tokens, None);
    }

    /// The two shapes do not mix: a key path on a text output, or an
    /// expression on a JSON body, find nothing.
    #[test]
    fn a_pointer_of_the_wrong_shape_finds_nothing() {
        let on_text = Declared {
            read: Shape::Text,
            total_tokens: path(&["usage", "input_tokens"]),
            ..Declared::default()
        };
        assert_eq!(read_declared(WRAPPED, &on_text).total_tokens, None);

        // THE ARM THAT COUNTS: the expression is written so as to match a real
        // KEY of the body. If the code treated an expression as a one-key
        // path, `Some(7)` would come out here — a number from the wrong place
        // that nobody would recognise as wrong by looking at it.
        let on_json = Declared {
            read: Shape::Json,
            total_tokens: Some(Pointer::Pattern("n".to_owned())),
            ..Declared::default()
        };
        assert_eq!(read_declared(r#"{"n":7}"#, &on_json).total_tokens, None);
        assert_eq!(read_declared(WRAPPED, &on_json).total_tokens, None);
    }

    #[test]
    fn thousands_separators_do_not_change_the_number() {
        assert_eq!(digits("13.910"), Some(13_910));
        assert_eq!(digits("13,910"), Some(13_910));
        assert_eq!(digits(" 13 910 "), Some(13_910));
        assert_eq!(digits("13910"), Some(13_910));
    }

    #[test]
    fn something_that_is_not_a_number_is_unknown_not_zero() {
        assert_eq!(digits("many"), None);
        assert_eq!(digits(""), None);
        assert_eq!(digits("-5"), None);
        assert_eq!(digits("12a"), None);
    }

    #[test]
    fn a_declaration_with_no_pointers_at_all_is_empty() {
        assert!(Declared::default().is_empty());
        assert!(!wrapped_declaration().is_empty());
    }
}
