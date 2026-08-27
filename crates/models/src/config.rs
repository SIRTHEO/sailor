//! La scelta dell'utente: quale modello vuole per ogni genere di lavoro.
//! È configurazione, non codice — il file su disco vive in `store.rs`, qui
//! sta solo la forma e la regola che la governa.

use crate::catalog::Catalog;
use std::collections::BTreeMap;

/// Il modello gratuito di ripiego finché l'utente non ne sceglie un altro:
/// deve restare in sincronia con `NOTTE_OPENROUTER_MODEL` in
/// `crates/notte/src/main.rs:151` — stesso valore, letto lì da `notte`.
pub const DEFAULT_FREE_MODEL: &str = "nvidia/nemotron-3-super-120b-a12b:free";

/// Un genere di lavoro (`"default"`, `"notte"`, `"codice"`, ...) mappato al
/// modello scelto per quel genere. Le chiavi sono libere di proposito:
/// Sailor ne definisce l'elenco altrove, questo file non lo indovina.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserConfig {
    choices: BTreeMap<String, String>,
}

impl UserConfig {
    pub fn parse(json: &str) -> Result<UserConfig, String> {
        let choices: BTreeMap<String, String> =
            serde_json::from_str(json).map_err(|e| format!("modelli.json non valido: {e}"))?;
        Ok(UserConfig { choices })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.choices).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn get(&self, kind: &str) -> Option<&str> {
        self.choices.get(kind).map(|s| s.as_str())
    }

    /// Scrive la scelta senza controllarla: la regola dei soli gratuiti vive
    /// in `set_choice`, non qui, perché questa funzione serve anche a
    /// `store::load` per rimettere in memoria ciò che è già su disco.
    pub fn set_unchecked(&mut self, kind: &str, model_id: &str) {
        self.choices.insert(kind.to_string(), model_id.to_string());
    }

    pub fn kinds(&self) -> impl Iterator<Item = (&str, &str)> {
        self.choices.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// La riga di Theo del 27/08/2026, testuale: *"lasciare che l'utente possa
/// configurarli per ora solo i free"*. Un tentativo di configurare un
/// modello a pagamento — o un identificatore assente dal catalogo — si
/// rifiuta qui, prima di toccare il disco.
pub fn set_choice(
    cfg: &mut UserConfig,
    catalog: &Catalog,
    kind: &str,
    model_id: &str,
) -> Result<(), String> {
    let model = catalog
        .find(model_id)
        .ok_or_else(|| format!("\"{model_id}\" non è nel catalogo"))?;
    if !model.free {
        return Err(format!(
            "\"{model_id}\" è a pagamento: per ora si possono configurare solo i modelli gratuiti"
        ));
    }
    cfg.set_unchecked(kind, model_id);
    Ok(())
}

/// Il modello davvero in uso per un genere di lavoro. Finché non è
/// configurato — o se la scelta salvata non punta (più, o ancora) a un
/// modello gratuito del catalogo — vale solo il gratuito di ripiego: è la
/// riga di Theo, imposta come regola qui e non scavalcabile da una lettura
/// distratta della configurazione altrove.
pub fn effective_model<'a>(
    catalog: &'a Catalog,
    cfg: &UserConfig,
    kind: &str,
) -> Option<&'a crate::catalog::Model> {
    if let Some(chosen_id) = cfg.get(kind) {
        if let Some(m) = catalog.find(chosen_id) {
            if m.free {
                return Some(m);
            }
        }
    }
    catalog.find(DEFAULT_FREE_MODEL)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::Catalog;

    fn sample_catalog() -> Catalog {
        Catalog::parse(include_str!("../tests/fixtures/catalog-sample.json")).unwrap()
    }

    #[test]
    fn parses_and_round_trips_through_json() {
        let cfg = UserConfig::parse(r#"{"default":"z-ai/glm-5.2:free","notte":"cohere/north-mini-code:free"}"#).unwrap();
        assert_eq!(cfg.get("default"), Some("z-ai/glm-5.2:free"));
        assert_eq!(cfg.get("notte"), Some("cohere/north-mini-code:free"));
        assert_eq!(cfg.get("assente"), None);
    }

    #[test]
    fn invalid_json_is_a_readable_error() {
        let err = UserConfig::parse("non è json").unwrap_err();
        assert!(err.contains("modelli.json"));
    }

    #[test]
    fn set_choice_accepts_a_free_model() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        set_choice(&mut cfg, &catalog, "default", "z-ai/glm-5.2:free").unwrap();
        assert_eq!(cfg.get("default"), Some("z-ai/glm-5.2:free"));
    }

    #[test]
    fn set_choice_refuses_a_paid_model() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        let err = set_choice(&mut cfg, &catalog, "default", "qwen/qwen3.8-flash").unwrap_err();
        assert!(err.contains("a pagamento"));
        assert_eq!(cfg.get("default"), None, "un rifiuto non deve scrivere niente");
    }

    #[test]
    fn set_choice_refuses_a_model_not_in_the_catalog() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        let err = set_choice(&mut cfg, &catalog, "default", "inventato/non-esiste:free").unwrap_err();
        assert!(err.contains("catalogo"));
    }

    #[test]
    fn effective_model_falls_back_to_the_free_default_when_unconfigured() {
        let catalog = sample_catalog();
        let cfg = UserConfig::default();
        let m = effective_model(&catalog, &cfg, "mai-visto").unwrap();
        assert_eq!(m.id, DEFAULT_FREE_MODEL);
    }

    #[test]
    fn effective_model_uses_the_configured_free_choice() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        cfg.set_unchecked("notte", "z-ai/glm-5.2:free");
        let m = effective_model(&catalog, &cfg, "notte").unwrap();
        assert_eq!(m.id, "z-ai/glm-5.2:free");
    }

    #[test]
    fn effective_model_ignores_a_paid_choice_written_by_hand_and_falls_back() {
        // Un file scritto a mano (non passato da `set_choice`) può contenere
        // un modello a pagamento: la regola vale comunque a lettura, non
        // solo in scrittura.
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        cfg.set_unchecked("default", "qwen/qwen3.8-flash");
        let m = effective_model(&catalog, &cfg, "default").unwrap();
        assert_eq!(m.id, DEFAULT_FREE_MODEL);
        assert!(m.free);
    }

    #[test]
    fn effective_model_ignores_a_choice_that_disappeared_from_the_catalog() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        cfg.set_unchecked("default", "non/piu-nel-catalogo:free");
        let m = effective_model(&catalog, &cfg, "default").unwrap();
        assert_eq!(m.id, DEFAULT_FREE_MODEL);
    }
}
