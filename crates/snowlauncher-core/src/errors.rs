//! Catalog of known Battle.net errors (`project.md`, section 8).
//!
//! Only codes we can actually explain are listed. Anything else found in
//! the logs is reported as unknown, never guessed at.
// TODO(v0.4): extend this catalog from the quirks repository.
// TODO(i18n): translate (pt-BR) once gettext is in place (v0.6).

/// A Battle.net error code Snow Launcher knows about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KnownError {
    pub code: &'static str,
    pub title: &'static str,
    pub meaning: &'static str,
    pub advice: &'static str,
}

pub const KNOWN_ERRORS: &[KnownError] = &[KnownError {
    code: "BLZBNTBNA00000005",
    title: "Battle.net Agent went to sleep",
    meaning: "The Battle.net app lost contact with its background Agent, which \
              handles installs, updates and launching games. Common under Wine, \
              especially after updates of the app.",
    advice: "Close Battle.net and make sure no Agent.exe is left in the prefix \
             (snowctl status), then open Battle.net again. If it keeps happening, \
             clearing the Agent's data in ProgramData/Battle.net usually helps; \
             Snow Launcher will offer to do this safely, with a backup, in a \
             future version.",
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
