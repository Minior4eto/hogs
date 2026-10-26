/// Human-readable size using binary units: 1536 -> "1.5 KiB".
pub fn fmt_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KiB", "MiB", "GiB", "TiB", "PiB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Keep the end of a string so it fits in `width` columns: "…/very/long/path".
pub fn truncate_left(s: &str, width: usize) -> String {
    let count = s.chars().count();
    if count <= width || width == 0 {
        return s.to_string();
    }
    let tail: String = s.chars().skip(count - (width - 1)).collect();
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(fmt_size(0), "0 B");
        assert_eq!(fmt_size(1023), "1023 B");
        assert_eq!(fmt_size(1536), "1.5 KiB");
        assert_eq!(fmt_size(5 * 1024 * 1024 * 1024), "5.0 GiB");
    }

    #[test]
    fn truncation() {
        assert_eq!(truncate_left("/a/b", 10), "/a/b");
        assert_eq!(truncate_left("/home/user/stuff", 6), "…stuff");
    }
}
