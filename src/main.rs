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
        Cmd::Version => println!("shadowtrack {}", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}
