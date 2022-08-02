use ::time::OffsetDateTime;
use sled::Db;
use tokio::time;
use tracing::{debug, instrument};

use crate::error::AppError;

/// Cron job: Scan all the keys in the `Tree` regularly and remove the expired ones.
///
/// The keys must be the format of `timestamp#id`. See [generate_nanoid_expire](../controller/fn.generate_nanoid_expire.html).
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
