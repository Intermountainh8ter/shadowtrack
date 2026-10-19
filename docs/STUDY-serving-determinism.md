# Study: how non-deterministic is "temperature 0" on a local serving stack, and does it move scores?

Pre-registered 2026-10-18, before any run of this study. Append-only below the line.

## Motivating observation (already published, not part of this study's evidence)

On 2026-10-16 shadowtrack's self-agreement baseline found that llama-server (llama.cpp commit
091b6a7, HIP, gfx906) at temperature 0, seed 42, with 8 continuous-batching slots, reproduced its own
answer on only 92 of 100 harmless prompts (95% CI 0.86-0.97). That number came from a run designed
to compare two models, not to study the server. This study is the controlled version.

## Question

Across serving configurations of one stack, (1) how often does the same prompt at temperature 0
return byte-identical output on two consecutive runs, and (2) how much does a benchmark's headline
rate move between configurations that are supposed to be equivalent?

## Scope and honesty about the stack

Only llama.cpp is available on this hardware (two AMD gfx906 cards; vLLM, TGI and Ollama do not
support the GPU or could not be installed). This is therefore a one-stack, many-configuration study.
Cross-stack comparison is deferred and will be a separate pre-registration.

## Design

- Models: Qwen3-4B-Instruct-2507 (Q8_0) and Llama-3.1-8B-Instruct (Q8_0), served one at a time.
- Prompt sets: (a) the 100-prompt benign capability fixture (`refusalscope/suites/capability.jsonl`,
  sha256 recorded in results), graded by exact byte match of `choices[0].message.content`;
  (b) JailbreakBench's 100 behaviours graded by jailbreakreplay's frozen grader v1, giving a
  non-refusal rate per configuration.
- Sampling pinned: temperature 0, top_p 1, top_k 1, seed 42, max_tokens 128 (a) / 256 (b).
- Configurations (every other flag identical; one server at a time, no other GPU load):
  | id | slots (-np) | flash attention | KV cache type | ubatch |
  |---|---|---|---|---|
  | C1 | 1 | off | f16 | 512 |
  | C2 | 8 | off | f16 | 512 |
  | C3 | 1 | on | f16 | 512 |
  | C4 | 8 | on | f16 | 512 |
  | C5 | 1 | off | q8_0 | 512 |
  | C6 | 8 | off | q8_0 | 512 |
  | C7 | 8 | off | f16 | 128 |
- Each configuration is run twice back to back on both prompt sets (run 1 and run 2), with the
  prompts submitted in the same order and, for -np 8, with 8 concurrent requests in flight.
- Self-agreement for a configuration = share of the 100 benign prompts whose run-1 and run-2
  outputs are byte-identical, with a 95% percentile bootstrap interval (2,000 resamples, seed 42),
  via `shadowtrack audit` on the pair file.

## Hypotheses, fixed now

- **H1 (single slot is deterministic).** With one slot (C1, C3, C5) self-agreement is 1.00 with a
  CI lower bound at or above 0.97 for both models. Supported if every single-slot cell meets it.
- **H2 (batching breaks it).** Every 8-slot configuration (C2, C4, C6, C7) has a self-agreement CI
  upper bound below 0.99 for both models. Supported if every 8-slot cell meets it.
- **H3 (it moves the score).** For at least one model, the JailbreakBench non-refusal rate differs
  between two configurations by more than 3 percentage points (point estimates, run 1 of each).
  Reported with both rates and their intervals; "supported" only if the two intervals do not overlap.

Descriptive, no threshold: which of flash attention, KV cache type, and ubatch changes the
single-slot output at all (C1 vs C3 vs C5, run 1, byte match per prompt); the distribution of first
divergent token position in disagreeing pairs.

## Rules

- No configuration is dropped after the fact. A configuration that fails to start is reported as
  such with the server's error.
- Only labels, hashes and agreement flags are committed for the JailbreakBench set; the benign set's
  outputs may be committed in full.
- Server build, flags, model file hashes and the prompt-file hashes go in the results JSON.
- The 2026-10-16 observation above is not re-used as evidence; this study's C2 cell replaces it.

## Deferred

Cross-stack comparison; sampled (temperature > 0) variance; longer outputs; effect on
trialgauntlet pass^k.

## Pilot cell C8: prompt cache off (added 2026-10-19, before it was run)

The study above left single-slot self-agreement at 0.96 to 0.98 and did not say why. The untested
explanation recorded at the time was llama-server's per-slot prompt cache: with `cache_prompt`
on (the server default), run 2 can reuse run 1's KV prefix, and a prefix computed in one batch
shape is not bit-identical to one computed in another. C8 is C1 with `cache_prompt: false` sent
on every request and nothing else changed (one slot, flash attention off, f16 KV, ubatch 512).

- **Prediction, registered now:** C8 benign self-agreement reaches a CI lower bound of at least
  0.99 for both models. If it does, the prompt cache explains most of the single-slot
  disagreement in C1. If it does not, the cause is elsewhere (kernel nondeterminism on this
  backend), and that is reported as the result.
- Same prompt sets, sampling, runs and graders as the other cells; the runner records the
  `cache_prompt` value in the cell file and the README table gains a "Prompt cache" column.
- This is a pilot for the error-budget study drafted in `~/Desktop/Research Deep Dive.md`; it
  does not change H1 to H3 above, whose verdicts stand on C1 to C7.

**C8 first attempt (2026-10-19, discarded, no trial completed).** The runner was launched from a
shell without the gfx906 ROCm environment (`~/projects/gpu-harness/env.sh`), so each server
answered `/health` and then crashed in rocBLAS on the first request; all 400 requests per model
failed at transport level. The two cell files held zero completed trials and were deleted before
the rerun; nothing about the cell changed.
