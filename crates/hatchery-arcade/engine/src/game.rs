//! The generic contract every mini-game hosted in the pet modal
//! implements. The engine drives this trait; it never inspects a game's
//! own internal state beyond what these five methods expose.

use std::time::Duration;

use crate::rng::EngineRng;

pub trait MiniGame: Sized {
    /// Construction-time configuration (map id, difficulty, ...) -- a
    /// game-specific, plain-data type, not a config bag.
    type Params: Clone;
    /// Player/host input. Must not implement `Hash`/`Eq` off field
    /// iteration order in a way that could vary run to run -- see
    /// [`crate::hash`]'s own determinism discipline.
    type Command: Clone + std::fmt::Debug;
    /// Transient, one-tick-lifetime notifications for the renderer/replay
    /// log (a shot fired, a kill, a wave started). Never read back by
    /// `advance` on a later tick -- a game's own persistent effects must
    /// live in its state, not be reconstructed from a replayed event
    /// list.
    type Event: Clone + std::fmt::Debug;
    /// Immutable, renderer-facing view of current state. Owned data, not
    /// a borrow of `Self` -- the renderer must be able to hold a
    /// `Snapshot` across the frame the sim is still ticking on the next
    /// call (no aliasing).
    type Snapshot;

    /// Fixed tick length. Typed per-game -- the engine imposes no single
    /// global tick rate; a 20Hz combat game and a 10Hz puzzle game are
    /// both legitimate, simultaneously-supportable values of this one
    /// associated const.
    const TICK: Duration;
    /// Bumped whenever `Command`/`Event`/tick semantics change in a way
    /// that would make an old `ReplayV1` non-reproducible. See
    /// [`crate::replay`].
    const RULES_VERSION: u32;

    fn new(seed: u64, params: Self::Params) -> Self;

    /// Advances EXACTLY one fixed tick. `commands` are whatever input
    /// arrived since the caller's last tick (see
    /// [`crate::runner::Runner::advance`]'s own batching note -- a host
    /// frame's worth of input is applied atomically on the first tick
    /// that runs during that host call, never split). All randomness this
    /// tick MUST flow through `rng`; a game must never hold its own
    /// separate RNG source.
    fn advance(&mut self, rng: &mut EngineRng, commands: &[Self::Command]) -> Vec<Self::Event>;

    fn snapshot(&self) -> Self::Snapshot;

    /// A stable hash of the CURRENT state, for replay verification and the
    /// headless harness's own determinism checks. See [`crate::hash`] for
    /// the required construction discipline (never `derive(Hash)` fed
    /// into `DefaultHasher`).
    fn stable_hash(&self) -> u64;

    /// `None` while still running.
    fn is_finished(&self) -> Option<RunOutcome>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunOutcome {
    Won,
    Lost,
}
