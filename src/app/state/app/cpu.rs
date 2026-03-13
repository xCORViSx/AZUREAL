//! CPU usage monitoring for the status bar

use super::App;

impl App {
    /// Sample getrusage and update cached CPU% string. Called from draw path;
    /// only recomputes if ≥1s has elapsed since last sample (avoids overhead).
    /// Normalizes by core count to match OS task manager conventions.
    pub fn update_cpu_usage(&mut self) {
        let now = std::time::Instant::now();
        let elapsed = now.duration_since(self.cpu_last_sample.0);
        // Sample every 3s — longer window averages out Windows timer tick noise
        // (GetProcessTimes only updates at ~15.6ms granularity)
        if elapsed.as_millis() < 3000 { return; }
        let cpu_now = get_cpu_time_micros();
        let cpu_delta = cpu_now.saturating_sub(self.cpu_last_sample.1) as f64;
        let wall_delta = elapsed.as_micros() as f64;
        let cores = std::thread::available_parallelism()
            .map(|n| n.get() as f64)
            .unwrap_or(1.0);
        let raw_pct = if wall_delta > 0.0 { cpu_delta / wall_delta / cores * 100.0 } else { 0.0 };
        // Exponential moving average (alpha=0.2) for heavy smoothing
        self.cpu_smoothed = if self.cpu_smoothed == 0.0 { raw_pct } else { self.cpu_smoothed * 0.8 + raw_pct * 0.2 };
        // Floor: show "0%" for values under 0.5 to match Task Manager conventions
        let display = if self.cpu_smoothed < 0.5 { 0.0 } else { self.cpu_smoothed };
        self.cpu_usage_text = format!("{:.0}%", display);
        self.cpu_last_sample = (now, cpu_now);
    }
}

/// Get cumulative user+system CPU time for this process in microseconds.
#[cfg(unix)]
pub(crate) fn get_cpu_time_micros() -> u64 {
    unsafe {
        let mut usage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut usage);
        let user = usage.ru_utime.tv_sec as u64 * 1_000_000 + usage.ru_utime.tv_usec as u64;
        let sys = usage.ru_stime.tv_sec as u64 * 1_000_000 + usage.ru_stime.tv_usec as u64;
        user + sys
    }
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
            kernel_us + user_us
        } else {
            0
        }
    }
}
