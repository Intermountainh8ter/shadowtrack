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

## Serving-determinism study, live

Pre-registered in `docs/STUDY-serving-determinism.md`; configurations in `examples/determinism/configs.json`;
run by `shadowtrack determinism`, evaluated by `shadowtrack determinism-analyze`. Benign outputs are
committed in full under `results/determinism/`; harmful completions never are (only agreement flags and
counts). The table below is rendered from `results/determinism/summary.json`.

<!-- DETERMINISM:START -->
| Model | Config | Slots | Flash attn | KV cache | ubatch | Benign self-agreement (run 1 vs 2) | JBB self-agreement | JBB non-refusal run 1 | run 2 | First divergence (chars, median) |
|---|---|---|---|---|---|---|---|---|---|---|
| llama-3.1-8b-instruct | C1 | 1 | off | f16 | 512 | 0.97 [0.94, 1.00] | 0.98 [0.95, 1.00] | 0.09 [0.04, 0.15] | 0.09 [0.04, 0.15] | 134 |
| llama-3.1-8b-instruct | C2 | 8 | off | f16 | 512 | 0.92 [0.86, 0.97] | 0.81 [0.73, 0.89] | 0.09 [0.04, 0.15] | 0.09 [0.04, 0.15] | 67 |
| llama-3.1-8b-instruct | C3 | 1 | on | f16 | 512 | 0.98 [0.95, 1.00] | 0.98 [0.95, 1.00] | 0.09 [0.04, 0.15] | 0.09 [0.04, 0.15] | 134 |
| llama-3.1-8b-instruct | C4 | 8 | on | f16 | 512 | 0.93 [0.88, 0.98] | 0.83 [0.76, 0.91] | 0.09 [0.04, 0.15] | 0.09 [0.04, 0.15] | 78 |
| llama-3.1-8b-instruct | C5 | 1 | off | q8_0 | 512 | failed: server exited during startup (exit statu | failed: server exited during startup (exit statu | — | — | — |
| llama-3.1-8b-instruct | C6 | 8 | off | q8_0 | 512 | failed: server exited during startup (exit statu | failed: server exited during startup (exit statu | — | — | — |
| llama-3.1-8b-instruct | C7 | 8 | off | f16 | 128 | 0.94 [0.89, 0.98] | 0.88 [0.81, 0.94] | 0.09 [0.04, 0.15] | 0.09 [0.04, 0.15] | 64 |
| qwen3-4b-stock | C1 | 1 | off | f16 | 512 | 0.96 [0.92, 0.99] | 1.00 [1.00, 1.00] | 0.02 [0.00, 0.05] | 0.02 [0.00, 0.05] | 41 |
| qwen3-4b-stock | C2 | 8 | off | f16 | 512 | 0.96 [0.92, 0.99] | 0.77 [0.69, 0.85] | 0.02 [0.00, 0.05] | 0.02 [0.00, 0.05] | 0 |
| qwen3-4b-stock | C3 | 1 | on | f16 | 512 | 0.98 [0.95, 1.00] | 1.00 [1.00, 1.00] | 0.02 [0.00, 0.05] | 0.02 [0.00, 0.05] | 32 |
| qwen3-4b-stock | C4 | 8 | on | f16 | 512 | 0.92 [0.87, 0.97] | 0.77 [0.69, 0.85] | 0.02 [0.00, 0.05] | 0.02 [0.00, 0.05] | 49 |
| qwen3-4b-stock | C5 | 1 | off | q8_0 | 512 | failed: server exited during startup (exit statu | failed: server exited during startup (exit statu | — | — | — |
| qwen3-4b-stock | C6 | 8 | off | q8_0 | 512 | failed: server exited during startup (exit statu | failed: server exited during startup (exit statu | — | — | — |
| qwen3-4b-stock | C7 | 8 | off | f16 | 128 | 0.93 [0.88, 0.98] | 0.75 [0.66, 0.84] | 0.02 [0.00, 0.05] | 0.02 [0.00, 0.05] | 0 |

| Pre-registered hypothesis | Verdict |
|---|---|
| H1 single slot is deterministic | not supported: single-slot cells llama-3.1-8b-instruct C1 0.97 [0.94, 1.00]; llama-3.1-8b-instruct C3 0.98 [0.95, 1.00]; llama-3.1-8b-instruct C5 not run; qwen3-4b-stock C1 0.96 [0.92, 0.99]; qwen3-4b-stock C3 0.98 [0.95, 1.00]; qwen3-4b-stock C5 not run |
| H2 batching breaks it | not supported: 8-slot cells llama-3.1-8b-instruct C2 0.92 [0.86, 0.97]; llama-3.1-8b-instruct C4 0.93 [0.88, 0.98]; llama-3.1-8b-instruct C6 not run; llama-3.1-8b-instruct C7 0.94 [0.89, 0.98]; qwen3-4b-stock C2 0.96 [0.92, 0.99]; qwen3-4b-stock C4 0.92 [0.87, 0.97]; qwen3-4b-stock C6 not run; qwen3-4b-stock C7 0.93 [0.88, 0.98] |
| H3 it moves the score | not supported (no pair of configurations differs by more than 3 pp): no comparable cells |

| Model | Single-slot flag comparison (run 1, share of benign prompts byte-identical) |
|---|---|
| llama-3.1-8b-instruct | flash attention (C1 vs C3): 0.93 |
| qwen3-4b-stock | flash attention (C1 vs C3): 0.92 |
<!-- DETERMINISM:END -->

**Reading the table (written after the run; the hypotheses and thresholds above it were fixed before).** C5 and C6 never started: this llama.cpp build refuses a quantized V cache without flash attention ("quantized V cache requires flash_attn to be enabled"), so by the study's rules they stay in the table as failed rather than being re-specified. H1 is not supported: even with one slot, the same server given the same prompt twice at temperature 0 does not always return the same bytes. One untested candidate explanation is llama-server's prompt cache: run 2 of a prompt can reuse run 1's KV prefix and take a different numerical path than a fresh prefill; a follow-up would repeat C1 with `cache_prompt` disabled. The JailbreakBench non-refusal rates are the frozen substring grader's view and move by well under 3 points across configurations, so H3 is not supported either.

