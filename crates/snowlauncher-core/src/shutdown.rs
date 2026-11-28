//! Closing the house when Battle.net does not close by itself (O15: a
//! stuck session ignored the close request and left Wine hanging).
//!
//! Gradual, as the project's rules ask: a normal close request, then
//! `wineserver`'s SIGTERM, then SIGKILL, each followed by a wait. Before
//! every step the house is checked again for games (or Windows programs
//! that could be games): if one shows up, it stops right there. Done means
//! the whole house is empty, `wineserver` included.
//!
//! Pure: the house is reached through [`HouseControl`].

use std::time::Duration;

/// One step of the shutdown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ForceStep {
    /// Wine's `taskkill` without `/f`: a normal close request (E10).
    Request,
    /// `wineserver -k15`: SIGTERM to the house's processes.
    Terminate,
    /// `wineserver -k`: SIGKILL to the house's processes.
    Kill,
}

impl ForceStep {
    /// The signal `wineserver -k` sends, if this step uses it.
    pub fn wineserver_signal(self) -> Option<u8> {
        match self {
            ForceStep::Request => None,
            ForceStep::Terminate => Some(15),
            ForceStep::Kill => Some(9),
        }
    }
}

/// The steps, in order, and how long to wait after each for the house to
/// empty. After a normal close the Agent and the launchers take ~20 s to
/// leave (O13).
pub const FORCE_STEPS: [(ForceStep, Duration); 3] = [
    (ForceStep::Request, Duration::from_secs(30)),
    (ForceStep::Terminate, Duration::from_secs(10)),
    (ForceStep::Kill, Duration::from_secs(10)),
];

/// The house, as the shutdown sees it.
pub trait HouseControl {
    type Error;

    /// Games, or Windows programs that could be games, running in the
    /// house, by name. Empty means it is safe to go on.
    fn blocking_programs(&mut self) -> Result<Vec<String>, Self::Error>;

    /// Everything still running in the house, by name.
    fn running(&mut self) -> Result<Vec<String>, Self::Error>;

    fn apply(&mut self, step: ForceStep) -> Result<(), Self::Error>;

    /// Waits up to `limit` for the house to be empty; `true` if it is.
    fn wait_empty(&mut self, limit: Duration) -> Result<bool, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ForceOutcome {
    /// Nothing was running.
    AlreadyEmpty,
    /// Stopped: these programs run in the house.
    ProgramsRunning(Vec<String>),
    /// The house emptied after this step.
    Emptied(ForceStep),
    /// Even SIGKILL left these running.
    StillRunning(Vec<String>),
}

/// Empties the house step by step (see the module docs).
pub fn force_quit<C: HouseControl>(house: &mut C) -> Result<ForceOutcome, C::Error> {
    if house.running()?.is_empty() {
        return Ok(ForceOutcome::AlreadyEmpty);
    }
    for (step, wait) in FORCE_STEPS {
        let blocking = house.blocking_programs()?;
        if !blocking.is_empty() {
            return Ok(ForceOutcome::ProgramsRunning(blocking));
        }
        house.apply(step)?;
        if house.wait_empty(wait)? {
            return Ok(ForceOutcome::Emptied(step));
        }
    }
    Ok(ForceOutcome::StillRunning(house.running()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A house that empties after a given step; a game may show up.
    struct FakeHouse {
        empties_after: Option<ForceStep>,
        game_appears_after: Option<ForceStep>,
        applied: Vec<ForceStep>,
        empty: bool,
    }

    impl FakeHouse {
        fn new(empties_after: Option<ForceStep>) -> Self {
            Self {
                empties_after,
                game_appears_after: None,
                applied: Vec::new(),
                empty: false,
            }
        }
    }

    impl HouseControl for FakeHouse {
        type Error = ();

        fn blocking_programs(&mut self) -> Result<Vec<String>, ()> {
            let game = self
                .game_appears_after
                .is_some_and(|after| self.applied.last() == Some(&after));
            Ok(if game {
                vec!["Wow.exe".to_owned()]
            } else {
                Vec::new()
            })
        }

        fn running(&mut self) -> Result<Vec<String>, ()> {
            Ok(if self.empty {
                Vec::new()
            } else {
                vec!["wineserver".to_owned()]
            })
        }

        fn apply(&mut self, step: ForceStep) -> Result<(), ()> {
            self.applied.push(step);
            Ok(())
        }

        fn wait_empty(&mut self, _limit: Duration) -> Result<bool, ()> {
            self.empty = self
                .empties_after
                .is_some_and(|after| self.applied.last() == Some(&after));
            Ok(self.empty)
        }
    }

    #[test]
    fn a_healthy_house_closes_on_the_normal_request() {
        let mut house = FakeHouse::new(Some(ForceStep::Request));
        assert_eq!(
            force_quit(&mut house),
            Ok(ForceOutcome::Emptied(ForceStep::Request))
        );
        assert_eq!(house.applied, [ForceStep::Request]);
    }

    #[test]
    fn a_stuck_house_gets_sigterm_then_sigkill() {
        let mut house = FakeHouse::new(Some(ForceStep::Kill));
        assert_eq!(
            force_quit(&mut house),
            Ok(ForceOutcome::Emptied(ForceStep::Kill))
        );
        assert_eq!(
            house.applied,
            [ForceStep::Request, ForceStep::Terminate, ForceStep::Kill]
        );

        let mut never = FakeHouse::new(None);
        assert_eq!(
            force_quit(&mut never),
            Ok(ForceOutcome::StillRunning(vec!["wineserver".to_owned()]))
        );
    }

    #[test]
    fn a_game_stops_the_shutdown_before_any_signal() {
        let mut house = FakeHouse::new(Some(ForceStep::Kill));
        house.game_appears_after = Some(ForceStep::Request);
        assert_eq!(
            force_quit(&mut house),
            Ok(ForceOutcome::ProgramsRunning(vec!["Wow.exe".to_owned()]))
        );
        // Only the normal request went out: no signal.
        assert_eq!(house.applied, [ForceStep::Request]);
    }

    #[test]
    fn an_empty_house_needs_nothing() {
        let mut house = FakeHouse::new(None);
        house.empty = true;
        assert_eq!(force_quit(&mut house), Ok(ForceOutcome::AlreadyEmpty));
        assert!(house.applied.is_empty());
    }

    #[test]
    fn signals_per_step() {
        let signals: Vec<Option<u8>> = FORCE_STEPS
            .iter()
            .map(|(step, _)| step.wineserver_signal())
            .collect();
        assert_eq!(signals, [None, Some(15), Some(9)]);
    }
}
