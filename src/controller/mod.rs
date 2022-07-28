//! ## model
//!
//! In order to generate auto increment id, we need to get the max id, so we have **x_count** key
//! to record the total number (we use **N** to refer this kind of value, and their type is [u64]).
//!
//! ### user
//! | tree             | key           | value      | set       | get                   |
//! |------------------|---------------|------------|-----------|-----------------------|
//! | default          | "users_count" | N          | [incr_id] | [get_count]           |
//! | "users"          | `uid`         | [`User`]   |           | [get_one]/[get_batch] |
//! | "usernames"      | `username`    | `uid`      |           | [get_uid_by_name]     |
//! | "user_following" | `uid#uid`     | `&[]`      |           | [get_ids_by_prefix]     |
//! | "user_followers" | `uid#uid`     | `&[]`      |           | [get_ids_by_prefix]     |
//!
//! ### notification
//! | tree            | key           | value                             |
//! |-----------------|---------------|-----------------------------------|
//! | "notifications" | `uid#pid#cid` | [notification_code][Notification] |
//!
//! ### static
//! | tree               | key   | value |
//! |--------------------|-------|-------|
//! | "static_user_post" | `uid` | `&[]` |
//! | "static_inn_post"  | `iid` | `&[]` |
//!
//! ### captcha
//! About key `timestamp#nanoid`, see [generate_nanoid_expire].
//!
//! | tree        | key                | value         |
//! |-------------|--------------------|---------------|
//! | "captcha"   | `timestamp#nanoid` | captcha_chars |
//!
//! ### pageviews
//! In order to anti-spam, keep three days pageviews of each user. For privacy,
//! the hour and minute has been striped, just date kept. See [Claim::get].
//!
//! | tree             | key             | value |
//! |------------------|-----------------|-------|
//! | "user_pageviews" | `timestamp#uid` | N     |
//! | "post_pageviews" | `pid`           | N     |
//!
//! ### solo
//! | tree               | key           | value            |
//! |--------------------|---------------|------------------|
//! | default            | "solos_count" | N                |
//! | "solos"            | `sid`         | [`Solo`]         |
//! | "user_solos_count" | `uid`         | N                |
//! | "user_solos_idx"   | `uid#idx`     | `sid#visibility` |
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
//! | "sessions" | `timestamp#nanoid` | [`Claim`] |
//!
//! ### site config
//! | tree      | key           | value          | set       | get               |
//! |-----------|---------------|----------------|-----------|-------------------|
//! | default   | "site_config" | [`SiteConfig`] |           | [get_site_config] |
//!
//! ### inn
//! | tree            | key           | value      |
//! |-----------------|---------------|------------|
//! | default         | "inns_count"  | N          |
//! | "inns"          | `iid`         | [`Inn`]    |
//! | "inn_names"     | `inn_name`    | `iid`      |
//! | "topics"        | `topic#iid`   | `&[]`      |
//! | "mod_inns"      | `uid#iid`     | `&[]`      |
//! | "user_inns"     | `uid#iid`     | `&[]`      |
//! | "inn_users"     | `iid#uid`     | `&[1/2/3]` |
//! | "inns_private"  | `iid`         | `&[]`      |
//!
//! ### post
//! | tree                | key                 | value                |
//! |-------------------- |---------------------|----------------------|
//! | default             | "posts_count"       | N                    |
//! | "posts"             | `pid`               | [`Post`]             |
//! | "inn_posts_count"   | `iid`               | N                    |
//! | "inn_posts_idx"     | `iid#idx`           | `pid`                |
//! | "user_posts_count"  | `uid`               | N                    |
//! | "user_posts_idx"    | `uid#idx`           | `iid#pid#visibility` |
//! | "tags"              | `tag#pid`           | `&[]`                |
//! | "post_upvotes"      | `pid#uid`           | `&[]`                |
//! | "post_downvotes"    | `pid#uid`           | `&[]`                |
//! | "post_timeline_idx" | `iid#pid`           | `timestamp`          |
//! | "post_timeline"     | `timestamp#iid#pid` | `visibility`         |
//!
//! ### comment
//! | tree                  | key                  | value       |
//! |-----------------------|----------------------|-------------|
//! | "post_comments_count" | `pid`                | N           |
//! | "post_comments"       | `pid#cid`            | [`Comment`] |
//! | "user_comments_count" | `uid`                | N           |
//! | "user_comments_idx"   | `uid#idx`            | `pid#cid`   |
//! | "comment_upvotes"     | `pid#cid#uid`        | `&[]`       |
//! | "comment_downvotes"   | `pid#cid#uid`        | `&[]`       |

/// user
///
/// ## role
/// * 10: who can create inn
/// * 255: super admin
#[derive(Default, Encode, Decode, Serialize)]
struct User {
    uid: u64,
    username: String,
    password_hash: String,
    created_at: i64,
    karma: u64,
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
#[derive(Encode, Decode, Serialize)]
struct Solo {
    sid: u64,
    uid: u64,
    visibility: u64,
    content: String,
    hashtags: Vec<String>,
    created_at: i64,
}

#[derive(Encode, Decode, Serialize)]
struct Inn {
    iid: u64,
    inn_name: String,
    about: String,
    description: String,
    description_html: String,
    topics: Vec<String>,
    mods: Vec<u64>,
    inn_type: String,
    created_at: i64,
    // different user_type has different interval && should be in different tree
    //  post_interval: u16,
    //  comment_interval: u16,
}

#[derive(Encode, Decode, Serialize)]
struct Post {
    pid: u64,
    uid: u64,
    iid: u64,
    title: String,
    tags: Vec<String>,
    content: String,
    content_html: String,
    created_at: i64,
    is_locked: bool,
}

#[derive(Encode, Decode, Serialize)]
struct Comment {
    cid: u64,
    pid: u64,
    uid: u64,
    reply_to: Option<u64>,
    content: String,
    created_at: i64,
    is_collapsed: bool,
}

/// Go to source code to see default value: [SiteConfig::default()]
// TODO: recaptcha && configuration
// TODO: validate
#[derive(Serialize, Deserialize, Encode, Decode, Validate)]
pub(super) struct SiteConfig {
    site_name: String,
    description: String,
    read_only: bool,
    title_max_length: usize,
    article_max_length: usize,
    comment_max_length: usize,
    solo_interval: usize,
    post_interval: usize,
    comment_interval: usize,
    per_page: usize,
    static_page: usize,
}

#[derive(Encode, Decode)]
struct Claim {
    uid: u64,
    username: String,
    role: u8,
    exp: i64,
}

use crate::{config::CONFIG, error::AppError, VERSION};
use askama::Template;
use axum::{
    async_trait,
    body::{self, BoxBody, Empty, Full},
    extract::{ContentLengthLimit, Form, FromRequest, Multipart, Query, RequestParts},
    headers::{Cookie, HeaderName},
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get_service, MethodRouter},
    BoxError, Extension, TypedHeader,
};
use bincode::config::standard;
use bincode::{Decode, Encode};
use comrak::{
    markdown_to_html_with_plugins, plugins::syntect::SyntectAdapter, ComrakOptions, ComrakPlugins,
};
use data_encoding::HEXLOWER;
use http_body::Body;
use nanoid::nanoid;
use once_cell::sync::Lazy;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sled::{Db, IVec, Iter, Tree};
use std::{env, fs::File, io, iter::Rev};
use time::OffsetDateTime;
use tokio::{fs, signal};
use tower_http::services::ServeDir;
use validator::Validate;

pub(super) mod admin;
pub(super) mod inn;
pub(super) mod solo;
pub(super) mod user;

/// Returns SHA256 of the current running executable.
/// Cookbook: [Calculate the SHA-256 digest of a file](https://rust-lang-nursery.github.io/rust-cookbook/cryptography/hashing.html)
pub(super) static CURRENT_SHA256: Lazy<String> = Lazy::new(|| {
    let path = env::current_exe().unwrap();
    let mut file = File::open(path).unwrap();
    let mut hasher = Sha256::new();
    io::copy(&mut file, &mut hasher).unwrap();
    let hash = hasher.finalize();

    HEXLOWER.encode(hash.as_ref())
});

static SEP: Lazy<IVec> = Lazy::new(|| IVec::from("#"));

fn into_response<T: Template>(t: &T, ext: &str) -> Response<BoxBody> {
    match t.render() {
        Ok(body) => Response::builder()
            .status(StatusCode::OK)
            .header("content-type", ext.to_string())
            .body(body::boxed(Full::from(body)))
            .unwrap(),
        Err(err) => Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(body::boxed(Full::from(format!("{err}"))))
            .unwrap(),
    }
}

pub(super) struct ValidatedForm<T>(pub(super) T);

#[async_trait]
impl<T, B> FromRequest<B> for ValidatedForm<T>
where
    T: DeserializeOwned + Validate,
    B: Body + Send,
    B::Data: Send,
    B::Error: Into<BoxError>,
{
    type Rejection = AppError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let Form(value) = Form::<T>::from_request(req).await?;
        value.validate()?;
        Ok(ValidatedForm(value))
    }
}

pub(crate) async fn home(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));
    if claim.is_some() {
        Ok(Redirect::to("/inn/0"))
    } else {
        Ok(Redirect::to("/static/inn/0/1/index.html"))
    }
}

#[derive(Deserialize)]
pub(crate) struct UploadPicParams {
    page_type: String,
    iid: Option<u64>,
}

/// `POST /mod/inn_icon` && `/user/avatar`
pub(crate) async fn upload_pic_post(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<UploadPicParams>,
    ContentLengthLimit(mut multipart): ContentLengthLimit<Multipart, { 3 * 1024 * 1024 }>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let target;
    let fname = match params.page_type.as_str() {
        "inn" => {
            if let Some(iid) = params.iid {
                let inn: Inn = get_one(&db, "inns", iid)?;
                if !inn.mods.contains(&claim.uid) {
                    return Err(AppError::Unauthorized);
                }
                target = format!("/mod/{}", iid);
                format!("./static/inn_icons/{}.png", iid)
            } else {
                return Err(AppError::NotFound);
            }
        }
        "user" => {
            target = "/user/setting".to_string();
            format!("./static/avatars/{}.png", claim.uid)
        }
        _ => unreachable!(),
    };

    if let Some(field) = multipart.next_field().await.unwrap() {
        let data = field.bytes().await.unwrap();
        let image_format_detected = image::guess_format(&data)?;
        image::load_from_memory_with_format(&data, image_format_detected)?;
        fs::write(fname, &data).await.unwrap();
    }

    Ok(Redirect::to(&target))
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
    let srv = get_service(ServeDir::new(path));
    srv.handle_error(|error: std::io::Error| async move {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Unhandled internal error: {}", error),
        )
    })
}

// TODO: CSS Better style
pub(super) async fn handler_404() -> impl IntoResponse {
    let html = format!(
        r#"<strong>Error:</strong>
        <p>{}</p>
        <p><a href="/">Home</p>"#,
        StatusCode::NOT_FOUND
    );
    let body = Html(html);
    (StatusCode::NOT_FOUND, body)
}

pub(crate) async fn main_css() -> (HeaderMap, String) {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/css"),
    );

    (headers, include_str!("../../css/main.css").to_string())
}

pub(crate) async fn bulma_css() -> (HeaderMap, String) {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/css"),
    );

    (headers, include_str!("../../css/bulma.min.css").to_string())
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

/// # notification
///
/// - Someone comments on your article
/// - Someone mentions you in a comment
///
/// ## notification_code
///
/// unread_code + 100 = read_code
///
/// |         | unread | read |
/// |---------|--------|------|
/// | comment | 0      | 100  |
/// | post    | 1      | 101  |
struct Notification {
    uid: u64,
    username: String,
    iid: u64,
    pid: u64,
    post_title: String,
    cid: u64,
    comment_content: String,
    notification_code: u8,
}

/// notification.html
#[derive(Template)]
#[template(path = "notification.html", escape = "none")]
struct NotificationPage<'a> {
    page_data: PageData<'a>,
    notifications: Vec<Notification>,
}

#[derive(Deserialize)]
pub(crate) struct NotifyParams {
    op_type: Option<String>,
    pid: Option<u64>,
    cid: Option<u64>,
}

/// work for [set_merge_operator](https://docs.rs/sled/latest/sled/struct.Db.html#method.set_merge_operator):
/// update notification code to read.
fn mark_read(old: Option<&[u8]>) -> Option<Vec<u8>> {
    match old {
        Some(bytes) => {
            if bytes[0] < 100 {
                Some([bytes[0] + 100].to_vec())
            } else {
                Some(bytes.to_vec())
            }
        }
        None => None,
    }
}

/// `GET /notification`
///
/// Batch mode:
///
/// 30 notifications in a batch and batch delete only if they has been marked read
pub(super) async fn notification(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<NotifyParams>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let prefix = u64_to_ivec(claim.uid);
    let tree = db.open_tree("notifications")?;

    // kv_paire: uid#pid#cid = notification_code
    if let Some(op_type) = params.op_type {
        match op_type.as_str() {
            "mark_batch" => {
                for (n, i) in tree.scan_prefix(&prefix).enumerate() {
                    let (key, _) = i?;
                    tree.update_and_fetch(key, mark_read)?;
                    if n >= 30 {
                        break;
                    }
                }
            }
            "delete_batch" => {
                for (n, i) in tree.scan_prefix(&prefix).enumerate() {
                    let (key, value) = i?;
                    // Delete notification if it is read
                    if value[0] >= 100 {
                        tree.remove(key)?;
                    }
                    if n >= 30 {
                        break;
                    }
                }
            }
            "mark" => {
                if let (Some(pid), Some(cid)) = (params.pid, params.cid) {
                    let k = [
                        &u64_to_ivec(claim.uid),
                        &SEP,
                        &u64_to_ivec(pid),
                        &SEP,
                        &u64_to_ivec(cid),
                    ]
                    .concat();
                    tree.update_and_fetch(k, mark_read)?;
                }
            }
            "delete" => {
                if let (Some(pid), Some(cid)) = (params.pid, params.cid) {
                    let k = [
                        &u64_to_ivec(claim.uid),
                        &SEP,
                        &u64_to_ivec(pid),
                        &SEP,
                        &u64_to_ivec(cid),
                    ]
                    .concat();
                    tree.remove(k)?;
                }
            }
            _ => {}
        }
    }

    let mut notifications = Vec::with_capacity(30);
    for (n, i) in tree.scan_prefix(&prefix).enumerate() {
        let (key, value) = i?;
        let pid = u8_slice_to_u64(&key[9..17]);
        let cid = u8_slice_to_u64(&key[18..26]);

        let k = [&u64_to_ivec(pid), &SEP, &u64_to_ivec(cid)].concat();
        let v = &db.open_tree("post_comments")?.get(k)?;

        if let Some(v) = v {
            let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
            let post: Post = get_one(&db, "posts", pid)?;
            let user: User = get_one(&db, "users", comment.uid)?;
            let notification_code = value[0];
            let notification = Notification {
                uid: comment.uid,
                username: user.username,
                pid,
                iid: post.iid,
                post_title: post.title,
                cid,
                comment_content: comment.content,
                notification_code,
            };
            notifications.push(notification);
        }
        if n >= 30 {
            break;
        }
    }
    notifications.reverse();

    let has_unread = has_unread(&db, claim.uid)?;
    let page_data = PageData::new(
        "notification",
        &site_config.site_name,
        Some(claim),
        has_unread,
    );
    let notification_page = NotificationPage {
        page_data,
        notifications,
    };

    Ok(into_response(&notification_page, "html"))
}

struct PageData<'a> {
    title: &'a str,
    site_name: &'a str,
    claim: Option<Claim>,
    has_unread: bool,
    sha256: String,
    version: String,
    footer_links: Vec<(String, String)>,
}

impl<'a> PageData<'a> {
    fn new(title: &'a str, site_name: &'a str, claim: Option<Claim>, has_unread: bool) -> Self {
        let mut footer_links = vec![];
        for (path, _, link) in &CONFIG.serve_dir {
            if !link.is_empty() {
                footer_links.push((path.to_owned(), link.to_owned()));
            }
        }
        Self {
            title,
            site_name,
            claim,
            has_unread,
            sha256: CURRENT_SHA256.to_string(),
            version: VERSION.to_string(),
            footer_links,
        }
    }
}

/// generate a new nanoid with expiration time that is hex encoded.
///
/// format: "hex_timestamp#nanoid"
///
/// # Examples
///
/// ```no_run
/// // format like: "624e97ca#sSUl_K03nbUmPQLFe2CWk"
/// let nanoid = generate_nanoid_expire();
/// ```
fn generate_nanoid_expire(seconds: i64) -> String {
    let nanoid = nanoid!();
    let exp = OffsetDateTime::now_utc().unix_timestamp() + seconds;
    format!("{:x}#{}", exp, nanoid)
}

/// Update the counter and return the new id. It is contiguous if every id is used.
///
/// # Examples
///
/// ```no_run
/// let new_user_id = incr_id(db, "users_count")?;
/// ```
fn incr_id<K>(tree: &Tree, key: K) -> Result<u64, AppError>
where
    K: AsRef<[u8]>,
{
    let ivec = tree.update_and_fetch(key, increment)?.unwrap();
    Ok(ivec_to_u64(&ivec))
}

/// work for [update_and_fetch](https://docs.rs/sled/latest/sled/struct.Db.html#method.update_and_fetch):
/// increment 1.
fn increment(old: Option<&[u8]>) -> Option<Vec<u8>> {
    let number = match old {
        Some(bytes) => {
            let array: [u8; 8] = bytes.try_into().unwrap();
            let number = u64::from_be_bytes(array);
            number + 1
        }
        None => 1,
    };

    Some(number.to_be_bytes().to_vec())
}

/// convert a `i64` timestamp to a date [`String`]
fn timestamp_to_date(timestamp: i64) -> Result<String, AppError> {
    let date = OffsetDateTime::from_unix_timestamp(timestamp);
    match date {
        Ok(timestamp) => Ok(timestamp.date().to_string()),
        Err(e) => Err(AppError::TimeError(e.to_string())),
    }
}

/// convert `u64` to [IVec]
fn u64_to_ivec(number: u64) -> IVec {
    IVec::from(number.to_be_bytes().to_vec())
}

/// convert [IVec] to u64
fn ivec_to_u64(iv: &IVec) -> u64 {
    u64::from_be_bytes(iv.to_vec().as_slice().try_into().unwrap())
}

/// convert `&[u8]` to `u64`
fn u8_slice_to_u64(bytes: &[u8]) -> u64 {
    u64::from_be_bytes(bytes.try_into().unwrap())
}

static MD_OPTION: Lazy<ComrakOptions> = Lazy::new(|| {
    let mut options = ComrakOptions::default();
    options.extension.strikethrough = true;
    options.extension.tagfilter = true;
    options.extension.table = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.extension.superscript = true;
    options.render.hardbreaks = true;
    options
});
/// convert latex and markdown to html
fn md2html(input: &str) -> Result<String, AppError> {
    let mut plugins = ComrakPlugins::default();
    let adapter = SyntectAdapter::new("InspiredGitHub");
    plugins.render.codefence_syntax_highlighter = Some(&adapter);

    let input = if input.contains('$') {
        latex2mathml::replace(input)?
    } else {
        return Ok(markdown_to_html_with_plugins(input, &MD_OPTION, &plugins));
    };

    if input.contains("<math") && input.contains("</math>") {
        let mut output = vec![];
        let mut contents = input.split("<math");
        let start = contents.next().unwrap_or_default();
        let start = markdown_to_html_with_plugins(start, &MD_OPTION, &plugins);
        output.push(
            start
                .trim()
                .to_string()
                .trim_end_matches("</p>")
                .to_string(),
        );

        for content in contents {
            let pairs = content.split_once("</math>");
            if let Some((math, res)) = pairs {
                output.push(" <math".to_string());
                output.push(math.to_owned());
                output.push("</math> ".to_string());
                let res = markdown_to_html_with_plugins(res, &MD_OPTION, &plugins);
                output.push(
                    res.trim()
                        .to_string()
                        .trim_start_matches("<p>")
                        .to_string()
                        .trim_end_matches("</p>")
                        .to_string(),
                );
            }
        }
        Ok(output.join(""))
    } else {
        Ok(markdown_to_html_with_plugins(&input, &MD_OPTION, &plugins))
    }
}

/// get uid by username
fn get_uid_by_name(db: &Db, name: &str) -> Result<Option<u64>, AppError> {
    let v = db.open_tree("usernames")?.get(name)?;
    Ok(v.map(|v| ivec_to_u64(&v)))
}

/// get [SiteConfig]
fn get_site_config(db: &Db) -> Result<SiteConfig, AppError> {
    let site_config = &db.get("site_config")?.unwrap_or_default();
    let (site_config, _): (SiteConfig, usize) =
        bincode::decode_from_slice(site_config, standard()).unwrap_or_default();
    Ok(site_config)
}

/// check if the user has unread notifications
fn has_unread(db: &Db, uid: u64) -> Result<bool, AppError> {
    let iter = db.open_tree("notifications")?.scan_prefix(u64_to_ivec(uid));
    for i in iter {
        let (_, v) = i?;
        if v[0] < 100 {
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
fn get_one<T>(db: &Db, tree_name: &str, id: u64) -> Result<T, AppError>
where
    T: Decode,
{
    let v = db.open_tree(tree_name)?.get(u64_to_ivec(id))?;
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
/// let comment_count = get_count(db, "post_comments_count", u64_to_ivec(pid))?
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
        Some(count) => ivec_to_u64(&count),
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
/// let prefix = [&u64_to_ivec(1), &SEP, &u64_to_ivec(3)].concat();
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
/// user_iins = get_ids_by_prefix(&db, "user_inns", u64_to_ivec(claim.uid), None).unwrap();
/// ```
fn get_ids_by_prefix(
    db: &Db,
    tree: &str,
    prefix: impl AsRef<[u8]>,
    page_params: Option<&ParamsPage>,
) -> Result<Vec<u64>, AppError> {
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
            let id = &k[prefix.as_ref().len() + 1..];
            res.push(u8_slice_to_u64(id));
        }
    } else {
        for i in iter {
            let (k, _) = i?;
            let id = &k[prefix.as_ref().len() + 1..];
            res.push(u8_slice_to_u64(id));
        }
    }

    Ok(res)
}

/// get user inn status
///
/// # Examples
///
/// ```no_run
/// let (uids, status) = get_inn_status_by_prefix(&db, "inn_users", id_ivec, Some(&page_params))?;
fn get_inn_status_by_prefix(
    db: &Db,
    tree: &str,
    prefix: impl AsRef<[u8]>,
    page_params: Option<&ParamsPage>,
) -> Result<(Vec<u64>, Vec<u8>), AppError> {
    let mut res = vec![];
    let mut status = vec![];
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
            let (k, v) = i?;
            let uid = &k[prefix.as_ref().len() + 1..];
            res.push(u8_slice_to_u64(uid));
            status.push(v[0]);
        }
    } else {
        for i in iter {
            let (k, v) = i?;
            let uid = &k[prefix.as_ref().len() + 1..];
            res.push(u8_slice_to_u64(uid));
            status.push(v[0]);
        }
    }

    Ok((res, status))
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
        let out: Result<T, AppError> = get_one(db, tree, i as u64);
        if let Ok(out) = out {
            output.push(out);
        }
    }
    if page_params.is_desc {
        output.reverse();
    }
    Ok(output)
}

fn set_index<V>(
    db: &Db,
    count_tree: &str,
    id: u64,
    index_tree: &str,
    target: V,
) -> Result<(), AppError>
where
    V: Into<IVec>,
{
    let id_ivec = u64_to_ivec(id);
    let idx = incr_id(&db.open_tree(count_tree)?, &id_ivec)?;
    let k = [&id_ivec, &SEP, &u64_to_ivec(idx)].concat();

    db.open_tree(index_tree)?.insert(k, target)?;
    Ok(())
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
    fn test_u64_to_ivec() {
        let iv = u64_to_ivec(12345678);
        assert_eq!(ivec_to_u64(&iv), 12345678);
    }

    #[test]
    fn test_extract_element() {
        let input = "hi, @cc this is a test. If no space at last, @notag";
        let out = extract_element(input, 3, '@');
        assert_eq!(out, vec!["cc"]);
    }
}
