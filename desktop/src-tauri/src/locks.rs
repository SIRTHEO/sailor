//! Taking a lock this shell holds.

use std::sync::{Mutex, MutexGuard, PoisonError};

/// The guard, even when another thread died holding the lock.
///
/// What sits under every lock in this shell is whole after a panic, and the
/// thread that panicked has already reported itself. Refusing the guard here
/// would end the window over a fault somebody else already named.
pub(crate) fn locked<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::locked;
    use std::sync::{Arc, Mutex};

    #[test]
    fn a_lock_a_dead_thread_held_still_hands_over_what_is_under_it() {
        let held = Arc::new(Mutex::new(vec!["said before".to_owned()]));
        let taken = held.clone();
        let died = std::thread::spawn(move || {
            let _guard = locked(&taken);
            panic!("a thread dies under the lock");
        });
        assert!(died.join().is_err(), "the thread had to die holding it");

        let mut under = locked(&held);
        under.push("said after".to_owned());
        assert_eq!(
            under.as_slice(),
            ["said before".to_owned(), "said after".to_owned()],
            "what was under a poisoned lock is still whole"
        );
    }
}
