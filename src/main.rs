#![doc = include_str!("../README.md")]

use chrono::Utc;
use freedit::{
    app_router::router,
    config::CONFIG,
    controller::{db_utils::clear_invalid, feed::cron_feed, meta_handler::shutdown_signal},
    error::AppError,
    CURRENT_SHA256, GIT_COMMIT, VERSION,
};
use once_cell::sync::Lazy;
use std::{fs, path::PathBuf};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    info!(GIT_COMMIT);

    let db_url = &CONFIG.db;
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open()?;
    info!(%db_url);

    if !*IS_DEBUG {
        create_snapshot(&db);
    }

    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&db2, "captcha").await {
                error!(%e);
            }
            if let Err(e) = clear_invalid(&db2, "sessions").await {
                error!(%e);
            }
            sleep_seconds(300).await;
        }
    });

    let db2 = db.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = cron_feed(&db2).await {
                error!(%e);
            }
            if let Err(e) = clear_invalid(&db2, "user_stats").await {
                error!(%e);
            }
            sleep_seconds(3600 * 8).await;
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

// TODO: TEST with https://github.com/hatoo/oha

fn create_snapshot(db: &sled::Db) {
    let checksum = db.checksum().unwrap();
    info!(%checksum);

    let ts = Utc::now().format("%Y-%m-%d-%H-%M-%S");
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

async fn sleep_seconds(seconds: u64) {
    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await
}
