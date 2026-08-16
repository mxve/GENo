use crate::db;
use crate::poller::PriceTick;
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc, watch};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConditionKind {
    PriceUp,
    PriceDown,
    PriceChanged,
    CrossedUp,
    CrossedDown,
    CrossedAny,
}

impl FromStr for ConditionKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "price_up" => Ok(Self::PriceUp),
            "price_down" => Ok(Self::PriceDown),
            "price_changed" => Ok(Self::PriceChanged),
            "crossed_up" => Ok(Self::CrossedUp),
            "crossed_down" => Ok(Self::CrossedDown),
            "crossed_any" => Ok(Self::CrossedAny),
            other => Err(format!("Unknown condition kind: {}", other)),
        }
    }
}

impl fmt::Display for ConditionKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PriceUp => write!(f, "Price went up"),
            Self::PriceDown => write!(f, "Price went down"),
            Self::PriceChanged => write!(f, "Price changed"),
            Self::CrossedUp => write!(f, "Crossed threshold ↑"),
            Self::CrossedDown => write!(f, "Crossed threshold ↓"),
            Self::CrossedAny => write!(f, "Crossed threshold ⇅"),
        }
    }
}

impl ConditionKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::PriceUp => "price_up",
            Self::PriceDown => "price_down",
            Self::PriceChanged => "price_changed",
            Self::CrossedUp => "crossed_up",
            Self::CrossedDown => "crossed_down",
            Self::CrossedAny => "crossed_any",
        }
    }

    pub fn requires_threshold(&self) -> bool {
        matches!(self, Self::CrossedUp | Self::CrossedDown | Self::CrossedAny)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PriceKind {
    Buy,
    Sell,
    Either,
}

impl FromStr for PriceKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            "either" => Ok(Self::Either),
            other => Err(format!("Unknown price kind: {}", other)),
        }
    }
}

impl fmt::Display for PriceKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Buy => write!(f, "Buy"),
            Self::Sell => write!(f, "Sell"),
            Self::Either => write!(f, "Either"),
        }
    }
}

impl PriceKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
            Self::Either => "either",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NotifyChannel {
    Discord(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlertFired {
    pub alert_id: i64,
    pub item_name: String,
    pub item_id: u32,
    pub condition: ConditionKind,
    pub price_kind: PriceKind,
    pub old_value: Option<i64>,
    pub new_value: i64,
    pub threshold: Option<i64>,
    pub channels: Vec<NotifyChannel>,
}

#[derive(Debug, Clone, Default)]
pub struct PreviousPrice {
    pub high: Option<i64>,
    pub low: Option<i64>,
}

pub fn evaluate(condition: ConditionKind, prev: i64, current: i64, threshold: Option<i64>) -> bool {
    match condition {
        ConditionKind::PriceUp => current > prev,
        ConditionKind::PriceDown => current < prev,
        ConditionKind::PriceChanged => current != prev,
        ConditionKind::CrossedUp => {
            if let Some(t) = threshold {
                prev < t && current >= t
            } else {
                false
            }
        }
        ConditionKind::CrossedDown => {
            if let Some(t) = threshold {
                prev > t && current <= t
            } else {
                false
            }
        }
        ConditionKind::CrossedAny => {
            if let Some(t) = threshold {
                (prev < t && current >= t) || (prev > t && current <= t)
            } else {
                false
            }
        }
    }
}

pub async fn run_engine(
    db_conn: Arc<Mutex<Connection>>,
    mut tick_rx: watch::Receiver<Option<PriceTick>>,
    alert_tx: mpsc::Sender<AlertFired>,
) {
    tracing::info!("Engine task started.");
    let mut prev_prices: HashMap<u32, PreviousPrice> = HashMap::new();

    while tick_rx.changed().await.is_ok() {
        let tick = match tick_rx.borrow_and_update().clone() {
            Some(t) => t,
            None => continue,
        };

        let (alerts, app_settings) = {
            let conn = db_conn.lock().await;
            let a = match db::get_enabled_alerts(&conn) {
                Ok(alerts) => alerts,
                Err(e) => {
                    tracing::error!("Engine failed to load enabled alerts: {}", e);
                    continue;
                }
            };
            let s = db::get_app_settings(&conn).unwrap_or_default();
            (a, s)
        };

        let now = Utc::now().timestamp();

        // don't fire multiple alerts for the same item in one tick
        let mut fired_items: HashSet<u32> = HashSet::new();

        for alert in alerts {
            if fired_items.contains(&alert.item_id) {
                continue;
            }

            let condition_kind = match ConditionKind::from_str(&alert.condition_kind) {
                Ok(k) => k,
                Err(e) => {
                    tracing::warn!("Invalid condition_kind in alert #{}: {}", alert.id, e);
                    continue;
                }
            };

            let price_kind = match PriceKind::from_str(&alert.price_kind) {
                Ok(k) => k,
                Err(e) => {
                    tracing::warn!("Invalid price_kind in alert #{}: {}", alert.id, e);
                    continue;
                }
            };

            let curr_item = match tick.prices.get(&alert.item_id) {
                Some(p) => p,
                None => continue,
            };

            // skip if we haven't seen an earlier price yet
            let prev_item = match prev_prices.get(&alert.item_id) {
                Some(p) => p,
                None => continue,
            };

            let mut checks = Vec::new();
            match price_kind {
                PriceKind::Buy => {
                    if let (Some(prev), Some(curr)) = (prev_item.high, curr_item.high) {
                        checks.push((PriceKind::Buy, prev, curr));
                    }
                }
                PriceKind::Sell => {
                    if let (Some(prev), Some(curr)) = (prev_item.low, curr_item.low) {
                        checks.push((PriceKind::Sell, prev, curr));
                    }
                }
                PriceKind::Either => {
                    if let (Some(prev), Some(curr)) = (prev_item.high, curr_item.high) {
                        checks.push((PriceKind::Buy, prev, curr));
                    }
                    if let (Some(prev), Some(curr)) = (prev_item.low, curr_item.low) {
                        checks.push((PriceKind::Sell, prev, curr));
                    }
                }
            }

            for (evaluated_kind, prev, curr) in checks {
                if evaluate(condition_kind, prev, curr, alert.threshold) {
                    let cooldown = if alert.cooldown_secs > 0 {
                        alert.cooldown_secs
                    } else {
                        app_settings.default_cooldown_secs
                    };

                    if let Some(last_fired) = alert.last_fired_at
                        && (now - last_fired) < cooldown as i64
                    {
                        tracing::debug!(
                            "Alert #{} triggered but is on cooldown ({}s remaining)",
                            alert.id,
                            (cooldown as i64) - (now - last_fired)
                        );
                        continue;
                    }

                    let mut channels = Vec::new();
                    let discord_target = alert
                        .discord_webhook
                        .as_ref()
                        .filter(|s| !s.trim().is_empty())
                        .or(app_settings
                            .discord_webhook
                            .as_ref()
                            .filter(|s| !s.trim().is_empty()));

                    if let Some(discord) = discord_target {
                        channels.push(NotifyChannel::Discord(discord.clone()));
                    }

                    if channels.is_empty() {
                        tracing::warn!(
                            "Alert #{} triggered but has no notification channels configured",
                            alert.id
                        );
                        continue;
                    }

                    let fired = AlertFired {
                        alert_id: alert.id,
                        item_name: alert.item_name.clone(),
                        item_id: alert.item_id,
                        condition: condition_kind,
                        price_kind: evaluated_kind,
                        old_value: Some(prev),
                        new_value: curr,
                        threshold: alert.threshold,
                        channels,
                    };

                    tracing::info!(
                        "Alert #{} FIRED for '{}' ({} -> {}, cond: {})",
                        alert.id,
                        alert.item_name,
                        prev,
                        curr,
                        condition_kind
                    );

                    {
                        let conn = db_conn.lock().await;
                        let _ = db::update_alert_last_fired(&conn, alert.id, now);
                        let _ = db::log_alert(
                            &conn,
                            &db::LogAlertParams {
                                alert_id: alert.id,
                                item_id: alert.item_id,
                                item_name: &alert.item_name,
                                price_kind: evaluated_kind.as_str(),
                                condition: condition_kind.as_str(),
                                old_price: Some(prev),
                                new_price: curr,
                                threshold: alert.threshold,
                                fired_at: now,
                            },
                        );
                    }

                    if let Err(e) = alert_tx.send(fired).await {
                        tracing::error!("Failed to dispatch AlertFired to notifier: {}", e);
                    }

                    fired_items.insert(alert.item_id);

                    // only trigger once if either matched
                    break;
                }
            }
        }

        for (item_id, price_item) in tick.prices {
            let entry = prev_prices.entry(item_id).or_default();
            if let Some(h) = price_item.high {
                entry.high = Some(h);
            }
            if let Some(l) = price_item.low {
                entry.low = Some(l);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_evaluate_price_up() {
        assert!(evaluate(ConditionKind::PriceUp, 100, 105, None));
        assert!(!evaluate(ConditionKind::PriceUp, 100, 100, None));
        assert!(!evaluate(ConditionKind::PriceUp, 100, 95, None));
    }

    #[test]
    fn test_evaluate_price_down() {
        assert!(evaluate(ConditionKind::PriceDown, 100, 95, None));
        assert!(!evaluate(ConditionKind::PriceDown, 100, 100, None));
        assert!(!evaluate(ConditionKind::PriceDown, 100, 105, None));
    }

    #[test]
    fn test_evaluate_price_changed() {
        assert!(evaluate(ConditionKind::PriceChanged, 100, 105, None));
        assert!(evaluate(ConditionKind::PriceChanged, 100, 95, None));
        assert!(!evaluate(ConditionKind::PriceChanged, 100, 100, None));
    }

    #[test]
    fn test_evaluate_crossed_up() {
        assert!(evaluate(ConditionKind::CrossedUp, 95, 100, Some(100)));
        assert!(evaluate(ConditionKind::CrossedUp, 95, 105, Some(100)));
        assert!(!evaluate(ConditionKind::CrossedUp, 100, 105, Some(100)));
        assert!(!evaluate(ConditionKind::CrossedUp, 80, 90, Some(100)));
    }

    #[test]
    fn test_evaluate_crossed_down() {
        assert!(evaluate(ConditionKind::CrossedDown, 105, 100, Some(100)));
        assert!(evaluate(ConditionKind::CrossedDown, 105, 95, Some(100)));
        assert!(!evaluate(ConditionKind::CrossedDown, 100, 95, Some(100)));
        assert!(!evaluate(ConditionKind::CrossedDown, 120, 110, Some(100)));
    }

    #[test]
    fn test_evaluate_crossed_any() {
        assert!(evaluate(ConditionKind::CrossedAny, 95, 105, Some(100)));
        assert!(evaluate(ConditionKind::CrossedAny, 105, 95, Some(100)));
        assert!(!evaluate(ConditionKind::CrossedAny, 100, 105, Some(100)));
    }
}
