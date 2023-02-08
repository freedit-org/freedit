//! ## model
//!
//! In order to generate auto increment id, we need to get the max id, so we have **x_count** key
//! to record the total number (we use **N** to refer this kind of value, and their type is [u32]).
//!
//! ### user
//! | tree             | key                  | value            | set       | get                   |
//! |------------------|----------------------|------------------|-----------|-----------------------|
//! | default          | "users_count"        | N                | [incr_id] | [get_count]           |
//! | "users"          | `uid`                | [`User`]         |           | [get_one]/[get_batch] |
//! | "usernames"      | `username`           | `uid`            |           | [get_uid_by_name]     |
//! | "user_following" | `uid#uid`            | `&[]`            |           | [get_ids_by_prefix]   |
//! | "user_followers" | `uid#uid`            | `&[]`            |           | [get_ids_by_prefix]   |
//! | "user_stats"     | `timestamp_uid_type` | N                |           |                       |
//! | "user_uploads"   | `uid#img_id#`        | `image_hash.ext` |           |                       |
//! | default          | "imgs_count"         | N                |           |                       |
//!
//! ### notification
//! | tree            | key                   | value             |
//! |-----------------|-----------------------|-------------------|
//! | default         | "notifications_count" | N                 |
//! | "notifications" | `uid#nid#nt_type`     | `id1#id2#is_read` |
//!
//! ### captcha
//! About key `timestamp_nanoid`, see [generate_nanoid_expire].
//!
//! | tree        | key                | value         |
//! |-------------|--------------------|---------------|
//! | "captcha"   | `timestamp_nanoid` | captcha_chars |
//!
//! ### solo
//! | tree               | key           | value            |
//! |--------------------|---------------|------------------|
//! | default            | "solos_count" | N                |
//! | "solos"            | `sid`         | [`Solo`]         |
//! | "user_solos"       | `uid#sid`     | `visibility` |
//! | "user_solos_like"  | `uid#sid`     | `&[]`            |
//! | "solo_users_like"  | `sid#uid`     | `&[]`            |
//! | "solo_timeline"    | `sid`         | `uid#visibility` |
//! | "hashtags"         | `hashtag#sid` | `&[]`            |
//!
//! ### session
//! About key `timestamp#nanoid`, see [generate_nanoid_expire](../controller/fn.generate_nanoid_expire.html).
//!
//! | tree       | key                | value                                     |
//! |------------|--------------------|-------------------------------------------|
//! | "sessions" | `timestamp_nanoid` | [`Claim`] |
//!
//! ### site config
//! | tree      | key           | value          | set       | get               |
//! |-----------|---------------|----------------|-----------|-------------------|
//! | default   | "site_config" | [`SiteConfig`] |           | [get_site_config] |
//!
//! ### inn
//! | tree            | key           | value               |
//! |-----------------|---------------|---------------------|
//! | default         | "inns_count"  | N                   |
//! | "inns"          | `iid`         | [`Inn`]             |
//! | "inn_names"     | `inn_name`    | `iid`               |
//! | "topics"        | `topic#iid`   | `&[]`               |
//! | "mod_inns"      | `uid#iid`     | `&[]`               |
//! | "user_inns"     | `uid#iid`     | `&[]`               |
//! | "inn_apply"     | `iid#uid`     | `&[]`               |
//! | "inn_users"     | `iid#uid`     | `&[1/2/3/4/5/8/10]` |
//! | "inns_private"  | `iid`         | `&[]`               |
//! | "drafts"        | `uid`         | [`FormPost`]        |
//!
//! ### post
//! | tree                | key                 | value                |
//! |-------------------- |---------------------|----------------------|
//! | default             | "posts_count"       | N                    |
//! | "posts"             | `pid`               | [`Post`]             |
//! | "inn_posts"         | `iid#pid`           | `&[]`                |
//! | "user_posts"        | `uid#pid`           | `iid#visibility`     |
//! | "tags"              | `tag#pid`           | `&[]`                |
//! | "post_upvotes"      | `pid#uid`           | `&[]`                |
//! | "post_downvotes"    | `pid#uid`           | `&[]`                |
//! | "post_timeline_idx" | `iid#pid`           | `timestamp`          |
//! | "post_timeline"     | `timestamp#iid#pid` | `visibility`         |
//! | "post_pageviews"    | `pid`               | N                    |
//!
//! ### comment
//! | tree                  | key                  | value       |
//! |-----------------------|----------------------|-------------|
//! | "post_comments_count" | `pid`                | N           |
//! | "post_comments"       | `pid#cid`            | [`Comment`] |
//! | "user_comments"       | `uid#pid#cid`        | `&[]`       |
//! | "comment_upvotes"     | `pid#cid#uid`        | `&[]`       |
//! | "comment_downvotes"   | `pid#cid#uid`        | `&[]`       |
//!
//! ### rss
//! | tree                  | key                  | value       |
//! |-----------------------|----------------------|-------------|
//! | default               | "feeds_count"        | N           |
//! | default               | "items_count"        | N           |
//! | "user_folders"        | `uid#folder#feed_id` | `&[0/1]`    |
//! | "feeds"               | `feed_id`            | [`Feed`]    |
//! | "feed_errs"           | `feed_id`            | "error msg" |
//! | "feed_items"          | `feed_id#item_id`    | `timestamp` |
//! | "feed_links"          | `feed_link`          | `feed_id`   |
//! | "item_links"          | `item_link`          | `item_id`   |
//! | "items"               | `item_id`            | [`Item`]    |
//! | "read"                | `uid#item_id`        | `&[]`       |
//! | "star"                | `uid#item_id`        | `timestamp` |

/// user
///
/// ### Permissions
/// | role     | code | post/solo | creat inn | site admin |
/// |----------|------|:---------:|:---------:|:----------:|
/// | Banned   | 0    |           |           |            |
/// | Standard | 10   | ✅        |           |            |
/// | Senior   | 100  | ✅        | ✅        |            |
/// | Admin    | 255  | ✅        | ✅        | ✅         |

#[derive(Default, Encode, Decode, Serialize, Debug)]
struct User {
    uid: u32,
    username: String,
    password_hash: String,
    recovery_hash: Option<String>,
    created_at: i64,
    role: u8,
    url: String,
    about: String,
}

/// solo
///
/// ## visibility
/// * 20: Just me (private)
/// * 10: Following
/// * 0: Everyone
///
#[derive(Encode, Decode, Serialize, Debug)]
struct Solo {
    sid: u32,
    uid: u32,
    visibility: u32,
    content: String,
    hashtags: Vec<String>,
    created_at: i64,
    reply_to: Option<u32>,
    replies: Vec<u32>,
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Inn {
    iid: u32,
    inn_name: String,
    about: String,
    description: String,
    topics: Vec<String>,
    inn_type: String,
    early_birds: u32,
    created_at: i64,
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Post {
    pid: u32,
    uid: u32,
    iid: u32,
    title: String,
    tags: Vec<String>,
    content: String,
    created_at: i64,
    is_locked: bool,
    is_hidden: bool,
}

/// Form data: `/inn/:iid/post/:pid` post create/edit page
#[derive(Debug, Default, Deserialize, Validate, Encode, Decode)]
pub(crate) struct FormPost {
    iid: u32,
    #[validate(length(min = 1, max = 256))]
    title: String,
    #[validate(length(min = 1, max = 128))]
    tags: String,
    #[validate(length(min = 1, max = 65535))]
    content: String,
    is_draft: Option<bool>,
    delete_draft: Option<bool>,
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Comment {
    cid: u32,
    pid: u32,
    uid: u32,
    reply_to: Option<u32>,
    content: String,
    created_at: i64,
    is_hidden: bool,
}

#[derive(Encode, Decode, Debug)]
struct Feed {
    link: String,
    title: String,
}

#[derive(Encode, Decode, Debug)]
struct Item {
    link: String,
    title: String,
    feed_title: String,
    updated: i64,
    content: String,
}

/// Go to source code to see default value: [SiteConfig::default()]
#[derive(Serialize, Deserialize, Encode, Decode, Validate, Debug)]
pub(super) struct SiteConfig {
    #[validate(length(max = 64))]
    site_name: String,
    // domain only used for inn feed
    domain: String,
    #[validate(length(max = 512))]
    description: String,
    read_only: bool,
    #[validate(range(max = 32))]
    inn_mod_max: usize,
    #[validate(range(max = 256))]
    title_max_length: usize,
    #[validate(range(max = 65535))]
    article_max_length: usize,
    #[validate(range(max = 65535))]
    comment_max_length: usize,
    #[validate(range(max = 3600))]
    solo_interval: i64,
    #[validate(range(max = 3600))]
    post_interval: i64,
    #[validate(range(max = 3600))]
    comment_interval: i64,
    #[validate(range(max = 100))]
    per_page: usize,
    captcha_difficulty: String,
    captcha_name: String,
}

#[derive(Encode, Decode)]
struct Claim {
    uid: u32,
    username: String,
    role: u8,
    last_write: i64,
    session_id: String,
}

use crate::{config::CONFIG, error::AppError, GIT_COMMIT, VERSION};
use askama::Template;
use axum::{
    async_trait,
    body::{self, BoxBody, Empty, Full},
    extract::{rejection::FormRejection, Form, FromRequest},
    headers::{HeaderName, Referer},
    http::{HeaderMap, HeaderValue, Request, StatusCode},
    response::{IntoResponse, Redirect, Response},
    routing::{get_service, MethodRouter},
    TypedHeader,
};
use bincode::config::standard;
use bincode::{Decode, Encode};
use chrono::{Days, NaiveDateTime, Utc};
use nanoid::nanoid;
use once_cell::sync::Lazy;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sled::{Db, IVec, Iter, Tree};
use std::iter::Rev;
use tokio::signal;
use tower_http::services::ServeDir;
use tracing::error;
use utils::CURRENT_SHA256;
use validator::Validate;

use self::utils::md2html;

pub(super) mod admin;
pub(super) mod feed;
pub(super) mod inn;
pub(super) mod notification;
pub(super) mod solo;
pub(super) mod upload;
pub(super) mod user;
pub(super) mod utils;

fn into_response<T: Template>(t: &T, ext: &str) -> Response<BoxBody> {
    match t.render() {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", ext)
            .body(body::boxed(Full::from(body)))
            .unwrap(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(body::boxed(Full::from(format!("{err}"))))
            .unwrap(),
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct PageError<'a> {
    page_data: PageData<'a>,
    status: String,
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::CaptchaError
            | AppError::NameExists
            | AppError::InnCreateLimit
            | AppError::UsernameInvalid
            | AppError::WrongPassword
            | AppError::ImageError(_)
            | AppError::Locked
            | AppError::Hidden
            | AppError::ReadOnly
            | AppError::ValidationError(_)
            | AppError::NoJoinedInn
            | AppError::AxumFormRejection(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::WriteInterval => StatusCode::TOO_MANY_REQUESTS,
            AppError::NonLogin => return Redirect::to("/signin").into_response(),
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Banned => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        error!("{}, {}", status, self);
        let site_config = SiteConfig::default();
        let page_data = PageData::new("Error", &site_config, None, false);
        let page_error = PageError {
            page_data,
            status: status.to_string(),
            error: self.to_string(),
        };

        into_response(&page_error, "html")
    }
}

pub(super) async fn handler_404() -> impl IntoResponse {
    AppError::NotFound.into_response()
}

pub(super) struct ValidatedForm<T>(pub(super) T);

#[async_trait]
impl<T, S, B> FromRequest<S, B> for ValidatedForm<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
    Form<T>: FromRequest<S, B, Rejection = FormRejection>,
    B: Send + 'static,
{
    type Rejection = AppError;

    async fn from_request(req: Request<B>, state: &S) -> Result<Self, Self::Rejection> {
        let Form(value) = Form::<T>::from_request(req, state).await?;
        value.validate()?;
        Ok(ValidatedForm(value))
    }
}

pub(crate) async fn home() -> impl IntoResponse {
    Redirect::to("/inn/0")
}

/// `GET /health_check`
pub(super) async fn health_check() -> Response<BoxBody> {
    Response::builder()
        .status(StatusCode::OK)
        .body(body::boxed(Empty::new()))
        .unwrap()
}

/// serve static directory
pub(super) async fn serve_dir(path: &str) -> MethodRouter {
    let fallback = tower::service_fn(|_| async {
        Ok::<_, std::io::Error>(Redirect::to("/signin").into_response())
    });
    let srv = get_service(ServeDir::new(path).precompressed_gzip().fallback(fallback));
    srv.handle_error(|error: std::io::Error| async move {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unhandled internal error: {error}"),
        )
    })
}

static CSS: Lazy<String> = Lazy::new(|| {
    let mut css = include_str!("../../static/css/bulma.min.css").to_string();
    css.push('\n');
    css.push_str(include_str!("../../static/css/main.css"));
    css
});

pub(crate) async fn style() -> (HeaderMap, &'static str) {
    let mut headers = HeaderMap::new();

    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/css"),
    );
    headers.insert(
        HeaderName::from_static("cache-control"),
        HeaderValue::from_static("public, max-age=1209600, s-maxage=86400"),
    );

    (headers, &CSS)
}

pub(super) async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    println!("signal received, starting graceful shutdown");
}

struct PageData<'a> {
    title: &'a str,
    site_name: &'a str,
    site_description: String,
    claim: Option<Claim>,
    has_unread: bool,
    sha256: &'a str,
    version: &'a str,
    git_commit: &'a str,
    footer_links: Vec<(&'a str, &'a str)>,
}

impl<'a> PageData<'a> {
    fn new(
        title: &'a str,
        site_config: &'a SiteConfig,
        claim: Option<Claim>,
        has_unread: bool,
    ) -> Self {
        let mut footer_links = vec![];
        for (path, _, link) in &CONFIG.serve_dir {
            if !link.is_empty() {
                footer_links.push((path.as_str(), link.as_str()));
            }
        }
        let site_description = md2html(&site_config.description);
        Self {
            title,
            site_name: &site_config.site_name,
            site_description,
            claim,
            has_unread,
            sha256: &CURRENT_SHA256,
            version: VERSION,
            git_commit: GIT_COMMIT,
            footer_links,
        }
    }
}

/// generate a new nanoid with expiration time that is hex encoded.
///
/// format: "hex_timestamp_nanoid"
///
/// # Examples
///
/// ```no_run
/// // format like: "624e97ca_sSUl_K03nbUmPQLFe2CWk"
/// let nanoid = generate_nanoid_expire();
/// ```
fn generate_nanoid_expire(seconds: i64) -> String {
    let nanoid = nanoid!();
    let exp = Utc::now().timestamp() + seconds;
    format!("{exp:x}_{nanoid}")
}

/// Update the counter and return the new id. It is contiguous if every id is used.
///
/// # Examples
///
/// ```no_run
/// let new_user_id = incr_id(db, "users_count")?;
/// ```
fn incr_id<K>(tree: &Tree, key: K) -> Result<u32, AppError>
where
    K: AsRef<[u8]>,
{
    let ivec = tree.update_and_fetch(key, increment)?.unwrap();
    Ok(ivec_to_u32(&ivec))
}

fn user_stats(db: &Db, uid: u32, stat_type: &str) -> Result<(), AppError> {
    let expire = Utc::now()
        .date_naive()
        .checked_add_days(Days::new(3))
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .timestamp();
    let key = format!("{expire:x}_{uid}_{stat_type}");
    incr_id(&db.open_tree("user_stats")?, key)?;
    Ok(())
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

fn get_referer(header: Option<TypedHeader<Referer>>) -> Option<String> {
    if let Some(TypedHeader(r)) = header {
        let referer = format!("{r:?}");
        let trimed = referer
            .trim_start_matches("Referer(\"")
            .trim_end_matches("\")");
        Some(trimed.to_owned())
    } else {
        None
    }
}

/// convert a `i64` timestamp to a date [`String`]
fn timestamp_to_date(timestamp: i64) -> String {
    NaiveDateTime::from_timestamp_opt(timestamp, 0)
        .unwrap()
        .format("%Y-%m-%d")
        .to_string()
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

/// convert `&[u8]` to `u32`
fn u8_slice_to_u32(bytes: &[u8]) -> u32 {
    u32::from_be_bytes(bytes.try_into().unwrap())
}

/// convert `i64` to [IVec]
#[inline]
fn i64_to_ivec(number: i64) -> IVec {
    IVec::from(number.to_be_bytes().to_vec())
}

/// convert `&[u8]` to `i64`
#[inline]
fn u8_slice_to_i64(bytes: &[u8]) -> i64 {
    i64::from_be_bytes(bytes.try_into().unwrap())
}

/// get uid by username
fn get_uid_by_name(db: &Db, name: &str) -> Result<Option<u32>, AppError> {
    let v = db.open_tree("usernames")?.get(name)?;
    Ok(v.map(|v| ivec_to_u32(&v)))
}

/// get [SiteConfig]
fn get_site_config(db: &Db) -> Result<SiteConfig, AppError> {
    let site_config = &db.get("site_config")?.unwrap_or_default();
    let (site_config, _): (SiteConfig, usize) =
        bincode::decode_from_slice(site_config, standard()).unwrap_or_default();
    Ok(site_config)
}

fn get_inn_role(db: &Db, iid: u32, uid: u32) -> Result<Option<u8>, AppError> {
    let inn_users_k = [&u32_to_ivec(iid), &u32_to_ivec(uid)].concat();
    Ok(db
        .open_tree("inn_users")?
        .get(inn_users_k)?
        .map(|role| role.to_vec()[0]))
}

fn is_mod(db: &Db, uid: u32, iid: u32) -> Result<bool, AppError> {
    let k = [&u32_to_ivec(uid), &u32_to_ivec(iid)].concat();
    Ok(db.open_tree("mod_inns")?.contains_key(k)?)
}

/// check if the user has unread notifications
fn has_unread(db: &Db, uid: u32) -> Result<bool, AppError> {
    let prefix = u32_to_ivec(uid);
    let iter = db.open_tree("notifications")?.scan_prefix(&prefix);
    for i in iter {
        let (_, v) = i?;
        if v[8] == 0 {
            return Ok(true);
        }
    }

    let mod_inns = get_ids_by_prefix(db, "mod_inns", &prefix, None)?;
    for i in mod_inns {
        if db
            .open_tree("inn_apply")?
            .scan_prefix(u32_to_ivec(i))
            .next()
            .is_some()
        {
            return Ok(true);
        }
    }

    Ok(false)
}

/// get one object that has been encoded by bincode
///
/// # Examples
///
/// ```no_run
/// // get the user whose uid is 3.
/// let user:User = get_one(&db, "users", 3)?;
/// ```
fn get_one<T>(db: &Db, tree_name: &str, id: u32) -> Result<T, AppError>
where
    T: Decode,
{
    let v = db.open_tree(tree_name)?.get(u32_to_ivec(id))?;
    if let Some(v) = v {
        let (one, _): (T, usize) = bincode::decode_from_slice(&v, standard())?;
        Ok(one)
    } else {
        Err(AppError::NotFound)
    }
}

/// Used for pagination.
fn get_range(count: usize, page_params: &ParamsPage) -> (usize, usize) {
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
///
/// # Examples
///
/// ```no_run
/// // get the comments count of the post 3.
/// let comment_count = get_count(db, "post_comments_count", u32_to_ivec(pid))?
/// ```
fn get_count<K>(db: &Db, count_tree: &str, key: K) -> Result<usize, AppError>
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
/// ```no_run
/// // get the third comment's upvotes of the post 1.
/// // key: pid#cid#uid
/// let prefix = [&u32_to_ivec(1), &u32_to_ivec(3)].concat();
/// let upvotes = get_count_by_prefix(&db, "comment_upvotes", &prefix).unwrap_or_default();
/// ```
fn get_count_by_prefix(db: &Db, tree: &str, prefix: &[u8]) -> Result<usize, AppError> {
    Ok(db.open_tree(tree)?.scan_prefix(prefix).count())
}

/// get batch ids by scanning the prefix of the key with the format of `prefix#id`
///
/// # Examples
///
/// ```no_run
/// // get the id of inns that someone has joined.
/// user_iins = get_ids_by_prefix(&db, "user_inns", u32_to_ivec(claim.uid), None).unwrap();
/// ```
fn get_ids_by_prefix(
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
fn get_ids_by_tag(
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

/// get objects in batch that has been encoded by bincode
///
/// # Examples
///
/// ```no_run
/// // get the inns which iid is between 101-110.
/// let page_params = ParamsPage { anchor: 100, n: 10, is_desc: false };
/// let inns: Vec<Inn> = get_batch(&db, "default", "inns_count", "inns", &page_params)?;
/// ```
fn get_batch<T, K>(
    db: &Db,
    count_tree: &str,
    key: K,
    tree: &str,
    page_params: &ParamsPage,
) -> Result<Vec<T>, AppError>
where
    T: Decode,
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

/// extract element from string
///
/// # Note
///
/// The tag length should be less than or equal to 25. And the results shoule be no more than `max_len`.
/// If no space is found after the `char`, the string will be ignored.
///
/// # Examples
///
/// ```no_run
/// let input = "hi, @cc this is a test. If no space at last, @notag";
/// let out = extract_element(input, 3, '@');
/// assert_eq!(out, vec!["cc"]);
/// ```
fn extract_element(input: &str, max_len: usize, char: char) -> Vec<String> {
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

struct ParamsPage {
    anchor: usize,
    n: usize,
    is_desc: bool,
}

enum IterType {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_u32_to_ivec() {
        let iv = u32_to_ivec(12345678);
        assert_eq!(ivec_to_u32(&iv), 12345678);
    }

    #[test]
    fn test_extract_element() {
        let input = "hi, @cc this is a test. If no space at last, @notag";
        let out = extract_element(input, 3, '@');
        assert_eq!(out, vec!["cc"]);
    }
}
