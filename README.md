# shadowtrack

**Shadow-mode behavioral diff for model, prompt-template, and
agent-framework upgrades.** Run new and old side-by-side on a fixture
set; pair outputs with a deterministic scripted grader; report the
agreement rate with 95% bootstrap CI and the pre-registered
safe-to-upgrade verdict (lower CI bound > 0.9 by default).

![status: initial scaffold](https://img.shields.io/badge/status-initial%20scaffold-lightgrey)
![language: Rust](https://img.shields.io/badge/language-Rust-blue)
![license: MIT](https://img.shields.io/badge/license-MIT-blue)

## Why

Everyone shadow-tests in production without a shared harness. The
pattern is well-known; the OSS tool is missing. Pairs with
`agentreplay` because the trajectories are replayable, and with
`tokenwitness` because the first thing to check on an upgrade is
whether the chat-template bytes shifted.

## What is in this scaffold

- `Pair`, `agreement_rate`, `agreement_bootstrap_ci`,
  `safe_to_upgrade`.
- `SelfBaseline` + `self_baseline(label, pairs, iters, seed)` →
  `Option<SelfBaseline>`: pre-reg §5's per-endpoint self-agreement
  baseline, from a Vec of A-vs-A (or B-vs-B) paired re-runs. Absent
  baseline is `None` at the type level — no NaN in JSON.
- `ShadowReport` + `audit(pairs, a_self, b_self, floor, iters, seed)`:
  full audit with pair agreement, both baselines, and the pre-reg §5
  confound flag (a self-agreement CI upper bound below the pair floor
  means that endpoint is non-deterministic enough that the "unsafe"
  verdict cannot be blamed on the upgrade).
- CLI: `shadowtrack score` (back-compat), `shadowtrack audit`. Exit
  codes: 0 safe, 1 unsafe, 2 self-agreement confound.
- `docs/PREREGISTRATION.md` locking the gate.

Not in this commit (openly tracked):

- `shadowtrack run --a <endpoint> --b <endpoint> --fixture <path>`
  driver.
- Scripted grader interface (plug a Rust closure in; no LLM judges).
- Fixture adapters for the common agent task suites.

## License

MIT. See `LICENSE`.
