use sled::{Db, IVec, Tree};

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("notifications").unwrap();
    // old kv: uid#pid#cid => notification_code
    // new kv: uid#nid#nt_type => id1#id2#is_read
    for i in tree.iter() {
        let (k, _) = i.unwrap();
        let uid = u8_slice_to_u32(&k[0..4]);
        let pid = u8_slice_to_u32(&k[4..8]);
        let cid = u8_slice_to_u32(&k[8..12]);

        if pid > 0 {
            add_notification(&db, uid, NtType::PostComment, pid, cid).unwrap();
        } else {
            add_notification(&db, uid, NtType::SoloComment, cid, pid).unwrap();
        }

        tree.remove(k).unwrap();
    }
}

fn u8_slice_to_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

fn add_notification(
    db: &Db,
    uid: u32,
    nt_type: NtType,
    id1: u32,
    id2: u32,
) -> Result<(), AppError> {
    let nid = incr_id(db, "notifications_count")?;
    let k = [
        &u32_to_ivec(uid),
        &u32_to_ivec(nid),
        &IVec::from(&[nt_type as u8]),
    ]
    .concat();
    let v = [&u32_to_ivec(id1), &u32_to_ivec(id2), &IVec::from(&[0])].concat();
    db.open_tree("notifications")?.insert(k, v)?;

    Ok(())
}

fn incr_id<K>(tree: &Tree, key: K) -> Result<u32, AppError>
where
    K: AsRef<[u8]>,
{
    let ivec = tree.update_and_fetch(key, increment)?.unwrap();
    Ok(ivec_to_u32(&ivec))
}

fn increment(old: Option<&[u8]>) -> Option<Vec<u8>> {
    let number = match old {
        Some(bytes) => {
            let array: [u8; 4] = bytes.try_into().unwrap();
            let number = u32::from_be_bytes(array);
            if let Some(new) = number.checked_add(1) {
                new
            } else {
                panic!("overflow")
            }
        }
        None => 1,
    };

    Some(number.to_be_bytes().to_vec())
}

#[repr(u8)]
enum NtType {
    PostComment = 1,
    PostMention = 2,
    SoloComment = 3,
    SoloMention = 4,
    InnNotification = 5,
    SiteNotification = 6,
}

impl From<u8> for NtType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::PostComment,
            2 => Self::PostMention,
            3 => Self::SoloComment,
            4 => Self::SoloMention,
            5 => Self::InnNotification,
            6 => Self::SiteNotification,
            _ => unreachable!(),
        }
    }
}

/// convert `u32` to [IVec]
#[inline]
fn u32_to_ivec(number: u32) -> IVec {
    IVec::from(number.to_be_bytes().to_vec())
}

/// convert [IVec] to u32
#[inline]
fn ivec_to_u32(iv: &IVec) -> u32 {
    u32::from_be_bytes(iv.to_vec().as_slice().try_into().unwrap())
}

use thiserror::Error;

#[derive(Error, Debug)]
enum AppError {
    // 5XX
    #[error("Sled db error: {}", .0)]
    SledError(#[from] sled::Error),
    #[error("Bincode encode error: {}", .0)]
    BincodeEnError(#[from] bincode::error::EncodeError),
    #[error("Bincode decode error: {}", .0)]
    BincodeDeError(#[from] bincode::error::DecodeError),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error(transparent)]
    ImageError(#[from] image::ImageError),
    #[error(transparent)]
    ValidationError(#[from] validator::ValidationErrors),
    #[error(transparent)]
    AxumFormRejection(#[from] axum::extract::rejection::FormRejection),
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
}
