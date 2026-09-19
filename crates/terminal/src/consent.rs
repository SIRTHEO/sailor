//! Who may write into a terminal the host holds, and on what authority.
//!
//! A person at the keyboard is their own authority: typing is the consent.
//! Everything else writes only where the terminal itself carries a consent
//! naming who gave it and what it covers. An idle prompt is not consent and a
//! deposited handover is not consent: neither is an answer to «may I».

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// Who is asking to write.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "hand", rename_all = "snake_case")]
pub enum Hand {
    /// A person at the keyboard: the typing is the consent, and there is
    /// nothing further to record.
    #[default]
    Person,
    Flow { flow: String, run: String },
}

impl Hand {
    pub fn flow(flow: &str, run: &str) -> Hand {
        Hand::Flow {
            flow: flow.to_owned(),
            run: run.to_owned(),
        }
    }
}

/// A consent recorded on one terminal: which flow it covers, who gave it, and
/// what they gave it for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Consent {
    pub flow: String,
    pub given_by: String,
    pub given_for: String,
}

impl Consent {
    /// Nothing, when the consent is fit to be recorded; otherwise what of it
    /// is missing. A consent with no giver names nobody, and a consent with
    /// no purpose covers everything.
    pub fn what_is_missing(&self) -> Option<String> {
        let mut absent = Vec::new();
        if self.flow.trim().is_empty() {
            absent.push("the flow it covers");
        }
        if self.given_by.trim().is_empty() {
            absent.push("who gave it");
        }
        if self.given_for.trim().is_empty() {
            absent.push("what it was given for");
        }
        if absent.is_empty() {
            return None;
        }
        Some(format!(
            "this consent records nothing: {} is missing",
            absent.join(", and ")
        ))
    }
}

/// The consents standing on one terminal.
#[derive(Debug, Default)]
pub struct Given {
    recorded: Mutex<Vec<Consent>>,
}

impl Given {
    pub fn new() -> Given {
        Given::default()
    }

    /// Records a consent, or says what of it was missing.
    pub fn record(&self, consent: Consent) -> Result<(), String> {
        if let Some(missing) = consent.what_is_missing() {
            return Err(missing);
        }
        let mut recorded = crate::locked(&self.recorded);
        recorded.retain(|standing| standing.flow != consent.flow);
        recorded.push(consent);
        Ok(())
    }

    /// Takes back every consent for a flow. Nothing given is given for ever.
    pub fn withdraw(&self, flow: &str) {
        crate::locked(&self.recorded).retain(|standing| standing.flow != flow);
    }

    pub fn all(&self) -> Vec<Consent> {
        crate::locked(&self.recorded).clone()
    }

    /// Nothing, when this hand may write into the terminal called `id`;
    /// otherwise the refusal, which names what was missing.
    pub fn refusal(&self, id: &str, hand: &Hand) -> Option<String> {
        let Hand::Flow { flow, run } = hand else {
            return None;
        };
        let standing = self.all();
        if standing.iter().any(|consent| &consent.flow == flow) {
            return None;
        }
        let missing = if standing.is_empty() {
            "no consent at all is recorded on it".to_owned()
        } else {
            let others: Vec<&str> = standing
                .iter()
                .map(|consent| consent.flow.as_str())
                .collect();
            format!(
                "the consents it carries are for {}, and none of them names this flow",
                others.join(", ")
            )
        };
        Some(format!(
            "«{id}» takes nothing from the flow «{flow}» of run «{run}»: {missing}. \
             A consent says who gave it and what for; a still screen and a painted prompt say neither."
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A person needs no record: the typing is the consent. A flow is refused
    /// until one names it, and the refusal says which of the two was missing.
    #[test]
    fn a_flow_is_refused_until_a_consent_names_it() {
        let given = Given::new();
        assert_eq!(given.refusal("t-1", &Hand::Person), None);

        let refused = given
            .refusal("t-1", &Hand::flow("empty-a-session", "r-9"))
            .expect("a flow with nothing recorded is refused");
        assert!(refused.contains("empty-a-session"), "{refused}");
        assert!(refused.contains("no consent at all"), "{refused}");

        given
            .record(Consent {
                flow: "tidy-the-notes".to_owned(),
                given_by: "the owner".to_owned(),
                given_for: "renaming the notes of a closed run".to_owned(),
            })
            .expect("a whole consent is recorded");
        let refused = given
            .refusal("t-1", &Hand::flow("empty-a-session", "r-9"))
            .expect("a consent for another flow is not one for this");
        assert!(refused.contains("tidy-the-notes"), "{refused}");

        given
            .record(Consent {
                flow: "empty-a-session".to_owned(),
                given_by: "the owner".to_owned(),
                given_for: "emptying a session that handed on".to_owned(),
            })
            .expect("recorded");
        assert_eq!(
            given.refusal("t-1", &Hand::flow("empty-a-session", "r-9")),
            None
        );

        given.withdraw("empty-a-session");
        assert!(given
            .refusal("t-1", &Hand::flow("empty-a-session", "r-9"))
            .is_some());
    }

    /// A consent that names nobody, or nothing to do, is not recorded: it
    /// would read as authority while carrying none.
    #[test]
    fn a_consent_that_records_nothing_is_refused() {
        let given = Given::new();
        let missing = given
            .record(Consent {
                flow: "empty-a-session".to_owned(),
                given_by: "  ".to_owned(),
                given_for: String::new(),
            })
            .expect_err("an empty consent is not a consent");
        assert!(missing.contains("who gave it"), "{missing}");
        assert!(missing.contains("what it was given for"), "{missing}");
        assert!(given.all().is_empty());
    }
}
