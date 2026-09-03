//! What identity an external process started with.
//!
//! **A TYPE AND NOT A STRING.** One text field held `<cli>/<profile>` when a
//! profile was in force and the empty string otherwise: six different facts in
//! one emptiness, one of them a lie. As *what is not a measure does not become
//! a zero* elsewhere, **what is not an absence does not become an empty string**.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

/// What identity the process this row talks about started with.
///
/// Every variant answers two questions at once — **which home** and **how it
/// was chosen** — because splitting them lets a right path be written with a
/// wrong reason and nobody notice. **No variant carries a token**: the path is
/// the ground a diagnosis stands on, and that home's contents are not ours.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EngineIdentity {
    /// A profile was in force and its home was put into the environment.
    ProfileInForce {
        cli_id: String,
        profile_name: String,
        /// **THE DATUM THAT COUNTS.** A profile name can be reused, moved or
        /// deleted; the path is what you go and look at when something went
        /// wrong.
        home_dir: PathBuf,
        /// Where the profile sent the command line instead of its maker's
        /// endpoint, so the spend is never attributed to the subscription.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        endpoint: Option<String>,
    },
    /// The step set the home variable itself, and it wins.
    ///
    /// **NOT A FAULT: THE DECISION.** A variable written inside a step says
    /// something precise about *that* call and must not be overridden by state
    /// living elsewhere. The fault was staying quiet about it — recording the
    /// active profile's name while the process started somewhere else.
    ChosenByTheStep { cli_id: String, home_dir: PathBuf },
    /// No profile in force: the process starts with the home of whoever opened
    /// the terminal.
    ///
    /// **"INHERITED" IS NOT "UNKNOWN"**: a real identity, and saying so tells
    /// the reader the call used the machine's credentials. No path, because
    /// this decision looks at neither environment nor disk — it would be a guess.
    InheritedFromTheTerminal { cli_id: String },
    /// The state names a profile the list no longer has.
    ///
    /// The process started with the inherited home, as above — but the reason
    /// differs, and this is the only one that asks for action: there is state to
    /// repair. Composing the path from the name would give an empty home with
    /// the air of an applied profile.
    ProfileVanished {
        cli_id: String,
        profile_name: String,
    },
    /// A profile exists, but this CLI does not move its home with an
    /// environment variable: the identity depends on where a file on disk points.
    NotMovedByAnEnvVar {
        cli_id: String,
        profile_name: String,
        /// Why it was not put in force, in the mechanism's own words.
        why: String,
    },
    /// The binary is not a CLI Sailor knows — `sh`, a script: there is no home
    /// to move, and giving it one would mean nothing.
    NotAKnownEngine,
    /// A handed-over step: the work was done by the agent already alive in the
    /// terminal.
    ///
    /// Sailor started nothing and does not know what identity that agent worked
    /// under. Writing any would be inventing it; silence would confuse it with
    /// "no profile".
    DeclaredByAnAgent,
    /// The row predates Sailor recording the identity.
    ///
    /// `legacy` is the text the old column carried. **It is not promoted to a
    /// declared profile**: that column named the active profile even when the
    /// step had overridden it, so it was already able to lie, and structuring it
    /// now would give an old lie the face of a new measure.
    Unrecorded {
        #[serde(default)]
        legacy: String,
    },
}

impl Default for EngineIdentity {
    fn default() -> Self {
        Self::Unrecorded {
            legacy: String::new(),
        }
    }
}

impl EngineIdentity {
    /// The CLI being talked about, when one is known.
    pub fn cli_id(&self) -> Option<&str> {
        match self {
            Self::ProfileInForce { cli_id, .. }
            | Self::ChosenByTheStep { cli_id, .. }
            | Self::InheritedFromTheTerminal { cli_id }
            | Self::ProfileVanished { cli_id, .. }
            | Self::NotMovedByAnEnvVar { cli_id, .. } => Some(cli_id),
            Self::NotAKnownEngine | Self::DeclaredByAnAgent | Self::Unrecorded { .. } => None,
        }
    }

    /// The home the process started with, when Sailor was the one who chose it.
    ///
    /// `None` does not mean "there was no home": it means this row does not know
    /// it, because somebody else chose it — the environment of whoever opened
    /// the terminal, a symlink on disk, an agent.
    pub fn home_dir(&self) -> Option<&std::path::Path> {
        match self {
            Self::ProfileInForce { home_dir, .. } | Self::ChosenByTheStep { home_dir, .. } => {
                Some(home_dir)
            }
            _ => None,
        }
    }

    /// How it is written into a text column.
    pub fn to_column(&self) -> String {
        // A `Serialize` type does not fail to serialize; if it ever did, the
        // column stays empty and reads back as "unrecorded", the only true
        // thing that can be said of a row whose field could not be written.
        serde_json::to_string(self).unwrap_or_default()
    }

    /// How it is read back from a text column.
    ///
    /// **TEXT THAT IS NOT OUR JSON IS NOT THROWN AWAY.** Every row written
    /// before this type existed carries `<cli>/<profile>` there, or nothing:
    /// those become [`EngineIdentity::Unrecorded`] holding that text, which is
    /// the only clue such a row has left.
    pub fn from_column(text: &str) -> Self {
        serde_json::from_str(text).unwrap_or_else(|_| Self::Unrecorded {
            legacy: text.to_owned(),
        })
    }
}

/// A line for a person: first **how** it was chosen, then **which home**.
impl fmt::Display for EngineIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProfileInForce {
                cli_id,
                profile_name,
                home_dir,
                endpoint: Some(endpoint),
            } => write!(
                formatter,
                "profile {cli_id}/{profile_name} at {endpoint} — home {}",
                home_dir.display()
            ),
            Self::ProfileInForce {
                cli_id,
                profile_name,
                home_dir,
                endpoint: None,
            } => write!(
                formatter,
                "profile {cli_id}/{profile_name} — home {}",
                home_dir.display()
            ),
            Self::ChosenByTheStep { cli_id, home_dir } => write!(
                formatter,
                "home chosen by the step ({cli_id}) — home {}",
                home_dir.display()
            ),
            Self::InheritedFromTheTerminal { cli_id } => write!(
                formatter,
                "identity inherited from whoever opened the terminal ({cli_id}): no profile in force"
            ),
            Self::ProfileVanished {
                cli_id,
                profile_name,
            } => write!(
                formatter,
                "inherited identity ({cli_id}): the state names profile \"{profile_name}\", which no longer exists"
            ),
            Self::NotMovedByAnEnvVar {
                cli_id,
                profile_name,
                why,
            } => write!(
                formatter,
                "profile {cli_id}/{profile_name} not put in force: {why}"
            ),
            Self::NotAKnownEngine => {
                write!(formatter, "not a command line that Sailor knows")
            }
            Self::DeclaredByAnAgent => write!(
                formatter,
                "declared by an agent: identity not known to Sailor"
            ),
            Self::Unrecorded { legacy } if legacy.is_empty() => {
                write!(formatter, "unrecorded")
            }
            Self::Unrecorded { legacy } => write!(
                formatter,
                "unrecorded (the old column said \"{legacy}\")"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **WHAT GOES INTO THE COLUMN COMES BACK UNCHANGED.** Without this the
    /// identity would be written and read back different, and nobody would
    /// notice until a diagnosis looked at the wrong row.
    #[test]
    fn every_shape_survives_the_column() {
        for identity in every_shape() {
            assert_eq!(
                EngineIdentity::from_column(&identity.to_column()),
                identity,
                "this shape does not survive the round trip through the store"
            );
        }
    }

    /// The text of an old column is neither thrown away nor promoted.
    #[test]
    fn an_old_column_becomes_unrecorded_with_its_text_kept() {
        assert_eq!(
            EngineIdentity::from_column("codex/lavoro"),
            EngineIdentity::Unrecorded {
                legacy: "codex/lavoro".to_owned()
            }
        );
        assert_eq!(
            EngineIdentity::from_column(""),
            EngineIdentity::Unrecorded {
                legacy: String::new()
            }
        );
    }

    /// **EVERY SHAPE READS DIFFERENTLY FROM THE OTHERS.** That is the cure: if
    /// two different facts produced the same line for the reader, the type
    /// would only have moved the emptiness further along.
    #[test]
    fn no_two_shapes_read_the_same() {
        let said: Vec<String> = every_shape().iter().map(ToString::to_string).collect();
        for (position, one) in said.iter().enumerate() {
            for other in said.iter().skip(position + 1) {
                assert_ne!(one, other, "two different facts read the same");
            }
        }
    }

    fn every_shape() -> Vec<EngineIdentity> {
        vec![
            EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "lavoro".to_owned(),
                home_dir: PathBuf::from("/case/codex/lavoro"),
                endpoint: None,
            },
            EngineIdentity::ProfileInForce {
                cli_id: "codex".to_owned(),
                profile_name: "altrove".to_owned(),
                home_dir: PathBuf::from("/case/codex/altrove"),
                endpoint: Some("http://localhost:11434/v1".to_owned()),
            },
            EngineIdentity::ChosenByTheStep {
                cli_id: "codex".to_owned(),
                home_dir: PathBuf::from("/una/casa/del/passo"),
            },
            EngineIdentity::InheritedFromTheTerminal {
                cli_id: "codex".to_owned(),
            },
            EngineIdentity::ProfileVanished {
                cli_id: "codex".to_owned(),
                profile_name: "sparito".to_owned(),
            },
            EngineIdentity::NotMovedByAnEnvVar {
                cli_id: "antigravity".to_owned(),
                profile_name: "lavoro".to_owned(),
                why: "no known environment variable".to_owned(),
            },
            EngineIdentity::NotAKnownEngine,
            EngineIdentity::DeclaredByAnAgent,
            EngineIdentity::Unrecorded {
                legacy: String::new(),
            },
            EngineIdentity::Unrecorded {
                legacy: "codex/lavoro".to_owned(),
            },
        ]
    }
}
