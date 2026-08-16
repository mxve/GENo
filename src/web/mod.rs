pub mod handlers;
pub mod templates;

use crate::config::Config;
use crate::poller::{PollerStatus, PriceTick};
use axum::{
    Router,
    routing::{get, post},
};
use reqwest::Client;
use rusqlite::Connection;
use std::sync::Arc;
use tokio::sync::{Mutex, watch};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub http_client: Arc<Client>,
    pub config: Arc<Config>,
    pub poller_status: Arc<Mutex<PollerStatus>>,
    pub latest_prices: watch::Receiver<Option<PriceTick>>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(handlers::dashboard))
        .route("/search", get(handlers::search))
        .route(
            "/items",
            get(handlers::items_list).post(handlers::items_add),
        )
        .route("/items/{id}", get(handlers::item_detail))
        .route("/items/{id}/delete", post(handlers::items_delete))
        .route(
            "/alerts",
            get(handlers::alerts_list).post(handlers::alerts_create),
        )
        .route("/alerts/new", get(handlers::alerts_new_form))
        .route(
            "/alerts/{id}/edit",
            get(handlers::alerts_edit_form).post(handlers::alerts_edit),
        )
        .route("/alerts/{id}/delete", post(handlers::alerts_delete))
        .route(
            "/settings",
            get(handlers::settings_view).post(handlers::settings_save),
        )
        .route("/settings/test", post(handlers::settings_test))
        .route("/icons/{id}", get(handlers::icon_proxy))
        .with_state(state)
}
