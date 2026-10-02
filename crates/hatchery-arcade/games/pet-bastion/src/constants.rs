//! All balance numbers for Pet Bastion: Night Garden, in one place, as named
//! integer/fixed-point constants. Nothing outside this module may hardcode a
//! magic balance number -- a future sweep varies these, it does not grep for
//! literals scattered through `rules`/`tower`/`enemy`/`wave`.
//!
//! Fixed-point convention: any quantity measured in tiles or tiles/second is
//! stored scaled by [`FIXED_SCALE`] so all rules arithmetic stays integer.
//! Percentages that combine multiplicatively (slow, splash falloff, armour
//! floor, ...) are stored in permille (parts per 1000) so rounding stays
//! exact for the values this design actually uses.

/// One simulation tick, in milliseconds. 20 Hz, per the engine contract.
pub const TICK_MS: u32 = 50;
/// Ticks per simulated second. `1000 / TICK_MS`.
pub const TICKS_PER_SECOND: i64 = 1000 / TICK_MS as i64;

/// Scale factor for tile-distance and tile-speed fixed-point values.
pub const FIXED_SCALE: i64 = 10_000;

/// Board size, in logical tiles.
pub const BOARD_WIDTH: i32 = 28;
pub const BOARD_HEIGHT: i32 = 14;

/// Heartseed starting Integrity on the Standard preset (also the plan's
/// canonical value; Cozy/Wild override it, see `Difficulty::preset`).
pub const STANDARD_INTEGRITY: i32 = 20;

/// Difficulty presets, in permille (1000 = x1.00), plus starting Integrity.
/// Order: threat, HP, reward, integrity.
///
/// `STANDARD_REWARD_PERMILLE`/`WILD_REWARD_PERMILLE` are the plan's own seed
/// values, unmodified. Only `COZY_REWARD_PERMILLE` moved (1100 -> 910); see
/// its own doc below for why. A prior calibration pass inflated all three
/// AND cut `BELLKEEPER_BASE_HP`/`NIGHT_MAW_BASE_HP` far below the plan's own
/// numbers, to compensate for a since-fixed defect: a boss body killed to 0
/// HP kept walking and could still reach the Heartseed and register a loss
/// (`advance_enemy_movement`, `sim.rs`, never checked `Boss::is_defeated`
/// before moving a body -- worse, once dead it could keep walking for as
/// long as that wave's escort minions took to clear, since
/// `check_wave_completion` only removes a defeated boss once
/// `all_spawned && no_enemies` too). That calibration was measuring a
/// walking corpse, not a live boss, so it is invalid and reverted here; see
/// `BELLKEEPER_BASE_HP`/`NIGHT_MAW_BASE_HP`'s own docs for the fresh,
/// post-fix numbers and how they were found.
pub const COZY_THREAT_PERMILLE: i64 = 850;
/// x1.044 of the pre-looping value (900) -- see `BELLKEEPER_BASE_HP`'s own
/// "x2.980" section for why Cozy needed a fresh fix at all (that section's
/// own 100.0%/100.0% reading, unchanged HP scalar) and for the full
/// methodology. Raising this (rather than only touching `COZY_REWARD_PERMILLE`,
/// the pre-looping profile's own sole Cozy lever) is new to this pass: unlike
/// the pre-looping profile, `COZY_REWARD_PERMILLE` alone could not reach the
/// band here (see its own doc's "hard step lands outside the band" finding),
/// and `COZY_HP_PERMILLE` -- this crate's own honest, boss/minion-HP-scaling
/// lever, the SAME kind `WILD_HP_PERMILLE` already relied on pre-looping --
/// moved smoothly instead: at fixed `COZY_INTEGRITY=16` (see that constant's
/// own doc), raising this from 900 to 1000 (Standard's own level) barely
/// moved `slow_stack` at all (96.0%, still far past the band) -- confirming
/// Cozy's own ease is NOT primarily an HP-scalar effect at this Integrity
/// level -- while 940 measured 56.0%/64.0% on two independent 100-seed
/// windows (0..100 / 5000..5100), inside the 50-70% band on both.
///
/// # 924, re-calibrated for Pet Charges (`PET_CHARGE_*`) and per-wave field
/// zones (`zone.rs`)
///
/// The two new mechanics (both drafted/rolled every run, not optional)
/// raised every difficulty's effective player power at once, and did so
/// unevenly: `ZONE_TOWER_DAMAGE_PERMILLE`/`ZONE_ENEMY_SPEED_PERMILLE`'s own
/// "50/37, re-calibrated" section found the SAME zone magnitude change moves
/// Standard's `slow_stack` win rate by double digits while barely touching
/// Cozy's (46.0% -> 44.0% at 100/75 -> 50/37, Cozy-only, `slow_stack`, 200
/// seeds, window 0..200) -- Cozy's own win/loss boundary sits on a much
/// harder Sap-affordability step than Standard's, so a shared-magnitude lever
/// tuned for Standard barely moves Cozy at all, and the value above (940)
/// left Cozy short of the band at the fresh, zone-halved calibration point:
/// `slow_stack` 44.0%/59.0% on two independent 200-seed windows (0..200 /
/// 5000..5200, at 940's OWN halved-zone state before this section's own
/// value was found) -- under the band's 50% floor on the first window.
///
/// Re-found by a direct-constant scan of THIS value alone (Cozy's own
/// isolated HP-scalar lever, per this doc's own established discipline),
/// at the fresh `ZONE_TOWER_DAMAGE_PERMILLE`/`ZONE_ENEMY_SPEED_PERMILLE`
/// halved-magnitude state, `slow_stack`, 200 seeds each window: 900 measured
/// 66.0%/79.0% (past the band's own 70% ceiling on the second window), 920
/// measured 53.0%/70.0% (the second window landing exactly on the ceiling,
/// same "on the edge, not past it" pattern this file's own history already
/// treats as valid -- see `COZY_REWARD_PERMILLE`'s own "935" section), 928
/// measured 49.0%/-- (under the 50% floor on the first window alone, no
/// second reading needed to reject it). 924 -- between the two bracketing
/// points, not pinned to either -- measured 51.0%/68.0%, both windows
/// comfortably inside 50-70% with real margin on both edges, the only one of
/// the four points tried that clears both windows without landing on either
/// boundary.
pub const COZY_HP_PERMILLE: i64 = 924;
/// x0.827 of the plan's own seed value (1100) -- see `BELLKEEPER_BASE_HP`'s
/// own doc for the full post-fix `hatchery-arcade-sweep` methodology this
/// was found under. Cozy's own boss-HP scalar (`COZY_HP_PERMILLE`) already
/// makes both bosses noticeably weaker for Cozy than for Standard at the
/// SAME base HP constant (both base HP constants are shared across all
/// three difficulties, unlike reward), which on its own pushed Cozy's
/// `slow_stack` (the sweep's strongest of its four policies -- see
/// `BELLKEEPER_BASE_HP`'s doc) win rate to ~100%, well past the task's own
/// 50-70% band, at a shared boss HP level chosen for Standard's own 20-40%
/// band. `COZY_HP_PERMILLE`/`COZY_THREAT_PERMILLE`/`COZY_INTEGRITY` were, at
/// the time this section was written, existing presets outside this fix's
/// scope (unchanged); the only free lever left to bring Cozy back down
/// without touching them was its own reward. The response to
/// `COZY_REWARD_PERMILLE` is a hard, single-permille
/// step (900-915 all measure identically; 920 jumps straight from ~53% to
/// ~90%) rather than a smooth curve -- a Sap-affordability threshold a
/// build crosses once, not a gradual effect -- so 910 (mid-plateau, not
/// pinned to either edge of the 900-915 flat) was the original robust
/// choice, not an arbitrarily narrow one.
///
/// # Superseded for `BUILD_RADIUS_FP` removal, Heartseed looping and
/// mandatory full clear -- see `COZY_HP_PERMILLE`/`COZY_INTEGRITY`'s own docs
///
/// Under the new rules, reward alone can no longer carry Cozy at all (see
/// `COZY_HP_PERMILLE`'s own doc): a coarse `--reward-mult` scan at the fresh
/// shared boss-HP level (`BELLKEEPER_BASE_HP`'s own "x2.980" section) found
/// the SAME kind of hard step this doc already describes, but landing on the
/// WRONG side of the band at every point tried -- 94%/98% at reward-mult
/// 600/650 (Sap-affordable, too easy) against 16-18% at 550-590
/// (Sap-starved, too hard), with no point in between reaching 50-70%. This
/// constant (935) stays unmoved; `COZY_HP_PERMILLE`/`COZY_INTEGRITY` are the
/// two levers that actually carry Cozy now.
///
/// # 935, re-calibrated alongside `BELLKEEPER_BASE_HP`'s own "x1.020"
/// section
///
/// The x1.020 shared boss-HP bump that re-tamed Standard/Wild under
/// combat-phase building and swarm waves (see that constant's own doc)
/// left Cozy's own `slow_stack` win rate at 48.0%/61.0% on two independent
/// 200-seed windows (0..200 / 5000..5200) -- inside the 50-70% band on one,
/// 2 points under on the other. A direct-constant scan of THIS value alone
/// (Cozy's own reward lever, isolated from Standard/Wild by construction)
/// found the SAME single-permille Sap-affordability step this constant's
/// own history already describes, moved from 920 to between 934 and 935 by
/// the intervening mechanic changes: 910-934 all measured window-0
/// 42.0-48.0% (still under band), 935 jumps straight to 53.0%. Measured at
/// 935, 200 seeds each on the same two windows: 53.0%/70.0% -- both inside
/// the 50-70% band (the second window lands exactly on its upper edge, not
/// past it); every value in the immediate plateau above 935 (937/940/950)
/// measured window-0 identically at 53.0%, so 935 -- the step's own lower
/// edge, not an arbitrary point further up the plateau -- is deliberately
/// the LOWEST-reward (hence safest against overshooting the band's own
/// upper edge) point that still clears window-0's own 50% floor.
pub const COZY_REWARD_PERMILLE: i64 = 935;
/// x0.64 of the pre-looping value (25) -- new to this pass, and the biggest
/// single move in it: Heartseed looping (`sim.rs`'s own `advance_enemy_
/// movement`) makes starting Integrity a genuine difficulty lever for the
/// first time (a defence that cannot kill what it faces now loses SLOWLY on
/// Integrity instead of instantly on a Heartseed breach -- the task's own
/// framing). `--integrity-mult` scanned at the fresh shared boss-HP level
/// (`BELLKEEPER_BASE_HP`'s own "x2.980" section), `COZY_HP_PERMILLE` still at
/// its pre-looping 900 for this scan: a whole-point CLIFF, not a gradient --
/// 0.0% at Integrity 15, a flat 58-70%-ish plateau at 16-18 (16 and 17
/// measured byte-identical at both n=100 and n=200, a real plateau not a
/// coincidence), 94-98% at 19-20. `BOSS_LAP_INTEGRITY_DAMAGE`'s own doc has
/// the even starker version of this same discrete-integer-cliff shape.
/// Combined with `COZY_HP_PERMILLE`'s own 900->940 move (both levers were
/// searched together, in that order: Integrity found the right plateau,
/// `COZY_HP_PERMILLE` then centred the window inside it -- see that
/// constant's own doc), 200 seeds each on two independent windows (0..200
/// and 5000..5200): `slow_stack` 51.0%/67.0% -- inside the 50-70% band on
/// both; `circuit` 40.0%/54.0% -- also in-band both windows (not required,
/// but notably tighter than Standard/Wild's own `circuit` gap here). Every
/// `slow_stack` loss lands on wave 8 alone (w4=0 both windows) -- the same
/// "all difficulty concentrates on Night Maw" shape `BELLKEEPER_BASE_HP`'s
/// own doc found for Standard.
pub const COZY_INTEGRITY: i32 = 16;

pub const STANDARD_THREAT_PERMILLE: i64 = 1000;
pub const STANDARD_HP_PERMILLE: i64 = 1000;
pub const STANDARD_REWARD_PERMILLE: i64 = 1000;

pub const WILD_THREAT_PERMILLE: i64 = 1120;
/// x0.963 of the plan's own seed value (1080) -- Wild's own isolated fix
/// for the task's own 0/400 finding: at 1080, `slow_stack` (this
/// difficulty's strongest of the sweep's four policies, same as Cozy/
/// Standard) never won a single one of 400 seeds across two independent
/// windows, which sits IN the task's own 0-10% band only in the "never
/// happens" sense the task itself calls out as a bad answer, not the
/// "happens rarely" sense the band means.
///
/// Found by a direct-constant grid search (coarse step, then fine step
/// around the found plateau) against the fixed engine (see
/// `BELLKEEPER_BASE_HP`'s own doc for why direct constants, not
/// `--*-mult`, is this crate's own calibration-search discipline),
/// scanning every one of `WILD_THREAT_PERMILLE`/`WILD_HP_PERMILLE`/
/// `WILD_REWARD_PERMILLE`/`WILD_INTEGRITY` in isolation (each is
/// difficulty-scoped, per `Difficulty::preset`, so none of the four can
/// touch Cozy/Standard by construction) before choosing which to move:
/// `WILD_THREAT_PERMILLE` barely moved the win rate at all over its own
/// plausible range and did so NON-monotonically (noise, not signal, at
/// 200 seeds); `WILD_INTEGRITY` moved it not at all up to +50% (Wild's
/// own losses are not Integrity-drain, the same way Standard's `circuit`
/// losses turned out not to be until its own build was fixed -- see
/// `sweep`'s own `pet-bastion` changelog); `WILD_REWARD_PERMILLE`
/// reproduced the EXACT known trap this task's own brief warned about (a
/// Sap-affordability step, not a curve: 0%/3%/3% at 1000/1050/1080 permille,
/// then 10% at 1100, then a hard jump to 41% at 1120 -- a single 20-permille
/// step quadrupling the win rate). `WILD_HP_PERMILLE` alone moved smoothly
/// and monotonically across its own full tested range (0%/5%/16%/42%/77%/
/// 94% at 1080/1040/1000/960/920/880), the honest lever the other three
/// are not. 1040 sits mid-plateau (1025-1060 all measured 3-5%, not
/// pinned to either edge), not chosen at the boundary of a region that
/// still climbs steeply on either side. Measured at 200 seeds on two
/// independent windows (0..200 and 5000..5200), `slow_stack`: 5.0%/7.0% --
/// inside the task's own 3-8% band on both, and Cozy (55.0%/51.0%) and
/// Standard (38.0%/39.0%) both measured byte-for-byte unchanged (this
/// constant cannot reach either preset).
///
/// # x0.712, re-calibrated for `BUILD_RADIUS_FP` removal, Heartseed looping
/// and mandatory full clear
///
/// At the fresh shared boss-HP level (`BELLKEEPER_BASE_HP`'s own "x2.980"
/// section) the value above (1040) was catastrophically too hard, not too
/// easy: `slow_stack` 0/100 across the ENTIRE 800-1000 permille range tried
/// (800/850/900/950/1000 all measured 0.0%), the SAME "never happens" trap
/// this constant's own original doc warned about, now reproduced by the
/// boss-HP jump instead of the original pre-fix number. This constant
/// remained the honest, smooth lever it always was, though: a direct-constant
/// scan (this crate's own standing discipline) below 800 found a real,
/// monotonic gradient once low enough to matter at all -- 60% at 700, then a
/// steep but smooth (not a Sap-affordability step) fall to 24%/4%/0%/0% at
/// 720/740/750/760. 740 sits just inside the gradient's own lower shoulder,
/// not on the cliff edge at 750.
///
/// Confirmed at 200 seeds each on two independent windows (0..200 and
/// 5000..5200), `slow_stack`: Wild 4.0%/8.0% -- inside the 3-12% band on
/// both. `circuit` measured 0.0%/0.0% both windows -- under the band, same
/// standing "weaker of the two strong policies, not a second gap to close"
/// pattern every other difficulty shows here too. All losses land on wave 8
/// alone (w4=0 both windows).
///
/// # 733, re-calibrated for Pet Charges (`PET_CHARGE_*`) and per-wave field
/// zones (`zone.rs`)
///
/// Pet Charges and field zones together pushed the value above (740) from
/// 4.0%/8.0% to 9.0%/3.0% -- still nominally inside 3-12% on both windows,
/// but the second window sat exactly on the floor with zero margin left in
/// either direction, and this whole profile's Wild reading already carries
/// real window-to-window swing at fixed seeds/policy (this section's own
/// readings below move by a factor of 2-3x across the same 200-seed window
/// for a handful of permille of movement) -- a floor-exact reading here is
/// one unlucky window away from reading under-band, not a comfortable rest
/// point. Re-scanned directly (Wild's own isolated HP-scalar lever, same
/// discipline as every section above), `slow_stack`, 200 seeds each window,
/// at the fresh `ZONE_TOWER_DAMAGE_PERMILLE`/`ZONE_ENEMY_SPEED_PERMILLE`
/// halved-magnitude state (`ZONE_TOWER_DAMAGE_PERMILLE`'s own "50/37"
/// section): 725 measured 15.0%/10.0% (past the 12% ceiling on the first
/// window), 730 measured 15.0%/8.0% (same ceiling breach), 736 measured
/// 12.0%/4.0% (both in-band, but the second window barely improved over the
/// unmoved value's own 3.0%). 733 measured 12.0%/6.0% -- both windows inside
/// 3-12%, the first landing exactly on the ceiling (the same "on the edge,
/// not past it" pattern this file's own history already treats as valid --
/// see `COZY_REWARD_PERMILLE`'s own "935" section) but the second now
/// carrying real margin (6.0%, double the unmoved value's floor-exact
/// 3.0%) instead of none. `circuit` measured 0.0%/0.0% both windows at this
/// value -- unchanged from the pre-mechanics reading, still under the band,
/// still not a second gap to close per this doc's own standing rule.
pub const WILD_HP_PERMILLE: i64 = 733;
/// # x1.016, re-scaled for free tower placement (`board.rs`'s `PadId`
/// removal)
///
/// The value above (950) was calibrated against `BELLKEEPER_BASE_HP`/
/// `NIGHT_MAW_BASE_HP`'s own former (fixed-pad-era) 567/1751. Once those
/// two moved to 1049/3239 (x1.850, see `BELLKEEPER_BASE_HP`'s own doc) to
/// re-tame Cozy/Standard under free placement, Wild's own win rate at the
/// UNCHANGED 950 reward dropped from that section's own already-measured
/// 0.0% to a still-0.0% (0/100 seeds, both `slow_stack`/`circuit`) --
/// expected, since a bigger shared boss-HP floor makes an unaffordable
/// build even less affordable, not more.
///
/// Re-found the same way the value above originally was: a
/// `hatchery-arcade-sweep --reward-mult` grid search (permille scoped to
/// Wild's OWN preset only, so this never touches Cozy/Standard, exactly
/// like the original search) at the NEW 1049/3239 boss HP, `slow_stack`.
/// Reproduced the same "Sap-affordability step" signature this constant's
/// own history already warns about: coarse step (1100/1200/1300/1400
/// permille) jumped straight to 22%/62%/88%/92% -- already well past the
/// 3-8% band at the very first probed point -- so the real threshold sits
/// below 1100; fine step (1000/1010/1020/1030/1040/1060/1080 permille)
/// found a genuine two-point plateau at 1010/1020 (6%/6%, byte-identical
/// avg tick counts -- the same effective Sap bucket after rounding),
/// bracketed by 0% at 1000 and 1030 on either side -- a real plateau, not
/// a lone spike. `950 * 1015 / 1000 (round_div) = 964`; baked DIRECTLY
/// (this constant IS the scaled value now, no override layer) as 965, the
/// plateau's own midpoint rather than either measured endpoint.
///
/// Confirmed at the DIRECT constant (965, `BalanceOverrides::default()`,
/// no override), 200 seeds each on two independent windows (0..200 and
/// 5000..5200), `slow_stack`: Wild 7.0%/5.0% -- inside the 3-8% band on
/// both. `circuit` measured 0.0%/0.0% at this same point (still the
/// weaker of the two strong policies here, same as it was pre-rescale);
/// the calibration target throughout this whole doc has only ever been
/// the STRONGEST policy (`BELLKEEPER_BASE_HP`'s own doc), so `circuit`
/// sitting under the band while `slow_stack` sits inside it is not a
/// second gap to close.
pub const WILD_REWARD_PERMILLE: i64 = 965;
pub const WILD_INTEGRITY: i32 = 14;

/// Integrity lost when a single enemy reaches the Heartseed. The enemy is
/// NOT removed for this any more (`sim.rs`'s own `advance_enemy_movement`)
/// -- it loops back to its own route start and keeps going, the same unit
/// continuing rather than a fresh spawn, so a lane a player never defends
/// keeps bleeding Integrity once per lap per enemy rather than stopping
/// once every enemy has passed through exactly once.
pub const LEAK_INTEGRITY_DAMAGE: i32 = 1;

/// Integrity lost when a BOSS (any body) completes a lap by reaching the
/// Heartseed, replacing the old instant `RunOutcome::Lost` a boss's own
/// Heartseed arrival used to register outright (`sim.rs`'s
/// `advance_enemy_movement`, before this constant existed) -- the owner's
/// own complaint ("сейчас босс ваншотит"): a boss now costs Integrity like
/// everything else that reaches the Heartseed, not an instant loss.
///
/// A single shared charge per tick, not per body: Night Maw's two split
/// bodies (`Boss::bodies`) can each independently loop, but a shared-tick
/// double arrival is one boss lap, not two -- see `advance_enemy_movement`'s
/// own doc for why this is batched per tick rather than per body.
///
/// Sized against [`WILD_INTEGRITY`] (14), the lowest Integrity budget of
/// any difficulty preset and so this constant's own worst case: `14 / 3 =
/// 4` laps before a boss alone drains a Wild run to zero, clearly more
/// than [`LEAK_INTEGRITY_DAMAGE`]'s own single point per minion lap (a
/// boss lap must hurt visibly more than one minion's) while still leaving
/// several laps' worth of margin for a player who is chipping the boss
/// down slowly rather than one who has already lost -- not one hit, not an
/// unlimited number either.
///
/// # Measured knife-edge at the whole profile's own calibration
/// (`BELLKEEPER_BASE_HP`'s "x2.980" section, `COZY_HP_PERMILLE`/
/// `COZY_INTEGRITY`/`WILD_HP_PERMILLE`'s own docs)
///
/// This value was NOT itself moved to hit the task's own bands -- boss HP
/// and starting Integrity already reached all three -- but a direct-constant
/// probe of it ALONE (Standard, otherwise-final constants, 100 seeds) found
/// it is, by a wide margin, the single most brittle constant in the whole
/// profile: 1 and 2 both measure 100.0% (`slow_stack` never loses a single
/// seed), 3 (this value) measures inside the 25-40% band, 4 and 5 both
/// measure 0.0% (`slow_stack` never wins a single seed). A whole-integer
/// step either way flips the run from "always wins" to "always loses" --
/// there is no fractional room between them to retune with, unlike every
/// permille-scaled lever in this file. The current calibration depends on
/// this EXACT value; changing it requires re-running the whole boss-HP/
/// Integrity search above, not a small nudge.
pub const BOSS_LAP_INTEGRITY_DAMAGE: i32 = 3;

// ---------------------------------------------------------------------------
// Living Circuit
// ---------------------------------------------------------------------------

/// Default number of towers the pet links at an anchor (before evolution).
pub const CIRCUIT_BASE_SLOTS: usize = 3;
/// Moth evolution: connects four towers.
pub const CIRCUIT_MOTH_SLOTS: usize = 4;
/// Wisp evolution: connects only two towers.
pub const CIRCUIT_WISP_SLOTS: usize = 2;
/// Night Maw final phase forces the circuit down to two towers regardless of
/// evolution (chosen: overrides evolution slot count while active).
pub const CIRCUIT_NIGHT_MAW_FINAL_SLOTS: usize = 2;

/// Attack-speed multiplier for linked towers, in permille (1300 = +30%).
pub const LINKED_ATTACK_SPEED_PERMILLE: i64 = 1300;

/// Link Burst reactivation cooldown per tower.
pub const LINK_BURST_COOLDOWN_TICKS: u64 = (6 * TICKS_PER_SECOND) as u64;

/// One Spark is generated every this many linked kills.
pub const SPARK_PER_KILLS: u32 = 5;
/// Spark storage cap.
pub const SPARK_CAP: u32 = 8;
/// Night Maw's final phase doubles Spark generation; modelled as crediting
/// two kills per linked kill instead of one.
pub const NIGHT_MAW_FINAL_PHASE_KILL_CREDIT: u32 = 2;

/// Pet Move travel duration (~1.2s), base (pre-evolution).
pub const PET_MOVE_TICKS: u64 = 24;
/// Moth evolution moves more slowly.
pub const PET_MOVE_TICKS_MOTH: u64 = 36;

/// Pet action Spark costs.
pub const BLINK_COST: u32 = 1;
pub const PET_PULSE_COST: u32 = 3;
pub const FULL_CIRCUIT_COST: u32 = 5;

/// Full Circuit duration: connects every placed attacking tower.
pub const FULL_CIRCUIT_TICKS: u64 = (4 * TICKS_PER_SECOND) as u64;

/// Pet Pulse: flat area damage (armour rule still applies, pierce 0),
/// radius in tiles (fixed-point), and knockback distance along the enemy's
/// own route (bosses are immune to the knockback component only).
pub const PET_PULSE_DAMAGE: i32 = 15;
pub const PET_PULSE_RADIUS_FP: i64 = 25_000; // 2.5 tiles
pub const PET_PULSE_KNOCKBACK_FP: i64 = 5_000; // 0.5 tile
/// Crab evolution strengthens Pet Pulse damage, in permille.
pub const CRAB_PET_PULSE_DAMAGE_PERMILLE: i64 = 1500; // +50%

/// Crab evolution: shield gained on every anchor arrival. Modelled as
/// absorbed Integrity points banked in a buffer that leaks drain before the
/// Heartseed itself loses Integrity.
pub const CRAB_SHIELD_ON_ARRIVAL: i32 = 2;
pub const CRAB_SHIELD_CAP: i32 = 6;

/// Wisp evolution: one free Blink every this many ticks.
pub const WISP_FREE_BLINK_INTERVAL_TICKS: u64 = (12 * TICKS_PER_SECOND) as u64;

/// Anchor rune ("Anchor"): Circuit buffs linger this long after the pet
/// leaves an anchor.
pub const ANCHOR_RUNE_LINGER_TICKS: u64 = (3 * TICKS_PER_SECOND) as u64;

// ---------------------------------------------------------------------------
// Combat-wide caps
// ---------------------------------------------------------------------------

/// Splash affects at most this many enemies (primary included).
pub const MAX_SPLASH_TARGETS: usize = 5;
/// Prism affects at most this many total targets (primary included).
pub const MAX_CHAIN_TARGETS: usize = 4;
/// Slow never combines above this ceiling, in permille.
pub const MAX_COMBINED_SLOW_PERMILLE: i64 = 600;
/// Armour never reduces a hit below this fraction of its original damage,
/// in permille.
pub const ARMOUR_DAMAGE_FLOOR_PERMILLE: i64 = 200;

/// Bell Link Burst "brief group stun" duration.
pub const BELL_GROUP_STUN_TICKS: u64 = (75 * TICKS_PER_SECOND as u64) / 100; // 0.75s
/// Prism's normal jump falloff (of the primary hit), in permille, applied in
/// order; Link Burst extends the same geometric ratio for any jump beyond
/// what this table lists, capped by [`MAX_CHAIN_TARGETS`].
pub const PRISM_JUMP_FALLOFF_PERMILLE: [i64; 2] = [700, 450];
/// Ratio applied to extend the falloff table geometrically past its last
/// entry (450/700, rounded to permille) for Link Burst's extra jumps.
pub const PRISM_JUMP_EXTRA_RATIO_PERMILLE: i64 = 643;

/// Ember Nest splash falloff applied to every non-primary splash target, in
/// permille of the primary hit.
pub const EMBER_SPLASH_FALLOFF_PERMILLE: i64 = 450;

/// Ember Nest Link Burst "immediate burning field": one immediate splash
/// application at full base damage, independent of its normal cooldown.
/// Moonwell Link Burst "lingering damage field" duration and per-tick share
/// of its base damage (permille of base damage dealt each tick).
pub const MOONWELL_LINGER_TICKS: u64 = 2 * TICKS_PER_SECOND as u64;
pub const MOONWELL_LINGER_TICK_DAMAGE_PERMILLE: i64 = 250;

/// Relay: amplifies another tower's Link Burst output by this permille bonus
/// when that tower bursts within [`RELAY_AMPLIFY_RADIUS_FP`] of a placed,
/// active Relay (flat bonus, does not stack across multiple Relays).
pub const RELAY_AMPLIFY_BONUS_PERMILLE: i64 = 250;
pub const RELAY_AMPLIFY_RADIUS_FP: i64 = 30_000; // 3.0 tiles

/// Husher: suppresses nearby tower fire rate by stretching their attack
/// interval, in permille (1300 = +30% interval = -23% dps).
pub const HUSHER_SUPPRESS_RADIUS_FP: i64 = 25_000; // 2.5 tiles
pub const HUSHER_SUPPRESS_INTERVAL_PERMILLE: i64 = 1300;

/// Mirror: after taking a hit from a damage family, resists that family for
/// a short window.
pub const MIRROR_RESIST_PERMILLE: i64 = 500; // -50% damage from that family
pub const MIRROR_RESIST_TICKS: u64 = 3 * TICKS_PER_SECOND as u64;

/// Splitter: enemies spawned on kill (not on leak).
pub const SPLITTER_SPAWN_COUNT: u32 = 3;

// ---------------------------------------------------------------------------
// Tower upgrades
// ---------------------------------------------------------------------------

/// L2 cost, in permille of base cost.
pub const UPGRADE_L2_COST_PERMILLE: i64 = 750;
/// L3 cost, in permille of base cost (paid on top of L2's cost).
pub const UPGRADE_L3_COST_PERMILLE: i64 = 1250;
/// L2 strengthens the tower's defined role via a flat damage multiplier.
pub const UPGRADE_L2_DAMAGE_PERMILLE: i64 = 1500;
/// L3 "Power" branch: total damage multiplier over base (replaces L2's
/// multiplier rather than stacking).
pub const UPGRADE_L3_POWER_DAMAGE_PERMILLE: i64 = 1800;
/// L3 "Utility" branch: slightly lower damage multiplier, plus one extra
/// point of pierce/chain/splash capacity (role-specific, see `tower.rs`).
pub const UPGRADE_L3_UTILITY_DAMAGE_PERMILLE: i64 = 1600;

/// Sell refund, in permille of total Sap spent on a tower (placement +
/// upgrades).
pub const SELL_REFUND_PERMILLE: i64 = 500;

/// Cooldown a tower starts with when placed while `RunPhase::Combat` is
/// already running (`Simulation::place_tower`), instead of the `0` a
/// Build-phase placement gets -- the cost side of allowing tower placement,
/// upgrades and selling during combat at all, not only the ten-second Build
/// countdown before a wave. 2 seconds (40 ticks at [`TICKS_PER_SECOND`]):
/// longer than every attacking tower's own base interval (13-48 ticks, see
/// `tower.rs`'s `TowerKind::base_stats`), so a tower dropped mid-wave never
/// out-paces the SAME tower placed a moment earlier during Build, but short
/// enough that reactive combat building still meaningfully helps the wave
/// it was placed for rather than only the next one.
pub const COMBAT_PLACEMENT_ARMING_TICKS: u32 = 2 * TICKS_PER_SECOND as u32;

// ---------------------------------------------------------------------------
// Run economy
// ---------------------------------------------------------------------------

pub const START_SAP: i32 = 120;
pub const WAVE_COUNT: u32 = 8;
pub const BUILD_PHASE_TICKS: u64 = 10 * TICKS_PER_SECOND as u64;

/// Per-wave combat target duration, in ticks (used to pace spawns across a
/// wave; not a hard end-of-wave timer -- a wave ends only when its whole
/// spawn plan is cleared).
pub const WAVE_COMBAT_TARGET_TICKS: [u64; 8] = [
    20 * TICKS_PER_SECOND as u64,
    25 * TICKS_PER_SECOND as u64,
    30 * TICKS_PER_SECOND as u64,
    50 * TICKS_PER_SECOND as u64,
    35 * TICKS_PER_SECOND as u64,
    40 * TICKS_PER_SECOND as u64,
    45 * TICKS_PER_SECOND as u64,
    60 * TICKS_PER_SECOND as u64,
];

/// `base_threat(w)`, taken directly from the plan's own worked table (its
/// defining formula, `round(60 * 1.27^(w-1))`, is computed once at design
/// time -- the plan's own literal table -- not re-derived at runtime via
/// floating point, per the "no floats in rules" requirement).
pub const BASE_THREAT: [u32; 8] = [60, 76, 97, 123, 156, 198, 252, 320];

/// Boss waves (1-indexed wave number).
pub const BOSS_WAVES: [u32; 2] = [4, 8];

/// Boss-wave effective-threat multiplier, as an exact rational (numerator /
/// denominator) so `effective_threat` stays integer: `x1.25 = 5/4`.
pub const BOSS_THREAT_MULT_NUM: i64 = 5;
pub const BOSS_THREAT_MULT_DEN: i64 = 4;

/// `hp_scalar(w) = 1 + 0.065*(w-1)`, in permille per wave-step.
pub const HP_SCALAR_BASE_PERMILLE: i64 = 1000;
pub const HP_SCALAR_STEP_PERMILLE: i64 = 65;

/// `clear_reward(w) = 18 + round(0.68 * effective_threat(w))`.
pub const CLEAR_REWARD_BASE: i32 = 18;
pub const CLEAR_REWARD_MULT_NUM: i64 = 68;
pub const CLEAR_REWARD_MULT_DEN: i64 = 100;

/// Wave generator must land its total spent threat within this permille
/// window of `effective_threat(w)` (950 = 95%, 1050 = 105%).
pub const WAVE_BUDGET_MIN_PERMILLE: i64 = 950;
pub const WAVE_BUDGET_MAX_PERMILLE: i64 = 1050;

/// Ticks between spawns within one wave's plan, floor value (a wave with a
/// very large enemy count still spaces spawns at least this far apart).
/// Doubles as the intra-pack gap [`WAVE_SPAWN_CLUSTER_SIZE`] uses to space
/// the members of one pack -- the same "how close together can two spawns
/// legitimately land" number either way.
pub const MIN_SPAWN_INTERVAL_TICKS: u64 = 6;

/// How many enemies land in one spawn pack (`wave.rs`'s `generate_wave`):
/// a wave's own spawn plan is grouped into packs of this size, each member
/// [`MIN_SPAWN_INTERVAL_TICKS`] after the previous one, with a larger gap
/// between one pack and the next -- so towers, Bell's slow, Ember Nest's
/// splash and Prism's chain regularly face several enemies at once instead
/// of a single-file trickle. 4 is large enough that a full pack is a real,
/// visible mass (bigger than [`MAX_SPLASH_TARGETS`]'s effective floor of
/// "more than one" but not so far past it that most of a pack goes
/// un-splashed by a single Ember Nest hit) and small enough that a two-lane
/// wave still puts a genuine 2 enemies per lane in the same pack (odd/even
/// `i % 2` route alternation, unchanged), not a lopsided single-lane clump.
pub const WAVE_SPAWN_CLUSTER_SIZE: usize = 4;

/// Reconstruction-time sampling weight for each [`crate::enemy::EnemyKind`],
/// read by `wave.rs`'s `closest_achievable_combo` when its own knapsack
/// search has already fixed exactly how much total threat a wave will
/// spend and needs to decide WHICH multiset of kinds fills it. A higher
/// weight means "more likely to be picked whenever it is one of several
/// kinds that could validly reach the still-remaining sum" -- the ONLY
/// lever this pass changes: `is_reachable`'s own search still targets the
/// identical [`WAVE_BUDGET_MIN_PERMILLE`]-[`WAVE_BUDGET_MAX_PERMILLE`]
/// window over the identical per-wave `effective_threat`, so a wave still
/// spends exactly the threat it always did -- only which enemies that
/// threat buys shifts, toward many cheap swarm bodies (Mite/Skitter) with a
/// real but minority mix of pricier specialists, replacing the old
/// uniform-among-choices reconstruction that had no reason to prefer
/// either and produced a handful of individually tough enemies per wave
/// (see this constant's own calibration doc/handoff for the measured
/// before/after enemy-count table). Every weight is a plain integer >= 1
/// (never 0 -- a kind present in `roster_for_wave` must stay reachable by
/// this sampling step whenever it is genuinely needed to hit an exact
/// remainder, even though it is rarely PREFERRED); the specific values are
/// tuned to prefer Mite and Skitter heavily while keeping every wave's
/// pricier specialist kinds (Splitter/Shellback/Husher/Mirror) a real,
/// occasional part of the mix, never mathematically excluded.
pub const ENEMY_SWARM_WEIGHT_MITE: u32 = 20;
pub const ENEMY_SWARM_WEIGHT_SKITTER: u32 = 8;
pub const ENEMY_SWARM_WEIGHT_SPLITTER: u32 = 2;
pub const ENEMY_SWARM_WEIGHT_SHELLBACK: u32 = 2;
pub const ENEMY_SWARM_WEIGHT_HUSHER: u32 = 1;
pub const ENEMY_SWARM_WEIGHT_MIRROR: u32 = 1;

/// Skitters sent through the opposite entrance at each Bellkeeper HP
/// threshold (75%, 50%, 25%).
pub const BELLKEEPER_ESCORT_COUNT: u32 = 3;
pub const BELLKEEPER_BELL_INTERVAL_TICKS: u64 = 10 * TICKS_PER_SECOND as u64;
pub const BELLKEEPER_SILENCE_TICKS: u64 = 2 * TICKS_PER_SECOND as u64;
/// x0.515 of the plan's own seed value (1100) -- roughly 1.5x the invalid
/// pre-fix calibration's 374 (x0.34), confirming the task's own expectation
/// that fixing the "walking dead boss" defect (see `COZY_REWARD_PERMILLE`'s
/// own doc) would let a boss keep noticeably more HP, though far from the
/// plan's own un-cut number: DPS output within a wave's own time/threat
/// budget, not the movement defect, is still the real ceiling here.
///
/// Found by a `hatchery-arcade-sweep` grid search (coarse permille step
/// -> fine step -> exhaustive single-permille scan across every flat
/// region) against the post-fix engine, run DIRECTLY on this constant (not
/// through `--bellkeeper-hp-mult`/`--night-maw-hp-mult`/`--reward-mult`,
/// which apply on top of whatever these five constants already hold and so
/// go through an extra rounding step this search deliberately avoided --
/// see the fix's own handoff for the concrete case where that extra
/// rounding step alone flipped a measured outcome). Target: the strongest
/// of the sweep's four policies (`slow_stack` -- the only one of the four
/// that ever wins at all in this constant's neighbourhood; `baseline`/
/// `greedy`/`circuit` stay at 0% throughout, bottlenecked by wave-4 escort
/// leaks rather than raw boss HP) lands Cozy/Standard/Wild win rate in the
/// task's own 50-70%/20-40%/0-10% bands. Both `NIGHT_MAW_BASE_HP` and
/// `COZY_REWARD_PERMILLE` were tuned alongside this constant -- see their
/// own docs -- since a single shared boss-HP level cannot by itself
/// satisfy three different difficulty bands at once (Cozy's own
/// `COZY_HP_PERMILLE` discount pulls its win rate up much faster than
/// Standard's or Wild's as this constant drops).
///
/// Measured at 200 seeds each on two independent windows (0..200 and
/// 5000..5200), `slow_stack`, both bosses at the constants of that time,
/// `BalanceOverrides::default()` (no sweep-time override at all): Cozy
/// 55.0%/51.0%, Standard 38.0%/39.0%, Wild 0.0%/0.0% -- every figure inside
/// its own band on both windows. Death-wave split for `slow_stack`'s own
/// losses spreads across BOTH boss waves (window 0: Cozy w4=8/w8=82,
/// Standard w4=14/w8=110, Wild w4=74/w8=126), not piled on one wave the way
/// the pre-fix walking-corpse calibration piled every loss onto wave 4
/// alone. Waves 5-7 still produce zero losses for every policy in both
/// windows (bar a handful of stray `circuit` wave-5 losses, <=6/200) --
/// this still held after the movement fix, so it was NOT an artifact of
/// that defect: at this Integrity budget and minion HP scalar, a build
/// strong enough to dent either boss is also strong enough to clear waves
/// 5-7 outright, every time.
///
/// # x1.850, re-scaled for free tower placement (`board.rs`'s `PadId`
/// removal)
///
/// The value above (567) was calibrated against ten FIXED pads, every one
/// at least 2.0 tiles from its nearest route leg. Once `Command::Place`
/// took a free `Tile` instead (any cell within `BUILD_RADIUS_FP`, as
/// little as 1 tile off a route -- see that constant's own doc), the same
/// 567 HP stopped meaning anything: `slow_stack`/`circuit` (this
/// constant's own target policies) both won every single one of 200 seeds
/// on EVERY difficulty (Cozy/Standard/Wild all 100.0%), because a build
/// that can cluster several towers within 1-2 tiles of the SAME route
/// stretch lands far more overlapping DPS on a passing boss than the old
/// geometrically-separated pads ever could -- not a bug, the literal
/// balance shift free placement was expected to cause.
///
/// Re-found by a `hatchery-arcade-sweep` grid search run through
/// `--bellkeeper-hp-mult`/`--night-maw-hp-mult` (both multipliers moved
/// together, since -- per this doc's own original methodology -- a single
/// shared boss-HP level cannot satisfy three difficulty bands alone;
/// `WILD_REWARD_PERMILLE` still carries Wild's own separate fix, see its
/// own doc): coarse step (1000/2000/3000/4000 permille) found the cliff
/// between x1.0 (100% Standard) and x3.0 (0% Standard) sits close to x2.0;
/// fine step (1200/1400/1600/1800/1850/1900/1950) found Standard's own
/// win rate is smooth and monotonic through that region (unlike a hard
/// Sap-affordability step) -- 100%/100%/96%/60%/28%/20%/4% at
/// 1200/1400/1600/1800/1850/1900/1950 (50 seeds) -- landing x1.850 mid-band
/// (28%, not pinned to either the 20% or 40% edge). This ratio is then
/// baked DIRECTLY into both base-HP constants below (`round_div(old_base *
/// 1850, 1000)`, the same "arithmetically equivalent to editing the
/// constant itself and re-running the unmodified formula" property
/// `wave.rs`'s own `generate_wave` documents right above its `boss_max_hp`
/// computation), not left as a permanent CLI override, per this doc's own
/// original "direct constants, not `--*-mult`" discipline.
///
/// Confirmed at the DIRECT constants (1049/3239, `BalanceOverrides::
/// default()`, no override), 200 seeds each on two independent windows
/// (0..200 and 5000..5200), `slow_stack` (Cozy/Standard's own strongest of
/// the four): Cozy 56.0%/51.0%, Standard 28.0%/32.0% -- every figure
/// inside its own 50-70%/20-40% band on both windows. `circuit` (this
/// policy's own closest competitor under free placement, see `sweep`'s
/// own `policy.rs` module doc) measured Cozy 43.0%/39.0%, Standard
/// 25.0%/24.0% -- also in-band both windows, so neither of the two strong
/// policies needed a second, separate calibration pass. See
/// `WILD_REWARD_PERMILLE`'s own doc for Wild's separate 0%-at-this-HP-level
/// fix.
/// # x1.020, re-calibrated for combat-phase building and swarm waves
///
/// The value above (1049) was calibrated against a game that could only
/// build/upgrade/sell during the 10-second Build phase, and whose waves
/// spent their threat budget on a handful of individually tough enemies
/// (`wave.rs`'s old uniform-among-choices `closest_achievable_combo`
/// reconstruction). Two changes -- `Simulation::build_actions_allowed` now
/// permitting Place/Upgrade/Sell during `RunPhase::Combat` too, and
/// `ENEMY_SWARM_WEIGHT_MITE`'s own swarm-composition bias spending the
/// SAME per-wave threat on many more, weaker enemies in tighter spawn
/// packs -- both raised `slow_stack`'s (this constant's own target policy)
/// win rate well past every difficulty's own band at the unchanged 1049:
/// splash/chain/slow now clear escort minions fast enough that far more
/// tower output reaches the boss itself, the direct mechanism behind the
/// jump (measured Standard 41.0%/Wild 14.0% at 1049 before this
/// re-calibration, both above their own 20-40%/3-8% bands).
///
/// Re-found by a `hatchery-arcade-sweep` grid search run through
/// `--bellkeeper-hp-mult`/`--night-maw-hp-mult` (both moved together, same
/// methodology as this doc's own original search), 100 seeds per point:
/// 1000/1010/1020/1030 permille measured `slow_stack` Standard
/// 30%/36%/28%/20%, Wild 16%/10%/4%/0% -- Wild's own band (3-8%) is the
/// tight constraint here (it clears the top of its band by 1030 and the
/// bottom by 1010), landing 1020 as the one point inside Wild's band with
/// Standard still comfortably inside its own. Confirmed at the DIRECT
/// constants below (`round_div(1049*1020,1000)=1070`,
/// `round_div(3239*1020,1000)=3304`), 200 seeds each on two independent
/// windows (0..200 and 5000..5200), `slow_stack`: Standard 27.0%/37.0%,
/// Wild 4.0%/6.0% -- both inside their own bands on both windows. Cozy
/// measured 48.0%/61.0% -- inside its own 50-70% band on window 5000..5200
/// but 2 points under on window 0..200; `COZY_REWARD_PERMILLE`'s own doc
/// carries the matching fix for that specific gap (Cozy has its own reward
/// lever precisely so a shared boss-HP level chosen for Standard/Wild never
/// has to also carry Cozy on its own).
///
/// # x2.980, re-calibrated for `BUILD_RADIUS_FP` removal, Heartseed looping
/// and mandatory full clear
///
/// Three rule changes landed together (the owner's own direction, not this
/// pass's choice -- `board.rs`, `enemy.rs`/`boss.rs`, `sim.rs`'s own module
/// docs carry each one): (1) `BUILD_RADIUS_FP` itself is gone -- a tower may
/// go on any in-bounds, non-route/anchor/occupied tile, not just within 2.0
/// tiles of a route; (2) a unit (minion OR boss body) that reaches the
/// Heartseed no longer leaks/loses the run -- it loops back to its own route
/// start and keeps walking, costing [`LEAK_INTEGRITY_DAMAGE`]/
/// [`BOSS_LAP_INTEGRITY_DAMAGE`] per lap instead; (3) `check_wave_completion`
/// now requires every spawned enemy to be KILLED, not merely survived past
/// once. Together these turned "kill the boss in one pass or lose instantly"
/// into "the boss WILL die eventually to any nonzero DPS, bounded only by
/// whether Integrity survives enough laps" -- at the value above (1070, itself
/// already re-scaled once for `BUILD_RADIUS_FP`'s ORIGINAL introduction), both
/// `slow_stack` and `circuit` won every single one of 200 seeds on EVERY
/// difficulty (Cozy/Standard/Wild all 100.0%), full board-wide free placement
/// plus unlimited retries via looping erasing all three bands at once.
///
/// Re-found by a `hatchery-arcade-sweep` grid search run through
/// `--bellkeeper-hp-mult`/`--night-maw-hp-mult` (both multipliers moved
/// together, same methodology as this doc's own original search), Standard,
/// `slow_stack`: coarse step (1500/2000/2500/3000 permille, 50 seeds) found
/// the cliff between x2.5 (100%) and x3.0 (4%) sits inside that window; fine
/// step (2900-3000, 50-100 seeds) narrowed it to a real, if steep, gradient
/// -- NOT a single-permille Sap-affordability step this time (see this
/// constant's own "x1.850" section above for what one of those looks like) --
/// 96%/82%/62%/40%/24% at 2900/2920/2950/2960/2980 (100 seeds). x2.980 was
/// picked, not the steeper-still region past it, because it is the point
/// whose OWN two-window spread (below) sits most centrally in the 25-40%
/// band rather than pinned to either edge -- 2960 (41%/unmeasured) and 2975
/// (36%/40%, the second window exactly on the ceiling) were both tried and
/// rejected for landing closer to the edge on at least one window.
///
/// Before trusting the override for this search, it was checked against a
/// directly-set constant once (the task's own explicit ask, after the prior
/// pass found an override bug): at mult=2960, baking `round_div(1070*2960,
/// 1000)=3167`/`round_div(3304*2960,1000)=9780` directly (no override at
/// all) reproduced the override run's own 40 won / 60 lost out of 100 EXACTLY
/// -- the fold-before-scale fix in `wave.rs`'s own `generate_wave` (see its
/// own doc) holds under the new rules too.
///
/// Confirmed at the DIRECT constants below (`round_div(1070*2980,1000)=
/// 3189`, `round_div(3304*2980,1000)=9848`), 200 seeds each on two
/// independent windows (0..200 and 5000..5200), `BalanceOverrides::default()`
/// (no override at all -- reproduced the override run's own 29%/31% exactly,
/// a second confirmation of the same equivalence): `slow_stack` Standard
/// 29.0%/31.0%, Cozy (before its own separate fix below) 100.0%/100.0%, Wild
/// (before its own separate fix below) 0.0%/0.0% -- Standard alone lands in
/// its own band at a shared boss-HP level; Cozy/Wild each need their own
/// additional lever, same structural shape as the pre-looping profile (see
/// `COZY_HP_PERMILLE`/`COZY_INTEGRITY`/`WILD_HP_PERMILLE`'s own docs).
/// `circuit` measured Standard 12.0%/17.0% -- under the band both windows,
/// same "weaker of the two strong policies" pattern as every prior pass; not
/// a second gap to close (this doc's own original methodology note, still
/// the standing rule).
///
/// Death-wave split for `slow_stack`'s own Standard losses lands ENTIRELY on
/// wave 8 (w4=0/w8=142 window 0, w4=0/w8=138 window 5000..5200) -- unlike the
/// pre-looping profile's own split across both boss waves (this doc's own
/// earlier section). At this calibration Bellkeeper (wave 4) is comfortably
/// inert for both strong policies: a direct probe at `--bellkeeper-hp-mult=
/// 3000` (an ADDITIONAL x3 stacked on the already-baked 3189, i.e. an
/// effective ~9567) was needed before wave-4 losses appeared at all (100%
/// w4 losses for both `circuit`/`slow_stack`, 100 seeds) -- all of THIS
/// profile's difficulty concentrates on Night Maw. Waves 5-7 still produce
/// zero losses for both strong policies on every difficulty, both windows --
/// the task's own suspected-still-true finding confirmed, not an artifact of
/// the pre-fix walking-corpse defect either time.
///
/// Night Maw's own isolated local sensitivity (`--night-maw-hp-mult` alone,
/// `--bellkeeper-hp-mult` left neutral, Standard, `slow_stack`, 100 seeds) is
/// dramatically steeper post-looping than the pre-looping profile's own
/// documented +-5%-swings-53-points reading: 78%/58%/40%/24%/12%/2% at
/// 985/990/995/1000/1005/1010 permille -- roughly 3 points of win rate per
/// SINGLE permille near the baked value, not per 5 permille. Smooth and
/// monotonic throughout (a real gradient, not a discrete step -- see
/// [`BOSS_LAP_INTEGRITY_DAMAGE`]'s own doc for a lever that IS a discrete
/// step here), but steep enough that the 200-seed, two-window confirmation
/// above is load-bearing: a single-window read at this constant's
/// neighbourhood is not trustworthy on its own. Night Maw remains, more than
/// ever, the brittle constant of this profile.
pub const BELLKEEPER_BASE_HP: i32 = 3189;
/// HP-percent thresholds (of max HP) that trigger a Skitter escort, checked
/// on first downward crossing, most-recently-crossed first.
pub const BELLKEEPER_ESCORT_THRESHOLDS_PERMILLE: [i64; 3] = [750, 500, 250];

/// x0.417 of the plan's own seed value (4200) -- more than double the
/// invalid pre-fix calibration's 861 (x0.205). See `BELLKEEPER_BASE_HP`'s
/// own doc for the shared methodology, target policy and measured win
/// rates (both bosses were tuned together against the same runs) at that
/// era's value (1751); see that same doc's own "x1.850, re-scaled for
/// free tower placement" section for why and how this became `round_div
/// (1751 * 1850, 1000) = 3239`, the SAME x1.850 ratio applied to both
/// bosses together.
///
/// # x1.020, re-calibrated for combat-phase building and swarm waves
///
/// See `BELLKEEPER_BASE_HP`'s own "x1.020" section: the SAME
/// `--night-maw-hp-mult`-found ratio, `round_div(3239*1020,1000)=3304`,
/// applied here together with that constant against the exact same runs.
///
/// # x2.980, re-calibrated for `BUILD_RADIUS_FP` removal, Heartseed looping
/// and mandatory full clear
///
/// See `BELLKEEPER_BASE_HP`'s own identically-named section: the SAME joint
/// `--night-maw-hp-mult`-found ratio, `round_div(3304*2980,1000)=9848`,
/// applied here together with that constant against the exact same runs.
/// This is now, by a wide margin, the profile's own brittle constant -- see
/// that section's own isolated-sensitivity paragraph (roughly 3 win-rate
/// points per SINGLE permille near this value) for the measurement.
pub const NIGHT_MAW_BASE_HP: i32 = 9848;
pub const NIGHT_MAW_SPLIT_THRESHOLD_PERMILLE: i64 = 500;
pub const NIGHT_MAW_FINAL_PHASE_THRESHOLD_PERMILLE: i64 = 250;
pub const NIGHT_MAW_CORRUPT_INTERVAL_TICKS: u64 = 10 * TICKS_PER_SECOND as u64;
pub const NIGHT_MAW_CORRUPT_DURATION_TICKS: u64 = 5 * TICKS_PER_SECOND as u64;
/// Hushers guaranteed as part of the wave-8 final escort, independent of the
/// random budget fill.
pub const NIGHT_MAW_ESCORT_HUSHER_COUNT: u32 = 2;

// ---------------------------------------------------------------------------
// Rune drafts
// ---------------------------------------------------------------------------

/// Waves after which a rune draft is offered.
pub const RUNE_DRAFT_AFTER_WAVES: [u32; 2] = [2, 6];
/// Number of options shown per draft.
pub const RUNE_DRAFT_OPTIONS: usize = 3;
/// Echo rune: every this-th attack from a tower is repeated by another
/// tower of the same type.
pub const ECHO_EVERY_NTH_ATTACK: u32 = 4;
/// Phase rune: every this-th hit ignores armour entirely.
pub const PHASE_EVERY_NTH_HIT: u32 = 3;
/// Symbiosis rune: adjacent (in tiles) towers of different types strengthen
/// each other's damage, in permille bonus per qualifying neighbour.
pub const SYMBIOSIS_DAMAGE_BONUS_PERMILLE: i64 = 100;
pub const SYMBIOSIS_ADJACENCY_FP: i64 = 15_000; // 1.5 tiles

/// Evolution choice becomes available once wave 4 (the Bellkeeper wave)
/// clears.
pub const EVOLUTION_AFTER_WAVE: u32 = 4;

// ---------------------------------------------------------------------------
// Pet Charges: the pet's own build, drafted through the Living Circuit
// ---------------------------------------------------------------------------

/// Waves after which a Pet Charge draft is offered -- every wave boundary
/// from 1 to 7 that [`RUNE_DRAFT_AFTER_WAVES`] (2, 6) and
/// [`EVOLUTION_AFTER_WAVE`] (4) leave untouched, so the run offers exactly
/// one decision point after every wave except the last (8, which ends the
/// run instead). The owner's own ask was a pet build the player chooses
/// "more than once per run" -- four picks, evenly spread across the whole
/// run rather than front- or back-loaded, gives every wave a fresh reason to
/// stop and look at the Circuit.
pub const PET_CHARGE_DRAFT_AFTER_WAVES: [u32; 4] = [1, 3, 5, 7];
/// Number of options shown per Pet Charge draft -- the same width as a rune
/// draft ([`RUNE_DRAFT_OPTIONS`]).
pub const PET_CHARGE_DRAFT_OPTIONS: usize = 3;

/// Surge charge: on top of the Living Circuit's own flat
/// [`LINKED_ATTACK_SPEED_PERMILLE`], a linked tower's attack interval is
/// divided by this permille too (1060 = an extra /1.06, roughly +6%
/// faster) -- same divisor style as the base Circuit bonus, so the two
/// compose the same way twice rather than needing a second formula.
///
/// Deliberately modest, not a design guess left unchecked: a direct
/// `hatchery-arcade-sweep` A/B (this constant, [`PET_CHARGE_FANG_DAMAGE_PERMILLE`]
/// and [`PET_CHARGE_BLOOM_RADIUS_PERMILLE`] all zeroed versus all three at
/// this file's own shipped values, 200 seeds, Standard/Wild, `slow_stack`)
/// found the four Pet Charges together move Standard from the standing
/// profile's own 29%/31% toward the low-to-mid 40s and Wild from 4%/8%
/// toward the high single digits -- a real, felt shift, NOT the 60%+/25%+
/// an early, unchecked first-pass magnitude (Surge 1150, Fang 150, Bloom
/// 250) produced when isolated the same way. See this crate's own
/// implementation report for the full before/after table; the size of that
/// remaining shift is a structural property of how thin
/// [`BOSS_LAP_INTEGRITY_DAMAGE`]'s own calibration already runs (its own
/// doc: "a single whole-integer step... flips the run from 'always wins'
/// to 'always loses'"), not something a smaller Pet Charge alone can zero
/// out without making the mechanic pointless -- see that constant's own
/// doc; moving IT is the owner's call, not this pass's.
pub const PET_CHARGE_SURGE_EXTRA_SPEED_PERMILLE: i64 = 1060;
/// Fang charge: extra damage permille bonus for a linked tower's hit,
/// combined additively with Symbiosis's own per-neighbour bonus (both are
/// "extra permille on top of base damage" terms resolved at the same point
/// in `sim.rs`'s own `fire_tower`). See [`PET_CHARGE_SURGE_EXTRA_SPEED_PERMILLE`]'s
/// own doc for the A/B methodology behind this value.
pub const PET_CHARGE_FANG_DAMAGE_PERMILLE: i64 = 60;
/// Bloom charge: for a linked tower's own splash/chain attack, extra reach
/// (radius or Prism's chain search radius) as a permille bonus, plus one
/// extra target slot on top of [`MAX_SPLASH_TARGETS`]/[`MAX_CHAIN_TARGETS`].
/// A tower with no splash/chain at all (Needle, Bell) has nothing for this
/// to widen -- Bloom is deliberately a build-shape choice, not a universal
/// number every tower benefits from equally. See
/// [`PET_CHARGE_SURGE_EXTRA_SPEED_PERMILLE`]'s own doc for the A/B
/// methodology behind this value.
pub const PET_CHARGE_BLOOM_RADIUS_PERMILLE: i64 = 100;

// ---------------------------------------------------------------------------
// Field modifiers: per-wave zones
// ---------------------------------------------------------------------------

/// The board is partitioned into this many columns x rows of equal-sized
/// sectors for the per-wave field-modifier draw (`zone.rs`) -- 4x2 over the
/// 28x14 board gives 7x7-tile sectors: large enough to comfortably hold a
/// small tower cluster or a real stretch of route, small enough that the two
/// zones drawn each wave (one tower-damage, one enemy-speed -- always
/// exactly two, never overlapping) cover a clearly bounded fraction of the
/// board, not most of it.
pub const ZONE_SECTOR_COLS: i32 = 4;
pub const ZONE_SECTOR_ROWS: i32 = 2;
pub const ZONE_SECTOR_COUNT: usize = (ZONE_SECTOR_COLS * ZONE_SECTOR_ROWS) as usize;

/// Tower-damage zone: extra damage permille bonus (Buff) or penalty
/// (Debuff) for an attacking tower whose own tile sits inside that wave's
/// tower-damage sector -- resolved the same way Symbiosis/Fang are, an
/// additive permille term on `fire_tower`'s own `effective_damage`.
///
/// Halved from an initial 200 after the same A/B discipline
/// [`PET_CHARGE_SURGE_EXTRA_SPEED_PERMILLE`]'s own doc describes (this
/// constant and [`ZONE_ENEMY_SPEED_PERMILLE`] zeroed versus both at their
/// shipped value, `slow_stack`, 200 seeds): at 200/150 the two zones ALONE
/// (Pet Charges zeroed) already moved Standard to 45% and Wild to 17%; at
/// 100/75 the same isolated measurement barely moved (45%/9%) -- the
/// zone's own balance weight comes from being a wave-to-wave VARIANCE
/// source at all, landing on some wave's own outcome-deciding moment, far
/// more than from its own permille size. This value stays a genuine,
/// felt +-10% rather than being shrunk toward zero chasing a smaller
/// measured shift that the brittleness above (see
/// [`PET_CHARGE_SURGE_EXTRA_SPEED_PERMILLE`]'s own doc) would not
/// meaningfully grant anyway.
///
/// # 50/37, re-calibrated for the Standard/Cozy convergence this pass's own
/// shipped 100/75 produced
///
/// The finding above ("100/75... barely moved") was measured in ISOLATION,
/// with Pet Charges zeroed out -- not the shipped state, where both
/// mechanics run together. Re-measured against the actual shipped build
/// (Pet Charges at their own shipped constants, not zeroed), halving 100/75
/// to 50/37 does NOT barely move Standard: `slow_stack`, 200 seeds, window
/// 0..200, Standard fell 46.0% -> 35.0%, an 11-point drop, while Cozy fell
/// only 46.0% -> 44.0% (2 points) and Wild moved 9.0% -> 10.0% (flat) over
/// the identical edit -- so the isolated-A/B finding does not hold once
/// Pet Charges are in the mix, and this pair of constants is very much a
/// working lever here, not a saturated one. The asymmetry is the reason it
/// works as a Standard/Cozy separator specifically: Cozy's own win/loss
/// boundary sits on a much harder Sap-affordability step (see
/// `COZY_REWARD_PERMILLE`'s own doc) than Standard's, so a shared-magnitude
/// environmental swing crosses many more Standard runs' own boundary than
/// Cozy's for the same edit.
///
/// This is what actually fixed the two-mechanics-pass regression
/// (Standard/Cozy converging at 46%/46%, `slow_stack`, window 0..200):
/// halving both zone constants was the single largest lever in restoring
/// the gradient, ahead of `COZY_HP_PERMILLE`'s own re-tune (see that
/// constant's own "924" section, needed only to lift Cozy back up after
/// this edit ALSO pulled it down slightly). Confirmed at 200 seeds each on
/// two independent windows (0..200 and 5000..5200) with every other
/// constant at this file's own final shipped values, `slow_stack`: Standard
/// 35.0%/40.0% -- inside the 25-40% band on both (the second window landing
/// exactly on the ceiling, not past it, the same pattern this file's own
/// history already treats as valid). `circuit` measured 41.0%/40.0% --
/// under the band on the first window, at the ceiling on the second, the
/// standing "weaker of the two strong policies" pattern.
pub const ZONE_TOWER_DAMAGE_PERMILLE: i64 = 50;
/// Enemy-speed zone: extra movement-speed permille bonus (Debuff, faster)
/// or penalty (Buff, slower) for an enemy/boss body currently standing
/// inside that wave's enemy-speed sector. Deliberately smaller than a
/// single Bell's own 35% slow, and applied as a SEPARATE multiplier outside
/// [`MAX_COMBINED_SLOW_PERMILLE`]'s own ceiling -- this is a positional
/// environmental rate, not another slow SOURCE feeding the tower-slow
/// stacking formula (`status.rs`'s own `SlowState::combined_permille`), so
/// it does not count against that formula's documented 60% cap. See
/// [`ZONE_TOWER_DAMAGE_PERMILLE`]'s own doc for the A/B methodology behind
/// this value, including its own "50/37" section: this constant was halved
/// from 75 to 37 in the exact same edit, alongside
/// [`ZONE_TOWER_DAMAGE_PERMILLE`]'s own 100 -> 50 halving, not moved
/// independently or re-searched on its own.
pub const ZONE_ENEMY_SPEED_PERMILLE: i64 = 37;
