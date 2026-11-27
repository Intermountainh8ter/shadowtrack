## 2026-11-27T06:32:18Z agreement-floor proposal:2884d8b409d32c7642b307fffdce6e6069a43620b9da4aff94cf4821bccaff8f

Hypothesis: on a seeded 1000-prompt fixture whose old model emits one majority label with probability 0.95 (4 other labels share the rest), a constant candidate that always emits the majority label reaches an agreement rate of at least 0.93 and its bootstrap lower bound exceeds the 0.9 floor, so the safe-to-upgrade verdict passes a degenerate candidate; a uniform-random candidate stays below 0.4.

Metric: `/degenerate/constant_candidate/agreement_rate` in `results/637-agreement-floor.json`.

Expected: at least 0.93 (range 0.93 to 0.97), constant-candidate safe verdict true.

Falsifier: constant-candidate agreement rate below 0.93, or its bootstrap lower bound at or below 0.9, on the seeded fixture (seed 7).
