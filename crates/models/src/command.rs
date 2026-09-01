//! The command, as a module ready to become `sailor models`: list, show the
//! current choice, change it. Everything here takes what it needs already in
//! hand (catalog, configuration) and gives back text — the caller decides
//! where to print it.

use crate::catalog::{Catalog, Filter, Modality, Model};
use crate::config::{self, UserConfig};

/// A command-line input kind (`text`, `image`, `audio`, `video`) into the
/// typed value. An unknown value is a readable error, not an `unwrap` that
/// takes the process down.
pub fn parse_modality_arg(raw: &str) -> Result<Modality, String> {
    match raw {
        "text" => Ok(Modality::Text),
        "image" => Ok(Modality::Image),
        "audio" => Ok(Modality::Audio),
        "video" => Ok(Modality::Video),
        other => Err(format!(
            "unknown input kind: \"{other}\" (use text, image, audio or video)"
        )),
    }
}

fn format_price(p: Option<f64>) -> String {
    match p {
        Some(v) => format!("{v:.4} USD/million"),
        None => "? (unknown)".to_string(),
    }
}

/// One line per model: id, free or paid, context window, accepted input kinds,
/// price per million in and out.
pub fn format_model_line(m: &Model) -> String {
    let kind = if m.free { "free" } else { "paid" };
    let ctx = m
        .context_length
        .map(|c| c.to_string())
        .unwrap_or_else(|| "?".to_string());
    let modalities = if m.input_modalities.is_empty() {
        "?".to_string()
    } else {
        m.input_modalities
            .iter()
            .map(|mo| mo.to_string())
            .collect::<Vec<_>>()
            .join("+")
    };
    format!(
        "{id}  {kind}  context {ctx}  accepts {modalities}  input {price_in}  output {price_out}",
        id = m.id,
        price_in = format_price(m.price_per_million_input),
        price_out = format_price(m.price_per_million_output),
    )
}

/// The filtered list, one line per model. Empty when the filter catches
/// nobody — that is not an error, it is the answer.
pub fn list(catalog: &Catalog, filter: &Filter) -> String {
    let hits = catalog.filter(filter);
    if hits.is_empty() {
        return "no model matches the filter".to_string();
    }
    hits.iter()
        .map(|m| format_model_line(m))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The model actually in use for a kind of work, saying plainly where the
/// choice came from: configured, or the fallback.
pub fn current(catalog: &Catalog, cfg: &UserConfig, kind: &str) -> String {
    let configured = cfg
        .get(kind)
        .and_then(|id| catalog.find(id))
        .filter(|m| m.free);
    match configured {
        Some(m) => format!("{kind}: {} (configured)", format_model_line(m)),
        None => match config::effective_model(catalog, cfg, kind) {
            Some(m) => format!("{kind}: {} (free fallback, not configured)", format_model_line(m)),
            None => format!("{kind}: no free model available in the catalog"),
        },
    }
}

/// Changes the choice for a kind of work. Refuses, writing nothing, when the
/// model is not in the catalog or is not free — see `config::set_choice` for
/// why.
pub fn set(
    cfg: &mut UserConfig,
    catalog: &Catalog,
    kind: &str,
    model_id: &str,
) -> Result<String, String> {
    config::set_choice(cfg, catalog, kind, model_id)?;
    Ok(format!("{kind}: set to {model_id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_catalog() -> Catalog {
        Catalog::parse(include_str!("../tests/fixtures/catalog-sample.json")).unwrap()
    }

    #[test]
    fn parses_the_four_known_modality_args() {
        assert_eq!(parse_modality_arg("text"), Ok(Modality::Text));
        assert_eq!(parse_modality_arg("image"), Ok(Modality::Image));
        assert_eq!(parse_modality_arg("audio"), Ok(Modality::Audio));
        assert_eq!(parse_modality_arg("video"), Ok(Modality::Video));
    }

    #[test]
    fn rejects_an_unknown_modality_arg_with_a_readable_message() {
        let err = parse_modality_arg("smell").unwrap_err();
        assert!(err.contains("smell"));
    }

    #[test]
    fn list_with_no_filter_includes_the_whole_catalog() {
        let catalog = sample_catalog();
        let out = list(&catalog, &Filter::default());
        assert_eq!(out.lines().count(), 22);
    }

    #[test]
    fn list_free_only_shows_only_free_models() {
        let catalog = sample_catalog();
        let out = list(&catalog, &Filter { free_only: true, ..Default::default() });
        assert_eq!(out.lines().count(), 17);
        assert!(out.lines().all(|l| l.contains("  free  context") && !l.contains("  paid  context")));
    }

    #[test]
    fn list_with_no_hits_says_so_instead_of_an_empty_string() {
        let catalog = sample_catalog();
        let out = list(&catalog, &Filter { min_context: Some(999_999_999), ..Default::default() });
        assert_eq!(out, "no model matches the filter");
    }

    #[test]
    fn current_reports_the_configured_free_model() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        config::set_choice(&mut cfg, &catalog, "notte", "z-ai/glm-5.2:free").unwrap();
        let out = current(&catalog, &cfg, "notte");
        assert!(out.contains("z-ai/glm-5.2:free"));
        assert!(out.contains("(configured)"));
    }

    #[test]
    fn current_reports_the_free_fallback_when_unconfigured() {
        let catalog = sample_catalog();
        let cfg = UserConfig::default();
        let out = current(&catalog, &cfg, "never-touched");
        assert!(out.contains(config::DEFAULT_FREE_MODEL));
        assert!(out.contains("not configured"));
    }

    #[test]
    fn set_refuses_a_paid_model_and_leaves_the_config_untouched() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        let err = set(&mut cfg, &catalog, "default", "qwen/qwen3.8-flash").unwrap_err();
        assert!(err.contains("is paid"));
        assert_eq!(cfg.get("default"), None);
    }

    #[test]
    fn set_accepts_a_free_model_and_says_so() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        let msg = set(&mut cfg, &catalog, "default", "z-ai/glm-5.2:free").unwrap();
        assert!(msg.contains("z-ai/glm-5.2:free"));
        assert_eq!(cfg.get("default"), Some("z-ai/glm-5.2:free"));
    }

    #[test]
    fn a_model_line_shows_a_question_mark_for_an_unknown_price_not_a_zero() {
        // Built by hand: a catalog model with no price (never seen so far, but
        // the structure has to hold it).
        let m = Model {
            id: "test/no-price".to_string(),
            name: "Test".to_string(),
            free: false,
            context_length: Some(1000),
            input_modalities: vec![Modality::Text],
            price_per_million_input: None,
            price_per_million_output: None,
        };
        let line = format_model_line(&m);
        assert!(line.contains("? (unknown)"));
        assert!(!line.contains("0.0000 USD"), "an unknown price must not read as zero");
    }
}
