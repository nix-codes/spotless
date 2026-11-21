//! MEGA's API, as a public folder's visitor uses it, with no account: the
//! folder's files, and where to download one.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};

const API: &str = "https://g.api.mega.co.nz/cs";
/// How long a request to the API has to be answered.
const ANSWER_TIME: Duration = Duration::from_secs(20);
/// How long a download has to connect, and then to send more: a stretch
/// streams for as long as it takes. MEGA's servers at times go silent
/// midway; such a connection is replaced well before a read waiting on it
/// gives up.
const CONNECT_TIME: Duration = Duration::from_secs(10);
const SILENCE_TIME: Duration = Duration::from_secs(5);
/// How often a request MEGA asks to be sent again (busy) is, and the wait
/// before the first time.
const TRIES: u32 = 4;
const FIRST_WAIT: Duration = Duration::from_millis(500);

/// A file or folder in a public folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    pub handle: String,
    /// The folder it's in.
    pub parent: String,
    pub folder: bool,
    /// Its key as given (`<owner>:<base 64>`), encrypted with the folder's.
    pub key: String,
    /// Its name and the like, encrypted with its key.
    pub attributes: String,
    pub size: u64,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    /// MEGA couldn't be reached, or was too busy to answer.
    #[error("MEGA couldn't be reached: {0}")]
    Unreachable(String),
    /// This connection used up its allowance of MEGA's traffic for now.
    #[error("over MEGA's download allowance")]
    OverQuota,
    /// The folder or file isn't there, or its link was taken down.
    #[error("not found on MEGA")]
    NotFound,
    /// Any other refusal, by MEGA's error number.
    #[error("MEGA answered {0}")]
    Refused(i64),
}

impl ApiError {
    /// MEGA's error number as an error.
    fn of(code: i64) -> Self {
        match code {
            -3 | -4 | -18 => Self::Unreachable(format!("busy ({code})")),
            -17 => Self::OverQuota,
            -2 | -9 | -11 | -16 => Self::NotFound,
            code => Self::Refused(code),
        }
    }
}

pub struct Api {
    client: reqwest::Client,
    /// For downloads, which have no time limit as a whole.
    downloads: reqwest::Client,
    /// Each request's number, as MEGA asks.
    sent: AtomicU64,
}

impl Default for Api {
    fn default() -> Self {
        Self::new()
    }
}

impl Api {
    pub fn new() -> Self {
        let client = reqwest::Client::builder()
            .timeout(ANSWER_TIME)
            .build()
            .unwrap_or_default();
        let downloads = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIME)
            .read_timeout(SILENCE_TIME)
            .build()
            .unwrap_or_default();
        Self {
            client,
            downloads,
            sent: AtomicU64::new(fastrand::u64(..1_000_000_000)),
        }
    }

    /// The client downloads go through.
    pub fn downloads(&self) -> &reqwest::Client {
        &self.downloads
    }

    /// Every file and folder in the public folder `folder`, itself first.
    pub async fn list(&self, folder: &str) -> Result<Vec<Node>, ApiError> {
        let answer = self.call(folder, json!({"a": "f", "c": 1, "ca": 1, "r": 1})).await?;
        let nodes = answer
            .get("f")
            .and_then(Value::as_array)
            .ok_or_else(|| ApiError::Unreachable("no files in the answer".into()))?;
        Ok(nodes.iter().filter_map(node).collect())
    }

    /// Where to download `file` of `folder` from, over HTTPS, and its size.
    pub async fn download(&self, folder: &str, file: &str) -> Result<(String, u64), ApiError> {
        let answer = self
            .call(folder, json!({"a": "g", "g": 1, "ssl": 2, "n": file}))
            .await?;
        if let Some(code) = answer.get("e").and_then(Value::as_i64) {
            return Err(ApiError::of(code));
        }
        let url = answer.get("g").and_then(Value::as_str);
        let size = answer.get("s").and_then(Value::as_u64);
        match (url, size) {
            (Some(url), Some(size)) if !url.is_empty() => Ok((url.to_string(), size)),
            _ => Err(ApiError::NotFound),
        }
    }

    /// Sends one request about `folder`, again while MEGA says it's busy.
    async fn call(&self, folder: &str, request: Value) -> Result<Value, ApiError> {
        let mut wait = FIRST_WAIT;
        let mut tries = 0;
        loop {
            tries += 1;
            match self.send(folder, &request).await {
                Err(ApiError::Unreachable(why)) if tries < TRIES => {
                    log::info!("MEGA didn't answer ({why}); asking again");
                    tokio::time::sleep(wait).await;
                    wait *= 2;
                }
                answer => return answer,
            }
        }
    }

    async fn send(&self, folder: &str, request: &Value) -> Result<Value, ApiError> {
        let id = self.sent.fetch_add(1, Ordering::Relaxed);
        let unreachable = |e: reqwest::Error| ApiError::Unreachable(e.without_url().to_string());
        let response = self
            .client
            .post(API)
            .query(&[("id", id.to_string()), ("n", folder.to_string())])
            .json(&json!([request]))
            .send()
            .await
            .map_err(unreachable)?;
        let status = response.status();
        if !status.is_success() {
            return Err(match status.as_u16() {
                509 => ApiError::OverQuota,
                _ => ApiError::Unreachable(format!("answered {status}")),
            });
        }
        let answer: Value = response.json().await.map_err(unreachable)?;
        answer_of(answer)
    }
}

/// The answer to the one request sent: an error number alone, or a list
/// holding the answer, or its error number.
fn answer_of(answer: Value) -> Result<Value, ApiError> {
    let first = match answer {
        Value::Array(mut answers) if !answers.is_empty() => answers.swap_remove(0),
        Value::Number(code) => return Err(ApiError::of(code.as_i64().unwrap_or_default())),
        _ => return Err(ApiError::Unreachable("an empty answer".into())),
    };
    match first {
        Value::Number(code) => Err(ApiError::of(code.as_i64().unwrap_or_default())),
        answer => Ok(answer),
    }
}

fn node(value: &Value) -> Option<Node> {
    let text = |name: &str| value.get(name).and_then(Value::as_str).map(str::to_string);
    let kind = value.get("t").and_then(Value::as_u64)?;
    Some(Node {
        handle: text("h")?,
        parent: text("p").unwrap_or_default(),
        folder: kind == 1,
        key: text("k")?,
        attributes: text("a")?,
        size: value.get("s").and_then(Value::as_u64).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_come_alone_or_in_the_list() {
        assert_eq!(answer_of(json!(-9)), Err(ApiError::NotFound));
        assert_eq!(answer_of(json!([-17])), Err(ApiError::OverQuota));
        assert!(matches!(answer_of(json!([-3])), Err(ApiError::Unreachable(_))));
        assert_eq!(answer_of(json!([{"g": "u"}])), Ok(json!({"g": "u"})));
    }

    #[test]
    fn a_listing_gives_files_and_folders() {
        let listed = json!({"h": "f1", "p": "root", "t": 0, "k": "root:abc", "a": "attr", "s": 12});
        assert_eq!(
            node(&listed),
            Some(Node {
                handle: "f1".into(),
                parent: "root".into(),
                folder: false,
                key: "root:abc".into(),
                attributes: "attr".into(),
                size: 12,
            })
        );
        assert_eq!(node(&json!({"h": "x", "t": 0})), None);
    }
}
