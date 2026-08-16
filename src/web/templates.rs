use crate::db::{AlertLogEntry, AlertRecord, AppSettings, DashboardStats, ItemInfo, WatchedItem};
use crate::notifier::format_number;
use crate::poller::{PollerStatus, PriceTick};
use chrono::DateTime;

const HEADER_ART: &str = r##"
<pre class="ascii-banner">
╔══════════════════════════════════════════════════════════════╗
║           ██████╗ ███████╗       ███╗  ██╗ ██████╗           ║
║          ██╔════╝ ██╔════╝       ████╗ ██║██╔═══██╗          ║
║          ██║  ███╗█████╗         ██╔██╗██║██║   ██║          ║
║          ██║   ██║██╔══╝         ██║╚████║██║   ██║          ║
║          ╚██████╔╝███████╗       ██║ ╚███║╚██████╔╝          ║
║           ╚═════╝ ╚══════╝       ╚═╝  ╚══╝ ╚═════╝           ║
╚══════════════════════════════════════════════════════════════╝</pre>
"##;

const STYLES: &str = include_str!("style.css");
const AUTOCOMPLETE_JS: &str = include_str!("autocomplete.js");

#[derive(Debug, Clone, Default)]
pub struct AlertFormDraft {
    pub item_id: Option<u32>,
    pub item_name: Option<String>,
    pub price_kind: String,
    pub condition_kind: String,
    pub threshold: Option<i64>,
    pub discord_webhook: Option<String>,
    pub cooldown_secs: u64,
    pub enabled: bool,
}

impl AlertFormDraft {
    pub fn from_record(alert: &AlertRecord) -> Self {
        Self {
            item_id: Some(alert.item_id),
            item_name: Some(alert.item_name.clone()),
            price_kind: alert.price_kind.clone(),
            condition_kind: alert.condition_kind.clone(),
            threshold: alert.threshold,
            discord_webhook: alert.discord_webhook.clone(),
            cooldown_secs: alert.cooldown_secs,
            enabled: alert.enabled,
        }
    }

    pub fn from_item(id: u32, name: &str, default_cooldown: u64) -> Self {
        Self {
            item_id: Some(id),
            item_name: Some(name.to_string()),
            price_kind: "buy".to_string(),
            condition_kind: "crossed_up".to_string(),
            threshold: None,
            discord_webhook: None,
            cooldown_secs: default_cooldown,
            enabled: true,
        }
    }
}

pub fn page(title: &str, body: &str, active_nav: &str) -> String {
    let nav_dash = if active_nav == "dashboard" {
        "class=\"active\""
    } else {
        ""
    };
    let nav_items = if active_nav == "items" {
        "class=\"active\""
    } else {
        ""
    };
    let nav_alerts = if active_nav == "alerts" {
        "class=\"active\""
    } else {
        ""
    };
    let nav_new_alert = if active_nav == "new_alert" {
        "class=\"active\""
    } else {
        ""
    };
    let nav_settings = if active_nav == "settings" {
        "class=\"active\""
    } else {
        ""
    };

    format!(
        "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n  <title>{} — GE Notifier</title>\n  <style>{}</style>\n</head>\n<body>\n{}\n<nav>\n  <a href=\"/\" {}>[ Dashboard ]</a>\n  <a href=\"/items\" {}>[ Watched Items ]</a>\n  <a href=\"/alerts\" {}>[ Alert Rules ]</a>\n  <a href=\"/alerts/new\" {}>[ + New Alert ]</a>\n  <a href=\"/settings\" {}>[ Options ]</a>\n</nav>\n<main>\n{}\n</main>\n<footer class=\"footer\">\n  GE Notifier • Real-Time OSRS Grand Exchange Price Notifier • <a href=\"https://prices.runescape.wiki/osrs\" target=\"_blank\" rel=\"noopener\">OSRS Wiki API</a>\n</footer>\n</body>\n</html>",
        title,
        STYLES,
        HEADER_ART,
        nav_dash,
        nav_items,
        nav_alerts,
        nav_new_alert,
        nav_settings,
        body
    )
}

pub fn dashboard_view(
    stats: &DashboardStats,
    poller_status: &PollerStatus,
    recent_logs: &[AlertLogEntry],
) -> String {
    let last_poll_str = poller_status
        .last_poll_at
        .map(|ts| {
            if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            } else {
                "Unknown".to_string()
            }
        })
        .unwrap_or_else(|| "Waiting for first poll...".to_string());

    let error_html = if let Some(ref err) = poller_status.last_error {
        format!(
            "<div class=\"alert-msg error\"><strong>Poller Warning:</strong> {}</div>",
            html_escape(err)
        )
    } else {
        String::new()
    };

    let mut logs_html = String::new();
    if recent_logs.is_empty() {
        logs_html.push_str("<tr><td colspan=\"6\" style=\"text-align:center; color:var(--text-dim);\">No alerts fired yet.</td></tr>");
    } else {
        for log in recent_logs {
            let time_str = if let Some(dt) = DateTime::from_timestamp(log.fired_at, 0) {
                dt.format("%H:%M:%S").to_string()
            } else {
                "-".to_string()
            };

            let badge_class = match log.price_kind.to_lowercase().as_str() {
                "buy" => "badge-buy",
                "sell" => "badge-sell",
                _ => "badge-either",
            };

            let thresh_str = log
                .threshold
                .map(|p| format!("{} gp", format_number(p)))
                .unwrap_or_else(|| "-".to_string());

            let item_link = if log.item_id > 0 {
                format!(
                    "<a href=\"/items/{}\"><strong>{}</strong></a>",
                    log.item_id,
                    html_escape(&log.item_name)
                )
            } else {
                format!("<strong>{}</strong>", html_escape(&log.item_name))
            };

            logs_html.push_str(&format!(
                "<tr>\n  <td>{}</td>\n  <td>{}</td>\n  <td><span class=\"badge {}\">{}</span></td>\n  <td>{}</td>\n  <td>{}</td>\n  <td style=\"color:var(--accent); font-weight:bold;\">{} gp</td>\n</tr>\n",
                time_str,
                item_link,
                badge_class,
                log.price_kind.to_uppercase(),
                html_escape(&log.condition),
                thresh_str,
                format_number(log.new_price)
            ));
        }
    }

    format!(
        r##"{error_html}
<div class="grid">
  <div class="box">
    <div class="box-title">┌─ System Status ────────────────────────────────────┐</div>
    <table>
      <tr><td>Watched Items</td><td><strong>{watched_count}</strong></td></tr>
      <tr><td>Active Alerts</td><td><strong>{active_alerts}</strong></td></tr>
      <tr><td>Last Poll</td><td>{last_poll}</td></tr>
      <tr><td>Items Tracked</td><td>{items_count} items</td></tr>
      <tr><td>Poll Cadence</td><td>Every {interval_secs}s</td></tr>
    </table>
    <div style="margin-top: 12px;">
      <a href="/items" class="btn">Manage Items</a>
      <a href="/alerts/new" class="btn">+ Add Alert</a>
      <a href="/settings" class="btn">Options</a>
    </div>
  </div>

  <div class="box">
    <div class="box-title">┌─ Quick Item Lookup ───────────────────────────────┐</div>
    <p style="margin-bottom: 10px; color: var(--text-dim);">Search OSRS items to directly view prices, metadata, and history:</p>
    <div class="autocomplete-wrapper">
      <input type="text" id="quick-search" placeholder="Type item name (e.g. Abyssal whip, Dragon bones)..." autocomplete="off">
      <div id="quick-dropdown" class="autocomplete-dropdown"></div>
    </div>
  </div>
</div>

<div class="box">
  <div class="box-title">┌─ Recent Alerts Triggered (Last 10) ───────────────┐</div>
  <table>
    <thead>
      <tr>
        <th>Time</th>
        <th>Item</th>
        <th>Type</th>
        <th>Condition</th>
        <th>Threshold</th>
        <th>New Price</th>
      </tr>
    </thead>
    <tbody>
      {logs_html}
    </tbody>
  </table>
</div>

<script>
{autocomplete_js}
initAutocomplete('quick-search', 'quick-dropdown', function(item) {{
  window.location.href = '/items/' + item.id;
}});
</script>"##,
        error_html = error_html,
        watched_count = stats.watched_items_count,
        active_alerts = stats.active_alerts_count,
        last_poll = last_poll_str,
        items_count = poller_status.last_poll_items_count,
        interval_secs = poller_status.interval_secs,
        logs_html = logs_html,
        autocomplete_js = AUTOCOMPLETE_JS,
    )
}

pub fn items_view(items: &[WatchedItem], prices: Option<&PriceTick>, msg: Option<&str>) -> String {
    let msg_html = if let Some(m) = msg {
        format!("<div class=\"alert-msg info\">{}</div>", html_escape(m))
    } else {
        String::new()
    };

    let mut rows_html = String::new();
    if items.is_empty() {
        rows_html.push_str("<tr><td colspan=\"6\" style=\"text-align:center; color:var(--text-dim);\">No watched items. Search and add an item below.</td></tr>");
    } else {
        for item in items {
            let added_str = if let Some(dt) = DateTime::from_timestamp(item.added_at, 0) {
                dt.format("%Y-%m-%d %H:%M").to_string()
            } else {
                "-".to_string()
            };

            let (buy_html, sell_html) = if let Some(tick) = prices {
                if let Some(ip) = tick.prices.get(&item.item_id) {
                    let buy = ip.high
                        .map(|v| {
                            let time = ip.high_time
                                .and_then(|t| DateTime::from_timestamp(t, 0))
                                .map(|dt| dt.format("%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| "-".to_string());
                            format!(
                                "<strong style=\"color:var(--accent);\">{} gp</strong><br><small style=\"color:var(--text-dim);\">{}</small>",
                                format_number(v), time
                            )
                        })
                        .unwrap_or_else(|| "<span style=\"color:var(--text-dim);\">-</span>".to_string());
                    let sell = ip.low
                        .map(|v| {
                            let time = ip.low_time
                                .and_then(|t| DateTime::from_timestamp(t, 0))
                                .map(|dt| dt.format("%m-%d %H:%M").to_string())
                                .unwrap_or_else(|| "-".to_string());
                            format!(
                                "<strong style=\"color:var(--accent);\">{} gp</strong><br><small style=\"color:var(--text-dim);\">{}</small>",
                                format_number(v), time
                            )
                        })
                        .unwrap_or_else(|| "<span style=\"color:var(--text-dim);\">-</span>".to_string());
                    (buy, sell)
                } else {
                    (
                        "<span style=\"color:var(--text-dim);\">-</span>".to_string(),
                        "<span style=\"color:var(--text-dim);\">-</span>".to_string(),
                    )
                }
            } else {
                (
                    "<span style=\"color:var(--text-dim);\">Waiting...</span>".to_string(),
                    "<span style=\"color:var(--text-dim);\">Waiting...</span>".to_string(),
                )
            };

            let wiki_slug = urlencoding_encode(&item.item_name.replace(' ', "_"));

            rows_html.push_str(&format!(
                "<tr>\n  <td><a href=\"/items/{}\" style=\"font-size:15px;\">{}<strong>{}</strong></a></td>\n  <td>{}</td>\n  <td>{}</td>\n  <td>{}</td>\n  <td>\n    <a href=\"/items/{}\" class=\"btn\">Details</a>\n    <a href=\"/alerts/new?item_id={}&item_name={}\" class=\"btn\">+ Alert</a>\n    <a href=\"https://prices.runescape.wiki/osrs/item/{}\" target=\"_blank\" rel=\"noopener noreferrer\" class=\"btn\" title=\"View live prices on OSRS Wiki\">Prices ↗</a>\n    <a href=\"https://oldschool.runescape.wiki/w/{}\" target=\"_blank\" rel=\"noopener noreferrer\" class=\"btn\" title=\"View OSRS Wiki page\">Wiki ↗</a>\n    <form method=\"POST\" action=\"/items/{}/delete\" style=\"display:inline-block;\" onsubmit=\"return confirm('Remove {} from watched items?');\">\n      <button type=\"submit\" class=\"btn btn-danger\">Remove</button>\n    </form>\n  </td>\n</tr>\n",
                item.item_id,
                item_icon_html(&item.icon, item.item_id, "item-icon"),
                html_escape(&item.item_name),
                buy_html,
                sell_html,
                added_str,
                item.item_id,
                item.item_id,
                urlencoding_encode(&item.item_name),
                item.item_id,
                wiki_slug,
                item.item_id,
                html_escape(&item.item_name)
            ));
        }
    }

    format!(
        r##"{msg_html}
<div class="box">
  <div class="box-title">┌─ Add Item to Watchlist ───────────────────────────┐</div>
  <p style="margin-bottom: 12px; color: var(--text-dim);">Search OSRS Wiki item catalog (type at least 2 characters):</p>
  <div class="grid" style="align-items: flex-start;">
    <div class="autocomplete-wrapper">
      <input type="text" id="item-search" placeholder="Type item name (e.g. Abyssal whip, Zulrah's scales)..." autocomplete="off">
      <div id="item-dropdown" class="autocomplete-dropdown"></div>
    </div>
    <form method="POST" action="/items" id="add-item-form" style="display: flex; gap: 8px;">
      <input type="hidden" name="item_id" id="selected-item-id" required>
      <input type="text" id="selected-item-name" placeholder="Selected item..." readonly style="color: var(--accent); font-weight: bold;">
      <button type="submit" class="btn" id="add-item-btn" disabled>+ Add to Watchlist</button>
    </form>
  </div>
</div>

<div class="box">
  <div class="box-title">┌─ Watched Items ({count}) ─────────────────────────────┐</div>
  <p style="margin-bottom: 10px; color: var(--text-dim);">Click on any item name to view its detailed metadata and price change history.</p>
  <table>
    <thead>
      <tr>
        <th>Item</th>
        <th style="width: 140px;">Buy Price</th>
        <th style="width: 140px;">Sell Price</th>
        <th style="width: 130px;">Added At</th>
        <th style="width: 350px;">Actions</th>
      </tr>
    </thead>
    <tbody>
      {rows_html}
    </tbody>
  </table>
</div>

<script>
{autocomplete_js}
initAutocomplete('item-search', 'item-dropdown', function(item) {{
  document.getElementById('selected-item-id').value = item.id;
  document.getElementById('selected-item-name').value = item.name + ' (#' + item.id + ')';
  document.getElementById('add-item-btn').disabled = false;
}});
</script>"##,
        msg_html = msg_html,
        count = items.len(),
        rows_html = rows_html,
        autocomplete_js = AUTOCOMPLETE_JS,
    )
}

pub fn item_detail_view(
    item: &ItemInfo,
    is_watched: bool,
    alerts: &[AlertRecord],
    logs: &[AlertLogEntry],
    msg: Option<&str>,
) -> String {
    let msg_html = if let Some(m) = msg {
        format!("<div class=\"alert-msg info\">{}</div>", html_escape(m))
    } else {
        String::new()
    };

    let members_badge = if item.members {
        "<span class=\"badge badge-members\">MEMBERS</span>"
    } else {
        "<span class=\"badge\">FREE TO PLAY</span>"
    };

    let examine_text = item
        .examine
        .as_deref()
        .unwrap_or("No examine information available.");
    let ge_limit_str = item
        .ge_limit
        .map(|l| format_number(l as i64))
        .unwrap_or_else(|| "None / Unknown".to_string());
    let highalch_str = item
        .highalch
        .map(|a| format!("{} gp", format_number(a)))
        .unwrap_or_else(|| "N/A".to_string());
    let lowalch_str = item
        .lowalch
        .map(|a| format!("{} gp", format_number(a)))
        .unwrap_or_else(|| "N/A".to_string());

    let watch_action_btn = if is_watched {
        format!(
            "<form method=\"POST\" action=\"/items/{}/delete\" style=\"display:inline-block;\">\n  <button type=\"submit\" class=\"btn btn-danger\">Remove from Watchlist</button>\n</form>",
            item.id
        )
    } else {
        format!(
            "<form method=\"POST\" action=\"/items\" style=\"display:inline-block;\">\n  <input type=\"hidden\" name=\"item_id\" value=\"{}\">\n  <button type=\"submit\" class=\"btn\">+ Add to Watchlist</button>\n</form>",
            item.id
        )
    };

    let mut alerts_html = String::new();
    if alerts.is_empty() {
        alerts_html.push_str("<tr><td colspan=\"6\" style=\"text-align:center; color:var(--text-dim);\">No alert rules configured for this item yet.</td></tr>");
    } else {
        for a in alerts {
            let badge_class = match a.price_kind.to_lowercase().as_str() {
                "buy" => "badge-buy",
                "sell" => "badge-sell",
                _ => "badge-either",
            };
            let status_badge = if a.enabled {
                "<span class=\"badge badge-buy\">ACTIVE</span>"
            } else {
                "<span class=\"badge badge-disabled\">DISABLED</span>"
            };
            let thresh_str = a
                .threshold
                .map(|t| format!("{} gp", format_number(t)))
                .unwrap_or_else(|| "-".to_string());

            alerts_html.push_str(&format!(
                "<tr>\n  <td><span class=\"badge {}\">{}</span></td>\n  <td>{}</td>\n  <td style=\"color:var(--accent);\">{}</td>\n  <td>{}s</td>\n  <td>{}</td>\n  <td>\n    <a href=\"/alerts/{}/edit\" class=\"btn\">Edit</a>\n    <form method=\"POST\" action=\"/alerts/{}/delete\" style=\"display:inline-block;\" onsubmit=\"return confirm('Delete alert rule?');\">\n      <button type=\"submit\" class=\"btn btn-danger\">Del</button>\n    </form>\n  </td>\n</tr>\n",
                badge_class,
                a.price_kind.to_uppercase(),
                html_escape(&a.condition_kind),
                thresh_str,
                a.cooldown_secs,
                status_badge,
                a.id,
                a.id
            ));
        }
    }

    let mut logs_html = String::new();
    if logs.is_empty() {
        logs_html.push_str("<tr><td colspan=\"7\" style=\"text-align:center; color:var(--text-dim); padding: 16px;\">No price change alerts have been triggered for this item yet.</td></tr>");
    } else {
        for log in logs {
            let time_str = if let Some(dt) = DateTime::from_timestamp(log.fired_at, 0) {
                dt.format("%Y-%m-%d %H:%M:%S UTC").to_string()
            } else {
                "-".to_string()
            };

            let badge_class = match log.price_kind.to_lowercase().as_str() {
                "buy" => "badge-buy",
                "sell" => "badge-sell",
                _ => "badge-either",
            };

            let old_price_str = log
                .old_price
                .map(|p| format!("{} gp", format_number(p)))
                .unwrap_or_else(|| "N/A".to_string());

            let thresh_str = log
                .threshold
                .map(|p| format!("{} gp", format_number(p)))
                .unwrap_or_else(|| "-".to_string());

            let delta_html = if let Some(old) = log.old_price {
                let diff = log.new_price - old;
                if diff > 0 {
                    format!(
                        "<span class=\"delta-up\">+{} gp</span>",
                        format_number(diff)
                    )
                } else if diff < 0 {
                    format!(
                        "<span class=\"delta-down\">-{} gp</span>",
                        format_number(diff.abs())
                    )
                } else {
                    "<span class=\"delta-neutral\">0 gp</span>".to_string()
                }
            } else {
                "<span class=\"delta-neutral\">-</span>".to_string()
            };

            logs_html.push_str(&format!(
                "<tr>\n  <td><code>{}</code></td>\n  <td><span class=\"badge {}\">{}</span></td>\n  <td>{}</td>\n  <td>{}</td>\n  <td style=\"color:var(--accent); font-weight:bold;\">{} gp</td>\n  <td>{}</td>\n  <td>{}</td>\n</tr>\n",
                time_str,
                badge_class,
                log.price_kind.to_uppercase(),
                html_escape(&log.condition),
                old_price_str,
                format_number(log.new_price),
                delta_html,
                thresh_str
            ));
        }
    }

    format!(
        r##"{msg_html}
<div class="box">
  <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px; border-bottom: 1px dashed var(--border-dim); padding-bottom: 8px;">
    <div>
      <h2 style="color: var(--accent); margin-bottom: 4px;">{item_icon}{item_name} <span style="color: var(--text-dim); font-size: 16px;">(#{item_id})</span></h2>
      <p style="color: var(--text); font-style: italic;">"{examine}"</p>
    </div>
    <div>
      {members_badge}
    </div>
  </div>

  <div class="grid" style="margin-bottom: 16px;">
    <table>
      <tr><td>Item ID</td><td><strong>#{item_id}</strong></td></tr>
      <tr><td>Members Only</td><td>{members_badge}</td></tr>
      <tr><td>GE Buy Limit</td><td><strong>{ge_limit}</strong></td></tr>
    </table>
    <table>
      <tr><td>High Alchemy</td><td style="color: var(--accent);"><strong>{highalch}</strong></td></tr>
      <tr><td>Low Alchemy</td><td>{lowalch}</td></tr>
      <tr><td>Watchlist Status</td><td><strong>{watch_status}</strong></td></tr>
      <tr><td>External Links</td><td><a href="https://prices.runescape.wiki/osrs/item/{item_id}" target="_blank" rel="noopener noreferrer">Wiki Prices ↗</a> &bull; <a href="https://oldschool.runescape.wiki/w/{wiki_slug}" target="_blank" rel="noopener noreferrer">OSRS Wiki ↗</a></td></tr>
    </table>
  </div>

  <div style="display: flex; gap: 8px; flex-wrap: wrap;">
    <a href="/alerts/new?item_id={item_id}&item_name={encoded_name}" class="btn" style="background-color: var(--accent); color: #000; font-weight: bold;">+ Create Alert Rule</a>
    {watch_action_btn}
    <a href="https://prices.runescape.wiki/osrs/item/{item_id}" target="_blank" rel="noopener noreferrer" class="btn">Wiki Prices ↗</a>
    <a href="https://oldschool.runescape.wiki/w/{wiki_slug}" target="_blank" rel="noopener noreferrer" class="btn">OSRS Wiki ↗</a>
    <a href="/items" class="btn">Back to Watched Items</a>
  </div>
</div>

<div class="box">
  <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 8px;">
    <div class="box-title" style="margin-bottom: 0;">┌─ Active Alert Rules for this Item ({alert_count}) ───────┐</div>
    <a href="/alerts/new?item_id={item_id}&item_name={encoded_name}" class="btn">+ Add Rule</a>
  </div>
  <table>
    <thead>
      <tr>
        <th>Type</th>
        <th>Condition</th>
        <th>Threshold</th>
        <th>Cooldown</th>
        <th>Status</th>
        <th style="width: 140px;">Actions</th>
      </tr>
    </thead>
    <tbody>
      {alerts_html}
    </tbody>
  </table>
</div>

<div class="box">
  <div class="box-title">┌─ Chronological Alerted Price Changes History ({log_count}) ──┐</div>
  <p style="color: var(--text-dim); margin-bottom: 10px;">Full timestamped history of alerts triggered for {item_name}:</p>
  <table>
    <thead>
      <tr>
        <th>Timestamp (UTC)</th>
        <th>Type</th>
        <th>Triggered Condition</th>
        <th>Old Price</th>
        <th>New Price</th>
        <th>Movement</th>
        <th>Threshold</th>
      </tr>
    </thead>
    <tbody>
      {logs_html}
    </tbody>
  </table>
</div>"##,
        msg_html = msg_html,
        item_icon = item_icon_html(&item.icon, item.id, "item-icon-large"),
        item_name = html_escape(&item.name),
        item_id = item.id,
        examine = html_escape(examine_text),
        members_badge = members_badge,
        ge_limit = ge_limit_str,
        highalch = highalch_str,
        lowalch = lowalch_str,
        watch_status = if is_watched {
            "<span class=\"badge badge-buy\">WATCHED</span>"
        } else {
            "<span class=\"badge\">NOT WATCHED</span>"
        },
        watch_action_btn = watch_action_btn,
        encoded_name = urlencoding_encode(&item.name),
        wiki_slug = urlencoding_encode(&item.name.replace(' ', "_")),
        alert_count = alerts.len(),
        alerts_html = alerts_html,
        log_count = logs.len(),
        logs_html = logs_html,
    )
}

pub fn alerts_view(alerts: &[AlertRecord], msg: Option<&str>) -> String {
    let msg_html = if let Some(m) = msg {
        format!("<div class=\"alert-msg info\">{}</div>", html_escape(m))
    } else {
        String::new()
    };

    let mut rows_html = String::new();
    if alerts.is_empty() {
        rows_html.push_str("<tr><td colspan=\"7\" style=\"text-align:center; color:var(--text-dim);\">No alert rules configured. Click '+ New Alert' to create one.</td></tr>");
    } else {
        for alert in alerts {
            let badge_class = match alert.price_kind.to_lowercase().as_str() {
                "buy" => "badge-buy",
                "sell" => "badge-sell",
                _ => "badge-either",
            };

            let condition_label = match alert.condition_kind.as_str() {
                "price_up" => "Price went up",
                "price_down" => "Price went down",
                "price_changed" => "Price changed",
                "crossed_up" => "Crossed ↑",
                "crossed_down" => "Crossed ↓",
                "crossed_any" => "Crossed ⇅",
                other => other,
            };

            let thresh_str = alert
                .threshold
                .map(|t| format!("{} gp", format_number(t)))
                .unwrap_or_else(|| "-".to_string());

            let targets_str = if alert.discord_webhook.is_some() {
                "Discord (override)".to_string()
            } else {
                "<span style=\"color:var(--text-dim);\">Global Defaults</span>".to_string()
            };

            let status_badge = if alert.enabled {
                "<span class=\"badge badge-buy\">ACTIVE</span>"
            } else {
                "<span class=\"badge badge-disabled\">DISABLED</span>"
            };

            let last_fired_str = alert
                .last_fired_at
                .map(|ts| {
                    if let Some(dt) = DateTime::from_timestamp(ts, 0) {
                        dt.format("%m-%d %H:%M").to_string()
                    } else {
                        "-".to_string()
                    }
                })
                .unwrap_or_else(|| "Never".to_string());

            rows_html.push_str(&format!(
                "<tr>\n  <td><a href=\"/items/{}\">{}<strong>{}</strong></a></td>\n  <td><span class=\"badge {}\">{}</span></td>\n  <td>{}</td>\n  <td style=\"color:var(--accent);\">{}</td>\n  <td>{}</td>\n  <td>{} <br><small style=\"color:var(--text-dim);\">Fired: {}</small></td>\n  <td>\n    <a href=\"/alerts/{}/edit\" class=\"btn\">Edit</a>\n    <form method=\"POST\" action=\"/alerts/{}/delete\" style=\"display:inline-block;\" onsubmit=\"return confirm('Delete this alert rule?');\">\n      <button type=\"submit\" class=\"btn btn-danger\">Del</button>\n    </form>\n  </td>\n</tr>\n",
                alert.item_id,
                item_icon_html(&alert.icon, alert.item_id, "item-icon"),
                html_escape(&alert.item_name),
                badge_class,
                alert.price_kind.to_uppercase(),
                condition_label,
                thresh_str,
                targets_str,
                status_badge,
                last_fired_str,
                alert.id,
                alert.id
            ));
        }
    }

    format!(
        r##"{msg_html}
<div class="box">
  <div style="display: flex; justify-content: space-between; align-items: center; margin-bottom: 12px;">
    <div class="box-title" style="margin-bottom: 0;">┌─ Alert Rules ({count}) ─────────────────────────────┐</div>
    <a href="/alerts/new" class="btn">+ New Alert Rule</a>
  </div>
  <table>
    <thead>
      <tr>
        <th>Item</th>
        <th>Type</th>
        <th>Condition</th>
        <th>Threshold</th>
        <th>Channels</th>
        <th>Status</th>
        <th style="width: 140px;">Actions</th>
      </tr>
    </thead>
    <tbody>
      {rows_html}
    </tbody>
  </table>
</div>"##,
        msg_html = msg_html,
        count = alerts.len(),
        rows_html = rows_html,
    )
}

pub fn settings_view(settings: &AppSettings, msg: Option<&str>, is_error: bool) -> String {
    let msg_html = if let Some(m) = msg {
        let cls = if is_error {
            "alert-msg error"
        } else {
            "alert-msg success"
        };
        format!("<div class=\"{}\">{}</div>", cls, html_escape(m))
    } else {
        String::new()
    };

    let discord_val = settings.discord_webhook.as_deref().unwrap_or("");
    let cooldown_val = settings.default_cooldown_secs;

    format!(
        r##"{msg_html}
<div class="box" style="max-width: 750px; margin: 0 auto 24px auto;">
  <div class="box-title">┌─ General Notification & System Options ────────────┐</div>
  <p style="color: var(--text-dim); margin-bottom: 16px;">
    Configure global notification endpoints. All alert rules without specific overrides will automatically route to these targets.
  </p>

  <form method="POST" action="/settings" id="settings-form">
    <div class="form-group">
      <label>1. Global Discord Webhook URL:</label>
      <input type="text" name="discord_webhook" placeholder="https://discord.com/api/webhooks/..." value="{discord_val}">
      <small style="color: var(--text-dim);">When configured, price alerts are delivered as rich GE gold embeds to this Discord channel.</small>
    </div>

    <div class="form-group">
      <label>2. Default Alert Cooldown (seconds):</label>
      <input type="number" name="default_cooldown_secs" value="{cooldown_val}" min="0">
      <small style="color: var(--text-dim);">Fallback cooldown for alert rules (3600 = 1 hour, 1800 = 30 minutes, 60 = 1 minute).</small>
    </div>

    <div style="margin-top: 20px; display: flex; gap: 12px; align-items: center;">
      <button type="submit" class="btn" style="background-color: var(--accent); color: #000; font-weight: bold;">Save Options</button>
      <a href="/" class="btn">Cancel</a>
    </div>
  </form>
</div>

<div class="box" style="max-width: 750px; margin: 0 auto;">
  <div class="box-title">┌─ Notification Test Dispatch ────────────────────────┐</div>
  <p style="color: var(--text-dim); margin-bottom: 12px;">
    Send a test notification event to the currently saved Discord webhook:
  </p>
  <form method="POST" action="/settings/test">
    <button type="submit" class="btn btn-secondary">Send Test Notification</button>
  </form>
</div>"##,
        msg_html = msg_html,
        discord_val = html_escape(discord_val),
        cooldown_val = cooldown_val,
    )
}

pub fn alert_form(
    action: &str,
    title: &str,
    draft: Option<&AlertFormDraft>,
    error: Option<&str>,
) -> String {
    let error_html = if let Some(e) = error {
        format!("<div class=\"alert-msg error\">{}</div>", html_escape(e))
    } else {
        String::new()
    };

    let item_id_val = draft
        .and_then(|d| d.item_id)
        .map(|id| id.to_string())
        .unwrap_or_default();
    let item_name_val = draft
        .and_then(|d| d.item_name.as_deref())
        .unwrap_or_default();
    let price_kind = draft.map(|d| d.price_kind.as_str()).unwrap_or("buy");
    let condition_kind = draft
        .map(|d| d.condition_kind.as_str())
        .unwrap_or("crossed_up");
    let threshold_val = draft
        .and_then(|d| d.threshold)
        .map(|t| t.to_string())
        .unwrap_or_default();
    let discord_val = draft
        .and_then(|d| d.discord_webhook.as_deref())
        .unwrap_or("");
    let cooldown_val = draft.map(|d| d.cooldown_secs).unwrap_or(3600);
    let enabled_checked = if draft.map(|d| d.enabled).unwrap_or(true) {
        "checked"
    } else {
        ""
    };

    let buy_checked = if price_kind == "buy" { "checked" } else { "" };
    let sell_checked = if price_kind == "sell" { "checked" } else { "" };
    let either_checked = if price_kind == "either" {
        "checked"
    } else {
        ""
    };

    let cond_price_up = if condition_kind == "price_up" {
        "checked"
    } else {
        ""
    };
    let cond_price_down = if condition_kind == "price_down" {
        "checked"
    } else {
        ""
    };
    let cond_price_changed = if condition_kind == "price_changed" {
        "checked"
    } else {
        ""
    };
    let cond_crossed_up = if condition_kind == "crossed_up" {
        "checked"
    } else {
        ""
    };
    let cond_crossed_down = if condition_kind == "crossed_down" {
        "checked"
    } else {
        ""
    };
    let cond_crossed_any = if condition_kind == "crossed_any" {
        "checked"
    } else {
        ""
    };

    format!(
        r##"{error_html}
<div class="box" style="max-width: 700px; margin: 0 auto;">
  <div class="box-title">┌─ {title} ──────────────────────────────────────┐</div>
  <form method="POST" action="{action}" id="alert-form">
    <div class="form-group">
      <label>1. Select Item:</label>
      <div class="autocomplete-wrapper">
        <input type="text" id="alert-item-search" placeholder="Type item name to search..." value="{item_name_val}" autocomplete="off">
        <div id="alert-item-dropdown" class="autocomplete-dropdown"></div>
      </div>
      <input type="hidden" name="item_id" id="alert-item-id" value="{item_id_val}" required>
      <small style="color: var(--text-dim);">Selected Item ID: <span id="item-id-display">{display_id}</span></small>
    </div>

    <div class="form-group">
      <label>2. Price Type to Monitor:</label>
      <div class="radio-group">
        <label class="radio-item"><input type="radio" name="price_kind" value="buy" {buy_checked}> <strong>Buy Price</strong> (Instant-buy / High)</label>
        <label class="radio-item"><input type="radio" name="price_kind" value="sell" {sell_checked}> <strong>Sell Price</strong> (Instant-sell / Low)</label>
        <label class="radio-item"><input type="radio" name="price_kind" value="either" {either_checked}> <strong>Either</strong> (Triggers on both)</label>
      </div>
    </div>

    <div class="form-group">
      <label>3. Trigger Condition:</label>
      <div class="radio-group" style="flex-direction: column; gap: 8px;">
        <label class="radio-item"><input type="radio" name="condition_kind" value="crossed_up" {cond_crossed_up} onchange="toggleThreshold()"> <strong>Crossed threshold ↑</strong> (Price crosses above threshold)</label>
        <label class="radio-item"><input type="radio" name="condition_kind" value="crossed_down" {cond_crossed_down} onchange="toggleThreshold()"> <strong>Crossed threshold ↓</strong> (Price crosses below threshold)</label>
        <label class="radio-item"><input type="radio" name="condition_kind" value="crossed_any" {cond_crossed_any} onchange="toggleThreshold()"> <strong>Crossed threshold ⇅</strong> (Price crosses in either direction)</label>
        <label class="radio-item"><input type="radio" name="condition_kind" value="price_up" {cond_price_up} onchange="toggleThreshold()"> <strong>Price went up</strong> (Any upward movement)</label>
        <label class="radio-item"><input type="radio" name="condition_kind" value="price_down" {cond_price_down} onchange="toggleThreshold()"> <strong>Price went down</strong> (Any downward movement)</label>
        <label class="radio-item"><input type="radio" name="condition_kind" value="price_changed" {cond_price_changed} onchange="toggleThreshold()"> <strong>Price changed</strong> (Any movement in either direction)</label>
      </div>
    </div>

    <div class="form-group" id="threshold-container">
      <label>4. Threshold Price (gp):</label>
      <input type="number" name="threshold" id="threshold-input" placeholder="e.g. 2000000" value="{threshold_val}">
      <small style="color: var(--text-dim);">Required for crossed threshold conditions.</small>
    </div>

    <div class="form-group">
      <label>5. Specific Notification Targets (optional override):</label>
      <p style="color: var(--text-dim); font-size: 12px; margin-bottom: 8px;">Leave blank to use the global targets configured on the <a href="/settings">Options</a> page.</p>
      <div style="margin-bottom: 8px;">
        <label style="font-size: 12px; color: var(--text);">Discord Webhook URL (override):</label>
        <input type="text" name="discord_webhook" placeholder="https://discord.com/api/webhooks/... (optional)" value="{discord_val}">
      </div>
    </div>

    <div class="form-group">
      <label>6. Cooldown (seconds):</label>
      <input type="number" name="cooldown_secs" value="{cooldown_val}" min="0">
      <small style="color: var(--text-dim);">Prevents alert spam (3600 = 1 hour, 60 = 1 minute).</small>
    </div>

    <div class="form-group">
      <label class="radio-item">
        <input type="checkbox" name="enabled" value="true" {enabled_checked}>
        <strong>Alert Rule Enabled</strong>
      </label>
    </div>

    <div style="margin-top: 20px; display: flex; gap: 12px;">
      <button type="submit" class="btn" style="background-color: var(--accent); color: #000; font-weight: bold;">Save Alert Rule</button>
      <a href="/alerts" class="btn">Cancel</a>
    </div>
  </form>
</div>

<script>
{autocomplete_js}
initAutocomplete('alert-item-search', 'alert-item-dropdown', function(item) {{
  document.getElementById('alert-item-id').value = item.id;
  document.getElementById('alert-item-search').value = item.name;
  document.getElementById('item-id-display').textContent = '#' + item.id;
}});

function toggleThreshold() {{
  const selected = document.querySelector('input[name="condition_kind"]:checked');
  const container = document.getElementById('threshold-container');
  if (selected && selected.value.startsWith('crossed_')) {{
    container.style.display = 'block';
  }} else {{
    container.style.display = 'none';
  }}
}}
toggleThreshold();
</script>"##,
        error_html = error_html,
        title = title,
        action = action,
        item_name_val = html_escape(item_name_val),
        item_id_val = item_id_val,
        display_id = if item_id_val.is_empty() {
            "None".to_string()
        } else {
            format!("#{}", item_id_val)
        },
        buy_checked = buy_checked,
        sell_checked = sell_checked,
        either_checked = either_checked,
        cond_crossed_up = cond_crossed_up,
        cond_crossed_down = cond_crossed_down,
        cond_crossed_any = cond_crossed_any,
        cond_price_up = cond_price_up,
        cond_price_down = cond_price_down,
        cond_price_changed = cond_price_changed,
        threshold_val = threshold_val,
        discord_val = html_escape(discord_val),
        cooldown_val = cooldown_val,
        enabled_checked = enabled_checked,
        autocomplete_js = AUTOCOMPLETE_JS,
    )
}

pub fn item_icon_html(icon: &Option<String>, item_id: u32, css_class: &str) -> String {
    match icon {
        Some(filename) if !filename.is_empty() => {
            format!(
                "<img src=\"/icons/{}\" class=\"{}\" alt=\"\">",
                item_id, css_class
            )
        }
        _ => String::new(),
    }
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#039;")
}

pub fn urlencoding_encode(s: &str) -> String {
    let mut encoded = String::new();
    for b in s.bytes() {
        match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(b as char);
            }
            _ => {
                encoded.push_str(&format!("%{:02X}", b));
            }
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_html_escape() {
        assert_eq!(
            html_escape("<script>alert('XSS');</script>"),
            "&lt;script&gt;alert(&#039;XSS&#039;);&lt;/script&gt;"
        );
        assert_eq!(html_escape("a & b \"c\""), "a &amp; b &quot;c&quot;");
    }

    #[test]
    fn test_urlencoding_encode() {
        assert_eq!(urlencoding_encode("Abyssal whip"), "Abyssal%20whip");
        assert_eq!(urlencoding_encode("Zulrah's scales"), "Zulrah%27s%20scales");
    }
}
