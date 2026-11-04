//! The clock: [`ClockPort`] over the system time, with RFC 3339 checked by a
//! real date library.
//!
//! The domain keeps a [`Timestamp`] as an opaque text and checks only its
//! outline; the calendar rules (month 13, day 31 of April) are checked here,
//! where `jiff` is available. [`validate_rfc3339`] is shared with the file
//! parser, which needs the same check for a measurement's time.

use whyshelf_core::app::ports::ClockPort;
use whyshelf_core::domain::actor::Timestamp;
use whyshelf_core::domain::error::ValueError;

/// Why the clock could not give a time.
#[derive(Debug, thiserror::Error)]
pub enum ClockError {
    /// The system time is outside the range the date library handles.
    #[error("the system time is out of range")]
    OutOfRange(#[source] jiff::Error),
    /// The domain refused the text the clock produced (a bug if it ever happens).
    #[error("the domain refused the time the clock produced")]
    Refused(#[source] ValueError),
}

/// Check a text as an RFC 3339 date and time with an offset (`Z` or `+09:00`).
///
/// Why here and not in the domain: calendar rules need a date library, and the
/// domain holds no such dependency.
pub fn validate_rfc3339(text: &str) -> Result<(), jiff::Error> {
    text.parse::<jiff::Timestamp>().map(|_| ())
}

/// The system clock, to the second, in UTC.
///
/// Whole seconds: the time of an event is for display, and a fraction would
/// only make the files longer.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl ClockPort for SystemClock {
    type Error = ClockError;

    async fn now(&self) -> Result<Timestamp, ClockError> {
        let now = jiff::Timestamp::now();
        let whole =
            jiff::Timestamp::from_second(now.as_second()).map_err(ClockError::OutOfRange)?;
        Timestamp::from_validated(whole.to_string()).map_err(ClockError::Refused)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_clock_gives_a_valid_rfc3339_time_in_whole_seconds() {
        let now = SystemClock.now().await.unwrap();
        assert!(validate_rfc3339(now.as_str()).is_ok(), "{now}");
        assert!(now.as_str().ends_with('Z'));
        assert!(!now.as_str().contains('.'));
    }

    #[test]
    fn calendar_rules_are_checked() {
        assert!(validate_rfc3339("2026-11-04T21:40:00+09:00").is_ok());
        assert!(validate_rfc3339("2026-13-04T21:40:00Z").is_err());
        assert!(validate_rfc3339("2026-04-31T21:40:00Z").is_err());
        assert!(validate_rfc3339("2026-11-04").is_err());
    }
}
