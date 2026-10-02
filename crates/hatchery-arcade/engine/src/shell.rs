//! The multi-game selection seam: which mini-game currently owns the pet
//! modal, sizing negotiation, and a single close/pause propagation path
//! that never special-cases by concrete game type.
//!
//! **Why this is a dyn-safe seam over a closed host enum, not a plugin
//! registry**: `MiniGame::Command`/`Snapshot` differ per concrete game, so
//! no single method could take or return either through a `dyn` trait
//! object -- a real Rust object-safety constraint, not a stylistic
//! choice. This module owns the GENERIC parts of the seam (selection
//! state, sizing negotiation math, pause/close propagation) as plain data
//! plus a dyn-safe trait with no generic/associated-type-bearing methods
//! ([`ArcadeOccupant`]); the host still writes one small, closed enum
//! naming its actual hosted games, e.g.:
//!
//! ```ignore
//! enum HostedGame {
//!     NightGarden(shell::GameScreen<night_garden::NightGarden>),
//!     RelayWorks(shell::GameScreen<relay_works::RelayWorks>),
//! }
//! impl HostedGame {
//!     fn as_occupant(&mut self) -> &mut dyn ArcadeOccupant {
//!         match self {
//!             Self::NightGarden(s) => s,
//!             Self::RelayWorks(s) => s,
//!         }
//!     }
//! }
//! ```
//!
//! `ArcadeShell` never needs to know this enum exists; every method it
//! exposes takes `&dyn ArcadeOccupant`/`&mut dyn ArcadeOccupant`, which
//! `as_occupant` produces in one line regardless of how many games are
//! ever added -- adding a third game means one new enum variant and one
//! new catalog entry, never a change to this module.

use std::time::Instant;

use crate::{
    cadence::Cadence,
    game::{MiniGame, RunOutcome},
    runner::Runner,
};

/// A terminal-cell size, independent of any render tier or board-tile
/// footprint -- what sizing negotiation actually compares. Deliberately
/// NOT `uzor_tui::rect::Rect` (a render-feature type) so this whole
/// module stays compilable in the headless `sweep` build.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CellArea {
    pub width: u16,
    pub height: u16,
}

/// Static per-game identity + sizing, queryable WITHOUT constructing an
/// instance -- sizing negotiation must be answerable before
/// [`Runner::start`] ever runs, since a modal that cannot fit a game at
/// all should never debit admission credit for it.
pub trait GameEntry: MiniGame {
    const ID: &'static str;
    const TITLE: &'static str;
    /// Optional slower cosmetic redraw cadence this game wants in
    /// addition to its own fixed sim tick (e.g. a ~150ms cosmetic
    /// pip-scroll). `None` (the default) means "no cadence beyond my own
    /// sim tick" -- most games need nothing here.
    const COSMETIC_INTERVAL: Option<std::time::Duration> = None;
    fn min_modal_size() -> CellArea;
    fn preferred_modal_size() -> CellArea;
}

/// Dyn-safe erased metadata for the game-SELECT list screen -- this is
/// what makes the top-level "which games exist" catalog genuinely
/// engine-owned DATA rather than a per-host hardcoded match, even though
/// actually INSTANTIATING a selected game still goes through the host's
/// own closed enum (see the module doc above).
#[derive(Clone, Copy, Debug)]
pub struct GameCatalogEntry {
    pub id: &'static str,
    pub title: &'static str,
    pub min_modal_size: CellArea,
    pub preferred_modal_size: CellArea,
}

/// One game's own Home/Run/Results progression -- "Paused" is not a
/// separate variant; it is `InRun` with `Runner::is_paused() == true`,
/// avoiding an enum-swap dance on every suspend/resume.
pub enum GameScreen<G: MiniGame> {
    Home,
    InRun(Runner<G>),
    Results { outcome: RunOutcome, final_hash: u64 },
}

/// Deliberately dyn-safe: no method here takes or returns `G::Command`/
/// `G::Snapshot`/`G::Params` -- this is exactly what lets a host hold
/// `&mut dyn ArcadeOccupant` without erasing `MiniGame` itself.
pub trait ArcadeOccupant {
    /// One call, regardless of which concrete game this is -- the layer
    /// that owns close/pause handling never special-cases by game type.
    fn set_suspended(&mut self, suspended: bool);
    fn is_running(&self) -> bool;
    fn footprints(&self) -> (CellArea, CellArea);
    /// This occupant's own currently-active cadences (sim tick + optional
    /// cosmetic), empty while suspended.
    fn cadences(&self, now: Instant) -> Vec<Cadence>;
}

impl<G: GameEntry> ArcadeOccupant for GameScreen<G> {
    fn set_suspended(&mut self, suspended: bool) {
        if let Self::InRun(runner) = self {
            runner.set_paused(suspended);
        }
    }

    fn is_running(&self) -> bool {
        matches!(self, Self::InRun(r) if !r.is_paused())
    }

    fn footprints(&self) -> (CellArea, CellArea) {
        (G::min_modal_size(), G::preferred_modal_size())
    }

    fn cadences(&self, now: Instant) -> Vec<Cadence> {
        let Self::InRun(runner) = self else { return Vec::new() };
        if runner.is_paused() {
            return Vec::new();
        }
        let mut out = vec![Cadence { interval: G::TICK, next_due: runner.next_tick_deadline(now) }];
        if let Some(cosmetic) = G::COSMETIC_INTERVAL {
            out.push(Cadence { interval: cosmetic, next_due: now + cosmetic });
        }
        out
    }
}

/// THE multi-game seam. Owns modal identity (which game is selected, if
/// any -- `None` = the game-select Home screen) and sizing negotiation;
/// routes close/pause through the SAME `ArcadeOccupant::set_suspended`
/// call regardless of which concrete game is selected. Holds no concrete
/// `MiniGame` type itself -- plain data plus pure functions.
pub struct ArcadeShell {
    catalog: &'static [GameCatalogEntry],
    selected: Option<usize>,
}

impl ArcadeShell {
    pub fn new(catalog: &'static [GameCatalogEntry]) -> Self {
        Self { catalog, selected: None }
    }

    pub fn selected_id(&self) -> Option<&'static str> {
        self.selected.map(|i| self.catalog[i].id)
    }

    pub fn select(&mut self, id: &str) -> bool {
        if let Some(i) = self.catalog.iter().position(|e| e.id == id) {
            self.selected = Some(i);
            true
        } else {
            false
        }
    }

    pub fn return_to_menu(&mut self) {
        self.selected = None;
    }

    /// "That layer... owns... sizing negotiation." `None` means "does not
    /// fit at all" -- the host renders a resize request. A real fit is
    /// clamped to `available`, never larger than `preferred_modal_size()`.
    pub fn negotiate_size(&self, occupant: &dyn ArcadeOccupant, available: CellArea) -> Option<CellArea> {
        let (min, preferred) = occupant.footprints();
        if available.width < min.width || available.height < min.height {
            return None;
        }
        Some(CellArea { width: preferred.width.min(available.width), height: preferred.height.min(available.height) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        admission::AdmissionCredit,
        test_support::{CounterSource, TestGame, TestGame2},
    };

    const CATALOG: [GameCatalogEntry; 2] = [
        GameCatalogEntry {
            id: TestGame::ID,
            title: TestGame::TITLE,
            min_modal_size: CellArea { width: 20, height: 10 },
            preferred_modal_size: CellArea { width: 40, height: 20 },
        },
        GameCatalogEntry {
            id: TestGame2::ID,
            title: TestGame2::TITLE,
            min_modal_size: CellArea { width: 60, height: 30 },
            preferred_modal_size: CellArea { width: 80, height: 40 },
        },
    ];

    #[test]
    fn arcade_shell_rejects_a_game_that_does_not_fit() {
        let shell = ArcadeShell::new(&CATALOG);
        let occupant: GameScreen<TestGame> = GameScreen::Home;

        let too_small = CellArea { width: 5, height: 5 };
        assert_eq!(shell.negotiate_size(&occupant, too_small), None);

        let plenty = CellArea { width: 100, height: 100 };
        assert_eq!(shell.negotiate_size(&occupant, plenty), Some(CellArea { width: 40, height: 20 }));

        let tight = CellArea { width: 25, height: 12 };
        assert_eq!(shell.negotiate_size(&occupant, tight), Some(CellArea { width: 25, height: 12 }));
    }

    #[test]
    fn set_suspended_is_a_single_call_regardless_of_which_game_is_selected() {
        let mut source = CounterSource(10);
        let runner_a = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 1, ()).unwrap();
        let runner_b = Runner::<TestGame2>::start(&mut source, AdmissionCredit(1), 2, ()).unwrap();

        let mut screen_a = GameScreen::InRun(runner_a);
        let mut screen_b = GameScreen::InRun(runner_b);

        assert!(screen_a.is_running());
        assert!(screen_b.is_running());

        fn suspend(occupant: &mut dyn ArcadeOccupant) {
            occupant.set_suspended(true);
        }

        suspend(&mut screen_a);
        suspend(&mut screen_b);

        // Both must now report not-running (paused), through the SAME
        // dispatch call site -- no per-game special-casing.
        assert!(!screen_a.is_running());
        assert!(!screen_b.is_running());
    }

    #[test]
    fn selecting_an_unknown_id_leaves_the_shell_on_the_menu() {
        let mut shell = ArcadeShell::new(&CATALOG);
        assert!(!shell.select("does-not-exist"));
        assert_eq!(shell.selected_id(), None);

        assert!(shell.select(TestGame::ID));
        assert_eq!(shell.selected_id(), Some(TestGame::ID));
    }

    #[test]
    fn cadences_are_empty_on_the_home_screen_and_while_paused_but_present_while_running() {
        let now = Instant::now();
        let home: GameScreen<TestGame> = GameScreen::Home;
        assert!(home.cadences(now).is_empty());

        let mut source = CounterSource(10);
        let runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 1, ()).unwrap();
        let mut in_run = GameScreen::InRun(runner);

        let cadences = in_run.cadences(now);
        assert_eq!(cadences.len(), 1, "TestGame has no COSMETIC_INTERVAL, so exactly one cadence (its own sim tick)");
        assert_eq!(cadences[0].interval, TestGame::TICK);

        in_run.set_suspended(true);
        assert!(in_run.cadences(now).is_empty(), "a suspended occupant contributes no cadences");
    }
}
