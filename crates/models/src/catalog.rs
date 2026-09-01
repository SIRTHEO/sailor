//! The OpenRouter model catalog: from the JSON of
//! `https://openrouter.ai/api/v1/models` to a filterable list.
//!
//! Everything here is pure: it takes a JSON string already in hand and gives
//! back values. Downloading lives in `fetch.rs`, on purpose — the tests here
//! run on a slice of catalog saved in the crate, never over the network.

use std::fmt;

/// An input kind a model accepts. The catalog lists other values too (e.g.
/// `"file"`): the ones we do not recognise are dropped silently rather than
/// failing the parse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
}

impl Modality {
    fn parse(raw: &str) -> Option<Modality> {
        match raw {
            "text" => Some(Modality::Text),
            "image" => Some(Modality::Image),
            "audio" => Some(Modality::Audio),
            "video" => Some(Modality::Video),
            _ => None,
        }
    }
}

impl fmt::Display for Modality {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Modality::Text => "text",
            Modality::Image => "image",
            Modality::Audio => "audio",
            Modality::Video => "video",
        };
        f.write_str(s)
    }
}

/// A catalog model, with only the fields Sailor needs.
///
/// `price_per_million_*` is `None` when the catalog carries no readable price
/// — never `0.0` for a merely missing value: `0.0` stays reserved for models
/// that really declare a zero price (the free ones). Same for
/// `context_length`.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
    /// True when the id ends in `:free`: the marker OpenRouter uses for
    /// zero-cost models, and the criterion the mandate names.
    pub free: bool,
    pub context_length: Option<u64>,
    pub input_modalities: Vec<Modality>,
    pub price_per_million_input: Option<f64>,
    pub price_per_million_output: Option<f64>,
}

impl Model {
    pub fn accepts(&self, m: Modality) -> bool {
        self.input_modalities.contains(&m)
    }
}

/// The whole catalog, already filterable.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub models: Vec<Model>,
}

/// A USD-per-token price from the catalog (a string such as `"0.00000015"`)
/// turned into USD per million tokens. `None` when the field is missing or is
/// not a readable number.
fn price_per_million(value: Option<&serde_json::Value>) -> Option<f64> {
    let raw = value?.as_str()?;
    let per_token: f64 = raw.parse().ok()?;
    Some(per_token * 1_000_000.0)
}

impl Catalog {
    /// Reads the JSON body returned by `GET /api/v1/models`. One malformed
    /// model does not take the whole catalog down: only that one is dropped.
    pub fn parse(body: &str) -> Result<Catalog, String> {
        let parsed: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("invalid JSON: {e}"))?;
        let entries = parsed
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| "the \"data\" field (the list of models) is missing".to_string())?;

        let models = entries
            .iter()
            .filter_map(|entry| {
                let id = entry.get("id")?.as_str()?.to_string();
                let name = entry
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or(&id)
                    .to_string();
                let context_length = entry.get("context_length").and_then(|c| c.as_u64());
                let input_modalities = entry
                    .get("architecture")
                    .and_then(|a| a.get("input_modalities"))
                    .and_then(|m| m.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .filter_map(Modality::parse)
                            .collect()
                    })
                    .unwrap_or_default();
                let pricing = entry.get("pricing");
                let price_per_million_input =
                    price_per_million(pricing.and_then(|p| p.get("prompt")));
                let price_per_million_output =
                    price_per_million(pricing.and_then(|p| p.get("completion")));
                let free = id.ends_with(":free");
                Some(Model {
                    id,
                    name,
                    free,
                    context_length,
                    input_modalities,
                    price_per_million_input,
                    price_per_million_output,
                })
            })
            .collect();
        Ok(Catalog { models })
    }

    pub fn find(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }

    pub fn filter<'a>(&'a self, f: &Filter) -> Vec<&'a Model> {
        self.models.iter().filter(|m| f.matches(m)).collect()
    }
}

/// The filter the mandate asks for: free/paid, input kind, minimum window.
/// Every field is optional and they combine with AND.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub free_only: bool,
    pub paid_only: bool,
    pub modality: Option<Modality>,
    pub min_context: Option<u64>,
}

impl Filter {
    pub fn matches(&self, m: &Model) -> bool {
        if self.free_only && !m.free {
            return false;
        }
        if self.paid_only && m.free {
            return false;
        }
        if let Some(modality) = self.modality {
            if !m.accepts(modality) {
                return false;
            }
        }
        if let Some(min) = self.min_context {
            match m.context_length {
                Some(len) if len >= min => {}
                _ => return false,
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../tests/fixtures/catalog-sample.json");

    #[test]
    fn parses_the_whole_sample_catalog() {
        let catalog = Catalog::parse(SAMPLE).expect("the sample must parse");
        // 17 free + 5 paid, captured live from OpenRouter and not hand-written:
        // 22 is a fact about the real catalog, not a sample size someone chose.
        assert_eq!(catalog.models.len(), 22);
    }

    #[test]
    fn counts_exactly_seventeen_free_models() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let free = catalog.models.iter().filter(|m| m.free).count();
        assert_eq!(free, 17);
    }

    #[test]
    fn free_detection_matches_the_id_suffix_not_a_guess() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let paid_with_free_id = catalog
            .models
            .iter()
            .find(|m| m.id == "qwen/qwen3.8-flash")
            .unwrap();
        assert!(!paid_with_free_id.free);
        let free = catalog
            .models
            .iter()
            .find(|m| m.id == "z-ai/glm-5.2:free")
            .unwrap();
        assert!(free.free);
    }

    #[test]
    fn reads_context_length_and_modalities_of_a_multimodal_free_model() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let inkling = catalog.find("thinkingmachines/inkling:free").unwrap();
        assert_eq!(inkling.context_length, Some(1_048_576));
        assert!(inkling.accepts(Modality::Text));
        assert!(inkling.accepts(Modality::Image));
        assert!(inkling.accepts(Modality::Audio));
        assert!(!inkling.accepts(Modality::Video));
    }

    #[test]
    fn unknown_modality_strings_are_dropped_not_fatal() {
        // "meta/muse-spark-1.2-contributor" also declares "file", which is not
        // one of the four kinds we track: it must simply disappear.
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let m = catalog.find("meta/muse-spark-1.2-contributor").unwrap();
        assert!(m.accepts(Modality::Audio));
        assert_eq!(m.input_modalities.len(), 4); // text, image, video, audio: "file" excluded
    }

    #[test]
    fn converts_price_per_token_to_price_per_million() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let qwen = catalog.find("qwen/qwen3.8-flash").unwrap();
        // 0.00000015 USD/token * 1_000_000 = 0.15 USD/million, with room for
        // floating-point rounding.
        let input = qwen.price_per_million_input.unwrap();
        assert!((input - 0.15).abs() < 1e-9, "expected ~0.15, got {input}");
        let output = qwen.price_per_million_output.unwrap();
        assert!((output - 0.47).abs() < 1e-9, "expected ~0.47, got {output}");
    }

    #[test]
    fn free_models_declare_a_real_zero_price_not_a_missing_one() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let free = catalog.find("liquid/lfm-2.5-2.6b:free").unwrap();
        assert_eq!(free.price_per_million_input, Some(0.0));
        assert_eq!(free.price_per_million_output, Some(0.0));
    }

    #[test]
    fn a_model_with_no_data_field_is_a_readable_error_not_a_panic() {
        let err = Catalog::parse(r#"{"oops":true}"#).unwrap_err();
        assert!(err.contains("data"));
    }

    #[test]
    fn an_entry_missing_the_id_is_dropped_not_fatal() {
        let body = r#"{"data":[{"name":"no id"},{"id":"a:free","context_length":1000,"architecture":{"input_modalities":["text"]},"pricing":{"prompt":"0","completion":"0"}}]}"#;
        let catalog = Catalog::parse(body).unwrap();
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].id, "a:free");
    }

    // ── the filter ─────────────────────────────────────────────────────

    #[test]
    fn filter_free_only_excludes_paid_models() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter {
            free_only: true,
            ..Default::default()
        };
        let hits = catalog.filter(&f);
        assert_eq!(hits.len(), 17);
        assert!(hits.iter().all(|m| m.free));
    }

    #[test]
    fn filter_paid_only_excludes_free_models() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter {
            paid_only: true,
            ..Default::default()
        };
        let hits = catalog.filter(&f);
        assert_eq!(hits.len(), 5);
        assert!(hits.iter().all(|m| !m.free));
    }

    #[test]
    fn filter_by_modality_keeps_only_models_that_accept_audio() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter {
            modality: Some(Modality::Audio),
            ..Default::default()
        };
        let hits = catalog.filter(&f);
        // Free with audio: inkling-small, inkling, nemotron-3-nano-omni.
        // Paid with audio: meta/muse-spark.
        assert_eq!(hits.len(), 4);
        assert!(hits.iter().all(|m| m.accepts(Modality::Audio)));
    }

    #[test]
    fn filter_by_min_context_excludes_smaller_windows() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter {
            min_context: Some(1_000_000),
            ..Default::default()
        };
        let hits = catalog.filter(&f);
        assert!(hits
            .iter()
            .all(|m| m.context_length.unwrap_or(0) >= 1_000_000));
        assert!(hits.iter().any(|m| m.id == "thinkingmachines/inkling:free"));
        assert!(!hits.iter().any(|m| m.id == "tencent/hy-mt2-1.8b")); // 8192, too small
    }

    #[test]
    fn filters_combine_in_and_not_or() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter {
            free_only: true,
            modality: Some(Modality::Video),
            min_context: Some(1_000_000),
            ..Default::default()
        };
        let hits = catalog.filter(&f);
        // Only minimax/minimax-m3:free satisfies all three at once.
        assert_eq!(
            hits.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["minimax/minimax-m3:free"]
        );
    }
}
