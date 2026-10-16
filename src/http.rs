//! Live HTTP sampler for shadow-mode agreement runs.
//!
//! A shadow run compares two versions (A = reference, B = candidate)
//! on the same prompt set. The sampler POSTs each prompt to both URLs
//! in sequence and extracts the output text at a configured JSON path,
//! handing back `Outputs { prompt_id, a, b }` ready for a Grader.

use crate::Outputs;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use thiserror::Error;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ShadowProfile {
    pub name: String,
    pub a_url: String,
    pub b_url: String,
    #[serde(default = "default_prompt_field")]
    pub prompt_field: String,
    pub output_json_path: String,
    /// Request body the prompt is written into at `prompt_field` (a
    /// `/`-separated path, e.g. `messages/0/content`). Carries fixed
    /// sampling parameters so both sides see identical settings. Absent
    /// or null means an empty object, i.e. the body is `{prompt_field:
    /// prompt}`.
    #[serde(default)]
    pub body_template: serde_json::Value,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default = "default_timeout_s")]
    pub timeout_s: u64,
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
}

fn default_prompt_field() -> String {
    "prompt".into()
}

fn default_timeout_s() -> u64 {
    60
}

fn default_max_retries() -> u32 {
    2
}

#[derive(Debug, Error)]
pub enum SampleError {
    #[error("api_key_env `{0}` is set but the environment variable is not set")]
    AuthMissing(String),
    #[error("endpoint `{side}` refused authentication (HTTP {status})")]
    AuthRejected { side: &'static str, status: u16 },
    #[error("endpoint `{side}` returned HTTP {status} after {attempts} attempts")]
    HttpStatus {
        side: &'static str,
        status: u16,
        attempts: u32,
    },
    #[error("endpoint `{side}` response missing field at JSON path `{path}`")]
    MissingField { side: &'static str, path: String },
    #[error("endpoint `{side}` response field at `{path}` is not a string")]
    NotAString { side: &'static str, path: String },
    #[error("endpoint `{side}` response could not be parsed as JSON")]
    MalformedJson { side: &'static str },
    #[error("transport error on `{side}` after {attempts} attempts: {source}")]
    Transport {
        side: &'static str,
        attempts: u32,
        #[source]
        source: reqwest::Error,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptRow {
    pub prompt_id: String,
    pub prompt: String,
}

fn pointer_from_path(path: &str) -> String {
    let trimmed = path.trim_matches('/');
    if trimmed.is_empty() {
        String::new()
    } else {
        let mut s = String::with_capacity(trimmed.len() + 1);
        for seg in trimmed.split('/') {
            s.push('/');
            s.push_str(seg);
        }
        s
    }
}

pub fn sample_pairs(
    profile: &ShadowProfile,
    prompts: &[PromptRow],
) -> Result<Vec<Outputs>, SampleError> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("current-thread runtime builds");
    rt.block_on(sample_pairs_async(profile, prompts))
}

pub async fn sample_pairs_async(
    profile: &ShadowProfile,
    prompts: &[PromptRow],
) -> Result<Vec<Outputs>, SampleError> {
    let auth = match &profile.api_key_env {
        None => None,
        Some(name) => Some(std::env::var(name).map_err(|_| SampleError::AuthMissing(name.clone()))?),
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(profile.timeout_s))
        .build()
        .map_err(|e| SampleError::Transport {
            side: "a",
            attempts: 0,
            source: e,
        })?;
    let mut out = Vec::with_capacity(prompts.len());
    for row in prompts {
        let body = request_body(&profile.body_template, &profile.prompt_field, &row.prompt);
        let a = fetch_text(&client, "a", &profile.a_url, auth.as_deref(), &body, &profile.output_json_path, profile.max_retries).await?;
        let b = fetch_text(&client, "b", &profile.b_url, auth.as_deref(), &body, &profile.output_json_path, profile.max_retries).await?;
        out.push(Outputs {
            prompt_id: row.prompt_id.clone(),
            a,
            b,
        });
    }
    Ok(out)
}

fn request_body(template: &serde_json::Value, prompt_field: &str, prompt: &str) -> serde_json::Value {
    let mut body = match template {
        serde_json::Value::Object(_) => template.clone(),
        _ => serde_json::json!({}),
    };
    set_at_path(&mut body, prompt_field, serde_json::Value::String(prompt.to_string()));
    body
}

/// Write `value` at a `/`-separated path, creating intermediate objects.
/// Arrays along the path must already exist in the template; an index
/// past the end appends.
fn set_at_path(root: &mut serde_json::Value, path: &str, value: serde_json::Value) {
    let segs: Vec<&str> = path.trim_matches('/').split('/').filter(|s| !s.is_empty()).collect();
    if segs.is_empty() {
        *root = value;
        return;
    }
    let mut cur = root;
    for seg in &segs[..segs.len() - 1] {
        cur = match cur {
            serde_json::Value::Array(arr) => {
                let i: usize = seg.parse().unwrap_or(arr.len());
                if i >= arr.len() {
                    arr.push(serde_json::json!({}));
                }
                let i = i.min(arr.len() - 1);
                &mut arr[i]
            }
            other => {
                if !other.is_object() {
                    *other = serde_json::json!({});
                }
                other
                    .as_object_mut()
                    .expect("object")
                    .entry((*seg).to_string())
                    .or_insert_with(|| serde_json::json!({}))
            }
        };
    }
    let last = segs[segs.len() - 1];
    match cur {
        serde_json::Value::Array(arr) => {
            let i: usize = last.parse().unwrap_or(arr.len());
            if i < arr.len() {
                arr[i] = value;
            } else {
                arr.push(value);
            }
        }
        other => {
            if !other.is_object() {
                *other = serde_json::json!({});
            }
            other.as_object_mut().expect("object").insert(last.to_string(), value);
        }
    }
}

async fn fetch_text(
    client: &reqwest::Client,
    side: &'static str,
    url: &str,
    auth: Option<&str>,
    body: &serde_json::Value,
    output_path: &str,
    max_retries: u32,
) -> Result<String, SampleError> {
    let mut attempt: u32 = 0;
    loop {
        attempt += 1;
        let mut req = client.post(url).json(&body);
        if let Some(k) = auth {
            req = req.bearer_auth(k);
        }
        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                if status.is_success() {
                    let doc: serde_json::Value =
                        resp.json().await.map_err(|_| SampleError::MalformedJson { side })?;
                    let p = pointer_from_path(output_path);
                    let v = doc.pointer(&p).ok_or(SampleError::MissingField {
                        side,
                        path: output_path.to_string(),
                    })?;
                    let s = v.as_str().ok_or(SampleError::NotAString {
                        side,
                        path: output_path.to_string(),
                    })?;
                    return Ok(s.to_string());
                }
                if status.as_u16() == 401 || status.as_u16() == 403 {
                    return Err(SampleError::AuthRejected {
                        side,
                        status: status.as_u16(),
                    });
                }
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if !retryable || attempt > max_retries {
                    return Err(SampleError::HttpStatus {
                        side,
                        status: status.as_u16(),
                        attempts: attempt,
                    });
                }
            }
            Err(e) => {
                let retryable = e.is_timeout() || e.is_connect() || e.is_request();
                if !retryable || attempt > max_retries {
                    return Err(SampleError::Transport {
                        side,
                        attempts: attempt,
                        source: e,
                    });
                }
            }
        }
        let delay_ms = 100u64.saturating_mul(1u64 << (attempt - 1).min(10));
        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn profile(a: &str, b: &str) -> ShadowProfile {
        ShadowProfile {
            name: "mock".into(),
            a_url: format!("{a}/run"),
            b_url: format!("{b}/run"),
            prompt_field: "prompt".into(),
            output_json_path: "text".into(),
            body_template: serde_json::Value::Null,
            api_key_env: None,
            timeout_s: 2,
            max_retries: 2,
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn pairs_are_collected_in_prompt_order() {
        let a = MockServer::start().await;
        let b = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/run"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "A"})))
            .mount(&a)
            .await;
        Mock::given(method("POST"))
            .and(path("/run"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "B"})))
            .mount(&b)
            .await;
        let prompts = vec![
            PromptRow {
                prompt_id: "p1".into(),
                prompt: "x".into(),
            },
            PromptRow {
                prompt_id: "p2".into(),
                prompt: "y".into(),
            },
        ];
        let outputs = sample_pairs_async(&profile(&a.uri(), &b.uri()), &prompts)
            .await
            .expect("ok");
        assert_eq!(outputs.len(), 2);
        assert_eq!(outputs[0].prompt_id, "p1");
        assert_eq!(outputs[0].a, "A");
        assert_eq!(outputs[0].b, "B");
        assert_eq!(outputs[1].prompt_id, "p2");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn b_side_error_names_side_b() {
        let a = MockServer::start().await;
        let b = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/run"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"text": "A"})))
            .mount(&a)
            .await;
        Mock::given(method("POST"))
            .and(path("/run"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&b)
            .await;
        let err = sample_pairs_async(
            &profile(&a.uri(), &b.uri()),
            &[PromptRow {
                prompt_id: "p".into(),
                prompt: "x".into(),
            }],
        )
        .await
        .expect_err("b fails");
        match err {
            SampleError::HttpStatus { side, status, .. } => {
                assert_eq!(side, "b");
                assert_eq!(status, 500);
            }
            e => panic!("wrong variant: {e:?}"),
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn missing_output_field_names_side_and_path() {
        let a = MockServer::start().await;
        let b = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/run"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"wrong": "A"})))
            .mount(&a)
            .await;
        Mock::given(method("POST"))
            .and(path("/run"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&b)
            .await;
        let err = sample_pairs_async(
            &profile(&a.uri(), &b.uri()),
            &[PromptRow {
                prompt_id: "p".into(),
                prompt: "x".into(),
            }],
        )
        .await
        .expect_err("missing");
        match err {
            SampleError::MissingField { side, path } => {
                assert_eq!(side, "a");
                assert_eq!(path, "text");
            }
            e => panic!("wrong variant: {e:?}"),
        }
    }
}
