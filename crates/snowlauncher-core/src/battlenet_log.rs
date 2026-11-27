//! Reading the Battle.net app's own log (`battle.net-*.log`).
//!
//! Lines look like:
//!
//! ```text
//! I 2026-11-27 01:01:51.945282 [Main] {Main} Logging started for ...
//! D 2026-11-27 01:02:42.881873 [UrlManager] {Main} Resolved URL: key=/client/error/BLZBNTBNA00000005; ...
//! ```
//!
//! Error codes have a fixed shape: `BLZ`, six capital letters (product and
//! component, e.g. `BNTBNA`) and eight hexadecimal digits.

/// `BLZ` + 6 letters + 8 hex digits.
const CODE_LEN: usize = 17;

/// Whether `text` is exactly an error code like `BLZBNTBNA00000005`.
pub fn is_error_code(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == CODE_LEN
        && bytes.starts_with(b"BLZ")
        && bytes[3..9].iter().all(u8::is_ascii_uppercase)
        && bytes[9..]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'A'..=b'F').contains(b))
}

/// An error code found in the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorSighting {
    pub code: String,
    /// `YYYY-MM-DD HH:MM:SS` of the line, if it has the usual format.
    pub timestamp: Option<String>,
}

/// What a second `Battle.net.exe` logs when it hands its arguments (open
/// the window, `--exec=launch …`) to the running one and exits.
const HANDOFF_MARKER: &str = "Leaving because another instance of battle.net is running";

/// Whether a log is from such a hand-off run, not a real session. Every
/// "Open Battle.net" or "Play" while Battle.net is open leaves one.
pub fn is_handoff(log: &str) -> bool {
    log.contains(HANDOFF_MARKER)
}

/// Every error code in a log, in order. A code repeated on the same line
/// counts once.
pub fn error_sightings(log: &str) -> Vec<ErrorSighting> {
    let mut sightings = Vec::new();
    for line in log.lines() {
        let mut codes: Vec<&str> = codes_in(line).collect();
        codes.dedup();
        for code in codes {
            sightings.push(ErrorSighting {
                code: code.to_owned(),
                timestamp: line_timestamp(line),
            });
        }
    }
    sightings
}

fn codes_in(line: &str) -> impl Iterator<Item = &str> {
    line.match_indices("BLZ")
        .filter_map(|(start, _)| line.get(start..start + CODE_LEN))
        .filter(|candidate| is_error_code(candidate))
}

/// `I 2026-11-27 01:01:51.945282 ...` → `2026-11-27 01:01:51`.
fn line_timestamp(line: &str) -> Option<String> {
    let stamp = line.get(2..21)?;
    let b = stamp.as_bytes();
    let shape_ok = line.as_bytes().get(1) == Some(&b' ')
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b' '
        && b[13] == b':'
        && b[16] == b':';
    shape_ok.then(|| stamp.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_handoff_runs() {
        // Real lines from a hand-off log (`--exec=launch WoWF`).
        let handoff = "I 2026-11-27 22:04:27.467932 [Main] {Main} Command line arguments: {[0]=--exec=launch WoWF}\n\
I 2026-11-27 22:04:27.468432 [Main] {Main} Leaving because another instance of battle.net is running\n\
D 2026-11-27 22:04:27.468496 [Main] {Main} Shutting down callback queue\n";
        assert!(is_handoff(handoff));
        let session =
            "I 2026-11-27 20:58:34.353143 [Agent] {Main} AgentManager is now initialized\n";
        assert!(!is_handoff(session));
    }

    // Real line from a Battle.net log under GE-Proton (URL trimmed).
    const AGENT_LINE: &str = "D 2026-11-27 01:02:42.881873 [UrlManager] {Main} Resolved URL: key=/client/error/BLZBNTBNA00000005; region=US; endpoint=https://example.invalid/BLZBNTBNA00000005";

    #[test]
    fn recognizes_codes() {
        assert!(is_error_code("BLZBNTBNA00000005"));
        assert!(is_error_code("BLZBNTBGS80000011"));
        assert!(!is_error_code("BLZBNTBNA0000000")); // too short
        assert!(!is_error_code("BLZbntbna00000005")); // lowercase letters
        assert!(!is_error_code("BLZBNTBNA0000000G")); // not hex
    }

    #[test]
    fn finds_code_and_time_once_per_line() {
        assert_eq!(
            error_sightings(AGENT_LINE),
            [ErrorSighting {
                code: "BLZBNTBNA00000005".to_owned(),
                timestamp: Some("2026-11-27 01:02:42".to_owned()),
            }]
        );
    }

    #[test]
    fn ignores_lines_without_codes_and_odd_formats() {
        let log = "I 2026-11-27 01:01:51.945282 [Main] {Main} Logging started\n\
                   garbage BLZ\n\
                   no timestamp BLZBNTBGS80000011 here";
        assert_eq!(
            error_sightings(log),
            [ErrorSighting {
                code: "BLZBNTBGS80000011".to_owned(),
                timestamp: None,
            }]
        );
    }
}
