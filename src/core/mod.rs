//! The erasing engine: no UI, no network. Used by the app and by `--cli`.

pub mod drives;
pub mod erase;
pub mod freespace;
pub mod method;
pub mod progress;
pub mod rng;

/// "1.5 GB" style sizes (decimal units, like the OS file managers on Windows/macOS).
pub fn format_bytes(b: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut v = b as f64;
    let mut u = 0;
    while v >= 1000.0 && u < UNITS.len() - 1 {
        v /= 1000.0;
        u += 1;
    }
    if u == 0 {
        format!("{b} B")
    } else if v >= 100.0 {
        format!("{v:.0} {}", UNITS[u])
    } else {
        format!("{v:.1} {}", UNITS[u])
    }
}

/// "1h 02m", "3m 05s", "12s".
pub fn format_duration(d: std::time::Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else if s >= 60 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else {
        format!("{s}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500_000), "1.5 MB");
        assert_eq!(format_bytes(250_000_000_000), "250 GB");
        assert_eq!(format_duration(std::time::Duration::from_secs(3725)), "1h 02m");
        assert_eq!(format_duration(std::time::Duration::from_secs(65)), "1m 05s");
    }
}
