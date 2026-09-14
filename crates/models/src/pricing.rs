//! The price list: what a million tokens costs, per model.
//!
//! **IT IS THE SOURCE OF TRUTH ON COST**: a cost arriving from the same place
//! as the spend verifies nothing — an engine's own figure is recorded apart, as
//! a comparison, and the sum is made here. Pure: text in, values out; whoever
//! reads the file from disk lives elsewhere, as for `catalog` and `usage`.

/// The price list the product carries with it, embedded as
/// `toolbox::descriptor::BUILTIN` is: a binary copied elsewhere keeps
/// answering, with no path to guess. **TWO LISTS EXIST AND THE HOME ONE WINS
/// BY `id`** — `$SAILOR_HOME/pricing.json`, because prices change more often
/// than anyone recompiles.
pub const BUILTIN: &str = include_str!("../pricing.default.json");

/// How to tell a reader where a price list that nobody wrote on this machine
/// came from. It is not a path and must not look like one: whoever went
/// looking for it on disk would find nothing.
pub const BUILTIN_SOURCE: &str = "built-in";

/// The list shipped with the product, and **THE CURE FOR FAULT 35**: with only
/// the home file, a machine without it left every `cost_micros` at `None` and
/// a `spend_cap_micros` was a brake that does not brake. **IT PANICS IF IT
/// DOES NOT PARSE**: an empty list here would put that fault back on its feet.
pub fn shipped() -> PriceList {
    PriceList::parse(BUILTIN).expect("the price list shipped with the product parses")
}

/// What this list can say about a model name.
///
/// **THREE OUTCOMES AND NOT TWO, BECAUSE THEY ARE TWO DIFFERENT REPAIRS.** An
/// unknown name wants an entry or an alias; an entry with no prices wants the
/// prices. The cost is unknown either way, so the number hides which.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Known {
    /// It is there, with the two prices a sum needs.
    Priced,
    /// It is there, but the input or the output price is missing: the cost
    /// stays unknown all the same, and the list looks complete from afar.
    ListedWithoutPrice,
    /// No entry carries this name, neither as `id` nor as an alias.
    Absent,
}

/// What a model costs, per million tokens. Every price is optional: an entry
/// can know the input price and not the cache one, and that is different
/// information from "the cache is free".
///
/// **NEVER 0 FOR A MISSING PRICE** — the rule already written in
/// `catalog::Model`, where `0.0` is kept for models that really are free.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Price {
    pub id: String,
    /// The other names the same model goes by — `sonnet`, `claude-sonnet-5`,
    /// `anthropic/claude-sonnet-5`. Whoever keeps the list says which are the
    /// same thing; the code does not guess it.
    pub aliases: Vec<String>,
    pub input_per_million: Option<f64>,
    pub output_per_million: Option<f64>,
    /// What it costs to **read** from the cache. As a rule a fraction of the
    /// input price.
    pub cached_per_million: Option<f64>,
    /// What it costs to **write** to the cache. As a rule **more** than input,
    /// and it is the entry that surprises: on one measured call it was 96% of
    /// the spend, with two input tokens and four output.
    pub cache_write_per_million: Option<f64>,
    /// What it costs to write to a **long-lived** cache, where the provider
    /// offers more than one. Absent means "unknown", and then those tokens
    /// stay unpriced instead of being counted at the short-write price.
    pub cache_write_long_per_million: Option<f64>,
}

/// How an engine's calls are priced when its answer names no model, or no
/// tokens at all: the pinned rules that make every call of the ledger carry an
/// equivalent cost. Each field is optional and a missing one leaves the cost
/// unknown for that shape of call, which `price_every_call` then reports.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EnginePricing {
    /// The entry to price the tokens with when the engine states no model.
    pub assumed_model: Option<String>,
    /// For an engine that runs on hardware and not on an account: the
    /// equivalent cost of one second of wall time.
    pub per_wall_second_micros: Option<i64>,
    /// For a call that left no reading at all: a pinned figure, taken from the
    /// engine's measured calls and written here so it does not move.
    pub unread_call_equivalent_micros: Option<i64>,
}

/// The whole list, with the currency declared once.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceList {
    /// The currency of every price. Declaring it per entry would let euros and
    /// dollars be summed in the same column with nobody noticing.
    pub currency: String,
    /// When the prices are from, as written by whoever keeps the file. It
    /// enters no sum: it is there so that whoever looks at a figure can tell
    /// whether the list behind it is from yesterday or from last year.
    pub dated: Option<String>,
    pub entries: Vec<Price>,
    /// Per engine id, the rules above.
    pub engines: std::collections::BTreeMap<String, EnginePricing>,
    /// Tools a step may name that burn no account: their rows cost nothing,
    /// and saying so is different from not knowing.
    pub not_models: Vec<String>,
}

impl Default for PriceList {
    fn default() -> Self {
        PriceList {
            currency: "USD".to_owned(),
            dated: None,
            entries: Vec::new(),
            engines: std::collections::BTreeMap::new(),
            not_models: Vec::new(),
        }
    }
}

impl PriceList {
    /// Reads the file. A malformed entry drops out on its own, as in the
    /// catalog: one badly written price must not take the list away from
    /// everybody else.
    pub fn parse(text: &str) -> Result<PriceList, String> {
        let parsed: serde_json::Value = serde_json::from_str(text)
            .map_err(|error| format!("the price list is not JSON: {error}"))?;
        let currency = parsed
            .get("currency")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("USD")
            .to_owned();
        let dated = parsed
            .get("dated")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned);
        let entries = parsed
            .get("models")
            .and_then(serde_json::Value::as_array)
            .map(|items| items.iter().filter_map(parse_price).collect())
            .unwrap_or_default();
        let engines = parsed
            .get("engines")
            .and_then(serde_json::Value::as_object)
            .map(|items| {
                items
                    .iter()
                    .map(|(id, rules)| (id.trim().to_lowercase(), parse_engine(rules)))
                    .collect()
            })
            .unwrap_or_default();
        let not_models = parsed
            .get("not_models")
            .and_then(serde_json::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(|id| id.trim().to_lowercase()))
                    .collect()
            })
            .unwrap_or_default();
        Ok(PriceList {
            currency,
            dated,
            entries,
            engines,
            not_models,
        })
    }

    /// The entry carrying this name, by `id` or by alias. Case and surrounding
    /// spaces are ignored — no provider promises not to change them, and it is
    /// the tolerance `unusable_when` already applies. No fuzzy resemblance, no
    /// prefix, no "most likely": an unknown name leaves the cost unknown, and
    /// guessing would charge one model another's price, unseen on the row.
    pub fn find(&self, name: &str) -> Option<&Price> {
        let wanted = name.trim().to_lowercase();
        if wanted.is_empty() {
            return None;
        }
        self.entries.iter().find(|price| {
            price.id.trim().to_lowercase() == wanted
                || price
                    .aliases
                    .iter()
                    .any(|alias| alias.trim().to_lowercase() == wanted)
        })
    }

    /// What is known about this name's cost, before spending. It serves
    /// `sailor flow check` and `sailor flow cost`: whoever has no price for a
    /// model must **know** it, not discover it as a zero. See [`Known`].
    pub fn knows(&self, name: &str) -> Known {
        match self.find(name) {
            None => Known::Absent,
            Some(entry) => {
                if entry.input_per_million.is_some() && entry.output_per_million.is_some() {
                    Known::Priced
                } else {
                    Known::ListedWithoutPrice
                }
            }
        }
    }

    /// This list with the user's home-written one on top.
    ///
    /// **A WHOLE ENTRY IS REPLACED, NOT ITS FIELDS**, aliases included: a field
    /// merge would leave a shipped alias standing that whoever rewrote the entry
    /// believed removed, found out only by seeing a name priced they no longer
    /// declared. Home entries come first; `same_currency` rules on currencies.
    pub fn overridden_by(self, home: PriceList) -> PriceList {
        if !same_currency(&self.currency, &home.currency) {
            return home;
        }
        let taken: Vec<String> = home
            .entries
            .iter()
            .map(|entry| entry.id.trim().to_lowercase())
            .collect();
        let mut entries = home.entries;
        entries.extend(
            self.entries
                .into_iter()
                .filter(|entry| !taken.contains(&entry.id.trim().to_lowercase())),
        );
        let mut engines = self.engines;
        engines.extend(home.engines);
        let mut not_models = self.not_models;
        for id in home.not_models {
            if !not_models.contains(&id) {
                not_models.push(id);
            }
        }
        PriceList {
            currency: home.currency,
            // The date belongs to whoever wrote last: whoever looks at a figure
            // wants to know how old the list behind it is, and the home one is
            // the only one anybody has touched.
            dated: home.dated.or(self.dated),
            entries,
            engines,
            not_models,
        }
    }

    /// The rules for this engine, if the list carries any.
    pub fn engine(&self, cli: &str) -> Option<&EnginePricing> {
        self.engines.get(&cli.trim().to_lowercase())
    }

    pub fn is_not_a_model(&self, cli: &str) -> bool {
        self.not_models.contains(&cli.trim().to_lowercase())
    }
}

/// Two currencies are the same when they are written alike up to case and
/// spaces: the same tolerance [`PriceList::find`] applies to model names.
///
/// **TWO CURRENCIES DO NOT MIX.** When they differ, [`PriceList::overridden_by`]
/// drops the shipped entries entirely: euros and dollars summed in one column
/// still produce a total — plausible, and meaningless.
fn same_currency(one: &str, other: &str) -> bool {
    one.trim().to_lowercase() == other.trim().to_lowercase()
}

fn parse_engine(value: &serde_json::Value) -> EnginePricing {
    let whole = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_i64)
            .filter(|micros| *micros >= 0)
    };
    EnginePricing {
        assumed_model: value
            .get("assumed_model")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        per_wall_second_micros: whole("per_wall_second_micros"),
        unread_call_equivalent_micros: whole("unread_call_equivalent_micros"),
    }
}

fn parse_price(value: &serde_json::Value) -> Option<Price> {
    let id = value.get("id")?.as_str()?.to_owned();
    if id.trim().is_empty() {
        return None;
    }
    let aliases = value
        .get("aliases")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(Price {
        id,
        aliases,
        input_per_million: money(value.get("input_per_million")),
        output_per_million: money(value.get("output_per_million")),
        cached_per_million: money(value.get("cached_per_million")),
        cache_write_per_million: money(value.get("cache_write_per_million")),
        cache_write_long_per_million: money(value.get("cache_write_long_per_million")),
    })
}

/// A price. Both a number and a string are accepted — a hand-written list
/// often ends up with `"3.00"` — but not a value of another type, and not a
/// negative number: a price below zero is a typo, and summing it would make a
/// spend go down.
fn money(value: Option<&serde_json::Value>) -> Option<f64> {
    let value = value?;
    let number = value
        .as_f64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))?;
    if number.is_finite() && number >= 0.0 {
        Some(number)
    } else {
        None
    }
}

/// A price per million expressed in **micro-units of currency**, the way the
/// store wants it. `3.0` dollars per million becomes `3_000_000`.
pub fn micros_per_million(price_per_million: f64) -> i64 {
    (price_per_million * 1_000_000.0).round() as i64
}

/// One entry's prices, in micro-units, ready for the store's row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PriceMicros {
    pub input: Option<i64>,
    pub output: Option<i64>,
    pub cached: Option<i64>,
    pub cache_write: Option<i64>,
    pub cache_write_long: Option<i64>,
}

impl Price {
    pub fn micros(&self) -> PriceMicros {
        PriceMicros {
            input: self.input_per_million.map(micros_per_million),
            output: self.output_per_million.map(micros_per_million),
            cached: self.cached_per_million.map(micros_per_million),
            cache_write: self.cache_write_per_million.map(micros_per_million),
            cache_write_long: self.cache_write_long_per_million.map(micros_per_million),
        }
    }
}

/// A call's counts, each under its own name.
///
/// **THEY SIT IN A STRUCT AND NOT IN FIVE ARGUMENTS.** They are all
/// `Option<u64>`: lined up on a signature, two swapped by mistake compile
/// perfectly well and get the sum wrong forever, in silence. With a name per
/// field the swap cannot be written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TokenCounts {
    pub input: Option<u64>,
    pub output: Option<u64>,
    /// Read from the cache.
    pub cached: Option<u64>,
    /// Written to the cache, short-lived.
    pub cache_write: Option<u64>,
    /// Written to the cache, long-lived.
    pub cache_write_long: Option<u64>,
}

/// A call's cost, in micro-units of currency, **in integer arithmetic**: it
/// lands in an `INTEGER` column, and rounding at a different point each time
/// would make the same rows sum to two different totals. Rounded once, at the
/// end. **WHEN IT STAYS UNKNOWN**: a known count whose price is missing would
/// be an underestimate with the face of a measure, so `None`. An unknown count
/// is **not** zero — "did not use the cache" is not "I do not know if it did".
pub fn cost_micros(counts: TokenCounts, prices: PriceMicros) -> Option<i64> {
    // At least one side must be measured: a cost computed on no tokens is
    // zero, and a zero here is indistinguishable from a free call.
    if counts.input.is_none() && counts.output.is_none() {
        return None;
    }
    let mut total: i128 = 0;
    for (tokens, price) in [
        (counts.input, prices.input),
        (counts.output, prices.output),
        (counts.cached, prices.cached),
        (counts.cache_write, prices.cache_write),
        (counts.cache_write_long, prices.cache_write_long),
    ] {
        let Some(tokens) = tokens else { continue };
        // The count is there and the price is not: ignoring it would lower the
        // total by an amount nobody would see missing.
        let price = price?;
        total += i128::from(tokens) * i128::from(price);
    }
    // Rounding to the micro-unit: half up, once.
    let rounded = (total + 500_000) / 1_000_000;
    i64::try_from(rounded).ok()
}

/// What is known of one call when its equivalent cost is worked out. Every
/// field may be missing; the rule that applies says so on the row's reading.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallFacts<'a> {
    pub cli: &'a str,
    pub model: Option<&'a str>,
    pub counts: TokenCounts,
    /// How many models the engine counted apart; above one, the counts are
    /// the whole call's and the model is only the first named.
    pub models_named: Option<u64>,
    pub error_type: Option<&'a str>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    /// The engine's own figure, when it gave one. It never enters our sum,
    /// but a call that carries one is already accounted for.
    pub declared_cost: Option<f64>,
}

/// Which rule gave a call its equivalent cost, written beside the number so
/// nobody mistakes a pinned estimate for a measure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rule {
    /// A tool that burns no account.
    NotAModelCall,
    /// Refused before generating anything, with no figure to its name.
    AnsweredNothing,
    /// The model the engine named, times the tokens it counted.
    EngineModel,
    /// Several models counted apart and no figure from the engine: the whole
    /// call's tokens at the first model's price, a floor and declaredly so.
    MixedModelsPricedAsFirst,
    /// The engine named no model: the tokens at the engine's assumed model.
    AssumedModel,
    /// Hardware, not an account: seconds of wall time at the pinned rate.
    WallSeconds,
    /// No reading at all: the engine's pinned figure for an unread call.
    UnreadCallEquivalent,
    /// None of the above could be applied.
    Unpriced,
}

/// A call's equivalent cost and how it was reached.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Priced {
    pub cost_micros: Option<i64>,
    pub rule: Rule,
    /// The prices applied, when tokens were priced.
    pub prices: PriceMicros,
    /// The entry the tokens were priced with, when one was.
    pub priced_as: Option<String>,
}

fn nothing_read(facts: &CallFacts<'_>) -> bool {
    facts.counts.input.is_none()
        && facts.counts.output.is_none()
        && facts.counts.cached.is_none()
        && facts.counts.cache_write.is_none()
        && facts.counts.cache_write_long.is_none()
}

/// The equivalent cost of one call, by the first rule that applies, in the
/// order of [`Rule`]. Deterministic: the same facts and the same list give
/// the same figure, whoever asks and whenever.
pub fn equivalent_cost(facts: &CallFacts<'_>, list: &PriceList) -> Priced {
    let unpriced = |rule: Rule| Priced {
        cost_micros: None,
        rule,
        prices: PriceMicros::default(),
        priced_as: None,
    };
    let flat = |micros: i64, rule: Rule| Priced {
        cost_micros: Some(micros),
        rule,
        prices: PriceMicros::default(),
        priced_as: None,
    };
    if list.is_not_a_model(facts.cli) {
        return flat(0, Rule::NotAModelCall);
    }
    let refused = matches!(
        facts.error_type,
        Some("quota_exhausted" | "exhausted" | "spawn_failed")
    );
    if refused && nothing_read(facts) && facts.declared_cost.is_none() {
        return flat(0, Rule::AnsweredNothing);
    }
    let engine = list.engine(facts.cli);
    let has_tokens = facts.counts.input.is_some() || facts.counts.output.is_some();
    let named = facts.model.map(str::trim).filter(|name| !name.is_empty());
    let whole_call = !matches!(facts.models_named, Some(named) if named > 1);
    if has_tokens {
        if let Some(entry) = named.and_then(|name| list.find(name)) {
            if whole_call || facts.declared_cost.is_none() {
                let prices = entry.micros();
                if let Some(micros) = cost_micros(facts.counts, prices) {
                    return Priced {
                        cost_micros: Some(micros),
                        rule: if whole_call {
                            Rule::EngineModel
                        } else {
                            Rule::MixedModelsPricedAsFirst
                        },
                        prices,
                        priced_as: Some(entry.id.clone()),
                    };
                }
            } else {
                return unpriced(Rule::Unpriced);
            }
        }
        // A name the list does not carry wants an entry, not a flat figure:
        // pricing it as unread would hide exactly the repair it asks for.
        if named.is_some() {
            return unpriced(Rule::Unpriced);
        }
        {
            if let Some(entry) = engine
                .and_then(|rules| rules.assumed_model.as_deref())
                .and_then(|name| list.find(name))
            {
                let prices = entry.micros();
                if let Some(micros) = cost_micros(facts.counts, prices) {
                    return Priced {
                        cost_micros: Some(micros),
                        rule: Rule::AssumedModel,
                        prices,
                        priced_as: Some(entry.id.clone()),
                    };
                }
            }
        }
    }
    if let Some(rate) = engine.and_then(|rules| rules.per_wall_second_micros) {
        if let Some(ended) = facts.ended_at {
            let seconds = (ended - facts.started_at).max(0);
            return flat(seconds.saturating_mul(rate), Rule::WallSeconds);
        }
    }
    if let Some(micros) = engine.and_then(|rules| rules.unread_call_equivalent_micros) {
        return flat(micros, Rule::UnreadCallEquivalent);
    }
    unpriced(Rule::Unpriced)
}

#[cfg(test)]
mod equivalent {
    use super::*;

    const LIST: &str = r#"{
      "currency": "USD",
      "models": [
        {"id": "model-a", "aliases": ["a"], "input_per_million": 1.0, "output_per_million": 10.0, "cached_per_million": 0.1},
        {"id": "model-b", "input_per_million": 2.0, "output_per_million": 20.0}
      ],
      "engines": {
        "engine-a": {"assumed_model": "model-b", "unread_call_equivalent_micros": 7000},
        "local-a": {"per_wall_second_micros": 14},
        "engine-c": {}
      },
      "not_models": ["hammer"]
    }"#;

    fn list() -> PriceList {
        PriceList::parse(LIST).unwrap()
    }

    fn tokens(input: u64, output: u64) -> TokenCounts {
        TokenCounts {
            input: Some(input),
            output: Some(output),
            ..TokenCounts::default()
        }
    }

    #[test]
    fn a_named_and_listed_model_is_priced_by_its_own_entry() {
        let priced = equivalent_cost(
            &CallFacts {
                cli: "engine-a",
                model: Some("a"),
                counts: tokens(1_000_000, 100_000),
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!(priced.rule, Rule::EngineModel);
        assert_eq!(priced.cost_micros, Some(2_000_000));
        assert_eq!(priced.priced_as.as_deref(), Some("model-a"));
    }

    #[test]
    fn tokens_without_a_model_are_priced_at_the_engines_assumed_model() {
        let priced = equivalent_cost(
            &CallFacts {
                cli: "engine-a",
                model: None,
                counts: tokens(1_000_000, 0),
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!(priced.rule, Rule::AssumedModel);
        assert_eq!(priced.cost_micros, Some(2_000_000));
        assert_eq!(priced.priced_as.as_deref(), Some("model-b"));
    }

    #[test]
    fn a_local_engine_is_priced_by_the_seconds_it_ran() {
        let priced = equivalent_cost(
            &CallFacts {
                cli: "local-a",
                counts: tokens(3839, 3700),
                started_at: 100,
                ended_at: Some(355),
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!(priced.rule, Rule::WallSeconds);
        assert_eq!(priced.cost_micros, Some(255 * 14));
    }

    #[test]
    fn a_call_that_left_no_reading_takes_the_engines_pinned_figure() {
        let priced = equivalent_cost(
            &CallFacts {
                cli: "engine-a",
                started_at: 1,
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!(priced.rule, Rule::UnreadCallEquivalent);
        assert_eq!(priced.cost_micros, Some(7000));
    }

    #[test]
    fn a_refusal_that_answered_nothing_costs_nothing_and_a_tool_costs_nothing() {
        let refused = equivalent_cost(
            &CallFacts {
                cli: "engine-a",
                error_type: Some("quota_exhausted"),
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!((refused.rule, refused.cost_micros), (Rule::AnsweredNothing, Some(0)));
        let tool = equivalent_cost(
            &CallFacts {
                cli: "Hammer",
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!((tool.rule, tool.cost_micros), (Rule::NotAModelCall, Some(0)));
    }

    #[test]
    fn mixed_models_stay_unknown_beside_a_declared_figure_and_become_a_floor_without_one() {
        let facts = CallFacts {
            cli: "engine-a",
            model: Some("model-a"),
            counts: tokens(1_000_000, 0),
            models_named: Some(2),
            declared_cost: Some(3.0),
            ..CallFacts::default()
        };
        assert_eq!(equivalent_cost(&facts, &list()).rule, Rule::Unpriced);
        let without = CallFacts {
            declared_cost: None,
            ..facts
        };
        let priced = equivalent_cost(&without, &list());
        assert_eq!(priced.rule, Rule::MixedModelsPricedAsFirst);
        assert_eq!(priced.cost_micros, Some(1_000_000));
    }

    #[test]
    fn a_named_model_the_list_does_not_carry_stays_unpriced_instead_of_taking_a_flat_figure() {
        let priced = equivalent_cost(
            &CallFacts {
                cli: "engine-a",
                model: Some("model-nobody-listed"),
                counts: tokens(10, 10),
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!((priced.rule, priced.cost_micros), (Rule::Unpriced, None));
    }

    #[test]
    fn an_engine_with_no_rule_that_fits_stays_unpriced() {
        let priced = equivalent_cost(
            &CallFacts {
                cli: "engine-c",
                counts: tokens(10, 10),
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!((priced.rule, priced.cost_micros), (Rule::Unpriced, None));
        let unknown = equivalent_cost(
            &CallFacts {
                cli: "nobody",
                ..CallFacts::default()
            },
            &list(),
        );
        assert_eq!(unknown.rule, Rule::Unpriced);
    }

    #[test]
    fn the_home_list_replaces_an_engines_rules_whole_and_adds_tools() {
        let home = PriceList::parse(r#"{"currency":"USD","engines":{"engine-a":{"unread_call_equivalent_micros":1}},"not_models":["saw","hammer"]}"#).unwrap();
        let merged = list().overridden_by(home);
        assert_eq!(merged.engine("engine-a").unwrap().assumed_model, None);
        assert_eq!(merged.engine("local-a").unwrap().per_wall_second_micros, Some(14));
        assert_eq!(merged.not_models, vec!["hammer", "saw"]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{
      "currency": "USD",
      "dated": "2026-08-29",
      "models": [
        {
          "id": "claude-sonnet-5",
          "aliases": ["sonnet", "anthropic/claude-sonnet-5"],
          "input_per_million": 3.0,
          "output_per_million": 15.0,
          "cached_per_million": 0.3
        },
        { "id": "no-prices" },
        { "id": "input-only", "input_per_million": "1.5" }
      ]
    }"#;

    #[test]
    fn reads_the_currency_the_date_and_every_entry() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert_eq!(prices.currency, "USD");
        assert_eq!(prices.dated.as_deref(), Some("2026-08-29"));
        assert_eq!(prices.entries.len(), 3);
    }

    #[test]
    fn finds_a_model_by_its_id_and_by_every_alias() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert_eq!(
            prices.find("claude-sonnet-5").unwrap().id,
            "claude-sonnet-5"
        );
        assert_eq!(prices.find("sonnet").unwrap().id, "claude-sonnet-5");
        assert_eq!(
            prices.find("  ANTHROPIC/Claude-Sonnet-5 ").unwrap().id,
            "claude-sonnet-5"
        );
    }

    /// THE ARM THAT COUNTS: a name that merely resembles is not the same name.
    /// If a prefix passed here, one model would pay another's price with
    /// nobody seeing it on the row.
    #[test]
    fn a_name_that_merely_resembles_one_is_not_found() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert!(prices.find("claude-sonnet").is_none());
        assert!(prices.find("claude-sonnet-5-20260101").is_none());
        assert!(prices.find("").is_none());
    }

    #[test]
    fn a_missing_price_is_none_never_zero() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let entry = prices.find("no-prices").unwrap();
        assert_eq!(entry.input_per_million, None);
        assert_eq!(entry.output_per_million, None);
        assert_eq!(entry.cached_per_million, None);
    }

    #[test]
    fn a_price_written_as_text_is_read_as_a_number() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        assert_eq!(
            prices.find("input-only").unwrap().input_per_million,
            Some(1.5)
        );
    }

    #[test]
    fn a_negative_price_is_refused_not_summed() {
        let prices =
            PriceList::parse(r#"{"models":[{"id":"x","input_per_million":-2.0}]}"#).unwrap();
        assert_eq!(prices.find("x").unwrap().input_per_million, None);
    }

    /// The counts almost every test here uses. The **written** cache has tests
    /// of its own further down, because it is the entry that used to be
    /// forgotten.
    fn counts(input: Option<u64>, output: Option<u64>, cached: Option<u64>) -> TokenCounts {
        TokenCounts {
            input,
            output,
            cached,
            ..TokenCounts::default()
        }
    }

    #[test]
    fn computes_the_cost_in_integer_micros_with_the_cache_priced_apart() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        // 1M input at $3 + 1M output at $15 + 1M cache at $0.30 = $18.30
        let cost = cost_micros(
            counts(Some(1_000_000), Some(1_000_000), Some(1_000_000)),
            prices,
        );
        assert_eq!(cost, Some(18_300_000));
    }

    /// THE ARM THAT COUNTS for criterion 3 of the mandate: were the cache
    /// counted at the input price, this number would be ten times bigger. A
    /// single input number would hide exactly that difference.
    #[test]
    fn cached_tokens_are_priced_at_their_own_rate_not_at_the_input_rate() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        let with_cache = cost_micros(counts(Some(0), Some(0), Some(1_000_000)), prices).unwrap();
        let as_if_input = cost_micros(counts(Some(1_000_000), Some(0), None), prices).unwrap();
        assert_eq!(with_cache, 300_000, "1M of cache at $0.30");
        assert_eq!(as_if_input, 3_000_000, "1M of input at $3");
        assert!(
            with_cache * 5 < as_if_input,
            "the difference is an order of magnitude"
        );
    }

    #[test]
    fn a_known_count_without_its_price_leaves_the_cost_unknown() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("input-only").unwrap().micros();
        assert_eq!(
            cost_micros(counts(Some(100), Some(100), None), prices),
            None,
            "the output is measured but has no price: the total would be an underestimate"
        );
    }

    #[test]
    fn an_unknown_cached_count_is_not_treated_as_zero_cache() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        // Not knowing how much cache it read does not stop the rest from being
        // counted, but it adds nothing: the cost is that of the two known
        // sides.
        assert_eq!(
            cost_micros(counts(Some(1_000_000), Some(1_000_000), None), prices),
            Some(18_000_000)
        );
    }

    #[test]
    fn without_any_token_count_there_is_no_cost_not_a_zero() {
        let prices = PriceList::parse(SAMPLE).unwrap();
        let prices = prices.find("sonnet").unwrap().micros();
        assert_eq!(cost_micros(counts(None, None, Some(10)), prices), None);
        assert_eq!(cost_micros(counts(None, None, None), prices), None);
    }

    #[test]
    fn rounds_to_the_nearest_micro_unit_once() {
        // 1 token at $1.50 per million = 1.5 micro-units → 2, not 1 nor 1.5.
        let prices = PriceMicros {
            input: Some(micros_per_million(1.5)),
            output: Some(0),
            ..PriceMicros::default()
        };
        assert_eq!(cost_micros(counts(Some(1), Some(0), None), prices), Some(2));
    }

    #[test]
    fn a_price_list_that_is_not_json_is_an_error_not_an_empty_list() {
        assert!(PriceList::parse("not json").is_err());
    }

    #[test]
    fn a_malformed_entry_is_dropped_without_taking_the_others_with_it() {
        let prices = PriceList::parse(r#"{"models":[{"no_id":true},{"id":"good"}]}"#).unwrap();
        assert_eq!(prices.entries.len(), 1);
        assert!(prices.find("good").is_some());
    }

    /// A really free model has `0.0` declared, which differs from a missing
    /// price: its cost is computed and comes out zero. The distinction is in
    /// the reader, not in the file — hence a list written here rather than the
    /// shipped one, which has no free model and must not gain a fake one just
    /// to make a test pass.
    #[test]
    fn a_declared_zero_is_a_measure_and_a_missing_price_is_not() {
        let prices = PriceList::parse(
            r#"{"models":[{"id":"free","input_per_million":0.0,"output_per_million":0.0}]}"#,
        )
        .unwrap();
        let a_million_each_side = TokenCounts {
            input: Some(1_000_000),
            output: Some(1_000_000),
            ..TokenCounts::default()
        };
        assert_eq!(
            cost_micros(a_million_each_side, prices.find("free").unwrap().micros()),
            Some(0),
            "a declared zero is a measure; an invented zero is not"
        );
        assert_eq!(
            cost_micros(
                a_million_each_side,
                PriceList::default()
                    .find("free")
                    .map(Price::micros)
                    .unwrap_or_default()
            ),
            None,
            "a model the list does not know does not cost zero: it is unknown"
        );
    }
}

/// **THE SAME DISEASE AS FAULT 18**, worth seeing once: Sailor had the datum in
/// its own home and did not use it. The list was there and did not travel with
/// the product; the toolkit was there and did not reach the engines. While a
/// datum lives only in the home of whoever runs it, the product is using the
/// neighbour's home.
#[cfg(test)]
mod the_shipped_price_list {
    use super::*;

    /// The price list shipped with the product must be readable by this code.
    /// [`shipped`] panics when it is not, so this test is where that panic
    /// comes to light before leaving the repository.
    #[test]
    fn the_shipped_price_list_is_readable() {
        let prices = shipped();
        assert_eq!(prices.currency, "USD");
        assert!(
            prices.dated.is_some(),
            "a list with no date does not tell its reader how old it is"
        );
    }

    /// **THE TEST FOR FAULT 35, WORTH MORE THAN EVERYTHING ELSE IN HERE.** A
    /// freshly installed machine has no `~/.config/sailor/pricing.json`. When
    /// that was the only list, `cost_micros` was `None` for every call, every
    /// run looked free, and a flow with a spend cap ran to the end without the
    /// cap ever firing. **Empty `models` in `pricing.default.json` and this
    /// goes red**: the original defect, put back exactly where it was.
    #[test]
    fn the_shipped_list_alone_prices_the_engine_used_most() {
        let prices = shipped();
        let entry = prices
            .find("claude-opus-5")
            .expect("the shipped list knows the model Sailor works with");
        let a_thousand_each_side = TokenCounts {
            input: Some(1_000),
            output: Some(1_000),
            cached: Some(1_000),
            ..TokenCounts::default()
        };
        assert!(
            cost_micros(a_thousand_each_side, entry.micros()).is_some(),
            "without the shipped list every call's cost stays unknown"
        );
    }

    /// **THE ARM THAT TIES THE SHIPPED LIST TO A REAL MEASURE.** A measured
    /// call declared $0.128541 on **2** input tokens and **4** output, the rest
    /// 9,922 read from the cache and **12,347 written** long-lived. The shipped
    /// list redoes that sum to the micro-unit; one wrong price of five would
    /// still give a plausible, false number. The name is the engine's own: lose
    /// that alias and the cost goes unknown, no price having changed.
    #[test]
    fn the_shipped_list_reproduces_a_real_call_to_the_micro_unit() {
        let prices = shipped();
        let entry = prices
            .find("claude-opus-5[1m]")
            .expect("the shipped list knows the name the engine declares");
        let measured = TokenCounts {
            input: Some(2),
            output: Some(4),
            cached: Some(9_922),
            cache_write: None,
            cache_write_long: Some(12_347),
        };
        assert_eq!(
            cost_micros(measured, entry.micros()),
            Some(128_541),
            "the sum on the shipped list and the engine's own must agree"
        );
    }

    /// **A NAME AN ENGINE WRITES IS NOT THE NAME THE LIST CARRIES.** See fault
    /// 104. An entry and not an alias of the version beside it: the two read
    /// the cache at different rates, and a wrong price is worse than none.
    #[test]
    fn the_shipped_list_prices_the_name_the_engine_writes_and_the_providers_one() {
        let prices = shipped();
        for name in ["claude-fable-5-1", "anthropic/claude-fable-5.1"] {
            assert_eq!(prices.knows(name), Known::Priced, "{name}");
        }
        assert_ne!(
            prices
                .find("claude-fable-5-1")
                .and_then(|entry| entry.cached_per_million),
            prices
                .find("claude-fable-5")
                .and_then(|entry| entry.cached_per_million),
            "two tariffs under one entry would be one of them charged wrong"
        );
    }

    /// Every shipped entry has the two prices a sum needs. A half-filled entry
    /// in the home list is a choice of whoever writes it; in the shipped list
    /// it would be an unknown cost nobody decided on, found out only by
    /// looking at a spend that does not add up.
    #[test]
    fn every_shipped_entry_can_actually_price_a_call() {
        for entry in &shipped().entries {
            assert_eq!(
                shipped().knows(&entry.id),
                Known::Priced,
                "the shipped entry «{}» has no prices to make a sum with",
                entry.id
            );
        }
    }

    /// Names do not repeat, neither among `id`s nor among aliases.
    /// [`PriceList::find`] takes the first entry that answers: a name declared
    /// twice would make one model pay another's price, and the row in the
    /// store would look right.
    #[test]
    fn no_shipped_name_is_declared_twice() {
        let mut seen: Vec<String> = Vec::new();
        for entry in &shipped().entries {
            for name in std::iter::once(&entry.id).chain(entry.aliases.iter()) {
                let name = name.trim().to_lowercase();
                assert!(!seen.contains(&name), "the name «{name}» is declared twice");
                seen.push(name);
            }
        }
    }

    /// **THE ARM WORTH MORE THAN ANY OTHER, AND IT COMES FROM A REAL MEASURE.**
    /// A call declared $0.128541 with **2** input tokens and **4** output; the
    /// rest was 9,922 read from the cache and **12,347 written** to a
    /// long-lived one. This test redoes that sum: leave the cache write out of
    /// it and the cost falls to a fortieth, and nobody notices because the
    /// number is there and looks plausible.
    #[test]
    fn writing_the_cache_is_what_that_call_actually_cost() {
        let opus = PriceList::parse(
            r#"{"currency":"USD","models":[{
                 "id":"claude-opus-5",
                 "input_per_million":5.0,
                 "output_per_million":25.0,
                 "cached_per_million":0.5,
                 "cache_write_long_per_million":10.0
               }]}"#,
        )
        .unwrap();
        let prices = opus.find("claude-opus-5").unwrap().micros();
        let measured = TokenCounts {
            input: Some(2),
            output: Some(4),
            cached: Some(9_922),
            cache_write: None,
            cache_write_long: Some(12_347),
        };

        let cost = cost_micros(measured, prices).expect("the sum can be made");
        // The engine had declared $0.128541: here 128,541 micro-units come
        // out, the same figure down to the micro.
        assert_eq!(cost, 128_541, "our sum matches the engine's");

        let without_the_write = cost_micros(
            TokenCounts {
                cache_write_long: None,
                ..measured
            },
            prices,
        )
        .unwrap();
        // 2 input tokens + 4 output + 9,922 read from the cache = 5,071
        // micro-units, i.e. half a cent instead of thirteen.
        assert_eq!(without_the_write, 5_071);
        assert!(
            cost > without_the_write * 25,
            "forgetting the written cache is off by more than twenty-five times"
        );
    }

    /// A cache-write count with no price for it leaves the cost **unknown**
    /// instead of counting those tokens as free. It is the same rule as the
    /// other sides, and worth testing on the new one: that is where an old
    /// list does not have the entry yet.
    #[test]
    fn a_cache_write_without_its_price_leaves_the_cost_unknown() {
        let without_write_price = PriceList::parse(
            r#"{"models":[{"id":"x","input_per_million":5.0,"output_per_million":25.0}]}"#,
        )
        .unwrap();
        let prices = without_write_price.find("x").unwrap().micros();
        assert_eq!(
            cost_micros(
                TokenCounts {
                    input: Some(10),
                    output: Some(10),
                    cache_write: Some(10_000),
                    ..TokenCounts::default()
                },
                prices
            ),
            None,
            "10,000 tokens written with no price: the total would be a mute underestimate"
        );
    }
}

/// The home list on top of the shipped one, with the descriptors' discipline.
#[cfg(test)]
mod the_home_list_wins {
    use super::*;

    const SHIPPED: &str = r#"{
      "currency": "USD",
      "dated": "2026-01-01",
      "models": [
        { "id": "one", "aliases": ["first"], "input_per_million": 5.0, "output_per_million": 25.0 },
        { "id": "two", "input_per_million": 1.0, "output_per_million": 2.0 }
      ]
    }"#;

    fn shipped_sample() -> PriceList {
        PriceList::parse(SHIPPED).unwrap()
    }

    /// What only the shipped list declares keeps answering: the home file adds
    /// and corrects, it does not wipe.
    #[test]
    fn what_only_the_shipped_list_declares_survives_the_override() {
        let home = PriceList::parse(r#"{"currency":"USD","models":[{"id":"one","input_per_million":9.0,"output_per_million":9.0}]}"#).unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.find("two").unwrap().input_per_million, Some(1.0));
    }

    /// **AN ENTRY IS REPLACED WHOLE, ALIASES INCLUDED.** The home price wins,
    /// and the alias the shipped list declared disappears with the entry that
    /// carried it: rewriting `one` without `first` says that name is no longer
    /// yours.
    #[test]
    fn an_id_already_there_is_replaced_whole_not_merged_field_by_field() {
        let home = PriceList::parse(
            r#"{"currency":"USD","models":[{"id":"one","input_per_million":9.0,"output_per_million":9.0}]}"#,
        )
        .unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.find("one").unwrap().input_per_million, Some(9.0));
        assert_eq!(
            merged.find("first"),
            None,
            "the alias of the replaced entry went away with it"
        );
    }

    /// A home alias colliding with a shipped entry's `id` must win, and the
    /// order is not cosmetic: [`PriceList::find`] takes the first that answers,
    /// home entries are placed first, and without that the user would have
    /// rewritten a name and gone on being served by the other entry.
    #[test]
    fn a_home_alias_beats_a_shipped_id_with_the_same_name() {
        let home = PriceList::parse(
            r#"{"currency":"USD","models":[{"id":"mine","aliases":["two"],"input_per_million":7.0,"output_per_million":7.0}]}"#,
        )
        .unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.find("two").unwrap().id, "mine");
    }

    /// **THE ARM THAT COUNTS: TWO CURRENCIES DO NOT MIX.** A home list in
    /// euros must not drag the shipped dollar entries along, or they would be
    /// summed in the same column and the total would still come out —
    /// plausible and meaningless. Remove the currency check in
    /// `overridden_by` and `two` answers again: that is precisely the defect.
    #[test]
    fn a_home_list_in_another_currency_stands_alone() {
        let home = PriceList::parse(
            r#"{"currency":"EUR","models":[{"id":"one","input_per_million":4.0,"output_per_million":4.0}]}"#,
        )
        .unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert_eq!(merged.currency, "EUR");
        assert_eq!(merged.find("one").unwrap().input_per_million, Some(4.0));
        assert_eq!(
            merged.find("two"),
            None,
            "a dollar entry got into a list in euros"
        );
    }

    /// The same currency written differently is still the same currency: the
    /// tolerance is the one `find` already applies to model names.
    #[test]
    fn the_same_currency_written_differently_still_merges() {
        let home = PriceList::parse(r#"{"currency":" usd ","models":[{"id":"three"}]}"#).unwrap();
        let merged = shipped_sample().overridden_by(home);
        assert!(merged.find("two").is_some());
    }

    /// The date belongs to whoever wrote last, and does not vanish when the
    /// home file declares none: whoever looks at a figure wants to know how
    /// old the prices behind it are.
    #[test]
    fn the_date_comes_from_whoever_wrote_last_and_never_vanishes() {
        let dated =
            PriceList::parse(r#"{"currency":"USD","dated":"2026-09-01","models":[]}"#).unwrap();
        assert_eq!(
            shipped_sample().overridden_by(dated).dated.as_deref(),
            Some("2026-09-01")
        );
        let undated = PriceList::parse(r#"{"currency":"USD","models":[]}"#).unwrap();
        assert_eq!(
            shipped_sample().overridden_by(undated).dated.as_deref(),
            Some("2026-01-01")
        );
    }
}

/// Whoever has no price for a model must **know** it rather than discover it
/// as a zero: the three answers of [`PriceList::knows`].
#[cfg(test)]
mod knowing_what_is_not_priced {
    use super::*;

    const LIST: &str = r#"{
      "currency": "USD",
      "models": [
        { "id": "whole", "aliases": ["shorthand"], "input_per_million": 5.0, "output_per_million": 25.0 },
        { "id": "half-done", "input_per_million": 5.0 }
      ]
    }"#;

    #[test]
    fn a_fully_priced_entry_is_priced_by_its_id_and_by_its_alias() {
        let prices = PriceList::parse(LIST).unwrap();
        assert_eq!(prices.knows("whole"), Known::Priced);
        assert_eq!(prices.knows("shorthand"), Known::Priced);
    }

    /// **THE TWO WAYS OF HAVING NO PRICE DO NOT GET CONFUSED**, because they
    /// are two different repairs: one is fixed by adding an entry, the other
    /// by writing the prices into the entry already there. Make the second
    /// case answer `Absent` too and the reader goes off to rewrite an entry
    /// that exists.
    #[test]
    fn an_entry_without_prices_is_not_the_same_as_a_name_nobody_declared() {
        let prices = PriceList::parse(LIST).unwrap();
        assert_eq!(prices.knows("half-done"), Known::ListedWithoutPrice);
        assert_eq!(prices.knows("never-seen"), Known::Absent);
        // And the cost stays unknown in both cases: that is why the difference
        // cannot be seen by looking at the number.
        let counts = TokenCounts {
            input: Some(10),
            output: Some(10),
            ..TokenCounts::default()
        };
        assert_eq!(
            cost_micros(counts, prices.find("half-done").unwrap().micros()),
            None
        );
    }
}
