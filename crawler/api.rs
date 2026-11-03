use anyhow::{Context, Result, bail};
use reqwest::{Client, StatusCode, header::RETRY_AFTER};
use serde_json::{Map, Value};
use tokio::time::{Duration, sleep};

const API_URL: &str = "https://genshin-impact.fandom.com/api/php";

pub async fn fetch_page(client: &Client, title: &str) -> Result<Value> {
    let result = client
        .get(API_URL)
        .query(&[
            ("action", "query"),
            ("prop", "revisions"),
            ("titles", title),
            ("rvprop", "ids|timestamp|content"),
            ("rvslots", "main"),
            ("redirects", "1"),
            ("format", "json"),
            ("formatversion", "2"),
        ])
        .send()
        .await?
        .error_for_status()?
        .json::<Value>()
        .await?;

    Ok(result)
}

pub async fn fetch_batch(
    client: &Client,
    continuation: Option<&Map<String, Value>>,
    batch_size: usize,
) -> Result<Value> {
    let mut params = vec![
        ("action".to_string(), "query".to_string()),
        ("generator".into(), "allpages".into()),
        ("gapnamespace".into(), "0".into()),
        ("gaplimit".into(), batch_size.to_string()),
        ("prop".into(), "revisions".into()),
        ("rvprop".into(), "ids|timestamp|content".into()),
        ("rvslots".into(), "main".into()),
        ("format".into(), "json".into()),
        ("formatversion".into(), "2".into()),
        ("maxlag".into(), "5".into()),
    ];

    // Continue from the previous batch.
    if let Some(tokens) = continuation {
        for (key, value) in tokens {
            params.push((
                key.clone(),
                value.as_str().context("Invalid continuation token")?.into(),
            ));
        }
    }

    // Retry temporary API failures.
    for attempt in 0..5u32 {
        let response = client.get(API_URL).query(&params).send().await?;

        let status = response.status();

        if status == StatusCode::TOO_MANY_REQUESTS || status == StatusCode::SERVICE_UNAVAILABLE {
            let delay = response
                .headers()
                .get(RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(5 * (1_u64 << attempt));

            eprintln!("HTTP {status}; retrying after {delay}s");
            sleep(Duration::from_secs(delay)).await;
            continue;
        }

        let data = response.error_for_status()?.json::<Value>().await?;

        if let Some(error) = data.get("error") {
            if error["code"] == "maxlag" {
                sleep(Duration::from_secs(5 * (1_u64 << attempt))).await;
                continue;
            }
            bail!("MediaWiki API error: {error}");
        }

        if data.get("query").is_none() {
            bail!("Unexpected API response: missing query object");
        }

        return Ok(data);
    }

    bail!("API request failed after five attempts")
}
