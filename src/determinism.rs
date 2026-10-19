//! Serving-determinism study runner (docs/STUDY-serving-determinism.md).
//!
//! For each model and configuration in the study file, this starts one `llama-server`,
//! waits until it is healthy, runs each prompt set twice in submission order with the
//! configured number of requests in flight, then stops the server before the next
//! configuration. Benign outputs are kept in full and compared byte for byte. Harmful-set
//! completions are written only to a scratch file for an external grader and never into
//! the results directory.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Deserialize)]
pub struct Study {
    pub study: String,
    pub common_flags: Vec<String>,
    pub models: Vec<Model>,
    pub configurations: Vec<Configuration>,
    pub prompt_sets: BTreeMap<String, PromptSet>,
    pub sampling: Sampling,
    pub runs_per_configuration: usize,
    pub concurrency_for_np8: usize,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Model {
    pub alias: String,
    pub file: String,
    #[serde(default)]
    pub qwen_template_flags: bool,
    #[serde(default)]
    pub chat_template_file: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Configuration {
    pub id: String,
    pub np: u32,
    pub flash_attn: String,
    pub cache_type_k: String,
    pub ubatch: u32,
    /// Per-request `cache_prompt` sent to the server; None leaves the server default (on).
    #[serde(default)]
    pub cache_prompt: Option<bool>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PromptSet {
    pub path: String,
    pub n: usize,
    pub grader: String,
    pub max_tokens: u32,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct Sampling {
    pub temperature: f64,
    pub top_p: f64,
    pub top_k: u32,
    pub seed: u64,
}

/// One run of one prompt set: the outputs in prompt order.
#[derive(Serialize, Deserialize)]
pub struct RunOutputs {
    pub run: usize,
    pub outputs: Vec<String>,
    pub errors: Vec<Option<String>>,
    pub wall_s: f64,
}

#[derive(Serialize)]
pub struct Cell {
    pub model: String,
    pub configuration: Configuration,
    pub server_flags: Vec<String>,
    pub started: bool,
    pub start_error: Option<String>,
    /// prompt set -> per-run outputs (benign only; harmful sets carry hashes, see `harmful_hashes`).
    pub benign_runs: Vec<RunOutputs>,
    pub benign_pairs: Vec<crate::Pair>,
    pub benign_self_agreement: Option<crate::SelfBaseline>,
    /// Character index of the first difference for each disagreeing benign pair.
    pub first_divergence_chars: Vec<(String, usize)>,
    /// sha256 of each harmful completion per run (no text), so two runs can be compared later.
    pub harmful_hashes: Vec<Vec<String>>,
    pub harmful_self_agreement: Option<crate::SelfBaseline>,
    /// Scratch file paths holding the harmful transcripts for the external grader (outside the repo).
    pub harmful_transcript_files: Vec<String>,
}

fn expand(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(h) = std::env::var("HOME") { return format!("{h}/{rest}"); }
    }
    p.to_string()
}

pub fn sha256_file(p: &Path) -> anyhow::Result<String> {
    let mut f = std::fs::File::open(p)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

fn sha256_str(s: &str) -> String { hex::encode(Sha256::digest(s.as_bytes())) }

#[derive(Deserialize)]
struct PromptLine { prompt: String, #[serde(default)] id: Option<String> }

fn load_prompts(path: &str, n: usize) -> anyhow::Result<Vec<(String, String)>> {
    let text = std::fs::read_to_string(expand(path))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().filter(|l| !l.trim().is_empty()).enumerate() {
        let l: PromptLine = serde_json::from_str(line)?;
        out.push((l.id.unwrap_or_else(|| format!("P-{i:04}")), l.prompt));
        if out.len() >= n { break; }
    }
    Ok(out)
}

fn server_flags(study: &Study, model: &Model, cfg: &Configuration, port: u16) -> Vec<String> {
    let mut f: Vec<String> = Vec::new();
    f.extend(["-m".to_string(), expand(&model.file), "--alias".into(), model.alias.clone(), "--port".into(), port.to_string()]);
    // common flags, minus the Qwen template pair when the model brings its own template
    let mut i = 0;
    let c = &study.common_flags;
    while i < c.len() {
        let k = &c[i];
        let is_qwen_tmpl = k == "--chat-template-file" || k == "--chat-template-kwargs" || k == "--reasoning-budget";
        if is_qwen_tmpl && !model.qwen_template_flags {
            i += 2; continue;
        }
        f.push(expand(k));
        i += 1;
    }
    if let Some(t) = &model.chat_template_file { f.extend(["--chat-template-file".to_string(), expand(t)]); }
    f.extend(["-np".to_string(), cfg.np.to_string(), "-fa".into(), cfg.flash_attn.clone(), "-ctk".into(), cfg.cache_type_k.clone(), "-ctv".into(), cfg.cache_type_k.clone(), "-ub".into(), cfg.ubatch.to_string()]);
    f
}

async fn wait_healthy(client: &reqwest::Client, port: u16, child: &mut tokio::process::Child, log: &Path) -> Result<(), String> {
    for _ in 0..120 {
        if let Ok(Some(status)) = child.try_wait() {
            let tail = std::fs::read_to_string(log).map(|s| s.lines().rev().take(5).collect::<Vec<_>>().join(" | ")).unwrap_or_default();
            return Err(format!("server exited during startup ({status}): {tail}"));
        }
        if let Ok(r) = client.get(format!("http://127.0.0.1:{port}/health")).send().await {
            if r.status().is_success() { return Ok(()); }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Err("server not healthy after 600 s".into())
}

/// Submit every prompt in order with at most `concurrency` requests in flight.
async fn run_set(client: &reqwest::Client, port: u16, prompts: &[(String, String)], max_tokens: u32, s: &Sampling, cache_prompt: Option<bool>, concurrency: usize, run: usize) -> RunOutputs {
    let started = std::time::Instant::now();
    let mut outputs = vec![String::new(); prompts.len()];
    let mut errors = vec![None; prompts.len()];
    let mut set: tokio::task::JoinSet<(usize, Result<String, String>)> = tokio::task::JoinSet::new();
    let url = format!("http://127.0.0.1:{port}/v1/chat/completions");
    for (i, (_, prompt)) in prompts.iter().enumerate() {
        if set.len() >= concurrency.max(1) {
            if let Some(Ok((j, r))) = set.join_next().await { match r { Ok(t) => outputs[j] = t, Err(e) => errors[j] = Some(e) } }
        }
        let mut body = serde_json::json!({
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens, "temperature": s.temperature, "top_p": s.top_p, "top_k": s.top_k, "seed": s.seed,
        });
        if let Some(cp) = cache_prompt { body["cache_prompt"] = serde_json::json!(cp); }
        let c = client.clone(); let u = url.clone();
        set.spawn_local(async move {
            let r = async {
                let resp = c.post(&u).json(&body).send().await.map_err(|e| format!("transport: {e}"))?;
                let status = resp.status();
                let doc: serde_json::Value = resp.json().await.map_err(|_| "malformed json".to_string())?;
                if !status.is_success() { return Err(format!("http {}: {}", status.as_u16(), doc["error"]["message"].as_str().unwrap_or("?"))); }
                doc.pointer("/choices/0/message/content").and_then(|v| v.as_str()).map(|s| s.to_string()).ok_or_else(|| "no content".to_string())
            }.await;
            (i, r)
        });
    }
    while let Some(Ok((j, r))) = set.join_next().await { match r { Ok(t) => outputs[j] = t, Err(e) => errors[j] = Some(e) } }
    RunOutputs { run, outputs, errors, wall_s: started.elapsed().as_secs_f64() }
}

fn first_divergence(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

fn pairs_of(ids: &[(String, String)], r1: &RunOutputs, r2: &RunOutputs) -> Vec<crate::Pair> {
    ids.iter().enumerate().map(|(i, (id, _))| crate::Pair { prompt_id: id.clone(), agree: r1.errors[i].is_none() && r2.errors[i].is_none() && r1.outputs[i] == r2.outputs[i] }).collect()
}

pub struct RunArgs {
    pub study: PathBuf,
    pub server: PathBuf,
    pub out_dir: PathBuf,
    pub scratch_dir: PathBuf,
    pub port: u16,
    pub gpu: String,
    pub only_model: Option<String>,
    pub only_config: Option<String>,
    pub limit: Option<usize>,
}

pub fn run(a: RunArgs) -> anyhow::Result<()> {
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build()?;
    let local = tokio::task::LocalSet::new();
    local.block_on(&rt, run_async(a))
}

async fn run_async(a: RunArgs) -> anyhow::Result<()> {
    let study: Study = serde_json::from_str(&std::fs::read_to_string(&a.study)?)?;
    std::fs::create_dir_all(&a.out_dir)?;
    std::fs::create_dir_all(&a.scratch_dir)?;
    let client = reqwest::Client::builder().timeout(Duration::from_secs(600)).build()?;
    let server_version = String::from_utf8_lossy(&std::process::Command::new(&a.server).arg("--version").output()?.stderr).lines().next().unwrap_or("?").to_string();
    let benign = study.prompt_sets.get("benign").ok_or_else(|| anyhow::anyhow!("study has no benign prompt set"))?.clone();
    let harmful = study.prompt_sets.get("jailbreakbench").ok_or_else(|| anyhow::anyhow!("study has no jailbreakbench prompt set"))?.clone();
    let mut benign_prompts = load_prompts(&benign.path, benign.n)?;
    let mut harmful_prompts = load_prompts(&harmful.path, harmful.n)?;
    if let Some(l) = a.limit { benign_prompts.truncate(l); harmful_prompts.truncate(l); }
    let prompt_hashes = serde_json::json!({
        "benign": sha256_file(Path::new(&expand(&benign.path)))?, "jailbreakbench": sha256_file(Path::new(&expand(&harmful.path)))?,
    });
    let mut model_hashes = BTreeMap::new();
    for m in &study.models {
        if a.only_model.as_deref().map(|x| x != m.alias).unwrap_or(false) { continue; }
        eprintln!("hashing {}", m.alias);
        model_hashes.insert(m.alias.clone(), sha256_file(Path::new(&expand(&m.file)))?);
    }
    let meta = serde_json::json!({"study": study.study, "server_version": server_version, "server_path": a.server, "prompt_file_sha256": prompt_hashes, "model_file_sha256": model_hashes, "sampling": study.sampling, "limit": a.limit, "started_utc": chrono_now()});
    std::fs::write(a.out_dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;

    for m in &study.models {
        if a.only_model.as_deref().map(|x| x != m.alias).unwrap_or(false) { continue; }
        for cfg in &study.configurations {
            if a.only_config.as_deref().map(|x| x != cfg.id).unwrap_or(false) { continue; }
            let cell_path = a.out_dir.join(format!("{}-{}.json", m.alias, cfg.id));
            if cell_path.exists() { eprintln!("skip {} {} (exists)", m.alias, cfg.id); continue; }
            let flags = server_flags(&study, m, cfg, a.port);
            let log = a.scratch_dir.join(format!("server-{}-{}.log", m.alias, cfg.id));
            eprintln!("== {} {} : starting server", m.alias, cfg.id);
            let logf = std::fs::File::create(&log)?;
            let mut child = tokio::process::Command::new(&a.server).args(&flags).env("HIP_VISIBLE_DEVICES", &a.gpu).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::from(logf)).kill_on_drop(true).spawn()?;
            let mut cell = Cell { model: m.alias.clone(), configuration: cfg.clone(), server_flags: flags.clone(), started: false, start_error: None, benign_runs: vec![], benign_pairs: vec![], benign_self_agreement: None, first_divergence_chars: vec![], harmful_hashes: vec![], harmful_self_agreement: None, harmful_transcript_files: vec![] };
            match wait_healthy(&client, a.port, &mut child, &log).await {
                Ok(()) => cell.started = true,
                Err(e) => { cell.start_error = Some(e); }
            }
            if cell.started {
                let conc = if cfg.np > 1 { study.concurrency_for_np8 } else { 1 };
                for run in 1..=study.runs_per_configuration {
                    eprintln!("   benign run {run} (concurrency {conc})");
                    cell.benign_runs.push(run_set(&client, a.port, &benign_prompts, benign.max_tokens, &study.sampling, cfg.cache_prompt, conc, run).await);
                }
                let mut harm_runs = Vec::new();
                for run in 1..=study.runs_per_configuration {
                    eprintln!("   jailbreakbench run {run} (concurrency {conc})");
                    let r = run_set(&client, a.port, &harmful_prompts, harmful.max_tokens, &study.sampling, cfg.cache_prompt, conc, run).await;
                    cell.harmful_hashes.push(r.outputs.iter().map(|o| sha256_str(o)).collect());
                    let tpath = a.scratch_dir.join(format!("transcripts-{}-{}-run{run}.jsonl", m.alias, cfg.id));
                    let mut text = String::new();
                    for o in &r.outputs { text.push_str(&serde_json::json!({"transcript": o}).to_string()); text.push('\n'); }
                    std::fs::write(&tpath, text)?;
                    cell.harmful_transcript_files.push(tpath.display().to_string());
                    harm_runs.push(r);
                }
                if cell.benign_runs.len() >= 2 {
                    cell.benign_pairs = pairs_of(&benign_prompts, &cell.benign_runs[0], &cell.benign_runs[1]);
                    cell.benign_self_agreement = crate::self_baseline(&format!("{}-{}", m.alias, cfg.id), &cell.benign_pairs, 2000, 42);
                    cell.first_divergence_chars = benign_prompts.iter().enumerate().filter(|(i, _)| !cell.benign_pairs[*i].agree).map(|(i, (id, _))| (id.clone(), first_divergence(&cell.benign_runs[0].outputs[i], &cell.benign_runs[1].outputs[i]))).collect();
                }
                if harm_runs.len() >= 2 {
                    let hp = pairs_of(&harmful_prompts, &harm_runs[0], &harm_runs[1]);
                    cell.harmful_self_agreement = crate::self_baseline(&format!("{}-{}-jbb", m.alias, cfg.id), &hp, 2000, 42);
                }
                if let Some(sb) = &cell.benign_self_agreement { eprintln!("   benign self-agreement {:.3} [{:.3}, {:.3}]", sb.rate, sb.ci_lo, sb.ci_hi); }
            } else {
                eprintln!("   FAILED to start: {}", cell.start_error.as_deref().unwrap_or("?"));
            }
            let _ = child.kill().await;
            let _ = child.wait().await;
            // make sure the port is free before the next configuration
            for _ in 0..30 { if client.get(format!("http://127.0.0.1:{}/health", a.port)).send().await.is_err() { break; } tokio::time::sleep(Duration::from_secs(1)).await; }
            std::fs::write(&cell_path, serde_json::to_string_pretty(&cell)?)?;
            eprintln!("   wrote {}", cell_path.display());
        }
    }
    Ok(())
}

fn chrono_now() -> String {
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    format!("{secs}")
}
