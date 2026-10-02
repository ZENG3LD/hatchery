//! Percentile summary for a batch of timing samples. The task's own
//! framing is explicit: a single mean hides the once-a-second stall that
//! actually breaks a game's feel, so every measurement in this crate
//! reports p50/p95/max, never a bare average.

#[derive(Clone, Copy, Debug)]
pub struct Percentiles {
    pub p50: f64,
    pub p95: f64,
    pub max: f64,
    pub mean: f64,
    pub n: usize,
}

/// `None` for an empty batch (a real caller skips reporting rather than
/// fabricating a zeroed row).
pub fn percentiles(samples: &[f64]) -> Option<Percentiles> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted: Vec<f64> = samples.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let at = |p: f64| -> f64 {
        let idx = ((n - 1) as f64 * p).round() as usize;
        sorted[idx.min(n - 1)]
    };
    let mean = sorted.iter().sum::<f64>() / n as f64;
    Some(Percentiles { p50: at(0.50), p95: at(0.95), max: sorted[n - 1], mean, n })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_is_none() {
        assert!(percentiles(&[]).is_none());
    }

    #[test]
    fn single_sample() {
        let p = percentiles(&[5.0]).expect("one sample must produce a Percentiles");
        assert_eq!(p.p50, 5.0);
        assert_eq!(p.p95, 5.0);
        assert_eq!(p.max, 5.0);
    }

    #[test]
    fn known_distribution() {
        let samples: Vec<f64> = (1..=100).map(|v| v as f64).collect();
        let p = percentiles(&samples).expect("100 samples must produce a Percentiles");
        assert_eq!(p.max, 100.0);
        assert!((p.p50 - 51.0).abs() < 1.0);
        assert!((p.p95 - 95.0).abs() < 1.0);
    }
}
