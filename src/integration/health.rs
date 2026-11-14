//! Bounded, asynchronous reachability checks for configured radio stations.

use crate::config::Station;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StationHealthResult {
    pub station_id: String,
    pub station_name: String,
    pub url: String,
    pub status_code: Option<u16>,
    pub latency_ms: u128,
    pub error: Option<String>,
}

impl StationHealthResult {
    pub fn is_reachable(&self) -> bool {
        self.status_code
            .is_some_and(|status| (200..300).contains(&status))
    }
}

/// Checks station endpoints concurrently, with a fixed timeout and concurrency limit.
pub async fn check_stations(stations: &[Station]) -> Result<Vec<StationHealthResult>, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .connect_timeout(Duration::from_secs(4))
        .user_agent("reso-radio-check/0.1")
        .build()
        .map_err(|error| format!("Failed to create HTTP client: {error}"))?;

    let mut results = futures_util::stream::iter(stations.iter().cloned())
        .map(|station| {
            let client = client.clone();
            async move {
                let started = tokio::time::Instant::now();
                let response = client.get(&station.url).send().await;
                let latency_ms = started.elapsed().as_millis();
                let (status_code, error) = match response {
                    Ok(response) => {
                        let status = response.status();
                        let code = status.as_u16();
                        if status.is_success() {
                            (Some(code), None)
                        } else {
                            (Some(code), Some(format!("HTTP {status}")))
                        }
                    }
                    Err(error) => (None, Some(error.to_string())),
                };
                StationHealthResult {
                    station_id: station.id,
                    station_name: station.name,
                    url: station.url,
                    status_code,
                    latency_ms,
                    error,
                }
            }
        })
        .buffer_unordered(8)
        .collect::<Vec<_>>()
        .await;

    results.sort_by(|left, right| left.station_name.cmp(&right.station_name));
    Ok(results)
}
