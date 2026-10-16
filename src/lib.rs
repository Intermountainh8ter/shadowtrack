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

/// Per-endpoint self-agreement baseline (pre-reg §5): the agreement
/// rate of an endpoint against itself on a sample of the fixture. A
/// non-deterministic endpoint will have self-agreement below 1.0, and
/// that ceiling places an upper bound on the pair agreement that is
/// achievable before you blame the upgrade.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelfBaseline {
    pub label: String,
    pub rate: f64,
    pub ci_lo: f64,
    pub ci_hi: f64,
    pub n: usize,
}

/// Compute a self-agreement baseline from a vector of A-vs-A (or B-vs-B)
/// paired re-runs. Returns `None` when no rows are supplied, so the
/// absence of a baseline is explicit at the type level (NaN would
/// round-trip badly through JSON).
pub fn self_baseline(
    label: &str,
    pairs: &[Pair],
    iters: usize,
    seed: u64,
) -> Option<SelfBaseline> {
    if pairs.is_empty() {
        return None;
    }
    let rate = agreement_rate(pairs);
    let (ci_lo, ci_hi) = agreement_bootstrap_ci(pairs, iters, seed);
    Some(SelfBaseline {
        label: label.into(),
        rate,
        ci_lo,
        ci_hi,
        n: pairs.len(),
    })
}

/// Full shadow-mode audit report. Carries the pair-agreement verdict
/// alongside the two self-agreement baselines so a reviewer can tell a
/// bad upgrade from a non-deterministic endpoint.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct ShadowReport {
    pub n_pairs: usize,
    pub pair_rate: f64,
    pub pair_ci_lo: f64,
    pub pair_ci_hi: f64,
    pub floor: f64,
    pub safe_to_upgrade: bool,
    pub a_self: Option<SelfBaseline>,
    pub b_self: Option<SelfBaseline>,
    /// Pre-reg §5: a self-agreement whose CI upper bound is below the
    /// pair-agreement floor is a confound; the "unsafe" verdict in that
    /// case cannot be blamed on the upgrade.
    pub self_agreement_confound: bool,
    /// Set only on a confound; names which side's self-agreement is
    /// under the floor.
    pub confound_label: Option<String>,
}

/// Run the full shadow audit: pair agreement + both self-agreement
/// baselines + the pre-reg §5 confound check.
pub fn audit(
    pairs: &[Pair],
    a_self: &[Pair],
    b_self: &[Pair],
    floor: f64,
    iters: usize,
    seed: u64,
) -> ShadowReport {
    assert!(!pairs.is_empty(), "pairs must not be empty");
    let pair_rate = agreement_rate(pairs);
    let (pair_ci_lo, pair_ci_hi) = agreement_bootstrap_ci(pairs, iters, seed);
    let safe = safe_to_upgrade((pair_ci_lo, pair_ci_hi), floor);
    // Deterministic sub-seeds so the two baselines don't share RNG state
    // with the pair bootstrap or with each other.
    let a = self_baseline("A", a_self, iters, seed.wrapping_add(1));
    let b = self_baseline("B", b_self, iters, seed.wrapping_add(2));
    // Confound: a self-baseline CI's UPPER bound below the pair floor
    // means that endpoint is non-deterministic enough that the pair
    // comparison can't fail an upgrade safely. An absent baseline is
    // treated as absent, not as a confound.
    let a_confound = a.as_ref().is_some_and(|s| s.ci_hi < floor);
    let b_confound = b.as_ref().is_some_and(|s| s.ci_hi < floor);
    let confound_label = match (a_confound, b_confound) {
        (true, true) => Some("A,B".into()),
        (true, false) => Some("A".into()),
        (false, true) => Some("B".into()),
        (false, false) => None,
    };
    ShadowReport {
        n_pairs: pairs.len(),
        pair_rate,
        pair_ci_lo,
        pair_ci_hi,
        floor,
        safe_to_upgrade: safe,
        a_self: a,
        b_self: b,
        self_agreement_confound: confound_label.is_some(),
        confound_label,
    }
}

/// Deterministic scripted grader (pre-reg §2 "no LLM judges"): given
/// the two systems' outputs on one prompt, returns whether they agree.
/// Implementors are repo-committed functions; the fixture author
/// documents the specific "agreement" definition in
/// `fixtures/<name>_grader.md`.
pub trait Grader {
    fn agree(&self, prompt_id: &str, a: &str, b: &str) -> bool;
}

/// Built-in grader: byte-identical strings agree.
pub struct ExactMatchGrader;

impl Grader for ExactMatchGrader {
    fn agree(&self, _prompt_id: &str, a: &str, b: &str) -> bool {
        a == b
    }
}

/// Built-in grader: strings agree after normalizing surrounding
/// whitespace and ASCII casing. The fixture author documents in
/// `fixtures/<name>_grader.md` why this normalization is appropriate.
pub struct NormalizedMatchGrader;

impl Grader for NormalizedMatchGrader {
    fn agree(&self, _prompt_id: &str, a: &str, b: &str) -> bool {
        normalize(a) == normalize(b)
    }
}

fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            for lc in c.to_lowercase() {
                out.push(lc);
            }
            prev_space = false;
        }
    }
    out.trim_end().to_string()
}

/// Paired output for one prompt — the inputs a `Grader` consumes.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Outputs {
    pub prompt_id: String,
    pub a: String,
    pub b: String,
}

/// Apply a scripted grader to a batch of paired outputs, producing the
/// `Pair` sequence the audit expects.
pub fn grade_all<G: Grader>(grader: &G, outputs: &[Outputs]) -> Vec<Pair> {
    outputs
        .iter()
        .map(|o| Pair {
            prompt_id: o.prompt_id.clone(),
            agree: grader.agree(&o.prompt_id, &o.a, &o.b),
        })
        .collect()
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

    #[test]
    fn self_baseline_is_none_when_no_rows_supplied() {
        assert_eq!(self_baseline("A", &[], 100, 1), None);
    }

    #[test]
    fn self_baseline_is_some_when_rows_supplied() {
        let rows: Vec<Pair> = (0..40).map(|i| p(&i.to_string(), true)).collect();
        let b = self_baseline("A", &rows, 500, 42).expect("baseline present");
        assert_eq!(b.label, "A");
        assert_eq!(b.n, 40);
        assert_eq!(b.rate, 1.0);
    }

    #[test]
    fn audit_reports_safe_when_pair_and_self_baselines_all_high() {
        let pairs: Vec<Pair> = (0..100).map(|i| p(&i.to_string(), true)).collect();
        let a: Vec<Pair> = (0..40).map(|i| p(&i.to_string(), true)).collect();
        let b: Vec<Pair> = (0..40).map(|i| p(&i.to_string(), true)).collect();
        let r = audit(&pairs, &a, &b, 0.9, 500, 1234);
        assert!(r.safe_to_upgrade);
        assert!(!r.self_agreement_confound);
        assert_eq!(r.confound_label, None);
    }

    #[test]
    fn audit_flags_confound_when_self_agreement_below_floor() {
        // A is wildly non-deterministic: agrees with itself only 30% of
        // the time. Pair agreement at 50% looks "bad" but the upgrade
        // isn't to blame.
        let pairs: Vec<Pair> = (0..100)
            .map(|i| p(&i.to_string(), i % 2 == 0))
            .collect();
        let a: Vec<Pair> = (0..40)
            .map(|i| p(&i.to_string(), i < 12))
            .collect();
        let b: Vec<Pair> = (0..40).map(|i| p(&i.to_string(), true)).collect();
        let r = audit(&pairs, &a, &b, 0.9, 500, 1234);
        assert!(!r.safe_to_upgrade);
        assert!(r.self_agreement_confound, "report: {r:?}");
        assert_eq!(r.confound_label.as_deref(), Some("A"));
    }

    #[test]
    fn audit_reports_round_trip_through_json() {
        let pairs: Vec<Pair> = (0..40).map(|i| p(&i.to_string(), i % 3 != 0)).collect();
        let r = audit(&pairs, &[], &[], 0.9, 200, 7);
        let s = serde_json::to_string(&r).unwrap();
        let r2: ShadowReport = serde_json::from_str(&s).unwrap();
        assert_eq!(r, r2);
    }

    #[test]
    fn exact_match_grader_byte_identity_only() {
        let g = ExactMatchGrader;
        assert!(g.agree("x", "hello", "hello"));
        assert!(!g.agree("x", "hello", "Hello"));
        assert!(!g.agree("x", "hello ", "hello"));
    }

    #[test]
    fn normalized_match_grader_handles_whitespace_and_case() {
        let g = NormalizedMatchGrader;
        assert!(g.agree("x", "Hello  World", "hello world"));
        assert!(g.agree("x", "  Hi\tthere  ", "hi there"));
        assert!(!g.agree("x", "hello world", "hello there"));
    }

    #[test]
    fn grade_all_builds_pair_sequence_from_outputs() {
        let outputs = vec![
            Outputs { prompt_id: "p0".into(), a: "x".into(), b: "x".into() },
            Outputs { prompt_id: "p1".into(), a: "x".into(), b: "y".into() },
            Outputs { prompt_id: "p2".into(), a: "x".into(), b: "x".into() },
        ];
        let pairs = grade_all(&ExactMatchGrader, &outputs);
        assert_eq!(pairs.len(), 3);
        assert!(pairs[0].agree);
        assert!(!pairs[1].agree);
        assert!(pairs[2].agree);
    }

    #[test]
    fn custom_grader_implementable_by_user_code() {
        struct JsonValueGrader;
        impl Grader for JsonValueGrader {
            fn agree(&self, _p: &str, a: &str, b: &str) -> bool {
                serde_json::from_str::<serde_json::Value>(a).ok()
                    == serde_json::from_str::<serde_json::Value>(b).ok()
            }
        }
        let g = JsonValueGrader;
        assert!(g.agree("x", "{\"a\":1}", " { \"a\" : 1 } "));
        assert!(!g.agree("x", "{\"a\":1}", "{\"a\":2}"));
    }
}
