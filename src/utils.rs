use crate::error::AppError;
use ::time::OffsetDateTime;
use data_encoding::HEXLOWER;
use once_cell::sync::Lazy;
use ring::digest::{Context, Digest, SHA256};
use sled::Db;
use std::{
    env,
    fs::File,
    io::{BufReader, Read},
};
use tokio::time;
use tracing::{debug, instrument};

/// Returns SHA256 of the current running executable.
/// Cookbook: [Calculate the SHA-256 digest of a file](https://rust-lang-nursery.github.io/rust-cookbook/cryptography/hashing.html)
pub(super) static CURRENT_SHA256: Lazy<String> = Lazy::new(|| {
    let file = env::current_exe().unwrap();
    let input = File::open(file).unwrap();

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

    let reader = BufReader::new(input);
    let digest = sha256_digest(reader);

    HEXLOWER.encode(digest.as_ref())
});

/// Cron job: Scan all the keys in the `Tree` regularly and remove the expired ones.
///
/// The keys must be the format of `timestamp_id`. See [generate_nanoid_expire](../controller/fn.generate_nanoid_expire.html).
#[instrument(skip(db))]
pub(super) async fn clear_invalid(db: &Db, tree_name: &str, interval: u64) -> Result<(), AppError> {
    let tree = db.open_tree(tree_name)?;
    for i in tree.iter() {
        let (k, _) = i?;
        let k_str = std::str::from_utf8(&k)?;
        let time_stamp = k_str
            .split_once('_')
            .and_then(|s| i64::from_str_radix(s.0, 16).ok());
        if let Some(time_stamp) = time_stamp {
            if time_stamp < OffsetDateTime::now_utc().unix_timestamp() {
                debug!("remove expired {}: {}", tree_name, k_str);
                tree.remove(k)?;
            }
        }
    }
    time::sleep(time::Duration::from_secs(interval)).await;
    Ok(())
}
