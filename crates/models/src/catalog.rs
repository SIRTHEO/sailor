//! Il catalogo dei modelli OpenRouter: dal JSON di
//! `https://openrouter.ai/api/v1/models` a un elenco filtrabile.
//!
//! Tutto qui è puro: prende una stringa JSON già in mano e restituisce
//! valori. Chi la scarica (`fetch.rs`) sta altrove, apposta — le prove di
//! questo file girano su un pezzo di catalogo salvato nel crate, mai sulla
//! rete.

use std::fmt;

/// Un genere di ingresso che un modello sa accettare. Il catalogo elenca
/// anche altri valori (es. `"file"`): quelli che non riconosciamo si
/// scartano in silenzio, non fanno fallire il parsing.
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

/// Un modello del catalogo, con solo i campi che a Sailor servono.
///
/// `price_per_million_*` è `None` quando il catalogo non riporta un prezzo
/// leggibile — mai `0.0` per un valore semplicemente mancante: `0.0` resta
/// riservato ai modelli che dichiarano davvero un prezzo nullo (i gratuiti).
/// Lo stesso vale per `context_length`.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub id: String,
    pub name: String,
    /// Vero se l'identificatore finisce in `:free`: è il segno che OpenRouter
    /// usa per i modelli a costo zero, il criterio che il mandato indica.
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

/// Il catalogo intero, già filtrabile.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub models: Vec<Model>,
}

/// Un prezzo USD-per-token del catalogo (stringa tipo `"0.00000015"`)
/// convertito in USD per milione di token. `None` se il campo manca o non è
/// un numero leggibile.
fn price_per_million(value: Option<&serde_json::Value>) -> Option<f64> {
    let raw = value?.as_str()?;
    let per_token: f64 = raw.parse().ok()?;
    Some(per_token * 1_000_000.0)
}

impl Catalog {
    /// Legge il corpo JSON restituito da `GET /api/v1/models`. Un modello
    /// singolo malformato non abbatte l'intero catalogo: si scarta lui solo.
    pub fn parse(body: &str) -> Result<Catalog, String> {
        let parsed: serde_json::Value =
            serde_json::from_str(body).map_err(|e| format!("JSON non valido: {e}"))?;
        let entries = parsed
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| "manca il campo \"data\" (elenco dei modelli)".to_string())?;

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

/// Il filtro richiesto dal mandato: gratuito/a pagamento, genere di
/// ingresso, finestra minima. Tutti i campi sono opzionali e si combinano in
/// AND.
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
        let catalog = Catalog::parse(SAMPLE).expect("il campione deve leggersi");
        // 17 gratuiti + 5 a pagamento, misurati dal vivo il 27/08/2026.
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
        // "meta/muse-spark-1.2-contributor" dichiara anche "file", che non è
        // uno dei quattro generi che tracciamo: deve solo sparire.
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let m = catalog.find("meta/muse-spark-1.2-contributor").unwrap();
        assert!(m.accepts(Modality::Audio));
        assert_eq!(m.input_modalities.len(), 4); // text, image, video, audio: "file" escluso
    }

    #[test]
    fn converts_price_per_token_to_price_per_million() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let qwen = catalog.find("qwen/qwen3.8-flash").unwrap();
        // 0.00000015 USD/token * 1_000_000 = 0.15 USD/milione, con un margine
        // per l'arrotondamento in virgola mobile.
        let input = qwen.price_per_million_input.unwrap();
        assert!((input - 0.15).abs() < 1e-9, "atteso ~0.15, letto {input}");
        let output = qwen.price_per_million_output.unwrap();
        assert!((output - 0.47).abs() < 1e-9, "atteso ~0.47, letto {output}");
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
        let body = r#"{"data":[{"name":"senza id"},{"id":"a:free","context_length":1000,"architecture":{"input_modalities":["text"]},"pricing":{"prompt":"0","completion":"0"}}]}"#;
        let catalog = Catalog::parse(body).unwrap();
        assert_eq!(catalog.models.len(), 1);
        assert_eq!(catalog.models[0].id, "a:free");
    }

    // ── filtro ─────────────────────────────────────────────────────────

    #[test]
    fn filter_free_only_excludes_paid_models() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter { free_only: true, ..Default::default() };
        let hits = catalog.filter(&f);
        assert_eq!(hits.len(), 17);
        assert!(hits.iter().all(|m| m.free));
    }

    #[test]
    fn filter_paid_only_excludes_free_models() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter { paid_only: true, ..Default::default() };
        let hits = catalog.filter(&f);
        assert_eq!(hits.len(), 5);
        assert!(hits.iter().all(|m| !m.free));
    }

    #[test]
    fn filter_by_modality_keeps_only_models_that_accept_audio() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter { modality: Some(Modality::Audio), ..Default::default() };
        let hits = catalog.filter(&f);
        // Gratuiti con audio: inkling-small, inkling, nemotron-3-nano-omni.
        // A pagamento con audio: meta/muse-spark.
        assert_eq!(hits.len(), 4);
        assert!(hits.iter().all(|m| m.accepts(Modality::Audio)));
    }

    #[test]
    fn filter_by_min_context_excludes_smaller_windows() {
        let catalog = Catalog::parse(SAMPLE).unwrap();
        let f = Filter { min_context: Some(1_000_000), ..Default::default() };
        let hits = catalog.filter(&f);
        assert!(hits.iter().all(|m| m.context_length.unwrap_or(0) >= 1_000_000));
        assert!(hits.iter().any(|m| m.id == "thinkingmachines/inkling:free"));
        assert!(!hits.iter().any(|m| m.id == "tencent/hy-mt2-1.8b")); // 8192, troppo piccolo
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
        // Solo minimax/minimax-m3:free rispetta tutti e tre insieme.
        assert_eq!(hits.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), vec!["minimax/minimax-m3:free"]);
    }
}
