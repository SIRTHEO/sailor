//! Il comando, come modulo pronto a diventare `sailor models`: elencare,
//! mostrare la scelta corrente, cambiarla. Tutto qui prende ciò che gli
//! serve già in mano (catalogo, configurazione) e restituisce testo —
//! chi lo invoca (oggi `main.rs`, domani `sailor`) decide dove stamparlo.

use crate::catalog::{Catalog, Filter, Modality, Model};
use crate::config::{self, UserConfig};

/// Un genere di ingresso da riga di comando (`text`, `image`, `audio`,
/// `video`) verso il tipo tipizzato. Un valore sconosciuto è un errore
/// leggibile, non uno `unwrap` che porta giù il processo.
pub fn parse_modality_arg(raw: &str) -> Result<Modality, String> {
    match raw {
        "text" => Ok(Modality::Text),
        "image" => Ok(Modality::Image),
        "audio" => Ok(Modality::Audio),
        "video" => Ok(Modality::Video),
        other => Err(format!(
            "genere di ingresso sconosciuto: \"{other}\" (usa text, image, audio o video)"
        )),
    }
}

fn format_price(p: Option<f64>) -> String {
    match p {
        Some(v) => format!("{v:.4} USD/milione"),
        None => "? (sconosciuto)".to_string(),
    }
}

/// Una riga per modello: identificatore, gratuito o a pagamento, finestra,
/// generi accettati, prezzo per milione in ingresso e in uscita.
pub fn format_model_line(m: &Model) -> String {
    let kind = if m.free { "gratuito" } else { "a pagamento" };
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
        "{id}  {kind}  contesto {ctx}  ingresso {modalities}  input {price_in}  output {price_out}",
        id = m.id,
        price_in = format_price(m.price_per_million_input),
        price_out = format_price(m.price_per_million_output),
    )
}

/// L'elenco filtrato, una riga per modello. Vuoto se il filtro non becca
/// nessuno — non è un errore, è la risposta.
pub fn list(catalog: &Catalog, filter: &Filter) -> String {
    let hits = catalog.filter(filter);
    if hits.is_empty() {
        return "nessun modello corrisponde al filtro".to_string();
    }
    hits.iter()
        .map(|m| format_model_line(m))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Il modello davvero in uso per un genere di lavoro, con la provenienza
/// della scelta dichiarata in chiaro: configurato o di ripiego.
pub fn current(catalog: &Catalog, cfg: &UserConfig, kind: &str) -> String {
    let configured = cfg
        .get(kind)
        .and_then(|id| catalog.find(id))
        .filter(|m| m.free);
    match configured {
        Some(m) => format!("{kind}: {} (configurato)", format_model_line(m)),
        None => match config::effective_model(catalog, cfg, kind) {
            Some(m) => format!("{kind}: {} (gratuito di ripiego, non configurato)", format_model_line(m)),
            None => format!("{kind}: nessun modello gratuito disponibile nel catalogo"),
        },
    }
}

/// Cambia la scelta per un genere di lavoro. Rifiuta, senza scrivere nulla,
/// se il modello non è nel catalogo o non è gratuito — vedi
/// `config::set_choice` per il perché.
pub fn set(
    cfg: &mut UserConfig,
    catalog: &Catalog,
    kind: &str,
    model_id: &str,
) -> Result<String, String> {
    config::set_choice(cfg, catalog, kind, model_id)?;
    Ok(format!("{kind}: impostato su {model_id}"))
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
        let err = parse_modality_arg("odore").unwrap_err();
        assert!(err.contains("odore"));
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
        assert!(out.lines().all(|l| l.contains("gratuito") && !l.contains("a pagamento")));
    }

    #[test]
    fn list_with_no_hits_says_so_instead_of_an_empty_string() {
        let catalog = sample_catalog();
        let out = list(&catalog, &Filter { min_context: Some(999_999_999), ..Default::default() });
        assert_eq!(out, "nessun modello corrisponde al filtro");
    }

    #[test]
    fn current_reports_the_configured_free_model() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        config::set_choice(&mut cfg, &catalog, "notte", "z-ai/glm-5.2:free").unwrap();
        let out = current(&catalog, &cfg, "notte");
        assert!(out.contains("z-ai/glm-5.2:free"));
        assert!(out.contains("(configurato)"));
    }

    #[test]
    fn current_reports_the_free_fallback_when_unconfigured() {
        let catalog = sample_catalog();
        let cfg = UserConfig::default();
        let out = current(&catalog, &cfg, "mai-toccato");
        assert!(out.contains(config::DEFAULT_FREE_MODEL));
        assert!(out.contains("non configurato"));
    }

    #[test]
    fn set_refuses_a_paid_model_and_leaves_the_config_untouched() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        let err = set(&mut cfg, &catalog, "default", "qwen/qwen3.8-flash").unwrap_err();
        assert!(err.contains("a pagamento"));
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
        // Costruito a mano: un modello del catalogo reale con prezzo assente
        // (mai visto finora, ma la struttura deve reggerlo).
        let m = Model {
            id: "prova/senza-prezzo".to_string(),
            name: "Prova".to_string(),
            free: false,
            context_length: Some(1000),
            input_modalities: vec![Modality::Text],
            price_per_million_input: None,
            price_per_million_output: None,
        };
        let line = format_model_line(&m);
        assert!(line.contains("? (sconosciuto)"));
        assert!(!line.contains("0.0000 USD"), "un prezzo ignoto non deve leggersi come zero");
    }
}
