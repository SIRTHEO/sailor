//! From the channel a descriptor declares to the reading: the one place the
//! word `oauth_usage` is turned into a reader.
//!
//! **THE CODE KNOWS KINDS OF CHANNEL, NEVER PROVIDERS.** A descriptor that
//! declares a kind this Sailor does not read gets a refusal naming the kind,
//! and an engine without a channel is simply not in the list.

use crate::descriptor::{Catalog, Descriptor};
use crate::probe::Machine;
use models::remaining::{read_oauth_usage, OauthUsageChannel, Remaining};

/// One engine's reading, or why there is none.
#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub engine: String,
    pub result: Result<Vec<Remaining>, String>,
}

/// The channel a descriptor declares, made concrete for this machine, or the
/// reason it cannot be; `None` when the descriptor declares none.
pub fn channel_of(descriptor: &Descriptor, machine: &Machine) -> Option<Result<OauthUsageChannel, String>> {
    let quota = descriptor.quota.as_ref()?;
    Some(match quota.reader.as_str() {
        "oauth_usage" => Ok(OauthUsageChannel {
            engine: descriptor.id.clone(),
            credentials: machine.expand(&quota.credentials).into(),
            token_pointer: quota.token_pointer.clone(),
            url: quota.url.clone(),
            headers: quota.headers.clone(),
        }),
        other => Err(format!(
            "descriptor «{}» declares a quota reader «{other}» this Sailor does not read",
            descriptor.id
        )),
    })
}

/// Reads one engine's quota through its declared channel; `None` when it
/// declares none.
pub fn read_one(descriptor: &Descriptor, machine: &Machine, observed_at: i64) -> Option<Reading> {
    let channel = channel_of(descriptor, machine)?;
    Some(Reading {
        engine: descriptor.id.clone(),
        result: channel.and_then(|channel| read_oauth_usage(&channel, observed_at).map_err(|why| why.to_string())),
    })
}

/// Every engine of the catalogue that declares a channel, read in turn.
pub fn read_all(catalog: &Catalog, machine: &Machine, observed_at: i64) -> Vec<Reading> {
    catalog
        .live()
        .into_iter()
        .filter_map(|loaded| read_one(&loaded.descriptor, machine, observed_at))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(text: &str) -> Descriptor {
        serde_json::from_str(text).expect("a descriptor parses")
    }

    /// The control first: no channel declared, nothing to read. Then a kind
    /// nobody reads is refused by name, and the known kind becomes a channel
    /// signed with the descriptor's id.
    #[test]
    fn a_channel_is_the_descriptors_and_an_unknown_kind_is_refused_by_name() {
        let machine = Machine::bare(std::path::PathBuf::from(crate::probe::NOWHERE));
        assert!(channel_of(&parsed(r#"{"id": "x", "family": "ai_cli"}"#), &machine).is_none());

        let odd = parsed(
            r#"{"id": "x", "family": "ai_cli", "quota": {"reader": "telepathy",
                "credentials": "/c", "token_pointer": [], "url": "u"}}"#,
        );
        let refused = channel_of(&odd, &machine).expect("declared").expect_err("unknown kind");
        assert!(refused.contains("telepathy") && refused.contains("«x»"), "{refused}");

        let known = parsed(
            r#"{"id": "y", "family": "ai_cli", "quota": {"reader": "oauth_usage",
                "credentials": "/nowhere/creds.json", "token_pointer": ["a", "b"],
                "url": "https://example.test/usage", "headers": ["h: v"]}}"#,
        );
        let channel = channel_of(&known, &machine).expect("declared").expect("known kind");
        assert_eq!(channel.engine, "y");
        assert_eq!(channel.token_pointer, vec!["a", "b"]);
        assert_eq!(channel.headers, vec!["h: v"]);
        // And reading it on a machine without that file is a refusal that
        // names the file, never an empty measure.
        let reading = read_one(&known, &machine, 0).expect("declared");
        let why = reading.result.expect_err("no credentials here");
        assert!(why.contains("creds.json"), "{why}");
    }
}
