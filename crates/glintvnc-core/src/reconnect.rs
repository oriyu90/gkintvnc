//! Pure deterministic policy; caller decides whether an error is retryable.
use std::time::Duration;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Transport,
    Timeout,
    Protocol,
    Authentication,
    Certificate,
    Cancelled,
}
pub fn retryable(f: Failure) -> bool {
    matches!(f, Failure::Transport | Failure::Timeout)
}

const MIN_SECS: u64 = 1;
const MAX_SECS: u64 = 30;

/// Plain bounded exponential backoff (1, 2, 4, 8, 16, 30, 30 ...).
/// Deterministic; used when a jitter source is not available.
pub fn backoff(attempt: u32) -> Duration {
    let power = attempt.saturating_sub(1).min(5);
    Duration::from_secs((1u64 << power).min(MAX_SECS))
}

/// Decorrelated exponential backoff with full jitter (ROADMAP P1-02):
/// `min(cap, random_between(base, base * 3))` per the "Full Jitter" /
/// "decorrelated jitter" literature. `rand_unit` must be in [0, 1); pass a
/// fresh random value per attempt. The result is always within [1s, 30s].
pub fn backoff_decorrelated(attempt: u32, rand_unit: f64) -> Duration {
    let base = backoff(attempt).as_secs_f64();
    let hi = (base * 3.0).min(MAX_SECS as f64).max(MIN_SECS as f64);
    let unit = if rand_unit.is_finite() {
        rand_unit.clamp(0.0, 0.999_999)
    } else {
        0.0
    };
    let secs = MIN_SECS as f64 + (hi - MIN_SECS as f64) * unit;
    Duration::from_secs_f64(secs.min(MAX_SECS as f64))
}

/// Runtime jitter source seeded from monotonic-ish clock nanos. Not used for
/// anything security-sensitive, only retry timing.
pub fn jitter_unit() -> f64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // xorshift64* on the nanos stamp; good enough for jitter.
    let mut x = (nanos as u64) ^ 0x9E37_79B9_7F4A_7C15;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    (v >> 11) as f64 / (1u64 << 53) as f64
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn auth_failure_never_retries() {
        assert!(!retryable(Failure::Authentication));
        assert!(!retryable(Failure::Certificate));
    }
    #[test]
    fn bounded() {
        assert_eq!(backoff(100), Duration::from_secs(30));
    }
    #[test]
    fn decorrelated_stays_in_bounds() {
        for attempt in 1..=12 {
            for u in [0.0, 0.25, 0.5, 0.75, 0.999_999] {
                let d = backoff_decorrelated(attempt, u);
                assert!(d >= Duration::from_secs(MIN_SECS));
                assert!(d <= Duration::from_secs(MAX_SECS));
            }
        }
    }
    #[test]
    fn decorrelated_monotonic_ceiling() {
        // Higher attempts should allow (but not force) longer waits.
        assert!(backoff_decorrelated(6, 0.999_999) >= backoff_decorrelated(1, 0.999_999));
    }
    #[test]
    fn jitter_unit_range() {
        for _ in 0..64 {
            let u = jitter_unit();
            assert!((0.0..1.0).contains(&u));
        }
    }
}
