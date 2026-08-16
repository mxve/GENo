use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use ge_notifier::{
    api::{self, MappingItem},
    config::Config,
    db::{self, AppSettings},
    poller::PollerStatus,
    web::{self, AppState},
};
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::Mutex;
use tower::ServiceExt;

fn setup_test_app() -> (axum::Router, Arc<Mutex<Connection>>) {
    let mut mapping_conn = Connection::open_in_memory().unwrap();
    db::init_schema(&mapping_conn).unwrap();
    let sample_items = vec![
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
    db::upsert_mapping(&mut mapping_conn, &sample_items).unwrap();

    let db_conn = Arc::new(Mutex::new(mapping_conn));
    let config = Arc::new(Config::default());
    let client = Arc::new(api::build_client(&config.poller.user_agent));
    let poller_status = Arc::new(Mutex::new(PollerStatus::new(60)));
    let (_tick_tx, tick_rx) = tokio::sync::watch::channel(None);
    let state = AppState {
        db: Arc::clone(&db_conn),
        http_client: client,
        config,
        poller_status,
        latest_prices: tick_rx,
    };

    (web::router(state), db_conn)
}

#[tokio::test]
async fn test_dashboard_endpoint() {
    let (app, _) = setup_test_app();

    let response = app
        .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);
    assert!(body_str.contains("Grand Exchange Price Notifier"));
    assert!(body_str.contains("System Status"));
}

#[tokio::test]
async fn test_search_endpoint() {
    let (app, _) = setup_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/search?q=whip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let results: Vec<db::ItemSearchResult> = serde_json::from_slice(&body).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, 4151);
    assert_eq!(results[0].name, "Abyssal whip");
}

#[tokio::test]
async fn test_items_and_alerts_flow() {
    let (app, db_conn) = setup_test_app();

    let add_item_req = Request::builder()
        .method("POST")
        .uri("/items")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from("item_id=4151"))
        .unwrap();

    let add_res = app.clone().oneshot(add_item_req).await.unwrap();
    assert_eq!(add_res.status(), StatusCode::SEE_OTHER);

    {
        let conn = db_conn.lock().await;
        let watched = db::get_watched_items(&conn).unwrap();
        assert_eq!(watched.len(), 1);
        assert_eq!(watched[0].item_id, 4151);
    }

    let add_alert_req = Request::builder()
        .method("POST")
        .uri("/alerts")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "item_id=4151&price_kind=buy&condition_kind=crossed_up&threshold=2000000&discord_webhook=https://discord.com/api/webhooks/test&cooldown_secs=1800",
        ))
        .unwrap();

    let alert_res = app.clone().oneshot(add_alert_req).await.unwrap();
    assert_eq!(alert_res.status(), StatusCode::SEE_OTHER);

    let alert_id = {
        let conn = db_conn.lock().await;
        let alerts = db::get_alerts(&conn).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].item_id, 4151);
        assert_eq!(alerts[0].condition_kind, "crossed_up");
        assert_eq!(alerts[0].threshold, Some(2000000));
        assert_eq!(alerts[0].cooldown_secs, 1800);
        alerts[0].id
    };

    let edit_get_req = Request::builder()
        .uri(format!("/alerts/{}/edit", alert_id))
        .body(Body::empty())
        .unwrap();
    let edit_get_res = app.clone().oneshot(edit_get_req).await.unwrap();
    assert_eq!(edit_get_res.status(), StatusCode::OK);

    let edit_post_req = Request::builder()
        .method("POST")
        .uri(format!("/alerts/{}/edit", alert_id))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "item_id=4151&price_kind=sell&condition_kind=crossed_down&threshold=1500000&discord_webhook=https://discord.com/api/webhooks/test&cooldown_secs=7200&enabled=true",
        ))
        .unwrap();
    let edit_post_res = app.clone().oneshot(edit_post_req).await.unwrap();
    assert_eq!(edit_post_res.status(), StatusCode::SEE_OTHER);

    {
        let conn = db_conn.lock().await;
        let alert = db::get_alert_by_id(&conn, alert_id).unwrap().unwrap();
        assert_eq!(alert.price_kind, "sell");
        assert_eq!(alert.condition_kind, "crossed_down");
        assert_eq!(alert.threshold, Some(1500000));
        assert_eq!(alert.cooldown_secs, 7200);
    }

    let del_alert_req = Request::builder()
        .method("POST")
        .uri(format!("/alerts/{}/delete", alert_id))
        .body(Body::empty())
        .unwrap();
    let del_alert_res = app.clone().oneshot(del_alert_req).await.unwrap();
    assert_eq!(del_alert_res.status(), StatusCode::SEE_OTHER);

    {
        let conn = db_conn.lock().await;
        assert!(db::get_alert_by_id(&conn, alert_id).unwrap().is_none());
    }

    let del_item_req = Request::builder()
        .method("POST")
        .uri("/items/4151/delete")
        .body(Body::empty())
        .unwrap();
    let del_item_res = app.clone().oneshot(del_item_req).await.unwrap();
    assert_eq!(del_item_res.status(), StatusCode::SEE_OTHER);

    {
        let conn = db_conn.lock().await;
        assert_eq!(db::get_watched_items(&conn).unwrap().len(), 0);
    }
}

#[tokio::test]
async fn test_validation_error_preserves_form_input() {
    let (app, _) = setup_test_app();

    // alert should fail validation when no webhooks are configured anywhere
    let invalid_req = Request::builder()
        .method("POST")
        .uri("/alerts")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "item_id=4151&price_kind=buy&condition_kind=crossed_up&threshold=2500000&cooldown_secs=1200",
        ))
        .unwrap();

    let res = app.oneshot(invalid_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);

    assert!(body_str.contains("No notification target found"));
    assert!(body_str.contains("2500000"));
    assert!(body_str.contains("#4151"));
}

#[tokio::test]
async fn test_empty_string_form_fields() {
    let (app, db_conn) = setup_test_app();

    let new_get_req = Request::builder()
        .uri("/alerts/new?item_id=&item_name=")
        .body(Body::empty())
        .unwrap();
    let new_get_res = app.clone().oneshot(new_get_req).await.unwrap();
    assert_eq!(new_get_res.status(), StatusCode::OK);

    let post_req = Request::builder()
        .method("POST")
        .uri("/alerts")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "item_id=4151&price_kind=buy&condition_kind=price_up&threshold=&discord_webhook=https://discord.com/api/webhooks/test&cooldown_secs=",
        ))
        .unwrap();

    let post_res = app.clone().oneshot(post_req).await.unwrap();
    assert_eq!(post_res.status(), StatusCode::SEE_OTHER);

    {
        let conn = db_conn.lock().await;
        let alerts = db::get_alerts(&conn).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].item_id, 4151);
        assert_eq!(alerts[0].condition_kind, "price_up");
        assert_eq!(alerts[0].threshold, None);
        assert_eq!(alerts[0].cooldown_secs, 3600);
    }
}

#[tokio::test]
async fn test_settings_save_and_view() {
    let (app, db_conn) = setup_test_app();

    let get_req = Request::builder()
        .uri("/settings")
        .body(Body::empty())
        .unwrap();
    let get_res = app.clone().oneshot(get_req).await.unwrap();
    assert_eq!(get_res.status(), StatusCode::OK);

    let post_req = Request::builder()
        .method("POST")
        .uri("/settings")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "discord_webhook=https://discord.com/api/webhooks/global&default_cooldown_secs=1800",
        ))
        .unwrap();
    let post_res = app.clone().oneshot(post_req).await.unwrap();
    assert_eq!(post_res.status(), StatusCode::SEE_OTHER);

    {
        let conn = db_conn.lock().await;
        let s = db::get_app_settings(&conn).unwrap();
        assert_eq!(
            s.discord_webhook,
            Some("https://discord.com/api/webhooks/global".to_string())
        );
        assert_eq!(s.default_cooldown_secs, 1800);
    }
}

#[tokio::test]
async fn test_item_detail_and_history_endpoint() {
    let (app, db_conn) = setup_test_app();

    {
        let conn = db_conn.lock().await;
        db::log_alert(
            &conn,
            &db::LogAlertParams {
                alert_id: 1,
                item_id: 4151,
                item_name: "Abyssal whip",
                price_kind: "buy",
                condition: "crossed_up",
                old_price: Some(1950000),
                new_price: 2050000,
                threshold: Some(2000000),
                fired_at: 1723740000,
            },
        )
        .unwrap();
    }

    let get_req = Request::builder()
        .uri("/items/4151")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(get_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .unwrap();
    let body_str = String::from_utf8_lossy(&body);

    assert!(body_str.contains("Abyssal whip"));
    assert!(body_str.contains("A weapon from the abyss."));
    assert!(body_str.contains("120,000 gp"));
    assert!(body_str.contains("2,050,000 gp"));
    assert!(body_str.contains("+100,000 gp"));
}

#[tokio::test]
async fn test_alert_creation_with_global_settings_fallback() {
    let (app, db_conn) = setup_test_app();

    {
        let conn = db_conn.lock().await;
        db::save_app_settings(
            &conn,
            &AppSettings {
                discord_webhook: Some("https://discord.com/api/webhooks/global".to_string()),
                default_cooldown_secs: 3600,
                updated_at: 0,
            },
        )
        .unwrap();
    }

    let post_req = Request::builder()
        .method("POST")
        .uri("/alerts")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(Body::from(
            "item_id=4151&price_kind=buy&condition_kind=crossed_up&threshold=2000000&discord_webhook=",
        ))
        .unwrap();

    let res = app.oneshot(post_req).await.unwrap();
    assert_eq!(res.status(), StatusCode::SEE_OTHER);

    {
        let conn = db_conn.lock().await;
        let alerts = db::get_alerts(&conn).unwrap();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].item_id, 4151);
        // alert has no specific override, so it falls back to global
        assert_eq!(alerts[0].discord_webhook, None);
    }
}
