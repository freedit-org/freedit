use super::meta_handler::ParamsPage;
use crate::error::AppError;
use bincode::{config::standard, Decode, Encode};
use jiff::Timestamp;
use nanoid::nanoid;
use sled::{Db, IVec, Iter, Tree};
use std::iter::Rev;

/// Cron job: Scan all the keys in the `Tree` regularly and remove the expired ones.
///
/// The keys must be the format of `timestamp_id`.
pub async fn clear_invalid(db: &Db, tree_name: &str) -> Result<(), AppError> {
    let tree = db.open_tree(tree_name)?;
    for i in tree.iter() {
        let (k, _) = i?;
        let k_str = std::str::from_utf8(&k)?;
        let time_stamp = k_str
            .split_once('_')
            .and_then(|s| i64::from_str_radix(s.0, 16).ok());
        if let Some(time_stamp) = time_stamp {
            if time_stamp < Timestamp::now().as_second() {
                tree.remove(k)?;
            }
        }
    }
    Ok(())
}

/// get one object that has been encoded by bincode
///
/// # Examples
///
/// ```ignore
/// // get the user whose uid is 3.
/// let user:User = get_one(&db, "users", 3)?;
/// ```
pub fn get_one<T>(db: &Db, tree_name: &str, id: u32) -> Result<T, AppError>
where
    T: Decode<()>,
{
    get_one_by_key(db, tree_name, u32_to_ivec(id))
}

fn get_one_by_key<T, K>(db: &Db, tree_name: &str, key: K) -> Result<T, AppError>
where
    T: Decode<()>,
    K: AsRef<[u8]>,
{
    let v = db.open_tree(tree_name)?.get(key)?;
    if let Some(v) = v {
        let (one, _): (T, usize) = bincode::decode_from_slice(&v, standard())?;
        Ok(one)
    } else {
        Err(AppError::NotFound)
    }
}

pub fn set_one<T>(db: &Db, tree_name: &str, id: u32, one: &T) -> Result<(), AppError>
where
    T: Encode,
{
    set_one_with_key(db, tree_name, u32_to_ivec(id), one)
}

pub(super) fn set_one_with_key<T, K>(
    db: &Db,
    tree_name: &str,
    key: K,
    one: &T,
) -> Result<(), AppError>
where
    T: Encode,
    K: AsRef<[u8]>,
{
    let encoded = bincode::encode_to_vec(one, standard())?;
    db.open_tree(tree_name)?.insert(key, encoded)?;
    Ok(())
}

/// get objects in batch that has been encoded by bincode
///
/// # Examples
///
/// ```ignore
/// // get the inns which iid is between 101-110.
/// let page_params = ParamsPage { anchor: 100, n: 10, is_desc: false };
/// let inns: Vec<Inn> = get_batch(&db, "default", "inns_count", "inns", &page_params)?;
/// ```
pub(super) fn get_batch<T, K>(
    db: &Db,
    count_tree: &str,
    key: K,
    tree: &str,
    page_params: &ParamsPage,
) -> Result<Vec<T>, AppError>
where
    T: Decode<()>,
    K: AsRef<[u8]>,
{
    let count = get_count(db, count_tree, key)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let (start, end) = get_range(count, page_params);

    let mut output = Vec::with_capacity(page_params.n);
    for i in start..=end {
        let out: Result<T, AppError> = get_one(db, tree, i as u32);
        if let Ok(out) = out {
            output.push(out);
        }
    }
    if page_params.is_desc {
        output.reverse();
    }
    Ok(output)
}

/// Used for pagination.
pub(super) fn get_range(count: usize, page_params: &ParamsPage) -> (usize, usize) {
    let anchor = page_params.anchor;
    let n = page_params.n;
    let is_desc = page_params.is_desc;

    let mut start = if anchor > count { count } else { anchor + 1 };
    let mut end = if start + n < count {
        start + n - 1
    } else {
        count
    };

    if is_desc {
        end = if anchor > count {
            count
        } else {
            count - anchor
        };
        start = if end > n { end - n + 1 } else { 1 };
    }
    (start, end)
}

/// get the count `N`
pub(super) fn get_count<K>(db: &Db, count_tree: &str, key: K) -> Result<usize, AppError>
where
    K: AsRef<[u8]>,
{
    let count = if count_tree == "default" {
        db.get(key)?
    } else {
        db.open_tree(count_tree)?.get(key)?
    };
    let count = match count {
        Some(count) => ivec_to_u32(&count),
        None => 0,
    };
    Ok(count as usize)
}

/// get the count `N` by scanning the prefix of the key
///
/// # Examples
///
/// ```ignore
/// // get the third comment's upvotes of the post 1.
/// // key: pid#cid#uid
/// let prefix = [&u32_to_ivec(1), &u32_to_ivec(3)].concat();
/// let upvotes = get_count_by_prefix(&db, "comment_upvotes", &prefix).unwrap_or_default();
/// ```
pub(super) fn get_count_by_prefix(db: &Db, tree: &str, prefix: &[u8]) -> Result<usize, AppError> {
    Ok(db.open_tree(tree)?.scan_prefix(prefix).count())
}

/// get batch ids by scanning the prefix of the key with the format of `prefix#id`
///
/// # Examples
///
/// ```ignore
/// // get the id of inns that someone has joined.
/// user_iins = get_ids_by_prefix(&db, "user_inns", u32_to_ivec(claim.uid), None).unwrap();
/// ```
pub(super) fn get_ids_by_prefix(
    db: &Db,
    tree: &str,
    prefix: impl AsRef<[u8]>,
    page_params: Option<&ParamsPage>,
) -> Result<Vec<u32>, AppError> {
    let mut res = vec![];
    let iter = db.open_tree(tree)?.scan_prefix(&prefix);
    if let Some(page_params) = page_params {
        let iter = if page_params.is_desc {
            IterType::Rev(iter.rev())
        } else {
            IterType::Iter(iter)
        };
        for (idx, i) in iter.enumerate() {
            if idx < page_params.anchor {
                continue;
            }
            if idx >= page_params.anchor + page_params.n {
                break;
            }
            let (k, _) = i?;
            let id = &k[prefix.as_ref().len()..];
            res.push(u8_slice_to_u32(id));
        }
    } else {
        for i in iter {
            let (k, _) = i?;
            let id = &k[prefix.as_ref().len()..];
            res.push(u8_slice_to_u32(id));
        }
    }

    Ok(res)
}

/// get batch ids by scanning the prefix of the tag with the format of `tag#id`
pub(super) fn get_ids_by_tag(
    db: &Db,
    tree: &str,
    tag: &str,
    page_params: Option<&ParamsPage>,
) -> Result<Vec<u32>, AppError> {
    let mut res = vec![];
    let iter = db.open_tree(tree)?.scan_prefix(tag);
    if let Some(page_params) = page_params {
        let iter = if page_params.is_desc {
            IterType::Rev(iter.rev())
        } else {
            IterType::Iter(iter)
        };
        for (idx, i) in iter.enumerate() {
            if idx < page_params.anchor {
                continue;
            }
            if idx >= page_params.anchor + page_params.n {
                break;
            }
            let (k, _) = i?;
            let len = k.len();
            let str = String::from_utf8_lossy(&k[0..len - 4]);
            if tag == str {
                let id = u8_slice_to_u32(&k[len - 4..]);
                res.push(id);
            }
        }
    } else {
        for i in iter {
            let (k, _) = i?;
            let len = k.len();
            let str = String::from_utf8_lossy(&k[0..len - 4]);
            if tag == str {
                let id = u8_slice_to_u32(&k[len - 4..]);
                res.push(id);
            }
        }
    }

    Ok(res)
}

pub(super) enum IterType {
    Iter(Iter),
    Rev(Rev<Iter>),
}

impl Iterator for IterType {
    type Item = Result<(IVec, IVec), sled::Error>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            IterType::Iter(iter) => iter.next(),
            IterType::Rev(iter) => iter.next(),
        }
    }
}

/// Update the counter and return the new id. It is contiguous if every id is used.
///
/// # Examples
///
/// ```ignore
/// let new_user_id = incr_id(db, "users_count")?;
/// ```
pub(super) fn incr_id<K>(tree: &Tree, key: K) -> Result<u32, AppError>
where
    K: AsRef<[u8]>,
{
    let ivec = tree.update_and_fetch(key, increment)?.unwrap();
    Ok(ivec_to_u32(&ivec))
}

/// work for [update_and_fetch](https://docs.rs/sled/latest/sled/struct.Db.html#method.update_and_fetch):
/// increment 1.
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

/// extract element from string
///
/// # Note
///
/// The tag length should be less than or equal to 25. And the results should be no more than `max_len`.
/// If no space is found after the `char`, the string will be ignored.
///
/// # Examples
///
/// ```ignore
/// let input = "hi, @cc this is a test. If no space at last, @notag";
/// let out = extract_element(input, 3, '@');
/// assert_eq!(out, vec!["cc"]);
/// ```
pub(super) fn extract_element(input: &str, max_len: usize, char: char) -> Vec<String> {
    let mut vec = vec![];
    for i in input.split(char).skip(1) {
        if i.contains(' ') {
            let tag: String = i.split(' ').take(1).collect();
            if !tag.is_empty() && tag.len() <= 25 {
                if vec.len() < max_len {
                    vec.push(tag);
                } else {
                    break;
                }
            }
        }
    }
    vec
}

pub(super) fn is_valid_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.chars().next().unwrap().is_numeric() {
        return false;
    }
    if name
        .chars()
        .any(|c| (!char::is_alphanumeric(c)) && c != '_' && c != ' ')
    {
        return false;
    }

    true
}

/// get id by name
pub(super) fn get_id_by_name(
    db: &Db,
    tree_name: &str,
    name: &str,
) -> Result<Option<u32>, AppError> {
    let v = db
        .open_tree(tree_name)?
        .get(name.replace(' ', "_").to_lowercase())?;
    Ok(v.map(|v| ivec_to_u32(&v)))
}

/// generate a new nanoid with expiration time that is hex encoded.
///
/// format: "hex_timestamp_nanoid"
///
/// # Examples
///
/// ```ignore
/// // format like: "624e97ca_sSUl_K03nbUmPQLFe2CWk"
/// let nanoid = generate_nanoid_ttl();
/// ```
pub(super) fn generate_nanoid_ttl(seconds: i64) -> String {
    let nanoid = nanoid!();
    let exp = Timestamp::now().as_second() + seconds;
    format!("{exp:x}_{nanoid}")
}

/// convert `u32` to [IVec]
#[inline]
pub(super) fn u32_to_ivec(number: u32) -> IVec {
    IVec::from(number.to_be_bytes().to_vec())
}

/// convert [IVec] to u32
#[inline]
pub fn ivec_to_u32(iv: &IVec) -> u32 {
    u32::from_be_bytes(iv.to_vec().as_slice().try_into().unwrap())
}

/// convert `&[u8]` to `u32`
#[inline]
pub fn u8_slice_to_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

/// convert `i64` to [IVec]
#[inline]
pub(super) fn i64_to_ivec(number: i64) -> IVec {
    IVec::from(number.to_be_bytes().to_vec())
}

/// convert `&[u8]` to `i64`
#[inline]
pub(super) fn u8_slice_to_i64(bytes: &[u8]) -> i64 {
    i64::from_be_bytes(bytes.try_into().unwrap())
}
