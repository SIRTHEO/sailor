//! When a flow is due, at what weight, and inside which perimeter. The twelve
//! nightly jobs lost all three when they became flows: not by carelessness, but
//! because the flow format had nowhere to put them, so they ended up in the
//! prose of the description, where no program reads them. Until they stay
//! there a cron cannot be converted — you would convert *what* it does and
//! lose *when* it does it.

use serde::{Deserialize, Serialize};

/// How often a flow must run. Two shapes, because two are what exist on this
/// machine: nightly work, which wants a time of day, and system crons, which
/// want an interval — the relay every 60 seconds, the swap log every 300. A
/// third gets added when a real case asks for one: a complete scheduling
/// language written before the case is the thing nobody uses and nobody dares
/// remove.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Recurrence {
    /// Every N seconds since the last run. A flow that never ran is due at once.
    EverySeconds { seconds: u64 },
    /// Once a day, from that local hour onwards.
    DailyAt { hour: u32, minute: u32 },
}

/// What a run costs, as declared by whoever writes the flow. It decides nothing
/// today, and saying so beats letting a reader assume otherwise: a figure the
/// engine reports, not a brake it applies. It exists because the distinction
/// was there in the nightly jobs — light ones under the minute, heavy ones up
/// to twelve — and losing it means losing the ability to say, later, why a
/// night went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weight {
    Light,
    Heavy,
}

/// When it runs, how much it weighs, where it may write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schedule {
    pub recurrence: Recurrence,
    pub weight: Weight,
    /// The directories this job may write inside, as the queue entry it came
    /// from declared them. Empty means "not declared", which is different from
    /// "no limit": a reader must be able to tell the two apart.
    #[serde(default)]
    pub perimeter: Vec<String>,
}

/// Is the flow due now? `now` is an argument and not a clock read in here: a
/// decision that reads the clock itself can only be tested by waiting, and a
/// test that waits never gets written. A flow that never ran (`last_run` is
/// `None`) is always due — a first run has no delay to wait out. `DailyAt` asks
/// "did it already run today, after that hour?", judged on the local day, and
/// not "have 24 hours passed": the two diverge every time a run slips.
pub fn is_due(schedule: &Schedule, last_run: Option<i64>, now: i64) -> bool {
    let Some(last) = last_run else {
        return true;
    };
    match schedule.recurrence {
        Recurrence::EverySeconds { seconds } => now.saturating_sub(last) >= seconds as i64,
        Recurrence::DailyAt { hour, minute } => {
            let today = start_of_local_day(now);
            let at = today + (hour as i64) * 3600 + (minute as i64) * 60;
            now >= at && last < at
        }
    }
}

/// Local midnight of the day containing `now`.
///
/// The offset is derived, not asked of a library: the workspace keeps its
/// dependencies to a minimum. `localtime_r` gives the local fields, from which
/// we recover seconds since midnight and subtract. It holds on days the clock
/// shifts: the offset is measured in that day, not assumed constant.
fn start_of_local_day(now: i64) -> i64 {
    let seconds_today = local_seconds_of_day(now);
    now - seconds_today
}

#[cfg(unix)]
fn local_seconds_of_day(now: i64) -> i64 {
    // `libc` is not a dependency, and one call does not justify adding it:
    // the declaration lives here, next to its use.
    extern "C" {
        fn localtime_r(time: *const i64, result: *mut Tm) -> *mut Tm;
    }
    #[repr(C)]
    #[derive(Default)]
    struct Tm {
        sec: i32,
        min: i32,
        hour: i32,
        mday: i32,
        mon: i32,
        year: i32,
        wday: i32,
        yday: i32,
        isdst: i32,
        gmtoff: i64,
        zone: *const i8,
    }
    let mut out = Tm {
        zone: std::ptr::null(),
        ..Default::default()
    };
    // SAFETY: `now` is a valid integer and `out` is a struct that lives for the
    // whole call; `localtime_r` writes only inside it.
    let filled = unsafe { localtime_r(&now, &mut out) };
    if filled.is_null() {
        // No local time: fall back to UTC, which is always computable.
        return now.rem_euclid(86_400);
    }
    (out.hour as i64) * 3600 + (out.min as i64) * 60 + out.sec as i64
}

#[cfg(not(unix))]
fn local_seconds_of_day(now: i64) -> i64 {
    now.rem_euclid(86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn every(seconds: u64) -> Schedule {
        Schedule {
            recurrence: Recurrence::EverySeconds { seconds },
            weight: Weight::Light,
            perimeter: Vec::new(),
        }
    }

    fn daily(hour: u32, minute: u32) -> Schedule {
        Schedule {
            recurrence: Recurrence::DailyAt { hour, minute },
            weight: Weight::Heavy,
            perimeter: vec!["~/.claude".to_string()],
        }
    }

    #[test]
    fn a_flow_that_never_ran_is_due_at_once() {
        assert!(is_due(&every(60), None, 1_000_000));
        assert!(is_due(&daily(3, 0), None, 1_000_000));
    }

    /// Both sides of the interval: one second early no, on the second yes. The
    /// 60 is not an arbitrary number — the relay runs every 60 seconds, and
    /// this is the threshold that has to hold.
    #[test]
    fn an_interval_is_due_only_once_the_seconds_have_passed() {
        let now = 1_000_000;
        assert!(!is_due(&every(60), Some(now - 59), now));
        assert!(is_due(&every(60), Some(now - 60), now));
        assert!(is_due(&every(60), Some(now - 6000), now));
    }

    /// A time of day asks "did it already run today?", not "have 24 hours
    /// passed". The third case is the one that matters: last night's run, later
    /// in the clock than today's hour, must not count as today's run.
    #[test]
    fn a_daily_flow_asks_whether_it_already_ran_today() {
        let midnight = start_of_local_day(1_800_000_000);
        let three_in_the_morning = midnight + 3 * 3600;
        let schedule = daily(3, 0);

        // Half past two: the hour has not arrived yet.
        assert!(!is_due(&schedule, Some(midnight - 100), midnight + 9000));
        // Three sharp, and the last run was yesterday: due.
        assert!(is_due(
            &schedule,
            Some(midnight - 100),
            three_in_the_morning
        ));
        // Four, but it already ran at one past three: it does not repeat.
        assert!(!is_due(
            &schedule,
            Some(three_in_the_morning + 60),
            midnight + 4 * 3600
        ));
    }

    /// The on-disk shape is a contract with the window and with whoever writes
    /// flows by hand: change it and those files stop loading, silently.
    #[test]
    fn the_schedule_reads_back_exactly_as_it_is_written() {
        let text = r#"{
            "recurrence": {"kind": "daily_at", "hour": 3, "minute": 30},
            "weight": "heavy",
            "perimeter": ["~/.claude", "~/progetti/sailor"]
        }"#;
        let parsed: Schedule = serde_json::from_str(text).unwrap();
        assert_eq!(
            parsed.recurrence,
            Recurrence::DailyAt {
                hour: 3,
                minute: 30
            }
        );
        assert_eq!(parsed.weight, Weight::Heavy);
        assert_eq!(parsed.perimeter.len(), 2);

        let again: Schedule =
            serde_json::from_str(&serde_json::to_string(&parsed).unwrap()).unwrap();
        assert_eq!(again, parsed);
    }

    /// An absent perimeter is not a declared empty one, though here the two
    /// coincide in shape. What matters is that the absence does not fail the
    /// load of a flow written before the field existed.
    #[test]
    fn an_older_flow_without_a_perimeter_still_loads() {
        let text = r#"{"recurrence": {"kind": "every_seconds", "seconds": 60}, "weight": "light"}"#;
        let parsed: Schedule = serde_json::from_str(text).unwrap();
        assert!(parsed.perimeter.is_empty());
    }
}
