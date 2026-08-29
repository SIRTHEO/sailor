//! Il conteggio esatto di token e contesto, dalla risposta di un motore.
//!
//! `None` è una risposta legittima — un numero che quel motore non dice —
//! `0` non lo è mai per un campo mancante: qui non si inventa nulla.

use crate::catalog::Model;

/// I token misurati per una singola chiamata. `total_tokens` può essere
/// noto anche quando `prompt_tokens`/`completion_tokens` non lo sono (è il
/// caso di Codex, che sull'uscita dice solo il totale).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TokenUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    /// Il costo in USD, quando il motore stesso lo dichiara (OpenRouter lo
    /// fa per ogni risposta, anche a zero sui modelli gratuiti).
    pub cost_usd: Option<f64>,
}

impl TokenUsage {
    /// Dal corpo JSON di una risposta `chat/completions` di OpenRouter.
    /// Un campo mancante o non numerico resta `None`, non fa fallire tutto
    /// il parsing: un corpo di errore (429, chiave non valida, ecc.) deve
    /// poter tornare un `TokenUsage` vuoto invece di un panico.
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

    /// Dall'uscita testuale di `codex exec`: riusa `parse_codex_tokens`
    /// invece di riscrivere il parsing (`"tokens used"` seguito dal numero
    /// con il punto come separatore delle migliaia). Codex non separa
    /// prompt e completamento, e non dichiara un costo: qui restano `None`.
    pub fn from_codex_output(output: &str) -> TokenUsage {
        let raw = parse_codex_tokens(output);
        TokenUsage { total_tokens: raw.parse().ok(), ..TokenUsage::default() }
    }
}

/// Il quadro completo di una chiamata: quanto è entrato, quanto è uscito,
/// quanto resta della finestra del modello, quanto è costata.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ContextAccounting {
    pub usage: TokenUsage,
    /// La finestra del modello usato: `None` se il modello non è nel
    /// catalogo (es. Codex, che non è un modello OpenRouter).
    pub context_length: Option<u64>,
    /// `context_length - total_tokens`. `None` se manca anche solo uno dei
    /// due addendi — un resto calcolato su un totale ignoto sarebbe un
    /// numero inventato con la faccia di una misura.
    pub remaining: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl ContextAccounting {
    /// Combina l'uso misurato con il modello del catalogo che l'ha servito
    /// (se lo si conosce). Se `usage.cost_usd` è già dichiarato dal motore
    /// (OpenRouter lo fa sempre) si usa quello; altrimenti si calcola dal
    /// listino del modello, quando il listino c'è.
    pub fn compute(usage: TokenUsage, model: Option<&Model>) -> ContextAccounting {
        let context_length = model.and_then(|m| m.context_length);
        let remaining = match (context_length, usage.total_tokens) {
            (Some(ctx), Some(total)) => Some(ctx.saturating_sub(total)),
            _ => None,
        };
        let cost_usd = usage.cost_usd.or_else(|| compute_cost(&usage, model));
        ContextAccounting { usage, context_length, remaining, cost_usd }
    }
}

/// Il costo dal listino del modello, quando il motore non l'ha già detto.
/// Serve entrambi i pezzi (prompt e completamento, prezzo e conteggio):
/// manca uno solo, il costo resta sconosciuto.
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

    // Corpo vero, catturato il 27/08/2026 su nvidia/nemotron-3-super-120b-a12b:free
    // (chiave letta da file, mai stampata; qui non ne resta traccia).
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
        let body = r#"{"error":{"code":429,"message":"limite"}}"#;
        let usage = TokenUsage::from_openrouter_body(body);
        assert_eq!(usage, TokenUsage::default());
    }

    #[test]
    fn garbage_input_gives_an_empty_usage_not_a_panic() {
        let usage = TokenUsage::from_openrouter_body("non è json");
        assert_eq!(usage, TokenUsage::default());
    }

    #[test]
    fn codex_output_reuses_the_shared_parser() {
        let output = "roba varia\ntokens used\n13.910\naltra roba";
        let usage = TokenUsage::from_codex_output(output);
        assert_eq!(usage.total_tokens, Some(13910));
        assert_eq!(usage.prompt_tokens, None);
        assert_eq!(usage.completion_tokens, None);
        assert_eq!(usage.cost_usd, None);
    }

    #[test]
    fn codex_output_without_the_marker_is_unknown_not_zero() {
        let usage = TokenUsage::from_codex_output("nessuna riga utile qui");
        assert_eq!(usage.total_tokens, None);
    }

    fn sample_model(id: &str) -> Model {
        let catalog = Catalog::parse(include_str!("../tests/fixtures/catalog-sample.json")).unwrap();
        catalog.find(id).unwrap().clone()
    }

    #[test]
    fn computes_remaining_context_against_the_model_window() {
        let model = sample_model("nvidia/nemotron-3-super-120b-a12b:free"); // 262144
        let usage = TokenUsage { total_tokens: Some(47), ..TokenUsage::default() };
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.context_length, Some(262144));
        assert_eq!(acc.remaining, Some(262144 - 47));
    }

    #[test]
    fn remaining_is_none_when_the_model_is_unknown_codex_case() {
        let usage = TokenUsage { total_tokens: Some(13910), ..TokenUsage::default() };
        let acc = ContextAccounting::compute(usage, None);
        assert_eq!(acc.context_length, None);
        assert_eq!(acc.remaining, None, "un resto su una finestra ignota sarebbe un numero inventato");
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
            cost_usd: Some(0.4242), // valore volutamente diverso dal calcolo, per provare che vince
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.cost_usd, Some(0.4242));
    }

    #[test]
    fn computes_cost_from_the_price_list_when_the_engine_says_nothing() {
        let model = sample_model("qwen/qwen3.8-flash"); // 0.15 / 0.47 USD per milione
        let usage = TokenUsage {
            prompt_tokens: Some(1_000_000),
            completion_tokens: Some(1_000_000),
            ..TokenUsage::default()
        };
        let acc = ContextAccounting::compute(usage, Some(&model));
        let cost = acc.cost_usd.unwrap();
        assert!((cost - 0.62).abs() < 1e-9, "atteso 0.15+0.47=0.62, letto {cost}");
    }

    #[test]
    fn cost_is_unknown_when_token_counts_are_only_partial() {
        let model = sample_model("qwen/qwen3.8-flash");
        let usage = TokenUsage { prompt_tokens: Some(10), ..TokenUsage::default() }; // manca completion
        let acc = ContextAccounting::compute(usage, Some(&model));
        assert_eq!(acc.cost_usd, None);
    }

    #[test]
    fn accepts_modality_check_is_reused_correctly() {
        let model = sample_model("thinkingmachines/inkling:free");
        assert!(model.accepts(Modality::Audio));
    }
}

/// I token dichiarati da `codex exec` nella sua uscita testuale.
///
/// Veniva da `notte`, rimosso dal repo il 29/08/2026. Sta qui perché leggere
/// quanto un motore dichiara di aver consumato è il mestiere di questo crate.
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
