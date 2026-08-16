use ge_notifier::api;
use ge_notifier::config::Config;
use ge_notifier::db;
use ge_notifier::engine;
use ge_notifier::notifier;
use ge_notifier::poller::{self, PollerStatus};
use ge_notifier::web::{self, AppState};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc, watch};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ge_notifier=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    tracing::info!("Starting GE Notifier...");

    let config = Config::load("config.toml")?;
    let addr_str = format!("{}:{}", config.server.host, config.server.port);
    tracing::info!(
        "Loaded configuration (Server: {}, Poller interval: {}s, DB: {})",
        addr_str,
        config.poller.interval_secs,
        config.db.path
    );

    let conn = db::open_db(&config.db.path)?;
    let db_conn = Arc::new(Mutex::new(conn));

    let http_client = Arc::new(api::build_client(&config.poller.user_agent));

    // sync item mapping on startup if it's stale or empty
    {
        let is_stale = {
            let conn_guard = db_conn.lock().await;
            db::is_mapping_stale(&conn_guard).unwrap_or(true)
        };

        if is_stale {
            tracing::info!(
                "Item mapping is empty or stale (>24h). Fetching /mapping from OSRS Wiki API..."
            );
            match api::fetch_mapping(&http_client).await {
                Ok(items) => {
                    let count = items.len();
                    let mut conn_guard = db_conn.lock().await;
                    if let Err(e) = db::upsert_mapping(&mut conn_guard, &items) {
                        tracing::error!("Failed to save item mapping to database: {}", e);
                    } else {
                        tracing::info!(
                            "Successfully synchronized {} items into item_mapping cache.",
                            count
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch initial item mapping from Wiki API: {}. Will use existing cache.",
                        e
                    );
                }
            }
        } else {
            tracing::info!("Item mapping cache is fresh.");
        }
    }

    let (tick_tx, tick_rx) = watch::channel(None);
    let (alert_tx, alert_rx) = mpsc::channel(64);

    let poller_status = Arc::new(Mutex::new(PollerStatus::new(config.poller.interval_secs)));

    let poller_handle = {
        let client = Arc::clone(&http_client);
        let interval = config.poller.interval_secs;
        let status = Arc::clone(&poller_status);
        tokio::spawn(async move {
            poller::run_poller(client, interval, tick_tx, status).await;
        })
    };

    let engine_handle = {
        let db_for_engine = Arc::clone(&db_conn);
        let engine_tick_rx = tick_rx.clone();
        tokio::spawn(async move {
            engine::run_engine(db_for_engine, engine_tick_rx, alert_tx).await;
        })
    };

    let notifier_handle = {
        let client = Arc::clone(&http_client);
        tokio::spawn(async move {
            notifier::run_notifier(client, alert_rx).await;
        })
    };

    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);

    let shutdown_signal_task = {
        let tx = shutdown_tx.clone();
        tokio::spawn(async move {
            shutdown_signal().await;
            let _ = tx.send(true);
        })
    };

    let task_supervisor = tokio::spawn(async move {
        tokio::select! {
            res = poller_handle => {
                tracing::error!("Poller background task terminated unexpectedly: {:?}", res);
                let _ = shutdown_tx.send(true);
            }
            res = engine_handle => {
                tracing::error!("Engine background task terminated unexpectedly: {:?}", res);
                let _ = shutdown_tx.send(true);
            }
            res = notifier_handle => {
                tracing::error!("Notifier background task terminated unexpectedly: {:?}", res);
                let _ = shutdown_tx.send(true);
            }
        }
    });

    let app_state = AppState {
        db: Arc::clone(&db_conn),
        http_client: Arc::clone(&http_client),
        config: Arc::new(config),
        poller_status: Arc::clone(&poller_status),
        latest_prices: tick_rx,
    };

    let router = web::router(app_state);
    let addr: SocketAddr = addr_str.parse()?;
    tracing::info!("Web interface listening on http://{}", addr);

    let listener = TcpListener::bind(addr).await?;
    let server = axum::serve(listener, router).with_graceful_shutdown(async move {
        while !*shutdown_rx.borrow_and_update() {
            if shutdown_rx.changed().await.is_err() {
                break;
            }
        }
    });

    if let Err(e) = server.await {
        tracing::error!("Server error: {}", e);
    }

    task_supervisor.abort();
    shutdown_signal_task.abort();
    tracing::info!("GE Notifier stopped gracefully.");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Shutdown signal received (Ctrl+C). Exiting...");
        },
        _ = terminate => {
            tracing::info!("Shutdown signal received (SIGTERM). Exiting...");
        },
    }
}
