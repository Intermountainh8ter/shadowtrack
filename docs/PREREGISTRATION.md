# shadowtrack pre-registration

Locked before the first upgrade-report result is committed.
Append-only.

## 1. Scope

Pairs under audit are (A, B) where each is one of: a model endpoint, a
prompt-template version, an agent-framework version. Fixtures are
pre-registered prompt sets with a deterministic scripted grader
committed in the fixture.

## 2. The audit

For (A, B) and fixture `F`:

1. Run `F` against `A` and `B` independently; capture every output.
2. Pair outputs by `prompt_id`.
3. For each pair, the scripted grader returns `agree` (bool).
4. Compute agreement rate and its 95% bootstrap CI.
5. **Safe to upgrade** iff CI lower bound > 0.9.

The grader is deterministic Rust code committed alongside the fixture.
No LLM judges. "Agreement" is whatever the grader says it is; the
fixture author documents the definition in `fixtures/<name>_grader.md`.

## 3. Pre-registered N

Fixtures ship at `N >= 100` by default. A smaller `N` is reported as a
weakness of the fixture, not of the upgrade.

## 4. What this repo refuses to do

- No LLM judges.
- No pooled claim across fixtures. Each fixture is its own row.
- No "almost safe" verdict. A floor is a floor.
- No reporting of a specific divergent example that would reveal
  proprietary business logic. The divergent-example tail is published
  only when the fixture author marks it releasable.

## 5. Threats to validity

- **Grader scope.** A permissive grader over-reports agreement; a
  strict one under-reports. Each fixture ships with an audit-grade
  calibration pass before results are allowed.
- **Non-deterministic endpoints.** Running `A` or `B` twice can give
  different outputs. The audit reports the per-endpoint
  self-agreement baseline on a sample of the fixture; a self-
  agreement below the pair-agreement floor is a confound.
