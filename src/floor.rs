//! Degenerate-candidate floor: what agreement would a candidate that ignores the prompt reach on a
//! skewed fixture? Model-free; seeded in-process generator.

use crate::{
    agreement_bootstrap_ci, agreement_rate, grade_all, safe_to_upgrade, NormalizedMatchGrader,
    Outputs,
};
use rand::{Rng, SeedableRng};
use rand_pcg::Pcg64Mcg;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct CandidateScore {
    pub agreement_rate: f64,
    pub ci_lo: f64,
    pub ci_hi: f64,
    pub safe_to_upgrade: bool,
}

#[derive(Debug, Serialize)]
pub struct FloorReport {
    pub n: usize,
    pub majority: f64,
    pub labels: usize,
    pub floor: f64,
    pub old_majority_share: f64,
    pub constant_candidate: CandidateScore,
    pub uniform_candidate: CandidateScore,
    pub copy_candidate: CandidateScore,
}

fn label(i: usize) -> String {
    format!("label-{i}")
}

fn score(old: &[String], new: &[String], floor: f64, boot: usize, seed: u64) -> CandidateScore {
    let outs: Vec<Outputs> = old
        .iter()
        .zip(new)
        .enumerate()
        .map(|(i, (a, b))| Outputs { prompt_id: format!("p{i}"), a: a.clone(), b: b.clone() })
        .collect();
    let pairs = grade_all(&NormalizedMatchGrader, &outs);
    let ci = agreement_bootstrap_ci(&pairs, boot, seed);
    CandidateScore {
        agreement_rate: agreement_rate(&pairs),
        ci_lo: ci.0,
        ci_hi: ci.1,
        safe_to_upgrade: safe_to_upgrade(ci, floor),
    }
}

/// Build the skewed fixture and score the three candidates. Label 0 is the majority.
pub fn run(n: usize, majority: f64, labels: usize, floor: f64, boot: usize, seed: u64) -> FloorReport {
    assert!(labels >= 2, "need at least two labels");
    let mut rng = Pcg64Mcg::seed_from_u64(seed);
    let old: Vec<String> = (0..n)
        .map(|_| {
            if rng.gen::<f64>() < majority {
                label(0)
            } else {
                label(rng.gen_range(1..labels))
            }
        })
        .collect();
    let constant = vec![label(0); n];
    let uniform: Vec<String> = (0..n).map(|_| label(rng.gen_range(0..labels))).collect();
    let share = old.iter().filter(|l| **l == label(0)).count() as f64 / n.max(1) as f64;
    FloorReport {
        n,
        majority,
        labels,
        floor,
        old_majority_share: share,
        constant_candidate: score(&old, &constant, floor, boot, seed),
        uniform_candidate: score(&old, &uniform, floor, boot, seed),
        copy_candidate: score(&old, &old, floor, boot, seed),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_matches_majority_share_and_copy_is_perfect() {
        let r = run(500, 0.95, 5, 0.9, 200, 7);
        assert_eq!(r.constant_candidate.agreement_rate, r.old_majority_share);
        assert_eq!(r.copy_candidate.agreement_rate, 1.0);
        assert!(r.uniform_candidate.agreement_rate < 0.5);
    }

    #[test]
    fn deterministic_for_seed() {
        let a = serde_json::to_string(&run(200, 0.9, 4, 0.9, 100, 3)).unwrap();
        let b = serde_json::to_string(&run(200, 0.9, 4, 0.9, 100, 3)).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn balanced_fixture_does_not_pass_constant() {
        let r = run(500, 0.2, 5, 0.9, 200, 7);
        assert!(!r.constant_candidate.safe_to_upgrade);
    }
}
