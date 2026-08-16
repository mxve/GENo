use crate::engine::{AlertFired, NotifyChannel};
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::mpsc;

pub fn format_number(val: i64) -> String {
    let s = val.abs().to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().rev().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }
    let formatted: String = result.chars().rev().collect();
    if val < 0 {
        format!("-{}", formatted)
    } else {
        formatted
    }
}

pub async fn send_discord(
    client: &Client,
    webhook_url: &str,
    fired: &AlertFired,
) -> Result<(), reqwest::Error> {
    let old_val_str = fired
        .old_value
        .map(format_number)
        .unwrap_or_else(|| "N/A".to_string());

    let threshold_str = fired
        .threshold
        .map(|v| format!("{} gp", format_number(v)))
        .unwrap_or_else(|| "None".to_string());

    let price_emoji = match fired.price_kind {
        crate::engine::PriceKind::Buy => "🟢",
        crate::engine::PriceKind::Sell => "🟠",
        crate::engine::PriceKind::Either => "🔵",
    };

    let title = format!(
        "{} {} {} gp → {} gp",
        price_emoji,
        fired.item_name,
        old_val_str,
        format_number(fired.new_value)
    );

    let payload = json!({
        "username": "GE Notifier",
        "embeds": [{
            "title": title,
            "url": format!("https://prices.runescape.wiki/osrs/item/{}", fired.item_id),
            "description": "Grand Exchange price condition met.",
            "color": 16726168,
            "fields": [
                { "name": "Price Type", "value": fired.price_kind.to_string(), "inline": true },
                { "name": "Condition", "value": fired.condition.to_string(), "inline": true },
                { "name": "Threshold", "value": threshold_str, "inline": true },
                { "name": "Old Price", "value": format!("{} gp", old_val_str), "inline": true },
                { "name": "New Price", "value": format!("{} gp", format_number(fired.new_value)), "inline": true }
            ],
            "footer": {
                "text": "GE Notifier • OSRS Wiki Prices"
            }
        }]
    });

    client
        .post(webhook_url)
        .json(&payload)
        .send()
        .await?
        .error_for_status()?;

    Ok(())
}

pub async fn send_test_notification(
    client: &Client,
    discord_webhook: Option<&str>,
) -> Option<Result<(), String>> {
    let fired = AlertFired {
        alert_id: 0,
        item_name: "Abyssal whip (Test Notification)".to_string(),
        item_id: 4151,
        condition: crate::engine::ConditionKind::CrossedUp,
        price_kind: crate::engine::PriceKind::Buy,
        old_value: Some(1980000),
        new_value: 2050000,
        threshold: Some(2000000),
        channels: vec![],
    };

    if let Some(url) = discord_webhook.filter(|s| !s.trim().is_empty()) {
        Some(
            send_discord(client, url, &fired)
                .await
                .map_err(|e| e.to_string()),
        )
    } else {
        None
    }
}

pub async fn run_notifier(client: Arc<Client>, mut alert_rx: mpsc::Receiver<AlertFired>) {
    tracing::info!("Notifier task started.");

    while let Some(fired) = alert_rx.recv().await {
        for channel in &fired.channels {
            match channel {
                NotifyChannel::Discord(webhook_url) => {
                    tracing::info!("Sending Discord alert for '{}' to webhook", fired.item_name);
                    if let Err(e) = send_discord(&client, webhook_url, &fired).await {
                        tracing::warn!("Failed to send Discord webhook: {}", e);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(123), "123");
        assert_eq!(format_number(1234), "1,234");
        assert_eq!(format_number(1234567), "1,234,567");
        assert_eq!(format_number(-987654), "-987,654");
    }
}
