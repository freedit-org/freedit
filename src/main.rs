#![doc = include_str!("../README.md")]

mod app_router;
mod config;
mod controller;
mod error;

use crate::{
    app_router::router,
    controller::{
        inn::{static_inn_all, static_inn_update},
        shutdown_signal,
        utils::{clear_invalid, CURRENT_SHA256},
    },
};
use config::CONFIG;
use error::AppError;
use once_cell::sync::Lazy;
use std::{
    fs,
    path::{Path, PathBuf},
};
use time::format_description;
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

const VERSION: &str = env!("CARGO_PKG_VERSION");
static IS_DEBUG: Lazy<bool> =
    Lazy::new(|| matches!(std::env::var("PROFILE"), Ok(key) if key.as_str() == "debug"));

#[tokio::main]
async fn main() -> Result<(), AppError> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    info!("sha256: {}", *CURRENT_SHA256);
    info!(VERSION);

    let db_url = &CONFIG.db;
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open()?;
    info!(%db_url);

    if !*IS_DEBUG {
        create_snapshot(&db);
    }

    check_path(&CONFIG.avatars_path);
    check_path(&CONFIG.inn_icons_path);
    check_path(&CONFIG.upload_path);
    check_path(&CONFIG.html_path);

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

    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&db2, "sessions", 300).await {
                error!(%e);
            }
        }
    });

    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&db2, "user_stats", 3600 * 6).await {
                error!(%e);
            }
        }
    });

    let app = router(db).await;

    let addr = CONFIG.addr.parse().unwrap();

    match CONFIG.tls_config().await {
        Some(tls_config) => {
            info!("listening on https://{}", addr);
            axum_server::bind_rustls(addr, tls_config)
                .serve(app.into_make_service())
                .await
                .unwrap();
        }
        None => {
            info!("listening on http://{}", addr);
            axum::Server::bind(&addr)
                .serve(app.into_make_service())
                .with_graceful_shutdown(shutdown_signal())
                .await
                .unwrap();
        }
    }

    Ok(())
}

// TODO: endorsements
// TODO: TEST with https://github.com/hatoo/oha
// TODO: book/music/movie/list=entity
// TODO: TOS

fn create_snapshot(db: &sled::Db) {
    let checksum = db.checksum().unwrap();
    info!(%checksum);

    let format =
        format_description::parse("[year]-[month]-[day]-[hour]-[minute]-[second]").unwrap();
    let ts = time::OffsetDateTime::now_utc().format(&format).unwrap();
    let mut snapshot_path = PathBuf::from("snapshots");
    if !snapshot_path.exists() {
        fs::create_dir_all(&snapshot_path).unwrap();
    }
    snapshot_path.push(format!("{VERSION}-{ts}-{checksum}"));
    let snapshot_cfg = sled::Config::default()
        .path(&snapshot_path)
        .use_compression(true);
    let snapshot = snapshot_cfg.open().unwrap();
    snapshot.import(db.export());
    info!("create snapshot: {}", snapshot_path.display());
    drop(snapshot);
}

fn check_path(path_str: &str) {
    let path = Path::new(path_str);
    if !path.exists() {
        fs::create_dir_all(path).unwrap();
    }
    info!("static path {path_str}");
}
