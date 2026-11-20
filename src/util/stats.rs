//! Process statistics used to report the performance numbers in the README
//! (startup time, peak resident memory).

/// Peak resident set size of this process in bytes, or 0 when unavailable.
#[cfg(target_os = "linux")]
pub fn peak_rss_bytes() -> u64 {
    // /proc/self/status carries VmHWM (peak resident set size) in kB.
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmHWM:") {
                let kb: u64 = rest
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                return kb * 1024;
            }
        }
    }
    0
}

/// Peak resident set size of this process in bytes, or 0 when unavailable.
#[cfg(windows)]
pub fn peak_rss_bytes() -> u64 {
    use core::ffi::c_void;

    #[repr(C)]
    struct ProcessMemoryCounters {
        cb: u32,
        page_fault_count: u32,
        peak_working_set_size: usize,
        working_set_size: usize,
        quota_peak_paged_pool_usage: usize,
        quota_paged_pool_usage: usize,
        quota_peak_non_paged_pool_usage: usize,
        quota_non_paged_pool_usage: usize,
        pagefile_usage: usize,
        peak_pagefile_usage: usize,
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut c_void;
    }

    #[link(name = "psapi")]
    extern "system" {
        fn GetProcessMemoryInfo(
            process: *mut c_void,
            counters: *mut ProcessMemoryCounters,
            size: u32,
        ) -> i32;
    }

    let mut counters = ProcessMemoryCounters {
        cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
        page_fault_count: 0,
        peak_working_set_size: 0,
        working_set_size: 0,
        quota_peak_paged_pool_usage: 0,
        quota_paged_pool_usage: 0,
        quota_peak_non_paged_pool_usage: 0,
        quota_non_paged_pool_usage: 0,
        pagefile_usage: 0,
        peak_pagefile_usage: 0,
    };
    // SAFETY: the struct and its size match what GetProcessMemoryInfo expects.
    let ok = unsafe {
        GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut counters,
            std::mem::size_of::<ProcessMemoryCounters>() as u32,
        )
    };
    if ok != 0 {
        counters.peak_working_set_size as u64
    } else {
        0
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn peak_rss_bytes() -> u64 {
    0
}

/// Resident set size in bytes (current, not peak), or 0 when unavailable.
#[cfg(target_os = "linux")]
pub fn rss_bytes() -> u64 {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(rest) = line.strip_prefix("VmRSS:") {
                let kb: u64 = rest
                    .trim()
                    .trim_end_matches(" kB")
                    .trim()
                    .parse()
                    .unwrap_or(0);
                return kb * 1024;
            }
        }
    }
    0
}

#[cfg(not(target_os = "linux"))]
pub fn rss_bytes() -> u64 {
    0
}

/// Human readable byte size ("12.3 MB").
pub fn human_bytes(bytes: u64) -> String {
    let bytes = bytes as f64;
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;
    if bytes >= GB {
        format!("{:.2} GB", bytes / GB)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes / MB)
    } else if bytes >= KB {
        format!("{:.0} KB", bytes / KB)
    } else {
        format!("{} B", bytes as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_bytes_formats_ranges() {
        assert_eq!(human_bytes(512), "512 B");
        assert_eq!(human_bytes(2048), "2 KB");
        assert_eq!(human_bytes(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(human_bytes(3 * 1024 * 1024 * 1024), "3.00 GB");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rss_is_positive_on_linux() {
        assert!(rss_bytes() > 0, "we are running, so RSS must be non-zero");
        assert!(peak_rss_bytes() >= rss_bytes());
    }
}
