//! Core primitives for shadow-mode behavioral diff.
//!
//! Given two systems (A, B) each run against the same fixture of N
//! prompts, pair their outputs and report the agreement rate with 95%
//! bootstrap CI. The pre-registered "safe to upgrade" gate is on the
//! lower bound of the agreement CI.

use rand::SeedableRng;
use rand_pcg::Pcg64Mcg;
use serde::{Deserialize, Serialize};

/// One paired observation from the shadow run.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pair {
    pub prompt_id: String,
    /// True iff a deterministic scripted grader considers A's and B's
    /// outputs equivalent on this prompt.
    pub agree: bool,
}

/// Agreement rate = fraction of pairs where `agree == true`.
pub fn agreement_rate(pairs: &[Pair]) -> f64 {
    if pairs.is_empty() {
        return 0.0;
    }
    let n = pairs.iter().filter(|p| p.agree).count();
    n as f64 / pairs.len() as f64
}

/// Bootstrap 95% CI for the agreement rate.
pub fn agreement_bootstrap_ci(pairs: &[Pair], iters: usize, seed: u64) -> (f64, f64) {
    assert!(!pairs.is_empty());
    let mut rng = Pcg64Mcg::seed_from_u64(seed);
    let mut rates = Vec::with_capacity(iters);
    let n = pairs.len();
    let mut sample = Vec::with_capacity(n);
    for _ in 0..iters {
        sample.clear();
        for _ in 0..n {
            let k = (rand::Rng::gen::<u32>(&mut rng) as usize) % n;
            sample.push(pairs[k].clone());
        }
        rates.push(agreement_rate(&sample));
    }
    rates.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let lo = rates[(iters as f64 * 0.025).floor() as usize];
    let hi = rates[((iters as f64 * 0.975).ceil() as usize).min(iters - 1)];
    (lo, hi)
}

/// Pre-registered: an upgrade is **safe** on this fixture when the lower
/// bound of the agreement-rate 95% CI exceeds `floor`.
pub fn safe_to_upgrade(ci: (f64, f64), floor: f64) -> bool {
    ci.0 > floor
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(id: &str, a: bool) -> Pair {
        Pair {
            prompt_id: id.into(),
            agree: a,
        }
    }

    #[test]
    fn agreement_rate_is_fraction_of_true() {
        let pairs = vec![p("0", true), p("1", true), p("2", false), p("3", true)];
        assert!((agreement_rate(&pairs) - 0.75).abs() < 1e-12);
    }

    #[test]
    fn bootstrap_ci_tight_for_perfect_agreement() {
        let pairs: Vec<Pair> = (0..100).map(|i| p(&i.to_string(), true)).collect();
        let (lo, hi) = agreement_bootstrap_ci(&pairs, 500, 42);
        assert_eq!(lo, 1.0);
        assert_eq!(hi, 1.0);
    }

    #[test]
    fn safe_to_upgrade_requires_lower_bound_above_floor() {
        assert!(safe_to_upgrade((0.92, 0.98), 0.9));
        assert!(!safe_to_upgrade((0.88, 0.98), 0.9));
    }
}
