//! shadowtrack CLI — scaffold.

use clap::{Parser, Subcommand};
use shadowtrack::{agreement_bootstrap_ci, agreement_rate, audit, safe_to_upgrade, Pair};

#[derive(Parser)]
#[command(name = "shadowtrack", version, about = "Shadow-mode behavioral diff")]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Score a pairs JSON file; print agreement rate, 95% CI, and the
    /// pre-registered safe-to-upgrade verdict.
    Score {
        pairs: std::path::PathBuf,
        #[arg(long, default_value_t = 2000)]
        boot: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 0.9)]
        floor: f64,
    },
    /// Full shadow audit: pair agreement, both self-agreement
    /// baselines, pre-reg §5 confound check. Exits 0 safe, 1 unsafe,
    /// 2 when a self-agreement confound makes the verdict unreliable.
    Audit {
        pairs: std::path::PathBuf,
        /// JSON of A-vs-A paired re-runs (same shape as `pairs`,
        /// `{"pairs": [...]}` document). Omit for no A baseline.
        #[arg(long)]
        a_self: Option<std::path::PathBuf>,
        /// JSON of B-vs-B paired re-runs. Omit for no B baseline.
        #[arg(long)]
        b_self: Option<std::path::PathBuf>,
        #[arg(long, default_value_t = 2000)]
        boot: usize,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 0.9)]
        floor: f64,
    },
    /// Sample both sides of a live A/B profile for every prompt, grade
    /// each pair with a built-in scripted grader, and write a
    /// `{"pairs": [...]}` document for `score` / `audit`. Raw outputs
    /// are kept only if `--outputs` names a file.
    Sample {
        /// JSON `ShadowProfile` (a_url, b_url, prompt_field,
        /// output_json_path, body_template, ...).
        #[arg(long)]
        profile: std::path::PathBuf,
        /// JSON-Lines prompts, one `{"prompt_id": "...", "prompt": "..."}` per line.
        #[arg(long)]
        prompts: std::path::PathBuf,
        /// Destination pairs JSON.
        #[arg(long)]
        out: std::path::PathBuf,
        /// `exact` (byte-identical) or `normalized` (whitespace/ASCII-case folded).
        #[arg(long, default_value = "normalized")]
        grader: String,
        /// Optional path to also write the raw `Outputs` (keep out of git).
        #[arg(long)]
        outputs: Option<std::path::PathBuf>,
    },
    /// Run the serving-determinism study (docs/STUDY-serving-determinism.md): one server at a
    /// time, each configuration twice on both prompt sets; writes one JSON per cell.
    Determinism {
        #[arg(long)]
        study: std::path::PathBuf,
        #[arg(long)]
        server: std::path::PathBuf,
        #[arg(long)]
        out_dir: std::path::PathBuf,
        /// Scratch directory for server logs and harmful transcripts (outside the repo).
        #[arg(long)]
        scratch_dir: std::path::PathBuf,
        #[arg(long, default_value_t = 8120)]
        port: u16,
        /// HIP_VISIBLE_DEVICES value for the server.
        #[arg(long, default_value = "0")]
        gpu: String,
        #[arg(long)]
        only_model: Option<String>,
        #[arg(long)]
        only_config: Option<String>,
        /// Smoke-test only: cap prompts per set (results with a limit are not study evidence).
        #[arg(long)]
        limit: Option<usize>,
    },
    Version,
}

#[derive(serde::Deserialize)]
struct Doc {
    pairs: Vec<Pair>,
}

fn load_pairs(p: Option<std::path::PathBuf>) -> anyhow::Result<Vec<Pair>> {
    match p {
        Some(path) => {
            let d: Doc = serde_json::from_str(&std::fs::read_to_string(&path)?)?;
            Ok(d.pairs)
        }
        None => Ok(vec![]),
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Score {
            pairs,
            boot,
            seed,
            floor,
        } => {
            let d: Doc = serde_json::from_str(&std::fs::read_to_string(&pairs)?)?;
            let r = agreement_rate(&d.pairs);
            let (lo, hi) = agreement_bootstrap_ci(&d.pairs, boot, seed);
            let verdict = if safe_to_upgrade((lo, hi), floor) {
                "SAFE"
            } else {
                "NOT SAFE"
            };
            println!("agreement = {r:.4}");
            println!("95%CI     = [{lo:.4}, {hi:.4}]  (boot={boot}, seed={seed})");
            println!("floor     = {floor:.4}");
            println!("verdict   = {verdict}");
        }
        Cmd::Audit {
            pairs,
            a_self,
            b_self,
            boot,
            seed,
            floor,
        } => {
            let d: Doc = serde_json::from_str(&std::fs::read_to_string(&pairs)?)?;
            let a = load_pairs(a_self)?;
            let b = load_pairs(b_self)?;
            let r = audit(&d.pairs, &a, &b, floor, boot, seed);
            let safe = r.safe_to_upgrade;
            let confound = r.self_agreement_confound;
            println!("{}", serde_json::to_string_pretty(&r)?);
            if confound {
                std::process::exit(2);
            } else if !safe {
                std::process::exit(1);
            }
        }
        Cmd::Sample {
            profile,
            prompts,
            out,
            grader,
            outputs,
        } => {
            let p: shadowtrack::http::ShadowProfile =
                serde_json::from_str(&std::fs::read_to_string(&profile)?)?;
            let rows: Vec<shadowtrack::http::PromptRow> = std::fs::read_to_string(&prompts)?
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(serde_json::from_str)
                .collect::<Result<_, _>>()?;
            let outs = shadowtrack::http::sample_pairs(&p, &rows)?;
            let pairs = match grader.as_str() {
                "exact" => shadowtrack::grade_all(&shadowtrack::ExactMatchGrader, &outs),
                "normalized" => shadowtrack::grade_all(&shadowtrack::NormalizedMatchGrader, &outs),
                other => anyhow::bail!("unknown grader {other:?}; use exact or normalized"),
            };
            if let Some(path) = outputs {
                std::fs::write(&path, serde_json::to_string_pretty(&outs)?)?;
            }
            let doc = serde_json::json!({
                "profile": p.name,
                "grader": grader,
                "n": pairs.len(),
                "pairs": pairs,
            });
            std::fs::write(&out, serde_json::to_string_pretty(&doc)?)?;
            println!(
                "sampled {} prompts; agreement = {:.4}",
                pairs.len(),
                agreement_rate(&pairs)
            );
        }
        Cmd::Determinism { study, server, out_dir, scratch_dir, port, gpu, only_model, only_config, limit } => {
            shadowtrack::determinism::run(shadowtrack::determinism::RunArgs { study, server, out_dir, scratch_dir, port, gpu, only_model, only_config, limit })?;
        }
        Cmd::Version => println!("shadowtrack {}", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}
