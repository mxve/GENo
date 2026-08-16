use crate::api::MappingItem;
use chrono::Utc;
use rusqlite::{Connection, OptionalExtension, Result, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemSearchResult {
    pub id: u32,
    pub name: String,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemInfo {
    pub id: u32,
    pub name: String,
    pub examine: Option<String>,
    pub members: bool,
    pub lowalch: Option<i64>,
    pub highalch: Option<i64>,
    pub ge_limit: Option<i32>,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatchedItem {
    pub id: i64,
    pub item_id: u32,
    pub item_name: String,
    pub added_at: i64,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRecord {
    pub id: i64,
    pub item_id: u32,
    pub item_name: String,
    pub price_kind: String,
    pub condition_kind: String,
    pub threshold: Option<i64>,
    pub discord_webhook: Option<String>,
    pub cooldown_secs: u64,
    pub last_fired_at: Option<i64>,
    pub enabled: bool,
    pub created_at: i64,
    pub icon: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertLogEntry {
    pub id: i64,
    pub alert_id: i64,
    pub item_id: u32,
    pub item_name: String,
    pub price_kind: String,
    pub condition: String,
    pub old_price: Option<i64>,
    pub new_price: i64,
    pub threshold: Option<i64>,
    pub fired_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub discord_webhook: Option<String>,
    pub default_cooldown_secs: u64,
    pub updated_at: i64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            discord_webhook: None,
            default_cooldown_secs: 3600,
            updated_at: Utc::now().timestamp(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardStats {
    pub watched_items_count: usize,
    pub active_alerts_count: usize,
}

#[derive(Debug, Clone)]
pub struct UpdateAlertParams<'a> {
    pub id: i64,
    pub price_kind: &'a str,
    pub condition_kind: &'a str,
    pub threshold: Option<i64>,
    pub discord_webhook: Option<&'a str>,
    pub cooldown_secs: u64,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct LogAlertParams<'a> {
    pub alert_id: i64,
    pub item_id: u32,
    pub item_name: &'a str,
    pub price_kind: &'a str,
    pub condition: &'a str,
    pub old_price: Option<i64>,
    pub new_price: i64,
    pub threshold: Option<i64>,
    pub fired_at: i64,
}

pub fn open_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA busy_timeout = 5000;
         PRAGMA synchronous = NORMAL;",
    )?;
    init_schema(&conn)?;
    Ok(conn)
}

pub fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS item_mapping (
            id          INTEGER PRIMARY KEY,
            name        TEXT    NOT NULL,
            examine     TEXT,
            members     INTEGER NOT NULL,
            lowalch     INTEGER,
            highalch    INTEGER,
            ge_limit    INTEGER,
            icon        TEXT,
            updated_at  INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_item_name ON item_mapping(name COLLATE NOCASE);

        CREATE TABLE IF NOT EXISTS watched_items (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id     INTEGER NOT NULL UNIQUE,
            added_at    INTEGER NOT NULL
        );

        CREATE TABLE IF NOT EXISTS alerts (
            id              INTEGER PRIMARY KEY AUTOINCREMENT,
            item_id         INTEGER NOT NULL,
            price_kind      TEXT NOT NULL,
            condition_kind  TEXT NOT NULL,
            threshold       INTEGER,
            discord_webhook TEXT,
            cooldown_secs   INTEGER NOT NULL DEFAULT 3600,
            last_fired_at   INTEGER,
            enabled         INTEGER NOT NULL DEFAULT 1,
            created_at      INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_alerts_item ON alerts(item_id);

        CREATE TABLE IF NOT EXISTS alert_log (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            alert_id    INTEGER NOT NULL,
            item_id     INTEGER NOT NULL DEFAULT 0,
            item_name   TEXT    NOT NULL,
            price_kind  TEXT    NOT NULL,
            condition   TEXT    NOT NULL,
            old_price   INTEGER,
            new_price   INTEGER NOT NULL,
            threshold   INTEGER,
            fired_at    INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_alert_log_item ON alert_log(item_id);
        CREATE INDEX IF NOT EXISTS idx_alert_log_fired ON alert_log(fired_at DESC);

        CREATE TABLE IF NOT EXISTS app_settings (
            id                    INTEGER PRIMARY KEY CHECK (id = 1),
            discord_webhook       TEXT,
            default_cooldown_secs INTEGER NOT NULL DEFAULT 3600,
            updated_at            INTEGER NOT NULL
        );

        INSERT OR IGNORE INTO app_settings (id, default_cooldown_secs, updated_at) VALUES (1, 3600, 0);",
    )?;

    let _ = conn.execute(
        "ALTER TABLE alert_log ADD COLUMN item_id INTEGER NOT NULL DEFAULT 0",
        [],
    );

    Ok(())
}

pub fn get_app_settings(conn: &Connection) -> Result<AppSettings> {
    conn.query_row(
        "SELECT discord_webhook, default_cooldown_secs, updated_at
         FROM app_settings WHERE id = 1",
        [],
        |row| {
            let cooldown_int: i64 = row.get(1)?;
            Ok(AppSettings {
                discord_webhook: row.get(0)?,
                default_cooldown_secs: cooldown_int.max(0) as u64,
                updated_at: row.get(2)?,
            })
        },
    )
    .optional()
    .map(|opt| opt.unwrap_or_default())
}

pub fn save_app_settings(conn: &Connection, settings: &AppSettings) -> Result<()> {
    let now = Utc::now().timestamp();
    conn.execute(
        "INSERT INTO app_settings (id, discord_webhook, default_cooldown_secs, updated_at)
         VALUES (1, ?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET
             discord_webhook = excluded.discord_webhook,
             default_cooldown_secs = excluded.default_cooldown_secs,
             updated_at = excluded.updated_at",
        params![
            settings.discord_webhook,
            settings.default_cooldown_secs as i64,
            now,
        ],
    )?;
    Ok(())
}

pub fn is_mapping_stale(conn: &Connection) -> Result<bool> {
    let now = Utc::now().timestamp();
    let row: Option<(i64, Option<i64>)> = conn
        .query_row(
            "SELECT COUNT(*), MAX(updated_at) FROM item_mapping",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;

    match row {
        Some((count, Some(max_updated))) if count > 0 => Ok(now - max_updated > 86_400),
        _ => Ok(true),
    }
}

pub fn upsert_mapping(conn: &mut Connection, items: &[MappingItem]) -> Result<()> {
    let now = Utc::now().timestamp();
    let tx = conn.transaction()?;
    {
        let mut stmt = tx.prepare_cached(
            "INSERT INTO item_mapping (id, name, examine, members, lowalch, highalch, ge_limit, icon, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                 name = excluded.name,
                 examine = excluded.examine,
                 members = excluded.members,
                 lowalch = excluded.lowalch,
                 highalch = excluded.highalch,
                 ge_limit = excluded.ge_limit,
                 icon = excluded.icon,
                 updated_at = excluded.updated_at",
        )?;

        for item in items {
            stmt.execute(params![
                item.id,
                item.name,
                item.examine,
                if item.members { 1 } else { 0 },
                item.lowalch,
                item.highalch,
                item.limit,
                item.icon,
                now,
            ])?;
        }
    }
    tx.commit()?;
    Ok(())
}

// escape like wildcards so user search strings match literally
fn escape_sql_wildcards(query: &str) -> String {
    let mut escaped = String::with_capacity(query.len() + 8);
    for c in query.chars() {
        if c == '%' || c == '_' || c == '\\' {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

pub fn search_items(conn: &Connection, query: &str, limit: usize) -> Result<Vec<ItemSearchResult>> {
    let clean = query.trim();
    if clean.is_empty() {
        return Ok(Vec::new());
    }

    let escaped_query = escape_sql_wildcards(clean);
    let mut stmt = conn.prepare_cached(
        "SELECT id, name, icon FROM item_mapping
         WHERE name LIKE ?1 ESCAPE '\\' COLLATE NOCASE
         ORDER BY
             CASE WHEN name LIKE ?2 ESCAPE '\\' COLLATE NOCASE THEN 0 ELSE 1 END,
             LENGTH(name) ASC,
             name ASC
         LIMIT ?3",
    )?;

    let prefix_pattern = format!("{}%", escaped_query);
    let anywhere_pattern = format!("%{}%", escaped_query);

    stmt.query_map(
        params![anywhere_pattern, prefix_pattern, limit as i64],
        |row| {
            Ok(ItemSearchResult {
                id: row.get(0)?,
                name: row.get(1)?,
                icon: row.get(2)?,
            })
        },
    )?
    .collect()
}

pub fn get_item_by_id(conn: &Connection, item_id: u32) -> Result<Option<ItemInfo>> {
    conn.query_row(
        "SELECT id, name, examine, members, lowalch, highalch, ge_limit, icon
         FROM item_mapping WHERE id = ?1",
        params![item_id],
        |row| {
            let members_int: i64 = row.get(3)?;
            Ok(ItemInfo {
                id: row.get(0)?,
                name: row.get(1)?,
                examine: row.get(2)?,
                members: members_int == 1,
                lowalch: row.get(4)?,
                highalch: row.get(5)?,
                ge_limit: row.get(6)?,
                icon: row.get(7)?,
            })
        },
    )
    .optional()
}

pub fn is_item_watched(conn: &Connection, item_id: u32) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM watched_items WHERE item_id = ?1",
        params![item_id],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

pub fn get_watched_items(conn: &Connection) -> Result<Vec<WatchedItem>> {
    let mut stmt = conn.prepare_cached(
        "SELECT w.id, w.item_id, COALESCE(m.name, 'Unknown Item #' || w.item_id), w.added_at, m.icon
         FROM watched_items w
         LEFT JOIN item_mapping m ON w.item_id = m.id
         ORDER BY w.added_at DESC",
    )?;

    stmt.query_map([], |row| {
        Ok(WatchedItem {
            id: row.get(0)?,
            item_id: row.get(1)?,
            item_name: row.get(2)?,
            added_at: row.get(3)?,
            icon: row.get(4)?,
        })
    })?
    .collect()
}

pub fn add_watched_item(conn: &Connection, item_id: u32) -> Result<()> {
    let now = Utc::now().timestamp();
    conn.execute(
        "INSERT OR IGNORE INTO watched_items (item_id, added_at) VALUES (?1, ?2)",
        params![item_id, now],
    )?;
    Ok(())
}

pub fn remove_watched_item(conn: &Connection, item_id: u32) -> Result<()> {
    conn.execute(
        "DELETE FROM watched_items WHERE item_id = ?1",
        params![item_id],
    )?;
    Ok(())
}

fn row_to_alert_record(row: &rusqlite::Row) -> Result<AlertRecord> {
    let cooldown_int: i64 = row.get(7)?;
    let enabled_int: i64 = row.get(9)?;
    Ok(AlertRecord {
        id: row.get(0)?,
        item_id: row.get(1)?,
        item_name: row.get(2)?,
        price_kind: row.get(3)?,
        condition_kind: row.get(4)?,
        threshold: row.get(5)?,
        discord_webhook: row.get(6)?,
        cooldown_secs: cooldown_int.max(0) as u64,
        last_fired_at: row.get(8)?,
        enabled: enabled_int == 1,
        created_at: row.get(10)?,
        icon: row.get(11)?,
    })
}

pub fn get_alerts(conn: &Connection) -> Result<Vec<AlertRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT a.id, a.item_id, COALESCE(m.name, 'Item #' || a.item_id),
                a.price_kind, a.condition_kind, a.threshold,
                a.discord_webhook, a.cooldown_secs,
                a.last_fired_at, a.enabled, a.created_at, m.icon
             FROM alerts a
             LEFT JOIN item_mapping m ON a.item_id = m.id
             ORDER BY a.created_at DESC",
    )?;

    stmt.query_map([], row_to_alert_record)?.collect()
}

pub fn get_enabled_alerts(conn: &Connection) -> Result<Vec<AlertRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT a.id, a.item_id, COALESCE(m.name, 'Item #' || a.item_id),
                a.price_kind, a.condition_kind, a.threshold,
                a.discord_webhook, a.cooldown_secs,
                a.last_fired_at, a.enabled, a.created_at, m.icon
             FROM alerts a
             LEFT JOIN item_mapping m ON a.item_id = m.id
             WHERE a.enabled = 1
             ORDER BY a.created_at DESC",
    )?;

    stmt.query_map([], row_to_alert_record)?.collect()
}

pub fn get_alerts_for_item(conn: &Connection, item_id: u32) -> Result<Vec<AlertRecord>> {
    let mut stmt = conn.prepare_cached(
        "SELECT a.id, a.item_id, COALESCE(m.name, 'Item #' || a.item_id),
                a.price_kind, a.condition_kind, a.threshold,
                a.discord_webhook, a.cooldown_secs,
                a.last_fired_at, a.enabled, a.created_at, m.icon
             FROM alerts a
             LEFT JOIN item_mapping m ON a.item_id = m.id
             WHERE a.item_id = ?1
             ORDER BY a.created_at DESC",
    )?;

    stmt.query_map(params![item_id], row_to_alert_record)?
        .collect()
}

pub fn get_alert_by_id(conn: &Connection, alert_id: i64) -> Result<Option<AlertRecord>> {
    conn.query_row(
        "SELECT a.id, a.item_id, COALESCE(m.name, 'Item #' || a.item_id),
                a.price_kind, a.condition_kind, a.threshold,
                a.discord_webhook, a.cooldown_secs,
                a.last_fired_at, a.enabled, a.created_at, m.icon
             FROM alerts a
             LEFT JOIN item_mapping m ON a.item_id = m.id
             WHERE a.id = ?1",
        params![alert_id],
        row_to_alert_record,
    )
    .optional()
}

pub fn create_alert(
    conn: &Connection,
    item_id: u32,
    price_kind: &str,
    condition_kind: &str,
    threshold: Option<i64>,
    discord_webhook: Option<&str>,
    cooldown_secs: u64,
) -> Result<i64> {
    let now = Utc::now().timestamp();
    conn.execute(
        "INSERT INTO alerts (item_id, price_kind, condition_kind, threshold, discord_webhook, cooldown_secs, enabled, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7)",
        params![
            item_id,
            price_kind,
            condition_kind,
            threshold,
            discord_webhook,
            cooldown_secs as i64,
            now,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn update_alert(conn: &Connection, params: &UpdateAlertParams) -> Result<()> {
    conn.execute(
        "UPDATE alerts SET
            price_kind = ?1,
            condition_kind = ?2,
            threshold = ?3,
            discord_webhook = ?4,
            cooldown_secs = ?5,
            enabled = ?6
         WHERE id = ?7",
        params![
            params.price_kind,
            params.condition_kind,
            params.threshold,
            params.discord_webhook,
            params.cooldown_secs as i64,
            if params.enabled { 1 } else { 0 },
            params.id,
        ],
    )?;
    Ok(())
}

pub fn delete_alert(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM alerts WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn update_alert_last_fired(conn: &Connection, id: i64, fired_at: i64) -> Result<()> {
    conn.execute(
        "UPDATE alerts SET last_fired_at = ?1 WHERE id = ?2",
        params![fired_at, id],
    )?;
    Ok(())
}

fn row_to_alert_log_entry(row: &rusqlite::Row) -> Result<AlertLogEntry> {
    Ok(AlertLogEntry {
        id: row.get(0)?,
        alert_id: row.get(1)?,
        item_id: row.get(2)?,
        item_name: row.get(3)?,
        price_kind: row.get(4)?,
        condition: row.get(5)?,
        old_price: row.get(6)?,
        new_price: row.get(7)?,
        threshold: row.get(8)?,
        fired_at: row.get(9)?,
    })
}

pub fn log_alert(conn: &Connection, params: &LogAlertParams) -> Result<()> {
    conn.execute(
        "INSERT INTO alert_log (alert_id, item_id, item_name, price_kind, condition, old_price, new_price, threshold, fired_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![
            params.alert_id,
            params.item_id,
            params.item_name,
            params.price_kind,
            params.condition,
            params.old_price,
            params.new_price,
            params.threshold,
            params.fired_at
        ],
    )?;

    // keep log size reasonable
    conn.execute(
        "DELETE FROM alert_log WHERE id NOT IN (
            SELECT id FROM alert_log ORDER BY fired_at DESC, id DESC LIMIT 1000
        )",
        [],
    )?;
    Ok(())
}

pub fn get_recent_alert_logs(conn: &Connection, limit: usize) -> Result<Vec<AlertLogEntry>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, alert_id, item_id, item_name, price_kind, condition, old_price, new_price, threshold, fired_at
         FROM alert_log
         ORDER BY fired_at DESC, id DESC
         LIMIT ?1",
    )?;

    stmt.query_map(params![limit as i64], row_to_alert_log_entry)?
        .collect()
}

pub fn get_alert_logs_for_item(
    conn: &Connection,
    item_id: u32,
    limit: usize,
) -> Result<Vec<AlertLogEntry>> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, alert_id, item_id, item_name, price_kind, condition, old_price, new_price, threshold, fired_at
         FROM alert_log
         WHERE item_id = ?1
         ORDER BY fired_at DESC, id DESC
         LIMIT ?2",
    )?;

    stmt.query_map(params![item_id, limit as i64], row_to_alert_log_entry)?
        .collect()
}

pub fn get_dashboard_stats(conn: &Connection) -> Result<DashboardStats> {
    let watched_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM watched_items", [], |r| r.get(0))?;
    let alerts_count: i64 =
        conn.query_row("SELECT COUNT(*) FROM alerts WHERE enabled = 1", [], |r| {
            r.get(0)
        })?;

    Ok(DashboardStats {
        watched_items_count: watched_count as usize,
        active_alerts_count: alerts_count as usize,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_schema_and_mapping() {
        let mut conn = setup_test_db();
        assert!(is_mapping_stale(&conn).unwrap());

        let items = vec![
            MappingItem {
                id: 4151,
                name: "Abyssal whip".to_string(),
                examine: Some("A weapon from the abyss.".to_string()),
                members: true,
                lowalch: Some(72000),
                highalch: Some(120000),
                limit: Some(70),
                icon: Some("Abyssal whip.png".to_string()),
            },
            MappingItem {
                id: 536,
                name: "Dragon bones".to_string(),
                examine: Some("Bones of a dragon.".to_string()),
                members: true,
                lowalch: Some(144),
                highalch: Some(240),
                limit: Some(10000),
                icon: Some("Dragon bones.png".to_string()),
            },
        ];

        upsert_mapping(&mut conn, &items).unwrap();
        assert!(!is_mapping_stale(&conn).unwrap());

        let info = get_item_by_id(&conn, 4151).unwrap().unwrap();
        assert_eq!(info.name, "Abyssal whip");
        assert_eq!(info.highalch, Some(120000));
        assert!(info.members);

        let search = search_items(&conn, "whip", 5).unwrap();
        assert_eq!(search.len(), 1);
        assert_eq!(search[0].id, 4151);
        assert_eq!(search[0].name, "Abyssal whip");
    }

    #[test]
    fn test_search_wildcard_escaping() {
        let mut conn = setup_test_db();
        let items = vec![
            MappingItem {
                id: 1,
                name: "100% pure essence".to_string(),
                examine: None,
                members: false,
                lowalch: None,
                highalch: None,
                limit: None,
                icon: None,
            },
            MappingItem {
                id: 2,
                name: "100_percent item".to_string(),
                examine: None,
                members: false,
                lowalch: None,
                highalch: None,
                limit: None,
                icon: None,
            },
            MappingItem {
                id: 3,
                name: "10000 normal item".to_string(),
                examine: None,
                members: false,
                lowalch: None,
                highalch: None,
                limit: None,
                icon: None,
            },
        ];
        upsert_mapping(&mut conn, &items).unwrap();

        let percent_results = search_items(&conn, "100%", 10).unwrap();
        assert_eq!(percent_results.len(), 1);
        assert_eq!(percent_results[0].name, "100% pure essence");

        let underscore_results = search_items(&conn, "100_", 10).unwrap();
        assert_eq!(underscore_results.len(), 1);
        assert_eq!(underscore_results[0].name, "100_percent item");
    }

    #[test]
    fn test_watched_items_crud() {
        let conn = setup_test_db();
        add_watched_item(&conn, 4151).unwrap();
        add_watched_item(&conn, 4151).unwrap();
        add_watched_item(&conn, 536).unwrap();

        assert!(is_item_watched(&conn, 4151).unwrap());
        assert!(!is_item_watched(&conn, 9999).unwrap());

        let watched = get_watched_items(&conn).unwrap();
        assert_eq!(watched.len(), 2);

        remove_watched_item(&conn, 4151).unwrap();
        let watched_after = get_watched_items(&conn).unwrap();
        assert_eq!(watched_after.len(), 1);
        assert_eq!(watched_after[0].item_id, 536);
        assert!(!is_item_watched(&conn, 4151).unwrap());
    }

    #[test]
    fn test_alerts_crud_and_logs() {
        let conn = setup_test_db();

        let alert_id = create_alert(
            &conn,
            4151,
            "buy",
            "crossed_up",
            Some(2000000),
            Some("https://discord.webhook"),
            3600,
        )
        .unwrap();

        let alert = get_alert_by_id(&conn, alert_id).unwrap().unwrap();
        assert_eq!(alert.item_id, 4151);
        assert_eq!(alert.price_kind, "buy");
        assert_eq!(alert.condition_kind, "crossed_up");
        assert_eq!(alert.threshold, Some(2000000));
        assert!(alert.enabled);

        update_alert(
            &conn,
            &UpdateAlertParams {
                id: alert_id,
                price_kind: "sell",
                condition_kind: "crossed_down",
                threshold: Some(1800000),
                discord_webhook: None,
                cooldown_secs: 1800,
                enabled: false,
            },
        )
        .unwrap();

        let updated = get_alert_by_id(&conn, alert_id).unwrap().unwrap();
        assert_eq!(updated.price_kind, "sell");
        assert_eq!(updated.condition_kind, "crossed_down");
        assert_eq!(updated.threshold, Some(1800000));
        assert_eq!(updated.cooldown_secs, 1800);
        assert!(!updated.enabled);

        let item_alerts = get_alerts_for_item(&conn, 4151).unwrap();
        assert_eq!(item_alerts.len(), 1);

        log_alert(
            &conn,
            &LogAlertParams {
                alert_id,
                item_id: 4151,
                item_name: "Abyssal whip",
                price_kind: "sell",
                condition: "crossed_down",
                old_price: Some(1900000),
                new_price: 1750000,
                threshold: Some(1800000),
                fired_at: Utc::now().timestamp(),
            },
        )
        .unwrap();

        let recent = get_recent_alert_logs(&conn, 10).unwrap();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].item_id, 4151);
        assert_eq!(recent[0].item_name, "Abyssal whip");
        assert_eq!(recent[0].new_price, 1750000);

        let item_logs = get_alert_logs_for_item(&conn, 4151, 10).unwrap();
        assert_eq!(item_logs.len(), 1);
        assert_eq!(item_logs[0].alert_id, alert_id);

        delete_alert(&conn, alert_id).unwrap();
        assert!(get_alert_by_id(&conn, alert_id).unwrap().is_none());
    }

    #[test]
    fn test_app_settings_persistence() {
        let conn = setup_test_db();
        let default_settings = get_app_settings(&conn).unwrap();
        assert_eq!(default_settings.default_cooldown_secs, 3600);
        assert!(default_settings.discord_webhook.is_none());

        let new_settings = AppSettings {
            discord_webhook: Some("https://discord.com/api/webhooks/global".to_string()),
            default_cooldown_secs: 1800,
            updated_at: Utc::now().timestamp(),
        };

        save_app_settings(&conn, &new_settings).unwrap();

        let loaded = get_app_settings(&conn).unwrap();
        assert_eq!(
            loaded.discord_webhook,
            Some("https://discord.com/api/webhooks/global".to_string())
        );
        assert_eq!(loaded.default_cooldown_secs, 1800);
    }
}
