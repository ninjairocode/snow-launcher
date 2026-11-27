//! What the desktop tells X apps (where Wine draws). Read-only.

use std::process::Command;

/// The desktop's `Xft.dpi`, as published to X apps (GNOME and KDE set it
/// from the chosen scale). `None` if there is no X server, `xrdb` is
/// missing or the value is not set.
pub fn desktop_dpi() -> Option<u32> {
    let output = Command::new("xrdb").arg("-query").output().ok()?;
    if !output.status.success() {
        return None;
    }
    parse_xft_dpi(&String::from_utf8_lossy(&output.stdout))
}

/// Finds `Xft.dpi:\t192` in `xrdb -query` output.
fn parse_xft_dpi(resources: &str) -> Option<u32> {
    resources.lines().find_map(|line| {
        let value = line.strip_prefix("Xft.dpi:")?.trim();
        // Some desktops write it as a float ("192.0").
        let whole = value.split('.').next()?;
        whole.parse().ok()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_xft_dpi() {
        let output = "Xcursor.size:\t24\nXft.antialias:\t1\nXft.dpi:\t192\nXft.hinting:\t1\n";
        assert_eq!(parse_xft_dpi(output), Some(192));
        assert_eq!(parse_xft_dpi("Xft.dpi: 120.0\n"), Some(120));
    }

    #[test]
    fn missing_or_odd_values_are_none() {
        assert_eq!(parse_xft_dpi("Xcursor.size:\t24\n"), None);
        assert_eq!(parse_xft_dpi("Xft.dpi:\tlots\n"), None);
        assert_eq!(parse_xft_dpi(""), None);
    }
}
