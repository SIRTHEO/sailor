//! A subscription window is fuel that expires: what is left of it at the
//! reset is lost. The dispatcher prefers the engine whose window would
//! otherwise expire unused, and says so.

use crate::remaining::Remaining;

/// One window of one engine, as fuel: how much is left and for how long.
#[derive(Debug, Clone, PartialEq)]
pub struct Fuel {
    pub engine: String,
    pub unit: String,
    /// From `0.0` (spent) to `1.0` (untouched).
    pub left_fraction: f64,
    /// Seconds to the reset, when the provider said an instant this reads.
    pub resets_in_secs: Option<i64>,
}

impl Fuel {
    pub fn from_remaining(remaining: &Remaining, now: i64) -> Fuel {
        Fuel {
            engine: remaining.engine.clone(),
            unit: remaining.unit.clone(),
            left_fraction: (1.0 - remaining.used_fraction).clamp(0.0, 1.0),
            resets_in_secs: remaining
                .resets_at
                .as_deref()
                .and_then(unix_secs_of_rfc3339)
                .map(|at| (at - now).max(0)),
        }
    }

    /// How much of this window expires per hour if nobody spends it: the
    /// number the preference sorts on. A window without a reset is `0.0`.
    pub fn expiring_per_hour(&self) -> f64 {
        match self.resets_in_secs {
            Some(secs) if self.left_fraction > 0.0 => self.left_fraction / (secs.max(1) as f64 / 3600.0),
            _ => 0.0,
        }
    }
}

/// The engine to prefer and the why, written for a person.
#[derive(Debug, Clone, PartialEq)]
pub struct Preference {
    pub engine: String,
    pub why: String,
}

/// Among the windows read, the engine whose fuel expires fastest unused; a
/// tie keeps the first. `None` when nothing was read or nothing expires.
pub fn prefer(fuels: &[Fuel]) -> Option<Preference> {
    let best = fuels
        .iter()
        .filter(|fuel| fuel.expiring_per_hour() > 0.0)
        .fold(None::<&Fuel>, |best, fuel| match best {
            Some(kept) if kept.expiring_per_hour() >= fuel.expiring_per_hour() => Some(kept),
            _ => Some(fuel),
        })?;
    Some(Preference {
        engine: best.engine.clone(),
        why: format!(
            "«{}» {}: {:.0} % left, resets in {}: expires unused",
            best.engine,
            best.unit,
            best.left_fraction * 100.0,
            spell(best.resets_in_secs.unwrap_or(0))
        ),
    })
}

fn spell(secs: i64) -> String {
    match secs {
        s if s < 3600 => format!("{} min", s / 60),
        s if s < 86_400 => format!("{} h", s / 3600),
        s => format!("{} days", s / 86_400),
    }
}

/// `2026-09-01T03:29:59.801054+00:00`, `...Z`, or `...+02:00` to unix seconds;
/// anything else is `None`, never a guessed instant.
pub fn unix_secs_of_rfc3339(text: &str) -> Option<i64> {
    let text = text.trim();
    let (date, rest) = text.split_once('T')?;
    let mut parts = date.splitn(3, '-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: u32 = parts.next()?.parse().ok()?;
    let day: u32 = parts.next()?.parse().ok()?;
    let (clock, offset) = match rest.find(['Z', 'z', '+']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => {
            let at = rest.rfind('-')?;
            (&rest[..at], &rest[at..])
        }
    };
    let clock = clock.split('.').next()?;
    let mut parts = clock.splitn(3, ':');
    let hour: i64 = parts.next()?.parse().ok()?;
    let minute: i64 = parts.next()?.parse().ok()?;
    let second: i64 = parts.next()?.parse().ok()?;
    let offset_secs = match offset {
        "Z" | "z" => 0,
        signed => {
            let sign = if signed.starts_with('-') { -1 } else { 1 };
            let (oh, om) = signed[1..].split_once(':')?;
            sign * (oh.parse::<i64>().ok()? * 3600 + om.parse::<i64>().ok()? * 60)
        }
    };
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset_secs)
}

/// Days since 1970-01-01 of a proleptic Gregorian date (Howard Hinnant's).
fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fuel(engine: &str, left: f64, resets_in: Option<i64>) -> Fuel {
        Fuel {
            engine: engine.to_owned(),
            unit: "five_hour".to_owned(),
            left_fraction: left,
            resets_in_secs: resets_in,
        }
    }

    #[test]
    fn the_window_that_expires_unused_soonest_is_preferred_and_says_why() {
        // 10 % left in an hour expires faster than 80 % left in six days.
        let soon = fuel("soon", 0.10, Some(3600));
        let later = fuel("later", 0.80, Some(6 * 86_400));
        let chosen = prefer(&[later.clone(), soon.clone()]).expect("one expires");
        assert_eq!(chosen.engine, "soon");
        assert!(chosen.why.contains("expires unused") && chosen.why.contains("10 % left"), "{}", chosen.why);
        // The control: a window without a reset, or already spent, is never preferred.
        assert_eq!(prefer(&[fuel("x", 0.5, None), fuel("y", 0.0, Some(60))]), None);
        // A tie keeps the order the windows were read in.
        assert_eq!(prefer(&[fuel("a", 0.5, Some(60)), fuel("b", 0.5, Some(60))]).unwrap().engine, "a");
    }

    #[test]
    fn a_provider_instant_reads_to_seconds_and_a_strange_one_reads_to_nothing() {
        assert_eq!(unix_secs_of_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(unix_secs_of_rfc3339("2026-09-03T00:00:00+00:00"), Some(1_788_393_600));
        assert_eq!(unix_secs_of_rfc3339("2026-09-03T02:00:00+02:00"), Some(1_788_393_600));
        assert_eq!(unix_secs_of_rfc3339("2026-09-01T03:29:59.801054+00:00"), Some(1_788_233_399));
        assert_eq!(unix_secs_of_rfc3339("at 7am"), None);
        assert_eq!(unix_secs_of_rfc3339("2026-13-01T00:00:00Z"), None);
    }
}
