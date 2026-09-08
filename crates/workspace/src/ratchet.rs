//! A seed weighed against what the tree holds: the one comparison every
//! ratchet of this repository makes, written once so it can be proved.

/// **Zero.** A seed is a number in a file, and a file merges: a merge keeping
/// the older side takes a re-measure back off with no conflict and no signal.
pub const HOW_STALE_A_SEED_MAY_BE: usize = 0;

/// Named for the tree and not for the debt: a count that may only fall and one
/// that may only rise read the same two sides in opposite ways.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weighed {
    Holds,
    TreeIsAbove(usize),
    TreeIsBelow(usize),
}

impl Weighed {
    pub fn holds(self) -> bool {
        matches!(self, Weighed::Holds)
    }
}

pub fn weigh(seed: usize, measured: usize) -> Weighed {
    if measured > seed + HOW_STALE_A_SEED_MAY_BE {
        Weighed::TreeIsAbove(measured - seed)
    } else if seed > measured + HOW_STALE_A_SEED_MAY_BE {
        Weighed::TreeIsBelow(seed - measured)
    } else {
        Weighed::Holds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEEDS_THE_TREE_CARRIES: &[usize] = &[0, 1, 4, 5, 12, 18, 138, 416, 589, 1927];

    #[test]
    fn a_tree_that_moved_one_past_its_seed_is_weighed_above_it() {
        for &seed in SEEDS_THE_TREE_CARRIES {
            assert_eq!(weigh(seed, seed + 1), Weighed::TreeIsAbove(1), "seed {seed}");
            assert!(!weigh(seed, seed + 1).holds(), "seed {seed}");
        }
    }

    #[test]
    fn a_seed_left_one_above_the_tree_is_weighed_below_it() {
        for &seed in SEEDS_THE_TREE_CARRIES.iter().filter(|seed| **seed > 0) {
            assert_eq!(weigh(seed, seed - 1), Weighed::TreeIsBelow(1), "seed {seed}");
            assert!(!weigh(seed, seed - 1).holds(), "seed {seed}");
        }
    }

    #[test]
    fn only_the_number_that_was_measured_holds() {
        for &seed in SEEDS_THE_TREE_CARRIES {
            assert!(weigh(seed, seed).holds(), "seed {seed}");
        }
    }

    #[test]
    fn the_distance_is_the_one_a_person_would_have_to_write() {
        assert_eq!(weigh(416, 420), Weighed::TreeIsAbove(4));
        assert_eq!(weigh(1927, 1900), Weighed::TreeIsBelow(27));
    }

    /// Every assertion above is worth what this constant is: raise it and both
    /// directions go quiet by one, the width of the merge that raises a seed.
    #[test]
    fn no_room_is_left_for_a_seed_that_was_not_re_measured() {
        assert_eq!(HOW_STALE_A_SEED_MAY_BE, 0);
    }
}
