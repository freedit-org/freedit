#![doc = include_str!("../README.md")]
#![warn(clippy::nursery)]
// #![warn(clippy::pedantic)]
// #![warn(clippy::unwrap_used)]

use freedit::{
    AppError, CONFIG, DB, Tan, VERSION, clear_invalid, cron_download_audio, cron_feed, router,
};
use jiff::Timestamp;
use std::{fs, net::SocketAddr, path::PathBuf};
use tokio::net::TcpListener;
use tracing::{error, info, warn};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[cfg(not(target_os = "windows"))]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

#[tokio::main]
async fn main() -> Result<(), AppError> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new("info,tantivy=warn"))
        .with(tracing_subscriber::fmt::layer())
        .init();

    // only create snapshot in release mode
    #[cfg(not(debug_assertions))]
    create_snapshot(&DB);

    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&DB, "captcha").await {
                error!(%e);
            }
            if let Err(e) = clear_invalid(&DB, "sessions").await {
                error!(%e);
            }
            if let Err(e) = cron_download_audio(&DB).await {
                error!(%e);
            }
            sleep_seconds(300).await;
        }
    });

    tokio::spawn(async move {
        loop {
            sleep_seconds(600).await;
            if let Err(e) = cron_feed(&DB).await {
                error!(%e);
            }
            if let Err(e) = clear_invalid(&DB, "user_stats").await {
                error!(%e);
            }
            sleep_seconds(3600 * 4).await;
        }
    });

    tokio::spawn(async move {
        let mut tan = Tan::init().unwrap();
        if CONFIG.rebuild_index == Some(true) {
            tan.rebuild_index(&DB).unwrap();
        }
        let tan_ks = DB.open_partition("tan", Default::default()).unwrap();
        for item in tan_ks.inner().iter() {
            let (k, _) = item.unwrap();
            let id = String::from_utf8_lossy(&k);
            tan.add_doc(&id, &DB).unwrap();
            tan_ks.take(k).unwrap();
        }
        tan.commit().unwrap();
        info!("tantivy indexer finished");
    });

    let app = router().await;
    let addr: SocketAddr = CONFIG.addr.parse().unwrap();
    let listener = TcpListener::bind(addr).await.unwrap();

    info!("listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

#[allow(dead_code)]
fn create_snapshot(db: &fjall::TransactionalKeyspace) {
    let disk_space = db.disk_space();
    info!("disk space: {} bytes", disk_space);
    let ts = Timestamp::now().strftime("%Y-%m-%d-%H-%M-%S");
    let mut snapshot_path = PathBuf::from(&CONFIG.snapshots_path);
    if !snapshot_path.exists() {
        fs::create_dir_all(&snapshot_path).unwrap();
    }
    snapshot_path.push(format!("{VERSION}-{ts}-{disk_space}"));
    let snapshot = fjall::Config::new(&snapshot_path)
        .open_transactional()
        .unwrap();
    for tree in db.list_partitions() {
        let src_ks = db
            .inner()
            .open_partition(&tree, Default::default())
            .unwrap();
        let dst_ks = snapshot
            .inner()
            .open_partition(&tree, Default::default())
            .unwrap();
        for item in src_ks.iter() {
            let (k, v) = item.unwrap();
            dst_ks.insert(k, v).unwrap();
        }
    }

    info!("create snapshot: {}", snapshot_path.display());
    drop(snapshot);
}

async fn sleep_seconds(seconds: u64) {
    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
}
