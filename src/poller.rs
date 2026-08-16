use crate::api;
use chrono::Utc;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Mutex, watch};
use tokio::time::{MissedTickBehavior, interval};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemPrice {
    pub high: Option<i64>,
    pub high_time: Option<i64>,
    pub low: Option<i64>,
    pub low_time: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PriceTick {
    pub timestamp: i64,
    pub prices: HashMap<u32, ItemPrice>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollerStatus {
    pub last_poll_at: Option<i64>,
    pub last_poll_items_count: usize,
    pub last_error: Option<String>,
    pub interval_secs: u64,
}

impl PollerStatus {
    pub fn new(interval_secs: u64) -> Self {
        Self {
            last_poll_at: None,
            last_poll_items_count: 0,
            last_error: None,
            interval_secs,
        }
    }
}

pub async fn run_poller(
    client: Arc<Client>,
    interval_secs: u64,
    tick_tx: watch::Sender<Option<PriceTick>>,
    status: Arc<Mutex<PollerStatus>>,
) {
    let mut ticker = interval(Duration::from_secs(interval_secs));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);

    tracing::info!(
        "Price poller task started with interval of {}s.",
        interval_secs
    );

    loop {
        ticker.tick().await;

        let now = Utc::now().timestamp();
        tracing::debug!("Polling OSRS wiki prices API...");

        match api::fetch_latest(&client).await {
            Ok(latest_resp) => {
                let mut prices = HashMap::with_capacity(latest_resp.data.len());
                for (id_str, item) in latest_resp.data {
                    if let Ok(id) = id_str.parse::<u32>() {
                        prices.insert(
                            id,
                            ItemPrice {
                                high: item.high,
                                high_time: item.high_time,
                                low: item.low,
                                low_time: item.low_time,
                            },
                        );
                    }
                }

                let count = prices.len();
                let tick = PriceTick {
                    timestamp: now,
                    prices,
                };

                {
                    let mut st = status.lock().await;
                    st.last_poll_at = Some(now);
                    st.last_poll_items_count = count;
                    st.last_error = None;
                }

                if let Err(e) = tick_tx.send(Some(tick)) {
                    tracing::error!("Failed to broadcast price tick: {}", e);
                } else {
                    tracing::info!("Poller received {} item prices.", count);
                }
            }
            Err(e) => {
                let err_msg = format!("Failed to fetch latest prices: {}", e);
                tracing::warn!("{}", err_msg);
                let mut st = status.lock().await;
                st.last_error = Some(err_msg);
            }
        }
    }
}
