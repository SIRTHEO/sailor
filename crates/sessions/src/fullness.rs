//! How full a context is, measured from outside.
//!
//! Asking the engine is not an option. One command line writes its own token
//! count into a transcript; others write nothing at all. A measure that reads
//! that file is a measure for one product, so this one counts what every
//! command line does without being asked: the bytes it moves.

/// What is known about how bytes turn into tokens.
///
/// The straight line has an intercept because a context costs tokens before a
/// single byte crosses the pipe: the prologue, the rules, the memories, the
/// tool definitions. A model without it reads the threshold far too early.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Model {
    pub per_byte: f64,
    pub overhead: u64,
}

impl Default for Model {
    /// Medians over 88 real sessions and 42,591 points; median error 6.0%.
    /// The write-up is the note on flows that patrol, under `docs/`.
    fn default() -> Model {
        Model {
            per_byte: 0.68,
            overhead: 60_129,
        }
    }
}

/// A pair a command line reported about itself: bytes seen, tokens declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    pub bytes: u64,
    pub tokens: u64,
}

impl Model {
    /// A model fitted to two real measurements, when a command line offers
    /// them.
    ///
    /// Two and not one: a single pair fixes a point, not a line. Fitting a
    /// line through one point means keeping one of the two numbers on faith
    /// while presenting both as measured.
    pub fn between(first: Observation, second: Observation) -> Option<Model> {
        let (earlier, later) = if first.bytes <= second.bytes {
            (first, second)
        } else {
            (second, first)
        };
        if later.bytes == earlier.bytes || later.tokens < earlier.tokens {
            return None;
        }
        let per_byte =
            (later.tokens - earlier.tokens) as f64 / (later.bytes - earlier.bytes) as f64;
        let carried = per_byte * earlier.bytes as f64;
        let overhead = (earlier.tokens as f64 - carried).max(0.0) as u64;
        Some(Model { per_byte, overhead })
    }

    pub fn tokens_for(&self, bytes: u64) -> u64 {
        self.overhead + (self.per_byte * bytes as f64) as u64
    }
}

/// What the estimate says, with the numbers it was made from.
///
/// The bytes travel with the verdict on purpose: a verdict alone cannot be
/// argued with, and this one is an estimate that has to be arguable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fullness {
    pub bytes: u64,
    pub estimated_tokens: u64,
    pub ceiling: u64,
    pub past_the_ceiling: bool,
}

/// How full a session is against a ceiling somebody declared.
///
/// The ceiling is an argument and never a constant found in here. What counts
/// as too full is a decision about a budget, and a decision that hides inside
/// the arithmetic is a decision nobody can change.
pub fn measure(bytes: u64, model: &Model, ceiling: u64) -> Fullness {
    let estimated_tokens = model.tokens_for(bytes);
    Fullness {
        bytes,
        estimated_tokens,
        ceiling,
        past_the_ceiling: ceiling > 0 && estimated_tokens >= ceiling,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_session_already_costs_what_was_loaded_before_it() {
        let quiet = measure(0, &Model::default(), 500_000);
        assert_eq!(quiet.estimated_tokens, 60_129);
        assert!(!quiet.past_the_ceiling);
    }

    #[test]
    fn the_estimate_grows_with_the_bytes() {
        let model = Model::default();
        assert!(model.tokens_for(1_000_000) > model.tokens_for(1_000));
    }

    #[test]
    fn a_ceiling_reached_is_a_ceiling_passed() {
        let model = Model {
            per_byte: 1.0,
            overhead: 0,
        };
        assert!(measure(500_000, &model, 500_000).past_the_ceiling);
        assert!(!measure(499_999, &model, 500_000).past_the_ceiling);
    }

    /// A ceiling of zero is nobody's decision, and a verdict made from it would
    /// declare every session full the moment it opened.
    #[test]
    fn no_ceiling_declared_means_no_verdict() {
        assert!(!measure(u64::MAX / 2, &Model::default(), 0).past_the_ceiling);
    }

    #[test]
    fn two_measurements_give_back_the_line_they_came_from() {
        let real = Model {
            per_byte: 0.5,
            overhead: 40_000,
        };
        let fitted = Model::between(
            Observation {
                bytes: 100_000,
                tokens: real.tokens_for(100_000),
            },
            Observation {
                bytes: 300_000,
                tokens: real.tokens_for(300_000),
            },
        )
        .expect("two different measurements fit a line");
        assert!((fitted.per_byte - 0.5).abs() < 0.001, "{fitted:?}");
        assert!(fitted.overhead.abs_diff(40_000) < 10, "{fitted:?}");
    }

    #[test]
    fn the_order_the_two_measurements_arrive_in_does_not_matter() {
        let early = Observation {
            bytes: 10_000,
            tokens: 50_000,
        };
        let late = Observation {
            bytes: 90_000,
            tokens: 90_000,
        };
        assert_eq!(Model::between(early, late), Model::between(late, early));
    }

    /// Tokens that went down mean the context was reset between the two, so the
    /// pair does not describe one line. Fitting it would give a negative slope
    /// and an estimate that falls as the session fills.
    #[test]
    fn a_pair_that_straddles_a_reset_fits_nothing() {
        let before = Observation {
            bytes: 10_000,
            tokens: 400_000,
        };
        let after = Observation {
            bytes: 20_000,
            tokens: 60_000,
        };
        assert_eq!(Model::between(before, after), None);
    }

    #[test]
    fn two_measurements_at_the_same_byte_count_fit_nothing() {
        let one = Observation {
            bytes: 10_000,
            tokens: 50_000,
        };
        let other = Observation {
            bytes: 10_000,
            tokens: 90_000,
        };
        assert_eq!(Model::between(one, other), None);
    }
}
