//! The catalogue, the choice, and how much of a quota is already gone.
//!
//! **THE JUDGEMENTS STAY IN `models`**: which model is really in force is
//! `config::effective_model`, refusing a paid one is `config::set_choice`.
//! Neither is re-decided here — this carries values across the bridge.

use serde::Serialize;

/// One quota window as it is shown.
///
/// **IT SAYS SPENT, NEVER LEFT.** The provider declares what is gone; on a
/// window that never states its ceiling the rest is an invention. `sailor
/// remaining` words it the same, or one of the two would be wrong to trust.
#[derive(Serialize)]
pub(crate) struct Window {
    engine: String,
    /// `five_hour`, `seven_day`, or a name this version does not know. **Not a
    /// closed set** — the provider adds windows.
    unit: String,
    /// From 0.0 to 1.0, a fraction and not a percentage.
    spent_fraction: f64,
    /// In the provider's own shape, kept as text on purpose (fault 14).
    resets_at: Option<String>,
    /// When we looked. A quota without its instant cannot be told from
    /// yesterday's.
    observed_at: i64,
}

impl From<::models::remaining::Remaining> for Window {
    fn from(one: ::models::remaining::Remaining) -> Self {
        Window {
            engine: one.engine,
            unit: one.unit,
            spent_fraction: one.used_fraction,
            resets_at: one.resets_at,
            observed_at: one.observed_at,
        }
    }
}

#[derive(Serialize)]
pub(crate) struct Priced {
    id: String,
    name: String,
    free: bool,
    context_length: Option<u64>,
    /// USD per million tokens. `None` is «the catalogue carries no price»,
    /// never `0.0` — zero is reserved for the models that really are free.
    price_in: Option<f64>,
    price_out: Option<f64>,
    modalities: Vec<String>,
}

/// A kind of work, and which model it actually runs on.
#[derive(Serialize)]
pub(crate) struct Choice {
    kind: String,
    /// What the configuration says, which is not always what runs.
    chosen: Option<String>,
    /// What `effective_model` returns: the free-only rule applied. When this
    /// differs from `chosen`, the configuration is pointing at something that
    /// no longer holds, and the screen has to say so rather than show the
    /// wish.
    in_force: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct Catalogue {
    models: Vec<Priced>,
    choices: Vec<Choice>,
}

/// **THIS READ GOES TO THE NETWORK**, through `curl`, and needs no key.
#[tauri::command]
pub(crate) fn models_catalogue() -> Result<Catalogue, String> {
    let catalog = ::models::catalog::Catalog::parse(&::models::fetch::catalog_body()?)?;
    let config = ::models::store::load(&::models::store::config_path());

    // The kinds are the ones somebody configured, plus `default`, which exists
    // whether or not it was written: leaving it out would hide the one line
    // that answers «and everything else runs on what?».
    let mut kinds: Vec<String> = config.kinds().map(|(kind, _)| kind.to_owned()).collect();
    if !kinds.iter().any(|kind| kind == "default") {
        kinds.insert(0, "default".to_owned());
    }

    Ok(Catalogue {
        models: catalog
            .models
            .iter()
            .map(|model| Priced {
                id: model.id.clone(),
                name: model.name.clone(),
                free: model.free,
                context_length: model.context_length,
                price_in: model.price_per_million_input,
                price_out: model.price_per_million_output,
                modalities: model
                    .input_modalities
                    .iter()
                    .map(|modality| format!("{modality:?}").to_lowercase())
                    .collect(),
            })
            .collect(),
        choices: kinds
            .into_iter()
            .map(|kind| Choice {
                chosen: config.get(&kind).map(str::to_owned),
                in_force: ::models::config::effective_model(&catalog, &config, &kind)
                    .map(|model| model.id.clone()),
                kind,
            })
            .collect(),
    })
}

/// **COSTS NOTHING AND CALLS NO MODEL**: it asks an address how much is already
/// spent. That is why it can be looked at before deciding to launch something,
/// which is the only moment it matters.
#[tauri::command]
pub(crate) fn quota() -> Result<Vec<Window>, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64);
    // Every engine whose descriptor declares a channel, none named here. The
    // error's own words say what to do — «the token has been revoked» is cured
    // by authenticating again — so they travel whole; a channel that does not
    // answer is never a quota of zero, which is the reassuring direction.
    let machine = toolbox::Machine::current();
    let catalog = toolbox::Catalog::load(&toolbox::default_sources(&machine));
    let readings = toolbox::quota::read_all(&catalog, &machine, now);
    if readings.is_empty() {
        return Err("no engine on this machine declares a channel to read its quota from".to_owned());
    }
    let mut windows = Vec::new();
    let mut refused = Vec::new();
    for reading in readings {
        match reading.result {
            Ok(found) => windows.extend(found.into_iter().map(Window::from)),
            Err(why) => refused.push(format!("{}: {why}", reading.engine)),
        }
    }
    if windows.is_empty() {
        return Err(refused.join("; "));
    }
    Ok(windows)
}

#[tauri::command]
pub(crate) fn model_set(kind: String, model_id: String) -> Result<(), String> {
    let catalog = ::models::catalog::Catalog::parse(&::models::fetch::catalog_body()?)?;
    let path = ::models::store::config_path();
    let mut config = ::models::store::load(&path);
    // The free-only rule is applied here by calling the engine's own check,
    // never re-stated: a second copy would be free to disagree, and this one
    // guards what gets written to disk.
    ::models::config::set_choice(&mut config, &catalog, &kind, &model_id)?;
    ::models::store::save(&path, &config).map_err(|error| format!("not saved: {error}"))
}

#[cfg(test)]
mod tests {
    /// **A PRICE THAT IS MISSING AND A PRICE OF ZERO ARE DIFFERENT FACTS**, and
    /// the bridge is where the difference is usually lost: `Option<f64>` turned
    /// into `0.0` by a careless `unwrap_or_default` reads as «free» on screen.
    /// This checks the shape that carries it, not a live catalogue.
    #[test]
    fn a_missing_price_does_not_cross_the_bridge_as_zero() {
        let priced = super::Priced {
            id: "an/unpriced-model".to_owned(),
            name: "Unpriced".to_owned(),
            free: false,
            context_length: None,
            price_in: None,
            price_out: None,
            modalities: vec!["text".to_owned()],
        };
        let json = serde_json::to_string(&priced).expect("it serialises");
        assert!(
            json.contains("\"price_in\":null"),
            "a missing price must arrive as null, not as a number: {json}",
        );
        assert!(
            !json.contains("\"price_in\":0"),
            "a missing price arrived as zero, which reads as free: {json}",
        );
    }
}
