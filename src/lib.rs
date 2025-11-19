#![doc = include_str!("../README.md")]

pub use app_router::router;
pub use config::CONFIG;
pub use controller::db_utils::{clear_invalid, get_one, ivec_to_u32, set_one, u8_slice_to_u32};
pub use controller::{feed::cron_download_audio, feed::cron_feed, tantivy::Tan};
pub use error::AppError;
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

mod app_router;
mod config;
mod controller;
mod error;

use data_encoding::HEXLOWER;
use fjall::{Config, TransactionalKeyspace};
use ring::digest::{Context, Digest, SHA256};
use std::sync::LazyLock;
use std::{
    env,
    fs::File,
    io::{BufReader, Read},
};
use tracing::info;

const GIT_COMMIT: &str = env!("GIT_COMMIT");

/// Returns SHA256 of the current running executable.
/// Cookbook: [Calculate the SHA-256 digest of a file](https://rust-lang-nursery.github.io/rust-cookbook/cryptography/hashing.html)
static CURRENT_SHA256: LazyLock<String> = LazyLock::new(|| {
    fn sha256_digest<R: Read>(mut reader: R) -> Digest {
        let mut context = Context::new(&SHA256);
        let mut buffer = [0; 1024];

        loop {
            let count = reader.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            context.update(&buffer[..count]);
        }
        context.finish()
    }

    let file = env::current_exe().unwrap();
    let input = File::open(file).unwrap();
    let reader = BufReader::new(input);
    let digest = sha256_digest(reader);

    HEXLOWER.encode(digest.as_ref())
});

pub static DB: LazyLock<TransactionalKeyspace> = LazyLock::new(|| {
    info!("sha256: {}", *CURRENT_SHA256);
    info!(VERSION);
    info!(GIT_COMMIT);

    let db_url = &CONFIG.db;
    let db = Config::new(db_url).open_transactional().unwrap();
    info!(%db_url);
    db
});
