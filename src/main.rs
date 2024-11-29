#![doc = include_str!("../README.md")]
#![warn(clippy::nursery)]
#![warn(clippy::pedantic)]
// #![warn(clippy::unwrap_used)]

use std::time::SystemTime;
use std::time::UNIX_EPOCH;
use std::{fs, net::SocketAddr, path::PathBuf};

use freedit::{
    router, AppError, CONFIG, DB, VERSION, {clear_invalid, cron_feed, Tan},
};
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
    tokio::spawn(async move {
        loop {
            let snapshot_path = PathBuf::from("snapshots");
            // create snapshot dir if needed
            if !snapshot_path.exists() {
                fs::create_dir_all(&snapshot_path).unwrap();
            }
            // create a snapshot
            create_snapshot(&snapshot_path, &DB);
            // remove snapshots older than 48 hours
            if let Err(e) = prune_snapshots(&snapshot_path) {
                error!(%e, "error pruning snapshots");
            }
            // snapshot every 30 mins
            sleep_seconds(60 * 30).await;
        }
    });

    tokio::spawn(async move {
        loop {
            if let Err(e) = clear_invalid(&DB, "captcha").await {
                error!(%e);
            }
            if let Err(e) = clear_invalid(&DB, "sessions").await {
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
        let mut subscriber = DB.open_tree("tan").unwrap().watch_prefix(vec![]);
        while let Some(event) = (&mut subscriber).await {
            let (k, op_type) = match event {
                sled::Event::Insert { key, value: _ } => (key, "add"),
                sled::Event::Remove { key } => (key, "delete"),
            };
            let id = String::from_utf8_lossy(&k);

            if op_type == "add" {
                tan.add_doc(&id, &DB).unwrap();
            }

            tan.commit().unwrap();
        }
    });

    let app = router().await;
    let addr: SocketAddr = CONFIG.addr.parse().unwrap();
    let listener = TcpListener::bind(addr).await.unwrap();

    info!("listening on http://{}", addr);
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

// TODO: TEST with https://github.com/hatoo/oha
#[allow(dead_code)]
fn create_snapshot(snapshot_path: &PathBuf, db: &sled::Db) {
    let checksum = db.checksum().unwrap();
    info!(%checksum);

    let timestamp = SystemTime::now()
       .duration_since(UNIX_EPOCH)
       .unwrap()
       .as_secs();

    let mut snapshot_path = snapshot_path.clone();
    snapshot_path.push(format!("{VERSION}_{timestamp}_{checksum}"));
    let snapshot_cfg = sled::Config::default().path(&snapshot_path);
    let snapshot = snapshot_cfg.open().unwrap();
    snapshot.import(db.export());
    info!("create snapshot: {}", snapshot_path.display());
    drop(snapshot);
}

#[allow(dead_code)]
fn prune_snapshots(snapshot_path: &PathBuf) -> Result<(), AppError> {
    let contents = fs::read_dir(snapshot_path)?;
    let now = SystemTime::now()
       .duration_since(UNIX_EPOCH)
       .unwrap()
       .as_secs();

    for name in contents {
        let name = name?;
        let file_name = name.file_name();
        let file_name = file_name.to_string_lossy();
        let split_contents: Vec<&str> = file_name.split('_').collect();
        // timestamp is the second element
        let ts = split_contents[1];
        let snapshot_ts = ts.parse::<u64>().unwrap();
        let diff = now - snapshot_ts;
        // 48 hours
        let max_age: u64 = 60 * 60 * 48;
        if diff > max_age {
            fs::remove_dir_all(name.path())?;
        }
    }
    Ok(())
}

async fn sleep_seconds(seconds: u64) {
    tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
}
