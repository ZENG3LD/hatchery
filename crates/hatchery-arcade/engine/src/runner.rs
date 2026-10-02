//! The fixed-tick game loop driver: a wall-clock accumulator, pause, and
//! bounded catch-up on top of one `G: MiniGame`.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crate::{
    admission::{AdmissionCredit, AdmissionError, AdmissionSource},
    game::MiniGame,
    rng::EngineRng,
};

/// At most this many accumulated ticks may run in a single
/// `Runner::advance` call, regardless of how large `elapsed` is --
/// inherited verbatim from the TD reference design's own accumulator rule
/// ("at most five accumulated ticks may run after a delayed redraw").
/// Applies identically regardless of `G::TICK`'s own value: a 10Hz game
/// and a 20Hz game each get their OWN 5-tick catch-up ceiling, measured in
/// that game's own ticks, never a shared wall-clock budget.
pub const MAX_CATCHUP_TICKS: u32 = 5;

/// Ceiling on how many undrained [`RecordedTick`]s a recording `Runner`
/// (see [`Runner::set_recording`]) will hold before it starts dropping
/// the OLDEST entry to make room for a new one -- a host that stops
/// calling [`Runner::drain_recorded`] (a bug, or a modal it stopped
/// redrawing) must not let this queue grow without bound. Dropping the
/// oldest is the correct direction here, not the newest: a combat effect
/// that has sat un-rendered long enough to get squeezed out is already
/// stale and worthless to show late, while the newest tick is the one a
/// host that resumes draining actually still wants. `64` is `12`
/// consecutive [`MAX_CATCHUP_TICKS`]-sized catch-up bursts (`64 / 5`,
/// rounded down) -- 3.2 real seconds of Pet Bastion's own 20Hz combat
/// (`TICK_MS = 50`) -- comfortably more slack than any host that is
/// merely a frame or two late, while still bounding memory for a host
/// that never drains at all.
const RECORDED_TICK_QUEUE_CAP: usize = 64;

/// One completed tick's own presentation-facing record -- exactly the
/// three pieces `hatchery_arcade_pet_bastion_render::effects::
/// EffectsLayer::ingest` requires (see that fn's own doc comment):
/// the tick index BEFORE this tick ran, `G`'s own snapshot taken
/// immediately BEFORE this tick's `advance` call, and the events that
/// `advance` call itself returned. `Runner` hands these over completely
/// unfiltered and uninterpreted -- deciding what a `Shot` or a `Kill`
/// LOOKS like is entirely a presentation-layer's job (`EffectsLayer`),
/// never this sim-core module's.
pub struct RecordedTick<G: MiniGame> {
    /// `Runner::tick_index()`'s own value immediately before this tick
    /// ran -- i.e. how many ticks had already completed. Also this
    /// crate's own convention for `sim_time`'s `tick_index` argument at
    /// `alpha = 0.0`, matching the pixel-tier presentation crate's own
    /// "a shot's own trail lands exactly as the tick that resolved it
    /// becomes the new `curr`" rule.
    pub tick_index_before: u64,
    /// `game.snapshot()`, taken immediately before this tick's own
    /// `advance` call -- every entity this tick's combat touched is
    /// still at its pre-tick position here, exactly where it was when
    /// the events below actually resolved.
    pub snapshot_before: G::Snapshot,
    /// The exact `Vec<G::Event>` this one tick's own `advance` call
    /// returned -- never merged with another tick's events.
    pub events: Vec<G::Event>,
}

/// Drives one `G: MiniGame` on its own fixed tick, with a wall-clock
/// accumulator, pause, and bounded catch-up.
pub struct Runner<G: MiniGame> {
    game: G,
    rng: EngineRng,
    accumulator: Duration,
    tick_index: u64,
    paused: bool,
    /// `None` unless a host has called [`Runner::set_recording`] with
    /// `true` -- see that method's own doc comment for exactly why this
    /// is `Option`, not a `bool` flag next to an always-allocated queue:
    /// a `Runner` nobody asked to record must cost nothing beyond what
    /// this crate's own tests already assert it costs today (see
    /// `hatchery-arcade-sweep`'s own dependency-boundary doc, restated
    /// on `set_recording` itself).
    record: Option<VecDeque<RecordedTick<G>>>,
}

impl<G: MiniGame> Runner<G> {
    /// Debits `cost` from `source` EXACTLY ONCE, at start. There is no
    /// other method on `Runner` that takes an `AdmissionSource` --
    /// "energy gates admission, never in-run power" is enforced by this
    /// being the only door, not by a runtime check.
    pub fn start<S: AdmissionSource>(
        source: &mut S,
        cost: AdmissionCredit,
        seed: u64,
        params: G::Params,
    ) -> Result<Self, AdmissionError> {
        source.try_debit(cost)?;
        Ok(Self {
            game: G::new(seed, params),
            rng: EngineRng::seed(seed),
            accumulator: Duration::ZERO,
            tick_index: 0,
            paused: false,
            record: None,
        })
    }

    /// Turns per-tick recording on (`true`) or off (`false`); OFF for
    /// every fresh `Runner` ([`Runner::start`] never turns it on) -- a
    /// host has to opt in explicitly. A `Runner` that never calls this
    /// pays NOTHING extra in [`Runner::advance`]: `self.record` stays
    /// `None`, so `advance`'s own recording branch (`self.game.
    /// snapshot()` plus a [`RecordedTick`] push) never runs at all, not
    /// merely runs against an empty queue. This is what keeps
    /// `hatchery-arcade-sweep`'s own headless balance meter (thousands
    /// of games per run) unaffected: `sweep` does not even construct a
    /// `Runner` -- its own harness drives `MiniGame::advance` directly
    /// via `sweep_api::simulate` (see that module's own doc comment,
    /// "bypassing `Runner`'s wall-clock accumulator entirely"), so this
    /// whole feature is invisible to it either way -- but staying
    /// opt-in-and-off-by-default here means the same would hold even for
    /// a future headless caller that DID use `Runner` directly.
    ///
    /// Turning recording ON discards any previously queued (undrained)
    /// entries -- a fresh recording session starts clean, matching how a
    /// host restarting a run also rebuilds its own presentation state
    /// (e.g. `hatchery-tui`'s own `PetArcade` resetting its
    /// `EffectsLayer` alongside its `FramePresenter` in `start_run`).
    pub fn set_recording(&mut self, recording: bool) {
        self.record = if recording { Some(VecDeque::new()) } else { None };
    }

    /// Drains every [`RecordedTick`] queued since the last drain, OLDEST
    /// first -- the exact order `EffectsLayer::ingest` must see them in
    /// (a later tick's events must never be ingested before an earlier
    /// tick's, or a multi-tick effect could resolve out of order). Empty,
    /// and never a panic, if recording is off or nothing has ticked since
    /// the last drain -- a host can call this unconditionally after every
    /// [`Runner::advance`] without checking [`Runner::set_recording`]'s
    /// own state first.
    pub fn drain_recorded(&mut self) -> Vec<RecordedTick<G>> {
        match &mut self.record {
            Some(queue) => queue.drain(..).collect(),
            None => Vec::new(),
        }
    }

    /// Clears the accumulator on any transition INTO paused (never a
    /// partial-carry) -- closing the modal, losing minimum size, or an
    /// explicit pause all freeze the simulation and clear its
    /// elapsed-time accumulator.
    pub fn set_paused(&mut self, paused: bool) {
        if paused && !self.paused {
            self.accumulator = Duration::ZERO;
        }
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Advances the accumulator by `elapsed` and runs as many fixed ticks
    /// as fit, capped at `MAX_CATCHUP_TICKS` per call regardless of how
    /// large `elapsed` is -- a reopened modal after a long pause never
    /// fast-forwards more than `MAX_CATCHUP_TICKS` ticks in one host
    /// frame. Returns `true` if at least one tick ran (the caller should
    /// mark its own frame dirty).
    ///
    /// Command batching: the WHOLE `commands` slice is applied on the
    /// FIRST tick that runs during this call, and an empty slice on every
    /// subsequent catch-up tick in the same call -- a host redraw's worth
    /// of input arrives at TUI-frame granularity, not fixed-tick
    /// granularity, so this mirrors how a real player's per-frame input
    /// already coalesces before it reaches any sim. One replay log entry
    /// is therefore one `(tick_index, Vec<Command>)` pair per host call,
    /// never split mid-command across ticks.
    ///
    /// Recording (see [`Runner::set_recording`]): each tick this call
    /// actually runs gets its OWN [`RecordedTick`] -- a catch-up burst
    /// that runs several ticks in this one call (bounded by
    /// [`MAX_CATCHUP_TICKS`]) never collapses them into one entry, since
    /// each tick's own `snapshot_before` must be THAT tick's pre-tick
    /// state, not the state before the whole burst. When recording is
    /// off, `self.game.snapshot()` is never called here at all -- this
    /// signature, and everything it does when `self.record` is `None`,
    /// is byte-for-byte what it was before recording existed.
    pub fn advance(&mut self, elapsed: Duration, commands: &[G::Command]) -> bool {
        if self.paused {
            return false;
        }
        self.accumulator += elapsed;
        let mut ran = false;
        let mut budget = MAX_CATCHUP_TICKS;
        while self.accumulator >= G::TICK && budget > 0 {
            let batch: &[G::Command] = if ran { &[] } else { commands };
            let tick_index_before = self.tick_index;
            // `self.record.is_some()` gates the ONLY extra cost recording
            // adds over today's baseline: one `G::snapshot()` call per
            // recorded tick. `self.game.advance` itself always returns a
            // `Vec<G::Event>` regardless (it did before recording
            // existed too, simply dropped) -- recording never changes
            // WHAT gets computed there, only whether the snapshot taken
            // just before it is kept.
            let snapshot_before = self.record.is_some().then(|| self.game.snapshot());
            let events = self.game.advance(&mut self.rng, batch);
            if let (Some(record), Some(snapshot_before)) = (self.record.as_mut(), snapshot_before) {
                if record.len() == RECORDED_TICK_QUEUE_CAP {
                    // Oldest-drops-first -- see `RECORDED_TICK_QUEUE_CAP`'s
                    // own doc comment for why the OLDEST, not the newest,
                    // is the one a full queue gives up.
                    record.pop_front();
                }
                record.push_back(RecordedTick { tick_index_before, snapshot_before, events });
            }
            self.accumulator -= G::TICK;
            self.tick_index += 1;
            budget -= 1;
            ran = true;
        }
        // A run that hit its catch-up ceiling still owes the REST of its
        // debt eventually -- but never faster than real time; excess
        // accumulator beyond the ceiling is intentionally DROPPED, not
        // carried into the next call, matching "never fast-forwards a
        // whole wave off-screen" rather than merely delaying it.
        if budget == 0 {
            self.accumulator = Duration::ZERO;
        }
        ran
    }

    pub fn snapshot(&self) -> G::Snapshot {
        self.game.snapshot()
    }

    pub fn tick_index(&self) -> u64 {
        self.tick_index
    }

    pub fn stable_hash(&self) -> u64 {
        self.game.stable_hash()
    }

    /// The wall-clock instant this Runner's own fixed tick next wants to
    /// fire, given its CURRENT accumulator -- the per-occupant
    /// contribution to [`crate::cadence::tightest_wake`]. Paused runners
    /// contribute nothing (see [`crate::shell::ArcadeOccupant::cadences`]).
    pub fn next_tick_deadline(&self, now: Instant) -> Instant {
        now + G::TICK.saturating_sub(self.accumulator)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{CounterSource, TestCommand, TestGame};

    #[test]
    fn runner_accumulator_clears_on_pause_transition() {
        let mut source = CounterSource(10);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 1, ()).unwrap();

        // Advance by less than one tick -- the accumulator now holds a
        // partial tick's worth of elapsed time, no tick has run yet.
        assert!(!runner.advance(TestGame::TICK / 2, &[]));
        assert_eq!(runner.tick_index(), 0);

        runner.set_paused(true);
        runner.set_paused(false);

        // If the partial carry had survived the pause, this second
        // half-tick would push the accumulator over `TICK` and a tick
        // would run. It must NOT survive.
        assert!(!runner.advance(TestGame::TICK / 2, &[]));
        assert_eq!(runner.tick_index(), 0);
    }

    #[test]
    fn runner_bounded_catchup_never_exceeds_max_catchup_ticks() {
        let mut source = CounterSource(10);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 1, ()).unwrap();

        // Ten ticks' worth of elapsed time in one call must still only run
        // MAX_CATCHUP_TICKS ticks, never ten.
        let elapsed = TestGame::TICK * 10;
        assert!(runner.advance(elapsed, &[TestCommand::Add(1)]));
        assert_eq!(runner.tick_index(), MAX_CATCHUP_TICKS as u64);
    }

    #[test]
    fn paused_runner_never_advances() {
        let mut source = CounterSource(10);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 1, ()).unwrap();
        runner.set_paused(true);
        assert!(!runner.advance(TestGame::TICK * 5, &[]));
        assert_eq!(runner.tick_index(), 0);
    }

    #[test]
    fn admission_debits_exactly_once_at_start_never_mid_run() {
        let mut source = CounterSource(10);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(4), 1, ()).unwrap();
        assert_eq!(source.available(), AdmissionCredit(6));

        // A real multi-tick, multi-call run -- the balance must not move
        // again no matter how many ticks actually execute across several
        // catch-up calls (proves "only once" across the runner's own
        // catch-up loop, not just at the call site that starts it).
        for _ in 0..5 {
            runner.advance(TestGame::TICK * 3, &[TestCommand::Add(1)]);
        }
        assert_eq!(source.available(), AdmissionCredit(6));
    }

    #[test]
    fn next_tick_deadline_reflects_the_current_accumulator() {
        let mut source = CounterSource(10);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 1, ()).unwrap();
        let now = Instant::now();

        // A fresh runner's own accumulator is zero -- the deadline is a
        // full tick away.
        assert_eq!(runner.next_tick_deadline(now), now + TestGame::TICK);

        // After banking half a tick, the deadline is only half a tick
        // away.
        runner.advance(TestGame::TICK / 2, &[]);
        let after = Instant::now();
        let expected = after + TestGame::TICK.saturating_sub(TestGame::TICK / 2);
        assert_eq!(runner.next_tick_deadline(after), expected);
    }

    #[test]
    fn recording_never_perturbs_stable_hash_rng_or_tick_ordering() {
        // Two runners, IDENTICAL seed and IDENTICAL sequence of `advance`
        // calls -- the only difference is `recording.set_recording(true)`.
        // `TestGame::advance` draws from `rng` and folds both the draw and
        // the command log into `value`/`stable_hash` (see its own doc
        // comment), so ANY divergence here -- an extra/missing `rng` draw,
        // a skipped or duplicated tick, a reordered command batch --
        // would show up as a hash mismatch. This is the test [`Runner::
        // set_recording`]'s own doc comment promises: recording is
        // presentation-only plumbing, provably never touching RNG draws,
        // tick ordering, or `stable_hash`.
        let mut plain_source = CounterSource(10);
        let mut plain = Runner::<TestGame>::start(&mut plain_source, AdmissionCredit(1), 42, ()).unwrap();

        let mut recording_source = CounterSource(10);
        let mut recording = Runner::<TestGame>::start(&mut recording_source, AdmissionCredit(1), 42, ()).unwrap();
        recording.set_recording(true);

        // The second call is a catch-up burst (more than one tick in one
        // `advance` call) -- the same shape `runner_bounded_catchup_
        // never_exceeds_max_catchup_ticks` exercises on its own, proving
        // recording survives `MAX_CATCHUP_TICKS` unchanged too.
        let calls: &[(Duration, &[TestCommand])] = &[
            (TestGame::TICK, &[TestCommand::Add(3)]),
            (TestGame::TICK * 10, &[TestCommand::Add(7)]),
            (TestGame::TICK / 2, &[]),
            (TestGame::TICK, &[TestCommand::Add(-5)]),
        ];

        for (elapsed, commands) in calls {
            plain.advance(*elapsed, commands);
            let recording_ran = recording.advance(*elapsed, commands);
            // Draining every call (not just the ones a real host happens
            // to redraw on) also proves draining itself never perturbs
            // anything -- a host that drains every frame and one that
            // drains sporadically must reach the identical final hash.
            if recording_ran {
                recording.drain_recorded();
            }
        }

        assert_eq!(plain.tick_index(), recording.tick_index());
        assert_eq!(plain.stable_hash(), recording.stable_hash());
    }

    #[test]
    fn recorded_ticks_are_one_per_tick_with_their_own_pre_tick_snapshot() {
        let mut source = CounterSource(10);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 7, ()).unwrap();
        runner.set_recording(true);

        // A catch-up burst: MAX_CATCHUP_TICKS ticks in ONE `advance` call
        // -- must yield exactly that many `RecordedTick` entries, never
        // fewer (collapsed together) or more.
        let elapsed = TestGame::TICK * 10;
        assert!(runner.advance(elapsed, &[TestCommand::Add(1)]));

        let recorded = runner.drain_recorded();
        assert_eq!(recorded.len(), MAX_CATCHUP_TICKS as usize);
        for (i, tick) in recorded.iter().enumerate() {
            assert_eq!(tick.tick_index_before, i as u64);
            // `TestSnapshot::tick` reflects `TestGame`'s own tick counter
            // AS OF the snapshot call -- taken BEFORE this recorded
            // tick's own `advance`, so it reads `i`, not `i + 1`.
            assert_eq!(tick.snapshot_before.tick, i as u64);
        }
        // Draining leaves nothing behind for a second drain.
        assert!(runner.drain_recorded().is_empty());
    }

    #[test]
    fn a_runner_that_never_records_has_no_recorded_ticks_to_drain() {
        // The off-by-default case a bare `Runner::start` produces --
        // draining it is a harmless no-op, never a panic, matching
        // `Runner::drain_recorded`'s own doc comment.
        let mut source = CounterSource(10);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 1, ()).unwrap();
        runner.advance(TestGame::TICK * 3, &[TestCommand::Add(1)]);
        assert!(runner.drain_recorded().is_empty());
    }

    #[test]
    fn recorded_queue_drops_oldest_when_a_host_never_drains() {
        let mut source = CounterSource(10_000);
        let mut runner = Runner::<TestGame>::start(&mut source, AdmissionCredit(1), 3, ()).unwrap();
        runner.set_recording(true);

        // Enough catch-up-ceiling calls, without ever draining, to push
        // well past RECORDED_TICK_QUEUE_CAP entries -- a host that forgot
        // to drain must never see this queue grow without bound.
        let calls_needed = RECORDED_TICK_QUEUE_CAP / MAX_CATCHUP_TICKS as usize + 4;
        for _ in 0..calls_needed {
            runner.advance(TestGame::TICK * MAX_CATCHUP_TICKS, &[]);
        }

        let recorded = runner.drain_recorded();
        assert_eq!(recorded.len(), RECORDED_TICK_QUEUE_CAP);
        // The OLDEST entries were the ones dropped -- the survivors are
        // exactly the most recent `RECORDED_TICK_QUEUE_CAP` ticks, i.e.
        // a contiguous run ending at the very last tick this `Runner`
        // completed.
        let last = recorded.last().unwrap().tick_index_before;
        let first = recorded.first().unwrap().tick_index_before;
        assert_eq!(last, runner.tick_index() - 1);
        assert_eq!(first, last + 1 - RECORDED_TICK_QUEUE_CAP as u64);
    }
}
