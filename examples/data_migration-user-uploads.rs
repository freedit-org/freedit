use sled::{IVec, Tree};

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("user_uploads").unwrap();
    // old kv: uid#image_hash.ext => &[]
    // new kv: uid#img_id => image_hash.ext
    for i in tree.iter() {
        let img_id = incr_id(&db, "imgs_count").unwrap();
        let (k, _) = i.unwrap();
        println!("{:?}", &k);

        let new_k = [&k[0..4], &u32_to_ivec(img_id)].concat();
        println!("{:?}", &new_k[0..4]);
        println!("{:?}", String::from_utf8_lossy(&k[4..]));
        tree.insert(new_k, &k[4..]).unwrap();
    }
}

/// convert `u32` to [IVec]
#[inline]
fn u32_to_ivec(number: u32) -> IVec {
    IVec::from(number.to_be_bytes().to_vec())
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
