//! `snowctl doctor`: looks for known problems. Read-only: it never
//! changes anything, it only explains and suggests.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;

use snowlauncher_core::battlenet_log::ErrorSighting;
use snowlauncher_core::errors::known_error;

/// How often an error code showed up in the sessions checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorSummary {
    pub code: String,
    /// Sessions (starts of Battle.net) whose log has the code.
    pub sessions: usize,
    /// Latest `YYYY-MM-DD HH:MM:SS` it was logged at, if known.
    pub last_seen: Option<String>,
}

/// Groups sightings by code. `per_session` has one entry per log file.
/// Most frequent first; ties by code, for a stable output.
pub fn summarize(per_session: &[Vec<ErrorSighting>]) -> Vec<ErrorSummary> {
    let mut by_code: BTreeMap<&str, ErrorSummary> = BTreeMap::new();
    for session in per_session {
        let mut counted_here = Vec::new();
        for sighting in session {
            let summary = by_code
                .entry(&sighting.code)
                .or_insert_with(|| ErrorSummary {
                    code: sighting.code.clone(),
                    sessions: 0,
                    last_seen: None,
                });
            if !counted_here.contains(&sighting.code.as_str()) {
                summary.sessions += 1;
                counted_here.push(&sighting.code);
            }
            // The timestamp format sorts chronologically as text.
            if sighting.timestamp > summary.last_seen {
                summary.last_seen.clone_from(&sighting.timestamp);
            }
        }
    }
    let mut summaries: Vec<ErrorSummary> = by_code.into_values().collect();
    summaries.sort_by(|a, b| b.sessions.cmp(&a.sessions).then(a.code.cmp(&b.code)));
    summaries
}

pub struct Report {
    pub prefix: PathBuf,
    /// Whether the snowd service is active; `None` if unknown.
    pub service_active: Option<bool>,
    pub sessions_checked: usize,
    /// Start of the oldest and newest session checked.
    pub period: Option<(String, String)>,
    pub errors: Vec<ErrorSummary>,
}

const WIDTH: usize = 76;

pub fn render(report: &Report) -> String {
    let mut out = String::new();
    // Writing to a String cannot fail, so the results are ignored.
    let _ = writeln!(out, "Prefix:      {}", report.prefix.display());
    let service = match report.service_active {
        Some(true) => "running as a service",
        Some(false) => "not running as a service (see `snowctl service install`)",
        None => "unknown (could not ask systemd)",
    };
    let _ = writeln!(out, "snowd:       {service}");

    if report.sessions_checked == 0 {
        let _ = writeln!(
            out,
            "Battle.net:  no logs found (has Battle.net been started in this prefix?)"
        );
        return out;
    }
    let period = report
        .period
        .as_ref()
        .map(|(from, to)| format!(" ({from} to {to})"))
        .unwrap_or_default();
    let _ = writeln!(
        out,
        "Battle.net:  {} recent session(s) checked{period}",
        report.sessions_checked
    );

    if report.errors.is_empty() {
        let _ = writeln!(out, "\nNo Battle.net error codes in these sessions.");
        return out;
    }
    for error in &report.errors {
        let _ = writeln!(out);
        match known_error(&error.code) {
            Some(known) => {
                let _ = writeln!(out, "{}: {}", error.code, known.title);
                let _ = writeln!(out, "  {}", seen(error, report.sessions_checked));
                wrap_into(&mut out, "  What it means: ", known.meaning);
                wrap_into(&mut out, "  What to do: ", known.advice);
            }
            None => {
                let _ = writeln!(out, "{}: not in Snow Launcher's catalog yet", error.code);
                let _ = writeln!(out, "  {}", seen(error, report.sessions_checked));
            }
        }
    }
    out
}

fn seen(error: &ErrorSummary, checked: usize) -> String {
    let last = error
        .last_seen
        .as_ref()
        .map(|time| format!(", last at {time}"))
        .unwrap_or_default();
    format!("Seen in {} of {checked} session(s){last}.", error.sessions)
}

/// Writes `label` + `text`, wrapped at [`WIDTH`] columns; continuation
/// lines are indented like the start of the text.
fn wrap_into(out: &mut String, label: &str, text: &str) {
    let indent = " ".repeat(4);
    let mut line = label.to_owned();
    let mut at_start = true;
    for word in text.split_whitespace() {
        if !at_start && line.len() + 1 + word.len() > WIDTH {
            let _ = writeln!(out, "{line}");
            line = indent.clone();
            at_start = true;
        }
        if !at_start {
            line.push(' ');
        }
        line.push_str(word);
        at_start = false;
    }
    let _ = writeln!(out, "{line}");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sighting(code: &str, time: &str) -> ErrorSighting {
        ErrorSighting {
            code: code.to_owned(),
            timestamp: Some(time.to_owned()),
        }
    }

    const AGENT: &str = "BLZBNTBNA00000005";
    const OTHER: &str = "BLZBNTBGS80000011";

    #[test]
    fn counts_sessions_not_lines_and_keeps_latest_time() {
        let sessions = vec![
            vec![
                sighting(AGENT, "2026-11-27 01:02:42"),
                sighting(AGENT, "2026-11-27 01:30:00"),
            ],
            vec![
                sighting(AGENT, "2026-11-26 14:29:52"),
                sighting(OTHER, "2026-11-26 18:16:26"),
            ],
            vec![],
        ];
        assert_eq!(
            summarize(&sessions),
            [
                ErrorSummary {
                    code: AGENT.to_owned(),
                    sessions: 2,
                    last_seen: Some("2026-11-27 01:30:00".to_owned()),
                },
                ErrorSummary {
                    code: OTHER.to_owned(),
                    sessions: 1,
                    last_seen: Some("2026-11-26 18:16:26".to_owned()),
                },
            ]
        );
    }

    fn report(errors: Vec<ErrorSummary>) -> Report {
        Report {
            prefix: PathBuf::from("/home/user/Games/battlenet"),
            service_active: Some(true),
            sessions_checked: 5,
            period: Some(("2026-11-25 22:01".to_owned(), "2026-11-27 01:01".to_owned())),
            errors,
        }
    }

    #[test]
    fn explains_known_errors_and_flags_unknown_ones() {
        let out = render(&report(vec![
            ErrorSummary {
                code: AGENT.to_owned(),
                sessions: 5,
                last_seen: Some("2026-11-27 01:02:42".to_owned()),
            },
            ErrorSummary {
                code: OTHER.to_owned(),
                sessions: 1,
                last_seen: None,
            },
        ]));
        assert!(out.contains("snowd:       running as a service\n"));
        assert!(out.contains("5 recent session(s) checked (2026-11-25 22:01 to 2026-11-27 01:01)"));
        assert!(out.contains("BLZBNTBNA00000005: Battle.net Agent went to sleep\n"));
        assert!(out.contains("Seen in 5 of 5 session(s), last at 2026-11-27 01:02:42."));
        assert!(out.contains("  What to do: "));
        assert!(out.contains("BLZBNTBGS80000011: not in Snow Launcher's catalog yet\n"));
        // Wrapped explanation lines fit the terminal.
        assert!(
            out.lines()
                .filter(|line| line.starts_with("  What") || line.starts_with("    "))
                .all(|line| line.len() <= WIDTH),
            "{out}"
        );
    }

    #[test]
    fn says_so_when_clean_or_without_logs() {
        assert!(render(&report(vec![])).contains("No Battle.net error codes"));

        let mut empty = report(vec![]);
        empty.sessions_checked = 0;
        empty.service_active = Some(false);
        let out = render(&empty);
        assert!(out.contains("no logs found"));
        assert!(out.contains("snowctl service install"));
    }
}
