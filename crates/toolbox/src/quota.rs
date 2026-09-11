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

/// The same channel, with the engine's home moved to `home`.
///
/// **A PROFILE MOVES THE ENGINE'S HOME, AND THE READING FOLLOWS IT.** The
/// credentials sit under the home the engine keeps when nothing moves it, so
/// the first segment under `~` is that home and a profile replaces it. Read
/// at the wrong home the answer is another account's, under this one's name.
pub fn channel_in_home(
    descriptor: &Descriptor,
    machine: &Machine,
    home: &std::path::Path,
) -> Option<Result<OauthUsageChannel, String>> {
    let quota = descriptor.quota.as_ref()?;
    let Some(under_home) = quota.credentials.strip_prefix("~/") else {
        return Some(Err(format!(
            "descriptor «{}» writes its credentials at «{}», which is not under the engine's own home: a profile cannot move it",
            descriptor.id, quota.credentials
        )));
    };
    let Some((_engine_home, rest)) = under_home.split_once('/') else {
        return Some(Err(format!(
            "descriptor «{}» writes its credentials at «{}», which names a home and no file inside it",
            descriptor.id, quota.credentials
        )));
    };
    Some(channel_of(descriptor, machine)?.map(|channel| OauthUsageChannel {
        credentials: home.join(rest),
        ..channel
    }))
}

/// One engine's quota as the account signed in at `home` sees it.
pub fn read_in_home(
    descriptor: &Descriptor,
    machine: &Machine,
    home: &std::path::Path,
    observed_at: i64,
) -> Option<Reading> {
    let channel = channel_in_home(descriptor, machine, home)?;
    Some(Reading {
        engine: descriptor.id.clone(),
        result: channel
            .and_then(|channel| read_oauth_usage(&channel, observed_at).map_err(|why| why.to_string())),
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

    /// **THE READING FOLLOWS THE PROFILE, OR IT ANSWERS FOR SOMEBODY ELSE.**
    /// A quota read against the engine's usual home while a profile moves it
    /// reports another account's remaining under this one's name, which is
    /// worse than no reading: it is a number nobody can tell is wrong.
    #[test]
    fn a_profile_home_takes_the_place_of_the_engine_s_own() {
        let machine = Machine::bare(std::path::PathBuf::from("/a/person"));
        let engine = parsed(
            r#"{"id": "y", "family": "ai_cli", "quota": {"reader": "oauth_usage",
                "credentials": "~/.engine/creds.json", "token_pointer": ["a"],
                "url": "https://example.test/usage"}}"#,
        );

        let usual = channel_of(&engine, &machine).expect("declared").expect("known");
        assert_eq!(usual.credentials, std::path::Path::new("/a/person/.engine/creds.json"));

        let moved = channel_in_home(&engine, &machine, std::path::Path::new("/homes/second"))
            .expect("declared")
            .expect("known");
        assert_eq!(moved.credentials, std::path::Path::new("/homes/second/creds.json"));
        assert_eq!(moved.url, usual.url);
        assert_eq!(moved.token_pointer, usual.token_pointer);
    }

    /// A descriptor whose credentials do not sit under the engine's own home
    /// is refused by name rather than read at a path built by guessing.
    #[test]
    fn credentials_outside_the_engine_s_home_refuse_to_be_moved() {
        let machine = Machine::bare(std::path::PathBuf::from("/a/person"));
        let absolute = parsed(
            r#"{"id": "y", "family": "ai_cli", "quota": {"reader": "oauth_usage",
                "credentials": "/etc/creds.json", "token_pointer": [], "url": "u"}}"#,
        );

        let why = channel_in_home(&absolute, &machine, std::path::Path::new("/homes/second"))
            .expect("declared")
            .expect_err("cannot be moved");

        assert!(why.contains("/etc/creds.json") && why.contains("«y»"), "{why}");
    }
}
