use super::{
    db_utils::{ivec_to_u32, set_one_with_key, u8_slice_to_u32, IterType},
    fmt::ts_to_date,
    meta_handler::{into_response, PageData, ValidatedForm},
    user::Role,
    Claim, Feed, FormPost, Item, SiteConfig,
};
use crate::{
    controller::{Comment, Inn, Post, Solo, User},
    error::AppError,
    DB,
};
use askama::Template;
use axum::{
    extract::{Query, TypedHeader},
    headers::Cookie,
    response::{IntoResponse, Redirect},
};
use bincode::config::standard;
use serde::Deserialize;
use snailquote::unescape;

/// Page data: `admin_view.html`
#[derive(Template)]
#[template(path = "admin_view.html", escape = "none")]
struct PageAdminView<'a> {
    page_data: PageData<'a>,
    tree_names: Vec<String>,
    ones: Vec<String>,
    tree_name: String,
    anchor: usize,
    is_desc: bool,
    n: usize,
}

/// url params: `admin_view.html`
#[derive(Deserialize)]
pub(crate) struct ParamsAdminView {
    tree_name: Option<String>,
    anchor: Option<usize>,
    is_desc: Option<bool>,
}

/// `GET /admin/view`
pub(crate) async fn admin_view(
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<ParamsAdminView>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);

    let mut tree_names = Vec::with_capacity(64);
    for i in DB.tree_names() {
        let name = String::from_utf8_lossy(&i);
        tree_names.push(name.to_string());
    }
    tree_names.sort_unstable();

    let mut ones = Vec::with_capacity(n);
    let tree_name = params
        .tree_name
        .unwrap_or_else(|| "__sled__default".to_owned());

    if tree_names.contains(&tree_name) {
        let tree = DB.open_tree(&tree_name)?;
        let iter = if is_desc {
            IterType::Rev(tree.iter().rev())
        } else {
            IterType::Iter(tree.iter())
        };

        for (idx, i) in iter.enumerate() {
            if idx < anchor {
                continue;
            }

            if idx >= anchor + n {
                break;
            }

            let (k, v) = i?;
            match tree_name.as_str() {
                "__sled__default" => {
                    let key = String::from_utf8_lossy(&k);
                    if key == "site_config" {
                        let (site_config, _): (SiteConfig, usize) =
                            bincode::decode_from_slice(&v, standard()).unwrap_or_default();
                        ones.push(format!("{key}: {site_config:?}"));
                    } else {
                        let v = ivec_to_u32(&v);
                        ones.push(format!("{key}: {v}"));
                    };
                }
                "users" => {
                    let key = ivec_to_u32(&k);
                    let (mut one, _): (User, usize) = bincode::decode_from_slice(&v, standard())?;
                    one.password_hash = String::from("******");
                    one.recovery_hash = None;
                    let one_fmt = unescape(&format!("{:?}", one)).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "solos" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Solo, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{:?}", one)).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "inns" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Inn, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{:?}", one)).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "posts" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Post, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{:?}", one)).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "post_comments" => {
                    let pid = u8_slice_to_u32(&k[0..4]);
                    let cid = u8_slice_to_u32(&k[4..8]);
                    let (one, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{:?}", one)).unwrap();
                    ones.push(format!("pid: {pid}, cid: {cid}, comment: {one_fmt}",));
                }
                "user_comments" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let pid = u8_slice_to_u32(&k[4..8]);
                    let cid = u8_slice_to_u32(&k[8..12]);
                    ones.push(format!("uid: {uid}, pid: {pid}, cid: {cid}"));
                }
                "comment_upvotes" | "comment_downvotes" => {
                    let pid = u8_slice_to_u32(&k[0..4]);
                    let cid = u8_slice_to_u32(&k[4..8]);
                    let uid = u8_slice_to_u32(&k[8..12]);
                    ones.push(format!("pid: {pid}, cid: {cid}, uid: {uid}"));
                }
                "post_timeline_idx" => {
                    let id = u8_slice_to_u32(&k[0..4]);
                    let idx = u8_slice_to_u32(&k[4..8]);
                    let v = ivec_to_u32(&v);
                    ones.push(format!("id: {id}, idx: {idx}, target: {v}"));
                }
                "user_posts" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let pid = u8_slice_to_u32(&k[4..8]);
                    let iid = u8_slice_to_u32(&v[0..4]);
                    let visibility = u8_slice_to_u32(&v[4..8]);
                    ones.push(format!(
                        "uid: {uid},  iid: {iid}, pid: {pid}, visibility: {visibility}"
                    ));
                }
                "post_comments_count" | "post_pageviews" => {
                    let id = u8_slice_to_u32(&k);
                    let count = ivec_to_u32(&v);
                    ones.push(format!("id: {id}, count: {count}"));
                }
                "hashtags" | "topics" | "tags" => {
                    let len = k.len();
                    let str = String::from_utf8_lossy(&k[0..len - 4]);
                    let id = u8_slice_to_u32(&k[len - 4..]);
                    ones.push(format!("{str}#{id}"));
                }
                "user_following" | "user_followers" | "mod_inns" | "user_inns" | "inn_users"
                | "inn_apply" | "post_upvotes" | "post_downvotes" | "user_solos_like"
                | "inn_posts" | "solo_users_like" | "feed_items" | "read" | "star"
                | "inn_feeds" | "inn_items" | "post_pins" => {
                    let id1 = u8_slice_to_u32(&k[0..4]);
                    let id2 = u8_slice_to_u32(&k[4..8]);
                    ones.push(format!("k: {id1}#{id2}, v: {v:?}"));
                }
                "user_stats" => {
                    let mut k_str = std::str::from_utf8(&k)?.split('_');
                    let timestamp = i64::from_str_radix(k_str.next().unwrap(), 16).unwrap();
                    let date = ts_to_date(timestamp);
                    let uid = k_str.next().unwrap();
                    let stat_type = k_str.next().unwrap().to_owned();
                    let count = ivec_to_u32(&v);
                    ones.push(format!(
                        "<a href='/user/{uid}'>{uid}</a> - {date} - {stat_type} - {count}"
                    ));
                }
                "inn_names" | "usernames" | "feed_links" | "item_links" => {
                    let name = std::str::from_utf8(&k)?;
                    let id = u8_slice_to_u32(&v);
                    ones.push(format!("name: {name}, id: {id}"));
                }
                "inns_private" => {
                    let id = u8_slice_to_u32(&k);
                    ones.push(format!("id: {id}"));
                }
                "user_solos" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let sid = u8_slice_to_u32(&k[4..8]);
                    let visibility = u8_slice_to_u32(&v);
                    ones.push(format!("uid: {uid}, sid: {sid}, visibility: {visibility}"));
                }
                "solo_timeline" => {
                    let sid = u8_slice_to_u32(&k[0..4]);
                    let uid = u8_slice_to_u32(&v[0..4]);
                    let visibility = u8_slice_to_u32(&v[4..8]);
                    ones.push(format!("sid: {sid}, uid: {uid}, visibility: {visibility}"));
                }
                "notifications" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let nid = u8_slice_to_u32(&k[4..8]);
                    let nt_type = &k[8];

                    let id1 = u8_slice_to_u32(&v[0..4]);
                    let id2 = u8_slice_to_u32(&v[4..8]);
                    let is_read = v[8] == 1;

                    ones.push(format!(
                        "uid: {uid}, nid: {nid}, nt_type: {nt_type}, id1:{id1}, id2:{id2}, is_read:{is_read}"
                    ));
                }
                "captcha" | "sessions" => {
                    let k_str = std::str::from_utf8(&k)?;
                    let time_stamp = k_str
                        .split_once('_')
                        .and_then(|s| i64::from_str_radix(s.0, 16).ok())
                        .unwrap();
                    ones.push(format!("timestamp: {time_stamp}"));
                }
                "post_timeline" => {
                    let timestamp = i64::from(u8_slice_to_u32(&k[0..4]));
                    let date = ts_to_date(timestamp);
                    let iid = u8_slice_to_u32(&k[4..8]);
                    let pid = u8_slice_to_u32(&k[8..12]);
                    let visibility = u8_slice_to_u32(&v);
                    ones.push(format!("{date} - {iid} - {pid} - {visibility}"));
                }
                "user_uploads" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let img_id = u8_slice_to_u32(&k[4..8]);
                    let img = String::from_utf8_lossy(&v);
                    ones.push(format!("{uid} - {img_id} - {img}"));
                }
                "user_folders" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let folder = String::from_utf8_lossy(&k[4..(k.len() - 4)]).to_string();
                    let feed_id = u8_slice_to_u32(&k[(k.len() - 4)..]);
                    let is_public = v[0] == 1;
                    ones.push(format!("{uid} - {folder} - {feed_id} - {is_public}"));
                }
                "feeds" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Feed, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{:?}", one)).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "feed_errs" => {
                    let feed_id = ivec_to_u32(&k);
                    let err = String::from_utf8_lossy(&v);
                    ones.push(format!("{feed_id}: {err}"));
                }
                "drafts" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let (draft, _): (FormPost, usize) = bincode::decode_from_slice(&v, standard())?;
                    ones.push(format!("{uid}: {draft:?}"));
                }
                "items" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Item, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{:?}", one)).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "home_pages" => {
                    let uid = u8_slice_to_u32(&k);
                    ones.push(format!("{uid}: {}", v[0]));
                }
                "tan" => {
                    let id = String::from_utf8_lossy(&k);
                    ones.push(format!("{id}: {:?}", v));
                }
                _ => ones.push(format!("{tree_name} has not been supported yet")),
            }
        }
    }

    let has_unread = User::has_unread(&DB, claim.uid)?;
    let page_data = PageData::new("Admin view", &site_config, Some(claim), has_unread);
    let page_admin_view = PageAdminView {
        page_data,
        tree_names,
        ones,
        tree_name,
        anchor,
        is_desc,
        n,
    };
    Ok(into_response(&page_admin_view))
}

/// Page data: `admin.html`
#[derive(Template)]
#[template(path = "admin.html")]
struct PageAdmin<'a> {
    site_config: &'a SiteConfig,
    page_data: PageData<'a>,
}

/// `GET /admin/site_setting`
pub(crate) async fn admin(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    let has_unread = User::has_unread(&DB, claim.uid)?;
    let page_data = PageData::new("Admin", &site_config, Some(claim), has_unread);
    let page_admin = PageAdmin {
        site_config: &site_config,
        page_data,
    };
    Ok(into_response(&page_admin))
}

/// `POST /admin`
pub(crate) async fn admin_post(
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<SiteConfig>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&DB, &cookie, &input).ok_or(AppError::NonLogin)?;
    if Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    set_one_with_key(&DB, "__sled__default", "site_config", &input)?;
    Ok(Redirect::to("/admin"))
}

impl Default for SiteConfig {
    fn default() -> Self {
        SiteConfig {
            site_name: "freedit".into(),
            domain: "http://127.0.0.1:3001".into(),
            description: "a forum powered by rust".into(),
            read_only: false,
            inn_mod_max: 5,
            title_max_length: 100,
            article_max_length: 65535,
            comment_max_length: 10_000,
            solo_interval: 5,
            post_interval: 10,
            comment_interval: 10,
            per_page: 30,
            captcha_difficulty: "Easy".into(),
            captcha_name: "Lucy".into(),
            home_page: 0,
        }
    }
}
