use super::{
    get_site_config, into_response, timestamp_to_date, u8_slice_to_u64, Claim, IterType, PageData,
    SiteConfig, ValidatedForm,
};
use crate::{
    controller::{ivec_to_u64, Comment, Inn, Post, Solo, User},
    error::AppError,
};
use askama::Template;
use axum::{
    extract::{Extension, Query, TypedHeader},
    headers::Cookie,
    response::{IntoResponse, Redirect},
};
use bincode::config::standard;
use serde::Deserialize;
use sled::Db;

#[derive(Template)]
#[template(path = "admin_view.html", escape = "none")]
struct AdminViewPage<'a> {
    page_data: PageData<'a>,
    tree_names: Vec<String>,
    ones: Vec<String>,
    tree_name: String,
    anchor: usize,
    is_desc: bool,
    n: usize,
}

/// url params: admin_view.html
#[derive(Deserialize)]
pub(crate) struct ParamsAdminView {
    tree_name: Option<String>,
    anchor: Option<usize>,
    is_desc: Option<bool>,
}

/// `GET /admin/view` admin view kv database
pub(crate) async fn admin_view(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<ParamsAdminView>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if claim.role != u8::MAX {
        return Err(AppError::Unauthorized);
    }

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);

    let mut tree_names = Vec::with_capacity(64);
    for i in db.tree_names() {
        let name = String::from_utf8_lossy(&i);
        tree_names.push(name.to_string());
    }
    tree_names.sort_unstable();

    let mut ones = Vec::with_capacity(n);
    let tree_name = params
        .tree_name
        .unwrap_or_else(|| "__sled__default".to_owned());

    if tree_names.contains(&tree_name) {
        let tree = db.open_tree(&tree_name)?;
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
                        let json = serde_json::to_string_pretty(&site_config).unwrap();
                        ones.push(format!("{}: {}", key, json));
                    } else {
                        let v = ivec_to_u64(&v);
                        ones.push(format!("{}: {}", key, v));
                    };
                }
                "users" => {
                    let key = ivec_to_u64(&k);
                    let (mut one, _): (User, usize) = bincode::decode_from_slice(&v, standard())?;
                    one.password_hash = String::from("******");
                    let json = serde_json::to_string_pretty(&one).unwrap();
                    let json = json.replace("\\\"", "'");
                    ones.push(format!("{}: {}", key, json));
                }
                "solos" => {
                    let key = ivec_to_u64(&k);
                    let (one, _): (Solo, usize) = bincode::decode_from_slice(&v, standard())?;
                    let json = serde_json::to_string_pretty(&one).unwrap();
                    let json = json.replace("\\\"", "'");
                    ones.push(format!("{}: {}", key, json));
                }
                "inns" => {
                    let key = ivec_to_u64(&k);
                    let (mut one, _): (Inn, usize) = bincode::decode_from_slice(&v, standard())?;
                    one.description_html = "".to_string();
                    let json = serde_json::to_string_pretty(&one).unwrap();
                    ones.push(format!("{}: {}", key, json));
                }
                "posts" => {
                    let key = ivec_to_u64(&k);
                    let (mut one, _): (Post, usize) = bincode::decode_from_slice(&v, standard())?;
                    one.content_html = "".to_string();
                    let json = serde_json::to_string_pretty(&one).unwrap();
                    ones.push(format!("{}: {}", key, json));
                }
                "post_comments" => {
                    let pid = u8_slice_to_u64(&k[0..8]);
                    let cid = u8_slice_to_u64(&k[9..17]);
                    let (one, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
                    let json = serde_json::to_string_pretty(&one).unwrap();
                    let json = json.replace("\\\"", "'");
                    ones.push(format!("pid: {}, cid: {}, comment: {}", pid, cid, json));
                }
                "comment_upvotes" | "comment_downvotes" => {
                    let pid = u8_slice_to_u64(&k[0..8]);
                    let cid = u8_slice_to_u64(&k[9..17]);
                    let uid = u8_slice_to_u64(&k[18..26]);
                    ones.push(format!("pid: {}, cid: {}, uid: {}", pid, cid, uid));
                }
                "inn_posts_idx" | "post_timeline_idx" => {
                    let id = u8_slice_to_u64(&k[0..8]);
                    let idx = u8_slice_to_u64(&k[9..17]);
                    let v = ivec_to_u64(&v);
                    ones.push(format!("id: {}, idx: {}, target: {}", id, idx, v));
                }
                "user_solos_idx" | "user_comments_idx" => {
                    let uid = u8_slice_to_u64(&k[0..8]);
                    let idx = u8_slice_to_u64(&k[9..17]);
                    let sid = u8_slice_to_u64(&k[0..8]);
                    let visibility = u8_slice_to_u64(&k[9..17]);
                    ones.push(format!(
                        "uid: {}, idx: {}, sid: {}, visibility: {}",
                        uid, idx, sid, visibility
                    ));
                }
                "user_posts_idx" => {
                    let uid = u8_slice_to_u64(&k[0..8]);
                    let idx = u8_slice_to_u64(&k[9..17]);
                    let iid = u8_slice_to_u64(&v[0..8]);
                    let pid = u8_slice_to_u64(&v[9..17]);
                    let visibility = u8_slice_to_u64(&v[18..26]);
                    ones.push(format!(
                        "uid: {}, idx: {}, iid: {}, pid: {}, visibility: {}",
                        uid, idx, iid, pid, visibility
                    ));
                }
                "user_solos_count"
                | "inn_posts_count"
                | "user_posts_count"
                | "post_comments_count"
                | "user_comments_count" => {
                    let id = u8_slice_to_u64(&k);
                    let count = ivec_to_u64(&v);
                    ones.push(format!("id: {}, count: {}", id, count));
                }
                "hashtags" | "topics" | "tags" => {
                    let len = k.len();
                    let str = String::from_utf8_lossy(&k[0..len - 9]);
                    let id = u8_slice_to_u64(&k[len - 8..]);
                    ones.push(format!("{}#{}", str, id));
                }
                "user_following" | "user_followers" | "mod_inns" | "user_inns" | "inn_users"
                | "post_upvotes" | "post_downvotes" | "user_solos_like" | "solo_users_like" => {
                    let id1 = u8_slice_to_u64(&k[0..8]);
                    let id2 = u8_slice_to_u64(&k[9..17]);
                    ones.push(format!("k: {}#{}, v: {:?}", id1, id2, v));
                }
                "user_pageviews" => {
                    let k_str = std::str::from_utf8(&k)?.split_once('_').unwrap();
                    let timestamp = i64::from_str_radix(k_str.0, 16).unwrap();
                    let date = timestamp_to_date(timestamp)?;
                    let uid = k_str.1;
                    let count = ivec_to_u64(&v);
                    ones.push(format!("user: {}, date: {}, count: {}", uid, date, count));
                }
                "inn_names" | "usernames" => {
                    let name = std::str::from_utf8(&k)?;
                    let id = u8_slice_to_u64(&v);
                    ones.push(format!("name: {}, id: {}", name, id));
                }
                "static_user_post" | "static_inn_post" => {
                    let id = u8_slice_to_u64(&k);
                    ones.push(format!("id: {}", id));
                }
                "solo_timeline" => {
                    let sid = u8_slice_to_u64(&k[0..8]);
                    let uid = u8_slice_to_u64(&v[0..8]);
                    let visibility = u8_slice_to_u64(&v[9..17]);
                    ones.push(format!(
                        "sid: {}, uid: {}, visibility: {}",
                        sid, uid, visibility
                    ));
                }
                _ => ones.push(format!("{} has not been supported yet", tree_name)),
            }
        }
    }

    let page_data = PageData::new("Admin view", &site_config.site_name, Some(claim), false);
    let admin_view_page = AdminViewPage {
        page_data,
        tree_names,
        ones,
        tree_name,
        anchor,
        is_desc,
        n,
    };
    Ok(into_response(&admin_view_page, "html"))
}

#[derive(Template)]
#[template(path = "admin.html")]
struct AdminPage<'a> {
    site_config: &'a SiteConfig,
    page_data: PageData<'a>,
}

/// `GET /admin/site_setting`
pub(crate) async fn admin(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if claim.role != u8::MAX {
        return Err(AppError::Unauthorized);
    }

    let page_data = PageData::new("Admin", &site_config.site_name, Some(claim), false);
    let admin_page = AdminPage {
        site_config: &site_config,
        page_data,
    };
    Ok(into_response(&admin_page, "html"))
}

/// `POST /admin`
pub(crate) async fn admin_post(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<SiteConfig>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &input).ok_or(AppError::NonLogin)?;
    if claim.role != u8::MAX {
        return Err(AppError::Unauthorized);
    }

    let site_config = bincode::encode_to_vec(&input, standard())?;
    db.insert("site_config", site_config)?;
    Ok(Redirect::to("/admin"))
}

impl Default for SiteConfig {
    fn default() -> Self {
        SiteConfig {
            site_name: "freedit".to_string(),
            description: "a forum powered by rust".to_string(),
            read_only: false,
            title_max_length: 100,
            article_max_length: 65535,
            comment_max_length: 10_000,
            solo_interval: 10,
            post_interval: 30,
            comment_interval: 10,
            per_page: 30,
            static_page: 10,
        }
    }
}

#[derive(Template)]
#[template(path = "admin_pageview.html")]
struct AdminPageviewPage<'a> {
    page_data: PageData<'a>,
    pageviews: Vec<(String, String, u64)>,
}

pub(crate) async fn admin_pageview(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if claim.role != u8::MAX {
        return Err(AppError::Unauthorized);
    }

    let mut pageviews = Vec::with_capacity(100);
    for i in &db.open_tree("user_pageviews")? {
        let (k, v) = i?;
        let k_str = std::str::from_utf8(&k)?.split_once('_').unwrap();
        let timestamp = i64::from_str_radix(k_str.0, 16).unwrap();
        let date = timestamp_to_date(timestamp)?;
        let uid = k_str.1.to_owned();
        let count = ivec_to_u64(&v);
        pageviews.push((uid, date, count));
    }

    pageviews.sort_unstable_by(|a, b| b.2.cmp(&a.2));
    if pageviews.len() > 100 {
        pageviews.truncate(100);
    }

    let page_data = PageData::new("Admin-pageview", &site_config.site_name, Some(claim), false);
    let admin_pageview_page = AdminPageviewPage {
        page_data,
        pageviews,
    };
    Ok(into_response(&admin_pageview_page, "html"))
}
