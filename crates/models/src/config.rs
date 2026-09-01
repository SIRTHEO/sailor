//! The user's choice: which model they want for each kind of work. This is
//! configuration, not code — the file on disk lives in `store.rs`; here sit
//! only the shape and the rule that governs it.

use crate::catalog::Catalog;
use std::collections::BTreeMap;

/// The free fallback model until the user picks another one. It had to stay in
/// sync with `NOTTE_OPENROUTER_MODEL` in `crates/notte/src/main.rs:151`, where
/// `notte` read the same value; `notte` is no longer in the repo, so that path
/// names a crate that is not there and there is nothing left to keep in sync.
pub const DEFAULT_FREE_MODEL: &str = "nvidia/nemotron-3-super-120b-a12b:free";

/// A kind of work (`"default"`, `"notte"`, `"codice"`, ...) mapped to the
/// model chosen for it. The keys are deliberately open: Sailor defines the
/// list elsewhere, and this file does not guess it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UserConfig {
    choices: BTreeMap<String, String>,
}

impl UserConfig {
    pub fn parse(json: &str) -> Result<UserConfig, String> {
        let choices: BTreeMap<String, String> =
            serde_json::from_str(json).map_err(|e| format!("modelli.json is not valid: {e}"))?;
        Ok(UserConfig { choices })
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.choices).unwrap_or_else(|_| "{}".to_string())
    }

    pub fn get(&self, kind: &str) -> Option<&str> {
        self.choices.get(kind).map(|s| s.as_str())
    }

    /// Writes the choice without checking it: the free-only rule lives in
    /// `set_choice`, not here, because this function also serves `store::load`
    /// when putting back into memory what is already on disk.
    pub fn set_unchecked(&mut self, kind: &str, model_id: &str) {
        self.choices.insert(kind.to_string(), model_id.to_string());
    }

    pub fn kinds(&self) -> impl Iterator<Item = (&str, &str)> {
        self.choices.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// The mandate's own line: for now the user may configure only the free ones.
/// An attempt to configure a paid model — or an id absent from the catalog — is
/// refused here, before anything touches the disk.
pub fn set_choice(
    cfg: &mut UserConfig,
    catalog: &Catalog,
    kind: &str,
    model_id: &str,
) -> Result<(), String> {
    let model = catalog
        .find(model_id)
        .ok_or_else(|| format!("\"{model_id}\" is not in the catalog"))?;
    if !model.free {
        return Err(format!(
            "\"{model_id}\" is paid: for now only free models can be configured"
        ));
    }
    cfg.set_unchecked(kind, model_id);
    Ok(())
}

/// The model actually in use for a kind of work. Until one is configured — or
/// when the saved choice no longer points at a free model in the catalog —
/// only the free fallback applies. The rule is enforced here so that a
/// careless read of the configuration elsewhere cannot get around it.
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
        let cfg = UserConfig::parse(
            r#"{"default":"z-ai/glm-5.2:free","notte":"cohere/north-mini-code:free"}"#,
        )
        .unwrap();
        assert_eq!(cfg.get("default"), Some("z-ai/glm-5.2:free"));
        assert_eq!(cfg.get("notte"), Some("cohere/north-mini-code:free"));
        assert_eq!(cfg.get("absent"), None);
    }

    #[test]
    fn invalid_json_is_a_readable_error() {
        let err = UserConfig::parse("not json").unwrap_err();
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
        assert!(err.contains("is paid"));
        assert_eq!(cfg.get("default"), None, "a refusal must write nothing");
    }

    #[test]
    fn set_choice_refuses_a_model_not_in_the_catalog() {
        let catalog = sample_catalog();
        let mut cfg = UserConfig::default();
        let err =
            set_choice(&mut cfg, &catalog, "default", "made-up/does-not-exist:free").unwrap_err();
        assert!(err.contains("catalog"));
    }

    #[test]
    fn effective_model_falls_back_to_the_free_default_when_unconfigured() {
        let catalog = sample_catalog();
        let cfg = UserConfig::default();
        let m = effective_model(&catalog, &cfg, "never-seen").unwrap();
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
        // A hand-written file (one that never went through `set_choice`) can
        // hold a paid model: the rule applies on read too, not only on write.
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
        cfg.set_unchecked("default", "gone/from-the-catalog:free");
        let m = effective_model(&catalog, &cfg, "default").unwrap();
        assert_eq!(m.id, DEFAULT_FREE_MODEL);
    }
}
