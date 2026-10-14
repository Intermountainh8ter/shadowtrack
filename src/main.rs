//! shadowtrack CLI — scaffold.

use clap::{Parser, Subcommand};
use shadowtrack::{agreement_bootstrap_ci, agreement_rate, safe_to_upgrade, Pair};

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
    Version,
}

#[derive(serde::Deserialize)]
struct Doc {
    pairs: Vec<Pair>,
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
        Cmd::Version => println!("shadowtrack {}", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}
