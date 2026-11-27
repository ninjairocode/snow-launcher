//! Catalog of known Battle.net errors (`project.md`, section 8).
//!
//! Only codes we can actually explain are listed. Anything else found in
//! the logs is reported as unknown, never guessed at.
// TODO(v0.4): extend this catalog from the quirks repository.
// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).

/// How much a logged error matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Logged, but usually harmless: the app recovers by itself and the
    /// user sees nothing. Only worth acting on together with a symptom.
    Harmless,
    /// A real problem the user will notice.
    Problem,
}

/// A Battle.net error code Snow Launcher knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownError {
    pub code: &'static str,
    pub title: &'static str,
    pub severity: Severity,
    pub meaning: &'static str,
    pub advice: &'static str,
}

pub const KNOWN_ERRORS: &[KnownError] = &[KnownError {
    code: "BLZBNTBNA00000005",
    title: "Battle.net Agent went to sleep",
    // Seen at every start under GE-Proton, about a minute in, with the
    // Agent alive and nothing shown to the user (docs/experimentos.md, O4).
    severity: Severity::Harmless,
    meaning: "The Battle.net app did not hear from its background Agent (which \
              handles installs, updates and launching games) in time. Under \
              Wine this is often logged about a minute after the app starts, \
              while the Agent is still starting, and the app recovers by \
              itself without showing anything.",
    advice: "Nothing, unless you see this error on screen or installs, updates \
             or Play stop working. Then close Battle.net, make sure no \
             Agent.exe is left in the prefix (snowctl status) and open it \
             again. If it keeps happening, clearing the Agent's data in \
             ProgramData/Battle.net usually helps; Snow Launcher will offer to \
             do this safely, with a backup, in a future version.",
}];

/// Looks up a code in the catalog.
pub fn known_error(code: &str) -> Option<&'static KnownError> {
    KNOWN_ERRORS.iter().find(|known| known.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_agent_error() {
        let agent = known_error("BLZBNTBNA00000005").unwrap();
        assert_eq!(agent.title, "Battle.net Agent went to sleep");
        assert_eq!(agent.severity, Severity::Harmless);
    }

    #[test]
    fn unknown_codes_are_not_guessed() {
        assert_eq!(known_error("BLZBNTBGS80000011"), None);
    }

    #[test]
    fn catalog_codes_are_well_formed_and_unique() {
        for (i, known) in KNOWN_ERRORS.iter().enumerate() {
            assert!(
                crate::battlenet_log::is_error_code(known.code),
                "{}",
                known.code
            );
            assert!(
                KNOWN_ERRORS[..i]
                    .iter()
                    .all(|other| other.code != known.code),
                "duplicate {}",
                known.code
            );
        }
    }
}
