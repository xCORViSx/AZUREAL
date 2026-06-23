//! CPU usage monitoring for the status bar

use super::App;

/// CPU usage sampling and formatting methods for application state.
impl App {
    /// Sample getrusage and update cached CPU% string. Called from draw path;
    /// only recomputes if ≥1s has elapsed since last sample (avoids overhead).
    /// Normalizes by core count to match OS task manager conventions.
    pub fn update_cpu_usage(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.cpu_last_sample.0);
        // Sample every 3s — longer window averages out Windows timer tick noise
        // (GetProcessTimes only updates at ~15.6ms granularity)
        if elapsed.as_millis() < 3000 {
            return;
        }
        let cpu_now = get_cpu_time_micros();
        let Some(cpu_delta) = cpu_delta_micros(self.cpu_last_sample.1, cpu_now) else {
            return;
        };
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        let raw_pct = cpu_percent_from_samples(cpu_delta, elapsed.as_micros(), cores);
        // Exponential moving average (alpha=0.2) for heavy smoothing
        self.cpu_smoothed = smooth_cpu_percent(self.cpu_smoothed, raw_pct);
        // Floor: show "0%" for values under 0.5 to match Task Manager conventions
        let display = display_cpu_percent(self.cpu_smoothed);
        self.cpu_usage_text = format!("{:.0}%", display);
        self.cpu_last_sample = (now, cpu_now);
    }
}

/// Return the CPU counter delta, rejecting counter regressions from failed samples.
fn cpu_delta_micros(previous: u64, current: u64) -> Option<u64> {
    current.checked_sub(previous)
}

/// Convert two monotonic samples into a normalized CPU percentage.
fn cpu_percent_from_samples(
    cpu_delta_micros: u64,
    wall_delta_micros: u128,
    core_count: usize,
) -> f64 {
    if wall_delta_micros == 0 || core_count == 0 {
        return 0.0;
    }

    let raw = cpu_delta_micros as f64 / wall_delta_micros as f64 / core_count as f64 * 100.0;
    if raw.is_finite() && raw >= 0.0 {
        raw
    } else {
        0.0
    }
}

/// Apply the status-bar smoothing function after sanitizing non-finite inputs.
fn smooth_cpu_percent(previous: f64, raw: f64) -> f64 {
    let previous = if previous.is_finite() && previous >= 0.0 {
        previous
    } else {
        0.0
    };
    let raw = if raw.is_finite() && raw >= 0.0 {
        raw
    } else {
        0.0
    };

    if previous == 0.0 {
        raw
    } else {
        previous * 0.8 + raw * 0.2
    }
}

/// Normalize the displayed percentage so invalid or tiny values render as zero.
fn display_cpu_percent(percent: f64) -> f64 {
    if percent.is_finite() && percent >= 0.5 {
        percent
    } else {
        0.0
    }
}

/// Get cumulative user+system CPU time for this process in microseconds.
#[cfg(unix)]
pub(crate) fn get_cpu_time_micros() -> u64 {
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        if libc::getrusage(libc::RUSAGE_SELF, &mut usage) != 0 {
            return 0;
        }
        rusage_cpu_time_micros(&usage).unwrap_or(0)
    }
}

/// Convert a Unix timeval into microseconds when it satisfies timeval invariants.
#[cfg(unix)]
fn timeval_to_micros(value: libc::timeval) -> Option<u64> {
    let seconds = u64::try_from(value.tv_sec).ok()?;
    let micros = u64::try_from(value.tv_usec).ok()?;
    if micros >= 1_000_000 {
        return None;
    }
    seconds.checked_mul(1_000_000)?.checked_add(micros)
}

/// Sum user and system CPU time from rusage, rejecting invalid kernel data.
#[cfg(unix)]
fn rusage_cpu_time_micros(usage: &libc::rusage) -> Option<u64> {
    let user = timeval_to_micros(usage.ru_utime)?;
    let system = timeval_to_micros(usage.ru_stime)?;
    user.checked_add(system)
}

/// Get cumulative user+system CPU time for this process in microseconds.
#[cfg(windows)]
pub(crate) fn get_cpu_time_micros() -> u64 {
    use std::mem::MaybeUninit;
    unsafe {
        let handle = windows_sys::Win32::System::Threading::GetCurrentProcess();
        let mut creation = MaybeUninit::zeroed();
        let mut exit = MaybeUninit::zeroed();
        let mut kernel = MaybeUninit::zeroed();
        let mut user = MaybeUninit::zeroed();
        if windows_sys::Win32::System::Threading::GetProcessTimes(
            handle,
            creation.as_mut_ptr(),
            exit.as_mut_ptr(),
            kernel.as_mut_ptr(),
            user.as_mut_ptr(),
        ) != 0
        {
            let k = kernel.assume_init();
            let u = user.assume_init();
            // FILETIME is 100ns intervals → divide by 10 for microseconds
            let kernel_us = (k.dwLowDateTime as u64 | (k.dwHighDateTime as u64) << 32) / 10;
            let user_us = (u.dwLowDateTime as u64 | (u.dwHighDateTime as u64) << 32) / 10;
            kernel_us.saturating_add(user_us)
        } else {
            0
        }
    }
}

#[cfg(test)]
/// Tests for CPU percentage normalization and OS time conversion.
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// CPU percentage calculations treat missing elapsed time as neutral.
    #[test]
    fn cpu_percent_from_samples_rejects_zero_wall_delta() {
        assert_eq!(cpu_percent_from_samples(1_000, 0, 8), 0.0);
    }

    /// CPU percentage calculations reject an invalid core count before division.
    #[test]
    fn cpu_percent_from_samples_rejects_zero_core_count() {
        assert_eq!(cpu_percent_from_samples(1_000, 1_000, 0), 0.0);
    }

    /// CPU counter deltas reject regressions instead of resetting the baseline.
    #[test]
    fn cpu_delta_micros_rejects_counter_regression() {
        assert_eq!(cpu_delta_micros(1_000, 900), None);
        assert_eq!(cpu_delta_micros(1_000, 1_250), Some(250));
    }

    /// CPU smoothing recovers from non-finite prior state and raw samples.
    #[test]
    fn smooth_cpu_percent_normalizes_non_finite_inputs() {
        assert_eq!(smooth_cpu_percent(f64::NAN, 25.0), 25.0);
        assert_eq!(smooth_cpu_percent(25.0, f64::INFINITY), 20.0);
        assert_eq!(smooth_cpu_percent(f64::NEG_INFINITY, f64::NAN), 0.0);
    }

    /// Display formatting suppresses non-finite and tiny percentages.
    #[test]
    fn display_cpu_percent_filters_invalid_values() {
        assert_eq!(display_cpu_percent(f64::NAN), 0.0);
        assert_eq!(display_cpu_percent(f64::INFINITY), 0.0);
        assert_eq!(display_cpu_percent(0.49), 0.0);
        assert_eq!(display_cpu_percent(1.25), 1.25);
    }

    /// Updating the app status recovers from a corrupted smoothing cache.
    #[test]
    fn update_cpu_usage_recovers_from_non_finite_smoothing_state() {
        let mut app = App::new();
        app.cpu_smoothed = f64::NAN;
        app.cpu_last_sample = (
            Instant::now() - Duration::from_secs(4),
            get_cpu_time_micros(),
        );

        app.update_cpu_usage();

        assert!(!app.cpu_smoothed.is_nan());
        assert!(!app.cpu_usage_text.contains("NaN"));
    }

    /// Unix timeval conversion accepts valid seconds and microseconds.
    #[cfg(unix)]
    #[test]
    fn timeval_to_micros_accepts_valid_values() {
        let timeval = libc::timeval {
            tv_sec: 2,
            tv_usec: 500_000,
        };
        assert_eq!(timeval_to_micros(timeval), Some(2_500_000));
    }

    /// Unix timeval conversion rejects negative metadata from the OS boundary.
    #[cfg(unix)]
    #[test]
    fn timeval_to_micros_rejects_negative_values() {
        let timeval = libc::timeval {
            tv_sec: -1,
            tv_usec: 0,
        };
        assert_eq!(timeval_to_micros(timeval), None);
    }

    /// Unix timeval conversion rejects out-of-range microsecond fields.
    #[cfg(unix)]
    #[test]
    fn timeval_to_micros_rejects_invalid_microseconds() {
        let timeval = libc::timeval {
            tv_sec: 1,
            tv_usec: 1_000_000,
        };
        assert_eq!(timeval_to_micros(timeval), None);
    }
}
