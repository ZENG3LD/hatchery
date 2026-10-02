//! Manual CLI argument parsing -- `std::env::args` only, no external
//! argument-parsing crate (per the task's own no-new-dependency rule).

use gate4agent_arcade_pet_bastion::wave::{BalanceOverrides, Difficulty};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DifficultyFilter {
    All,
    One(Difficulty),
}

impl DifficultyFilter {
    pub fn selected(self) -> Vec<Difficulty> {
        match self {
            DifficultyFilter::All => vec![Difficulty::Cozy, Difficulty::Standard, Difficulty::Wild],
            DifficultyFilter::One(d) => vec![d],
        }
    }

    fn parse(token: &str) -> Result<Self, String> {
        match token.to_ascii_lowercase().as_str() {
            "all" => Ok(DifficultyFilter::All),
            "cozy" => Ok(DifficultyFilter::One(Difficulty::Cozy)),
            "standard" => Ok(DifficultyFilter::One(Difficulty::Standard)),
            "wild" => Ok(DifficultyFilter::One(Difficulty::Wild)),
            other => Err(format!("unknown --difficulty value '{other}' (expected cozy|standard|wild|all)")),
        }
    }
}

pub struct Cli {
    pub seeds: u64,
    pub start_seed: u64,
    pub max_ticks: u64,
    pub difficulty: DifficultyFilter,
    /// Boss-HP/reward calibration overrides, layered on top of the
    /// difficulty preset -- see `wave::BalanceOverrides`'s own doc. This is
    /// the grid-search knob the balance calibration task asked for: run the
    /// same seeds/policies/difficulties through several multiplier
    /// combinations without recompiling `constants.rs`.
    pub balance_overrides: BalanceOverrides,
    pub help: bool,
}

impl Default for Cli {
    fn default() -> Self {
        Self {
            // 50 seeds is enough to see a real win-rate signal per
            // (difficulty, policy) combo without a multi-minute CLI run --
            // raise via --seeds for a higher-confidence sweep.
            seeds: 50,
            start_seed: 0,
            // One standard run is designed to last 6-8 minutes at 20Hz
            // (constants::TICK_MS), i.e. roughly 7,200-9,600 ticks. 30,000
            // ticks (25 simulated minutes) is a generous ceiling: any run
            // that has not resolved by then is reported as `Unresolved`
            // rather than silently truncated into a fake `Lost`.
            max_ticks: 30_000,
            difficulty: DifficultyFilter::All,
            balance_overrides: BalanceOverrides::default(),
            help: false,
        }
    }
}

impl Cli {
    pub fn usage() -> &'static str {
        "USAGE: gate4agent-arcade-sweep [OPTIONS]\n\n\
         OPTIONS:\n  \
         --seeds=<N>                 number of seeds per (difficulty, policy) combo [default: 50]\n  \
         --start-seed=<N>            first seed in the sweep range [default: 0]\n  \
         --max-ticks=<N>             per-run simulation tick ceiling [default: 30000]\n  \
         --difficulty=<VALUE>        cozy | standard | wild | all [default: all]\n  \
         --bellkeeper-hp-mult=<PM>   extra permille multiplier on Bellkeeper's (wave 4) effective HP, on top of the difficulty preset [default: 1000]\n  \
         --night-maw-hp-mult=<PM>    extra permille multiplier on Night Maw's (wave 8) effective HP, on top of the difficulty preset [default: 1000]\n  \
         --reward-mult=<PM>          extra permille multiplier on every wave's clear reward, on top of the difficulty preset [default: 1000]\n  \
         --late-minion-hp-mult=<PM>  extra permille multiplier on minion HP for waves 5-8 only [default: 1000]\n  \
         --integrity-mult=<PM>       extra permille multiplier on starting Integrity [default: 1000]\n  \
         --help                      print this message"
    }

    pub fn parse<I: Iterator<Item = String>>(args: I) -> Result<Self, String> {
        let mut cli = Cli::default();
        let mut args = args.peekable();
        while let Some(arg) = args.next() {
            let (flag, inline_value) = match arg.split_once('=') {
                Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
                None => (arg.clone(), None),
            };
            match flag.as_str() {
                "--help" | "-h" => cli.help = true,
                "--seeds" => cli.seeds = parse_u64(&flag, next_value(inline_value, &mut args, &flag)?)?,
                "--start-seed" => cli.start_seed = parse_u64(&flag, next_value(inline_value, &mut args, &flag)?)?,
                "--max-ticks" => cli.max_ticks = parse_u64(&flag, next_value(inline_value, &mut args, &flag)?)?,
                "--difficulty" => {
                    cli.difficulty = DifficultyFilter::parse(&next_value(inline_value, &mut args, &flag)?)?
                }
                "--bellkeeper-hp-mult" => {
                    cli.balance_overrides.bellkeeper_hp_permille =
                        parse_permille(&flag, next_value(inline_value, &mut args, &flag)?)?
                }
                "--night-maw-hp-mult" => {
                    cli.balance_overrides.night_maw_hp_permille =
                        parse_permille(&flag, next_value(inline_value, &mut args, &flag)?)?
                }
                "--reward-mult" => {
                    cli.balance_overrides.reward_permille =
                        parse_permille(&flag, next_value(inline_value, &mut args, &flag)?)?
                }
                "--late-minion-hp-mult" => {
                    cli.balance_overrides.late_minion_hp_permille =
                        parse_permille(&flag, next_value(inline_value, &mut args, &flag)?)?
                }
                "--integrity-mult" => {
                    cli.balance_overrides.integrity_permille =
                        parse_permille(&flag, next_value(inline_value, &mut args, &flag)?)?
                }
                other => return Err(format!("unknown flag '{other}'")),
            }
        }
        if cli.seeds == 0 {
            return Err("--seeds must be at least 1".to_string());
        }
        if cli.max_ticks == 0 {
            return Err("--max-ticks must be at least 1".to_string());
        }
        Ok(cli)
    }
}

/// Resolves one flag's value: an inline `--flag=value` takes precedence,
/// otherwise the NEXT token in the stream is consumed as the value (a
/// separate `--flag value` form).
fn next_value<I: Iterator<Item = String>>(
    inline: Option<String>,
    args: &mut std::iter::Peekable<I>,
    flag: &str,
) -> Result<String, String> {
    if let Some(value) = inline {
        return Ok(value);
    }
    args.next().ok_or_else(|| format!("flag '{flag}' requires a value"))
}

fn parse_u64(flag: &str, raw: String) -> Result<u64, String> {
    raw.parse::<u64>().map_err(|_| format!("flag '{flag}' expects an integer, got '{raw}'"))
}

/// Parses a strictly positive permille multiplier -- `0` or negative would
/// zero out or invert a boss's HP/a wave's reward, which is never a useful
/// calibration point (the game has no "boss has no HP" mechanic to test).
fn parse_permille(flag: &str, raw: String) -> Result<i64, String> {
    let value = raw.parse::<i64>().map_err(|_| format!("flag '{flag}' expects an integer, got '{raw}'"))?;
    if value <= 0 {
        return Err(format!("flag '{flag}' must be a positive permille value, got '{value}'"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_apply_when_no_flags_are_given() {
        let cli = Cli::parse(std::iter::empty()).unwrap();
        assert_eq!(cli.seeds, 50);
        assert_eq!(cli.start_seed, 0);
        assert_eq!(cli.max_ticks, 30_000);
        assert_eq!(cli.difficulty, DifficultyFilter::All);
        assert_eq!(cli.balance_overrides, BalanceOverrides::default());
    }

    #[test]
    fn balance_override_flags_parse() {
        let args = [
            "--bellkeeper-hp-mult=700",
            "--night-maw-hp-mult=850",
            "--reward-mult=1200",
            "--late-minion-hp-mult=1800",
            "--integrity-mult=600",
        ]
        .into_iter()
        .map(String::from);
        let cli = Cli::parse(args).unwrap();
        assert_eq!(cli.balance_overrides.bellkeeper_hp_permille, 700);
        assert_eq!(cli.balance_overrides.night_maw_hp_permille, 850);
        assert_eq!(cli.balance_overrides.reward_permille, 1200);
        assert_eq!(cli.balance_overrides.late_minion_hp_permille, 1800);
        assert_eq!(cli.balance_overrides.integrity_permille, 600);
    }

    #[test]
    fn zero_or_negative_permille_override_is_rejected() {
        let zero = ["--bellkeeper-hp-mult=0"].into_iter().map(String::from);
        assert!(Cli::parse(zero).is_err());
        let negative = ["--reward-mult=-5"].into_iter().map(String::from);
        assert!(Cli::parse(negative).is_err());
    }

    #[test]
    fn inline_equals_form_parses() {
        let args = ["--seeds=10", "--start-seed=5", "--max-ticks=100", "--difficulty=wild"]
            .into_iter()
            .map(String::from);
        let cli = Cli::parse(args).unwrap();
        assert_eq!(cli.seeds, 10);
        assert_eq!(cli.start_seed, 5);
        assert_eq!(cli.max_ticks, 100);
        assert_eq!(cli.difficulty, DifficultyFilter::One(Difficulty::Wild));
    }

    #[test]
    fn separate_value_form_parses() {
        let args = ["--seeds", "7", "--difficulty", "cozy"].into_iter().map(String::from);
        let cli = Cli::parse(args).unwrap();
        assert_eq!(cli.seeds, 7);
        assert_eq!(cli.difficulty, DifficultyFilter::One(Difficulty::Cozy));
    }

    #[test]
    fn unknown_flag_is_rejected() {
        let args = ["--bogus"].into_iter().map(String::from);
        assert!(Cli::parse(args).is_err());
    }

    #[test]
    fn zero_seeds_is_rejected() {
        let args = ["--seeds=0"].into_iter().map(String::from);
        assert!(Cli::parse(args).is_err());
    }
}
