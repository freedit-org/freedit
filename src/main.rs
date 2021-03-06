// #![warn(clippy::unwrap_used)]
#![doc = include_str!("../README.md")]

mod app_router;
mod config;
mod controller;
mod cronjob;
mod error;

use crate::{
    app_router::router,
    controller::{
        inn::{static_inn_all, static_inn_update},
        shutdown_signal, CURRENT_SHA256,
    },
    cronjob::clear_invalid,
};
use config::CONFIG;
use error::AppError;
use std::{fs, path::Path};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[tokio::main]
async fn main() -> Result<(), AppError> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "RUST_LOG=info,tower_http=DEBUG,freedit=DEBUG".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("sha256: {}", *CURRENT_SHA256);
    info!(VERSION);

    let db_url = &CONFIG.db;
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open()?;
    info!(%db_url);

    let avatars_path = Path::new(&CONFIG.avatars_path);
    if !avatars_path.exists() {
        fs::create_dir_all(avatars_path).unwrap();
    }
    info!("avatars path: {}", &CONFIG.avatars_path);

    let inn_icons_path = Path::new(&CONFIG.inn_icons_path);
    if !inn_icons_path.exists() {
        fs::create_dir_all(inn_icons_path).unwrap();
    }
    info!("inn icons path: {}", &CONFIG.inn_icons_path);

    let html_path = Path::new(&CONFIG.html_path);
    if !html_path.exists() {
        fs::create_dir_all(html_path).unwrap();
    }
    info!("html path: {}", &CONFIG.html_path);

    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = static_inn_all(&db2, 60).await {
                error!(%e);
            }
        }
    });

    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = static_inn_update(&db2, 10).await {
                error!(%e);
            }
        }
    });

    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&db2, "captcha", 3600 * 6).await {
                error!(%e);
            }
        }
    });

    // scan every 60s, to clear expired sessions
    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&db2, "sessions", 300).await {
                error!(%e);
            }
        }
    });

    // scan every 6 hours, to clear user pageviews data
    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&db2, "user_pageviews", 3600 * 6).await {
                error!(%e);
            }
        }
    });

    let app = router(db).await;

    let addr = CONFIG.addr.parse().unwrap();

    info!("listening on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    Ok(())
}
