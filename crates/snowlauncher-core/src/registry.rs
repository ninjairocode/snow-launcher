//! Reading Wine's registry files (`user.reg`, `system.reg`) as text, to
//! check that a change really took effect. Read-only; writing goes through
//! Wine's own tools (`regedit`, `winecfg`).
//!
//! The files look like:
//!
//! ```text
//! [Software\\Wine\\Fonts] 1795804783
//! #time=1dd7e9eab9a2f98
//! "Codepages"="1252,850"
//! "LogPixels"=dword:000000c0
//! ```

/// Raw value of `name` in `section` (as written in the file, with doubled
/// backslashes), e.g. `dword:000000c0` or `"Microsoft Windows 11"`.
pub fn value<'a>(registry: &'a str, section: &str, name: &str) -> Option<&'a str> {
    let header = format!("[{section}]");
    let quoted_name = format!("\"{name}\"=");
    let mut in_section = false;
    for line in registry.lines() {
        if line.starts_with('[') {
            // `[Section] 1795804783`: the timestamp after the name varies.
            // Compare the whole header: names may contain spaces
            // (`Control Panel`).
            in_section = line
                .strip_prefix(header.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '));
        } else if in_section {
            if let Some(raw) = line.strip_prefix(&quoted_name) {
                return Some(raw);
            }
        }
    }
    None
}

/// A `dword:000000c0` value as a number.
pub fn dword(raw: &str) -> Option<u32> {
    u32::from_str_radix(raw.strip_prefix("dword:")?, 16).ok()
}

/// A `"text"` value without the quotes.
pub fn string(raw: &str) -> Option<&str> {
    raw.strip_prefix('"')?.strip_suffix('"')
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shaped like the user.reg of a prefix created by umu + GE-Proton.
    const USER_REG: &str = "WINE REGISTRY Version 2\n\
;; All keys relative to \\\\User\\\\S-1-5-21-0-0-0-1000\n\
\n\
[Control Panel\\\\Desktop] 1795802381\n\
#time=1dd7e9e00000000\n\
\"LogPixels\"=dword:000000c0\n\
\n\
[Software\\\\Wine\\\\Fonts] 1795804783\n\
#time=1dd7e9eab9a2f98\n\
\"Codepages\"=\"1252,850\"\n\
\"LogPixels\"=dword:00000060\n";

    #[test]
    fn finds_values_in_the_right_section() {
        assert_eq!(
            value(USER_REG, r"Control Panel\\Desktop", "LogPixels"),
            Some("dword:000000c0")
        );
        assert_eq!(
            value(USER_REG, r"Software\\Wine\\Fonts", "LogPixels"),
            Some("dword:00000060")
        );
        assert_eq!(value(USER_REG, r"Software\\Wine\\Fonts", "Missing"), None);
        assert_eq!(value(USER_REG, r"Software\\Wine", "LogPixels"), None);
    }

    #[test]
    fn decodes_dwords_and_strings() {
        assert_eq!(dword("dword:000000c0"), Some(192));
        assert_eq!(dword("\"192\""), None);
        assert_eq!(
            string("\"Microsoft Windows 11\""),
            Some("Microsoft Windows 11")
        );
        assert_eq!(string("dword:1"), None);
    }
}
