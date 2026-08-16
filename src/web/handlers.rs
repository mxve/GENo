use crate::db::{self, AppSettings};
use crate::engine::ConditionKind;
use crate::notifier;
use crate::web::AppState;
use crate::web::templates;
use axum::{
    Form, Json,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::str::FromStr;

fn empty_string_as_none<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt.as_deref() {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => s
            .trim()
            .parse::<T>()
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn empty_string_as_none_str<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;
    match opt {
        None => Ok(None),
        Some(s) if s.trim().is_empty() => Ok(None),
        Some(s) => Ok(Some(s.trim().to_string())),
    }
}

#[derive(Deserialize)]
pub struct SearchQuery {
    #[serde(default)]
    pub q: String,
}

#[derive(Deserialize)]
pub struct NewAlertQuery {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub item_id: Option<u32>,
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    pub item_name: Option<String>,
}

#[derive(Deserialize, Default)]
pub struct StatusQuery {
    pub msg: Option<String>,
    pub err: Option<String>,
}

#[derive(Deserialize)]
pub struct AddItemForm {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub item_id: Option<u32>,
}

#[derive(Deserialize, Debug, Default)]
pub struct AlertFormData {
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub item_id: Option<u32>,
    #[serde(default)]
    pub price_kind: String,
    #[serde(default)]
    pub condition_kind: String,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub threshold: Option<i64>,
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    pub discord_webhook: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub cooldown_secs: Option<u64>,
    #[serde(default)]
    pub enabled: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
pub struct SettingsFormData {
    #[serde(default, deserialize_with = "empty_string_as_none_str")]
    pub discord_webhook: Option<String>,
    #[serde(default, deserialize_with = "empty_string_as_none")]
    pub default_cooldown_secs: Option<u64>,
}

pub async fn dashboard(State(state): State<AppState>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let stats = db::get_dashboard_stats(&conn).unwrap_or(db::DashboardStats {
        watched_items_count: 0,
        active_alerts_count: 0,
    });
    let recent_logs = db::get_recent_alert_logs(&conn, 10).unwrap_or_default();
    let poller_status = state.poller_status.lock().await.clone();

    let body = templates::dashboard_view(&stats, &poller_status, &recent_logs);
    Html(templates::page("Dashboard", &body, "dashboard"))
}

pub async fn search(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    if params.q.trim().is_empty() {
        return Json(Vec::<db::ItemSearchResult>::new());
    }

    let conn = state.db.lock().await;
    let results = db::search_items(&conn, &params.q, 10).unwrap_or_default();
    Json(results)
}

pub async fn items_list(
    State(state): State<AppState>,
    Query(status): Query<StatusQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let items = db::get_watched_items(&conn).unwrap_or_default();
    let prices = state.latest_prices.borrow().clone();
    let body = templates::items_view(
        &items,
        prices.as_ref(),
        status.msg.as_deref().or(status.err.as_deref()),
    );
    Html(templates::page("Watched Items", &body, "items"))
}

pub async fn item_detail(
    State(state): State<AppState>,
    Path(id): Path<u32>,
    Query(status): Query<StatusQuery>,
) -> Response {
    let conn = state.db.lock().await;
    match db::get_item_by_id(&conn, id) {
        Ok(Some(item)) => {
            let is_watched = db::is_item_watched(&conn, id).unwrap_or(false);
            let alerts = db::get_alerts_for_item(&conn, id).unwrap_or_default();
            let logs = db::get_alert_logs_for_item(&conn, id, 100).unwrap_or_default();
            let body = templates::item_detail_view(
                &item,
                is_watched,
                &alerts,
                &logs,
                status.msg.as_deref().or(status.err.as_deref()),
            );
            Html(templates::page(&item.name, &body, "items")).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Html(templates::page(
                "Item Not Found",
                "<div class=\"box\"><p>Item not found in OSRS database.</p><br><a href=\"/items\" class=\"btn\">Back to Watched Items</a></div>",
                "items",
            )),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch item #{}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(templates::page(
                    "Error",
                    "<div class=\"box\"><p>Database error loading item.</p></div>",
                    "items",
                )),
            )
                .into_response()
        }
    }
}

pub async fn items_add(
    State(state): State<AppState>,
    Form(form): Form<AddItemForm>,
) -> impl IntoResponse {
    if let Some(item_id) = form.item_id.filter(|&id| id > 0) {
        let conn = state.db.lock().await;
        if let Err(e) = db::add_watched_item(&conn, item_id) {
            tracing::error!("Failed to add watched item #{}: {}", item_id, e);
        }
    }
    Redirect::to("/items")
}

pub async fn items_delete(State(state): State<AppState>, Path(id): Path<u32>) -> impl IntoResponse {
    let conn = state.db.lock().await;
    if let Err(e) = db::remove_watched_item(&conn, id) {
        tracing::error!("Failed to remove watched item #{}: {}", id, e);
    }
    Redirect::to("/items")
}

pub async fn alerts_list(
    State(state): State<AppState>,
    Query(status): Query<StatusQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let alerts = db::get_alerts(&conn).unwrap_or_default();
    let body = templates::alerts_view(&alerts, status.msg.as_deref().or(status.err.as_deref()));
    Html(templates::page("Alert Rules", &body, "alerts"))
}

pub async fn alerts_new_form(
    State(state): State<AppState>,
    Query(query): Query<NewAlertQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let settings = db::get_app_settings(&conn).unwrap_or_default();

    let draft = match (query.item_id, query.item_name) {
        (Some(id), Some(name)) => Some(templates::AlertFormDraft::from_item(
            id,
            &name,
            settings.default_cooldown_secs,
        )),
        _ => None,
    };

    let body = templates::alert_form("/alerts", "Create New Alert Rule", draft.as_ref(), None);
    Html(templates::page("New Alert Rule", &body, "new_alert"))
}

pub async fn alerts_create(
    State(state): State<AppState>,
    Form(form): Form<AlertFormData>,
) -> Response {
    let (validation_err, app_settings) = {
        let conn = state.db.lock().await;
        let s = db::get_app_settings(&conn).unwrap_or_default();
        let err = validate_alert_form(&form, &s);
        (err, s)
    };

    let item_id = form.item_id.unwrap_or(0);
    if let Some(err) = validation_err {
        let conn = state.db.lock().await;
        let item_name = if item_id > 0 {
            db::get_item_by_id(&conn, item_id)
                .ok()
                .flatten()
                .map(|i| i.name)
        } else {
            None
        };
        let draft = templates::AlertFormDraft {
            item_id: if item_id > 0 { Some(item_id) } else { None },
            item_name,
            price_kind: if form.price_kind.is_empty() {
                "buy".to_string()
            } else {
                form.price_kind.clone()
            },
            condition_kind: if form.condition_kind.is_empty() {
                "crossed_up".to_string()
            } else {
                form.condition_kind.clone()
            },
            threshold: form.threshold,
            discord_webhook: form.discord_webhook.clone(),
            cooldown_secs: form
                .cooldown_secs
                .unwrap_or(app_settings.default_cooldown_secs),
            enabled: form.enabled.as_deref() == Some("true")
                || form.enabled.as_deref() == Some("on")
                || form.enabled.is_none(),
        };
        let body =
            templates::alert_form("/alerts", "Create New Alert Rule", Some(&draft), Some(&err));
        return (
            StatusCode::BAD_REQUEST,
            Html(templates::page("New Alert Rule", &body, "new_alert")),
        )
            .into_response();
    }

    let discord = form
        .discord_webhook
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    let cooldown = form
        .cooldown_secs
        .unwrap_or(app_settings.default_cooldown_secs);

    let conn = state.db.lock().await;
    match db::create_alert(
        &conn,
        item_id,
        &form.price_kind,
        &form.condition_kind,
        form.threshold,
        discord,
        cooldown,
    ) {
        Ok(_) => {
            // auto watch item when creating an alert for it
            let _ = db::add_watched_item(&conn, item_id);
            Redirect::to("/alerts").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to create alert: {}", e);
            let draft = templates::AlertFormDraft {
                item_id: if item_id > 0 { Some(item_id) } else { None },
                item_name: None,
                price_kind: form.price_kind.clone(),
                condition_kind: form.condition_kind.clone(),
                threshold: form.threshold,
                discord_webhook: form.discord_webhook.clone(),
                cooldown_secs: cooldown,
                enabled: true,
            };
            let body = templates::alert_form(
                "/alerts",
                "Create New Alert Rule",
                Some(&draft),
                Some("Database error while creating alert."),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(templates::page("New Alert Rule", &body, "new_alert")),
            )
                .into_response()
        }
    }
}

pub async fn alerts_edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> Response {
    let conn = state.db.lock().await;
    match db::get_alert_by_id(&conn, id) {
        Ok(Some(alert)) => {
            let action_url = format!("/alerts/{}/edit", id);
            let draft = templates::AlertFormDraft::from_record(&alert);
            let body = templates::alert_form(&action_url, "Edit Alert Rule", Some(&draft), None);
            Html(templates::page("Edit Alert Rule", &body, "alerts")).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Html(templates::page(
                "Not Found",
                "<div class=\"box\"><p>Alert not found.</p></div>",
                "alerts",
            )),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to fetch alert #{}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(templates::page(
                    "Error",
                    "<div class=\"box\"><p>Database error.</p></div>",
                    "alerts",
                )),
            )
                .into_response()
        }
    }
}

pub async fn alerts_edit(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<AlertFormData>,
) -> Response {
    let (validation_err, app_settings) = {
        let conn = state.db.lock().await;
        let s = db::get_app_settings(&conn).unwrap_or_default();
        let err = validate_alert_form(&form, &s);
        (err, s)
    };

    let item_id = form.item_id.unwrap_or(0);
    if let Some(err) = validation_err {
        let action_url = format!("/alerts/{}/edit", id);
        let conn = state.db.lock().await;
        let item_name = if item_id > 0 {
            db::get_item_by_id(&conn, item_id)
                .ok()
                .flatten()
                .map(|i| i.name)
        } else {
            None
        };
        let draft = templates::AlertFormDraft {
            item_id: if item_id > 0 { Some(item_id) } else { None },
            item_name,
            price_kind: if form.price_kind.is_empty() {
                "buy".to_string()
            } else {
                form.price_kind.clone()
            },
            condition_kind: if form.condition_kind.is_empty() {
                "crossed_up".to_string()
            } else {
                form.condition_kind.clone()
            },
            threshold: form.threshold,
            discord_webhook: form.discord_webhook.clone(),
            cooldown_secs: form
                .cooldown_secs
                .unwrap_or(app_settings.default_cooldown_secs),
            enabled: form.enabled.as_deref() == Some("true")
                || form.enabled.as_deref() == Some("on"),
        };
        let body = templates::alert_form(&action_url, "Edit Alert Rule", Some(&draft), Some(&err));
        return (
            StatusCode::BAD_REQUEST,
            Html(templates::page("Edit Alert Rule", &body, "alerts")),
        )
            .into_response();
    }

    let discord = form
        .discord_webhook
        .as_deref()
        .filter(|s| !s.trim().is_empty());
    let cooldown = form
        .cooldown_secs
        .unwrap_or(app_settings.default_cooldown_secs);
    let enabled = form.enabled.as_deref() == Some("true") || form.enabled.as_deref() == Some("on");

    let conn = state.db.lock().await;
    let update_params = db::UpdateAlertParams {
        id,
        price_kind: &form.price_kind,
        condition_kind: &form.condition_kind,
        threshold: form.threshold,
        discord_webhook: discord,
        cooldown_secs: cooldown,
        enabled,
    };
    match db::update_alert(&conn, &update_params) {
        Ok(_) => Redirect::to("/alerts").into_response(),
        Err(e) => {
            tracing::error!("Failed to update alert #{}: {}", id, e);
            let action_url = format!("/alerts/{}/edit", id);
            let draft = templates::AlertFormDraft {
                item_id: if item_id > 0 { Some(item_id) } else { None },
                item_name: None,
                price_kind: form.price_kind.clone(),
                condition_kind: form.condition_kind.clone(),
                threshold: form.threshold,
                discord_webhook: form.discord_webhook.clone(),
                cooldown_secs: cooldown,
                enabled,
            };
            let body = templates::alert_form(
                &action_url,
                "Edit Alert Rule",
                Some(&draft),
                Some("Database error while updating alert."),
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(templates::page("Edit Alert Rule", &body, "alerts")),
            )
                .into_response()
        }
    }
}

pub async fn alerts_delete(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    if let Err(e) = db::delete_alert(&conn, id) {
        tracing::error!("Failed to delete alert #{}: {}", id, e);
    }
    Redirect::to("/alerts")
}

pub async fn settings_view(
    State(state): State<AppState>,
    Query(query): Query<StatusQuery>,
) -> impl IntoResponse {
    let conn = state.db.lock().await;
    let settings = db::get_app_settings(&conn).unwrap_or_default();
    let is_err = query.err.is_some();
    let msg = query.err.as_deref().or(query.msg.as_deref());

    let body = templates::settings_view(&settings, msg, is_err);
    Html(templates::page("Options", &body, "settings"))
}

pub async fn settings_save(
    State(state): State<AppState>,
    Form(form): Form<SettingsFormData>,
) -> impl IntoResponse {
    let settings = AppSettings {
        discord_webhook: form.discord_webhook,
        default_cooldown_secs: form.default_cooldown_secs.unwrap_or(3600),
        updated_at: chrono::Utc::now().timestamp(),
    };

    let conn = state.db.lock().await;
    match db::save_app_settings(&conn, &settings) {
        Ok(_) => Redirect::to("/settings?msg=Options+saved+successfully.").into_response(),
        Err(e) => {
            tracing::error!("Failed to save options: {}", e);
            Redirect::to("/settings?err=Database+error+saving+options.").into_response()
        }
    }
}

pub async fn settings_test(State(state): State<AppState>) -> impl IntoResponse {
    let settings = {
        let conn = state.db.lock().await;
        db::get_app_settings(&conn).unwrap_or_default()
    };

    let discord_opt = settings.discord_webhook.as_deref();

    if discord_opt.is_none() {
        return Redirect::to(
            "/settings?err=No+notification+targets+configured.+Please+set+a+Discord+webhook+URL.",
        )
        .into_response();
    }

    let discord_res = notifier::send_test_notification(&state.http_client, discord_opt).await;

    if let Some(res) = discord_res {
        match res {
            Ok(_) => Redirect::to("/settings?msg=Discord+test+sent+successfully.").into_response(),
            Err(e) => {
                let encoded = templates::urlencoding_encode(&format!("Discord test failed: {}", e));
                Redirect::to(&format!("/settings?err={}", encoded)).into_response()
            }
        }
    } else {
        Redirect::to("/settings?err=No+Discord+webhook+configured.").into_response()
    }
}

// fetch item icon from wiki and cache on disk
pub async fn icon_proxy(State(state): State<AppState>, Path(id): Path<u32>) -> Response {
    let icons_dir = std::path::Path::new("icons");
    let cached_path = icons_dir.join(format!("{}.png", id));

    if cached_path.exists() {
        match tokio::fs::read(&cached_path).await {
            Ok(bytes) => {
                return (
                    StatusCode::OK,
                    [
                        (header::CONTENT_TYPE, "image/png"),
                        (header::CACHE_CONTROL, "public, max-age=604800"),
                    ],
                    bytes,
                )
                    .into_response();
            }
            Err(e) => {
                tracing::warn!("Failed to read cached icon for item #{}: {}", id, e);
            }
        }
    }

    let icon_filename = {
        let conn = state.db.lock().await;
        match db::get_item_by_id(&conn, id) {
            Ok(Some(item)) => item.icon,
            _ => None,
        }
    };

    let filename = match icon_filename {
        Some(f) if !f.is_empty() => f,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };

    let url_name = filename.replace(' ', "_");
    let url = format!("https://oldschool.runescape.wiki/images/{}", url_name);

    let response = match state.http_client.get(&url).send().await {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to fetch icon for item #{}: {}", id, e);
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    if !response.status().is_success() {
        tracing::warn!(
            "Wiki returned {} for icon '{}' (item #{})",
            response.status(),
            filename,
            id
        );
        return StatusCode::NOT_FOUND.into_response();
    }

    let bytes = match response.bytes().await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("Failed to read icon response for item #{}: {}", id, e);
            return StatusCode::BAD_GATEWAY.into_response();
        }
    };

    if let Err(e) = tokio::fs::create_dir_all(icons_dir).await {
        tracing::warn!("Failed to create icons directory: {}", e);
    } else if let Err(e) = tokio::fs::write(&cached_path, &bytes).await {
        tracing::warn!("Failed to cache icon for item #{}: {}", id, e);
    }

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/png"),
            (header::CACHE_CONTROL, "public, max-age=604800"),
        ],
        bytes.to_vec(),
    )
        .into_response()
}

fn validate_alert_form(form: &AlertFormData, app_settings: &AppSettings) -> Option<String> {
    match form.item_id {
        Some(id) if id > 0 => {}
        _ => return Some("Please select a valid item from the search autocomplete.".to_string()),
    }

    let condition = match ConditionKind::from_str(&form.condition_kind) {
        Ok(c) => c,
        Err(_) => return Some("Invalid condition selected.".to_string()),
    };

    if condition.requires_threshold() {
        match form.threshold {
            Some(t) if t > 0 => {}
            _ => return Some(
                "A positive threshold price in gp is required for crossed threshold conditions."
                    .to_string(),
            ),
        }
    }

    let has_discord = form
        .discord_webhook
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false)
        || app_settings
            .discord_webhook
            .as_ref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);

    if !has_discord {
        return Some(
            "No notification target found. Please enter a Discord Webhook URL or configure a global default in the Options page."
                .to_string(),
        );
    }

    None
}
