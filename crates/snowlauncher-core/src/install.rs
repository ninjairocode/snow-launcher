//! Watching the Battle.net installer (project.md, section 5: "instalação
//! vigiada"). Pure: it is fed observations and says what is going on.
//!
//! From experiment E6: under umu + GE-Proton the installer brought the
//! Battle.net folder to ~800 MB, closed itself and opened Battle.net in
//! ~78 s. The classic failure is an installer stuck at 0%.

use std::time::Duration;

/// One look at the prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    pub elapsed: Duration,
    /// The installer's process is still there.
    pub installer_running: bool,
    /// The Battle.net app's main process is there.
    pub battlenet_running: bool,
    /// Size of the Battle.net install folder so far.
    pub installed_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    Installing {
        installed_bytes: u64,
    },
    /// Battle.net is open and the installer is gone.
    Done,
    /// The installer is open but nothing grew for too long.
    Stalled,
    /// The installer closed and Battle.net did not show up.
    ExitedWithoutBattlenet,
    TimedOut,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallLimits {
    /// No growth of the install folder for this long: stuck.
    pub stall_after: Duration,
    /// Time allowed for Battle.net to appear after the installer closes.
    pub exit_grace: Duration,
    /// Give up after this long overall.
    pub total: Duration,
}

impl Default for InstallLimits {
    fn default() -> Self {
        Self {
            stall_after: Duration::from_secs(5 * 60),
            exit_grace: Duration::from_secs(60),
            total: Duration::from_secs(30 * 60),
        }
    }
}

/// Turns observations into an [`InstallState`].
#[derive(Debug)]
pub struct InstallWatch {
    limits: InstallLimits,
    largest_bytes: u64,
    last_growth: Duration,
    installer_gone_since: Option<Duration>,
}

impl InstallWatch {
    pub fn new(limits: InstallLimits) -> Self {
        Self {
            limits,
            largest_bytes: 0,
            last_growth: Duration::ZERO,
            installer_gone_since: None,
        }
    }

    pub fn observe(&mut self, now: Observation) -> InstallState {
        if now.installed_bytes > self.largest_bytes {
            self.largest_bytes = now.installed_bytes;
            self.last_growth = now.elapsed;
        }
        if now.battlenet_running && !now.installer_running {
            return InstallState::Done;
        }
        if now.elapsed >= self.limits.total {
            return InstallState::TimedOut;
        }
        if now.installer_running {
            self.installer_gone_since = None;
            if now.elapsed.saturating_sub(self.last_growth) >= self.limits.stall_after {
                return InstallState::Stalled;
            }
        } else if !now.battlenet_running {
            let gone_since = *self.installer_gone_since.get_or_insert(now.elapsed);
            if now.elapsed.saturating_sub(gone_since) >= self.limits.exit_grace {
                return InstallState::ExitedWithoutBattlenet;
            }
        }
        InstallState::Installing {
            installed_bytes: now.installed_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(secs: u64, installer: bool, battlenet: bool, mb: u64) -> Observation {
        Observation {
            elapsed: Duration::from_secs(secs),
            installer_running: installer,
            battlenet_running: battlenet,
            installed_bytes: mb * 1_000_000,
        }
    }

    fn run(observations: &[Observation]) -> Vec<InstallState> {
        let mut watch = InstallWatch::new(InstallLimits::default());
        observations.iter().map(|&o| watch.observe(o)).collect()
    }

    #[test]
    fn the_e6_install_ends_done() {
        // Shape of experiment E6: Agent up, files arrive, installer hands
        // over to Battle.net at ~78 s.
        let states = run(&[
            at(0, true, false, 0),
            at(45, true, false, 0),
            at(55, true, false, 147),
            at(70, true, false, 1600),
            at(78, false, true, 779),
        ]);
        assert_eq!(states.last(), Some(&InstallState::Done));
        assert!(
            matches!(states[2], InstallState::Installing { installed_bytes } if installed_bytes == 147_000_000)
        );
    }

    #[test]
    fn stuck_at_zero_is_stalled_after_five_minutes() {
        let states = run(&[
            at(0, true, false, 0),
            at(299, true, false, 0),
            at(300, true, false, 0),
        ]);
        assert!(matches!(states[1], InstallState::Installing { .. }));
        assert_eq!(states[2], InstallState::Stalled);
    }

    #[test]
    fn growth_resets_the_stall_clock() {
        let states = run(&[
            at(0, true, false, 0),
            at(250, true, false, 10),
            at(500, true, false, 10),
            at(550, true, false, 10),
        ]);
        assert!(matches!(states[2], InstallState::Installing { .. }));
        assert_eq!(states[3], InstallState::Stalled);
    }

    #[test]
    fn installer_closing_without_battlenet_is_reported_after_a_grace() {
        let states = run(&[
            at(0, true, false, 0),
            at(30, false, false, 5),
            at(80, false, false, 5),
            at(90, false, false, 5),
        ]);
        assert!(matches!(states[2], InstallState::Installing { .. }));
        assert_eq!(states[3], InstallState::ExitedWithoutBattlenet);
    }

    #[test]
    fn both_open_is_still_installing_and_there_is_a_deadline() {
        let states = run(&[at(0, true, true, 100), at(1800, true, true, 900)]);
        assert!(matches!(states[0], InstallState::Installing { .. }));
        assert_eq!(states[1], InstallState::TimedOut);
    }
}
