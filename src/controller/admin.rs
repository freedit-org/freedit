use super::{
    get_site_config, into_response, u8_slice_to_u64, Claim, PageData, SiteConfig, ValidatedForm,
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
    tree_name: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct QueryTree {
    tree_name: Option<String>,
}

/// `GET /admin/view` admin view kv database
pub(crate) async fn admin_view(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(query_tree): Query<QueryTree>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if claim.role != u8::MAX {
        return Err(AppError::Unauthorized);
    }

    let mut tree_names = Vec::with_capacity(40);
    for i in db.tree_names() {
        let name = String::from_utf8_lossy(&i);
        tree_names.push(name.to_string());
    }
    tree_names.sort_unstable();

    let mut ones = Vec::new();
    if let Some(ref tree_name) = query_tree.tree_name {
        if tree_names.contains(tree_name) {
            for (n, i) in db.open_tree(tree_name)?.iter().enumerate() {
                if n >= 30 {
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
                        let (mut one, _): (User, usize) =
                            bincode::decode_from_slice(&v, standard())?;
                        one.password_hash = String::from("******");
                        one.salt = String::from("unique salt");
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
                        let (mut one, _): (Inn, usize) =
                            bincode::decode_from_slice(&v, standard())?;
                        one.description_html = "".to_string();
                        let json = serde_json::to_string_pretty(&one).unwrap();
                        ones.push(format!("{}: {}", key, json));
                    }
                    "posts" => {
                        let key = ivec_to_u64(&k);
                        let (mut one, _): (Post, usize) =
                            bincode::decode_from_slice(&v, standard())?;
                        one.content_html = "".to_string();
                        let json = serde_json::to_string_pretty(&one).unwrap();
                        ones.push(format!("{}: {}", key, json));
                    }
                    "post_comments" => {
                        let mut iter = k.splitn(2, |num| *num == 35);
                        let pid = u8_slice_to_u64(iter.next().unwrap());
                        let cid = u8_slice_to_u64(iter.next().unwrap());
                        let (one, _): (Comment, usize) =
                            bincode::decode_from_slice(&v, standard())?;
                        let json = serde_json::to_string_pretty(&one).unwrap();
                        let json = json.replace("\\\"", "'");
                        ones.push(format!("pid: {}, cid: {}, comment: {}", pid, cid, json));
                    }
                    "comment_upvotes" => {
                        let mut iter = k.splitn(3, |num| *num == 35);
                        let pid = u8_slice_to_u64(iter.next().unwrap());
                        let cid = u8_slice_to_u64(iter.next().unwrap());
                        let uid = u8_slice_to_u64(iter.next().unwrap());
                        ones.push(format!("pid: {}, cid: {}, uid: {}", pid, cid, uid));
                    }
                    "inn_posts_idx" => {
                        let mut iter = k.splitn(2, |num| *num == 35);
                        let id = u8_slice_to_u64(iter.next().unwrap());
                        let idx = u8_slice_to_u64(iter.next().unwrap());
                        let v = ivec_to_u64(&v);
                        ones.push(format!("id: {}, idx: {}, target: {}", id, idx, v));
                    }
                    "user_solos_idx" => {
                        let mut iter = k.splitn(2, |num| *num == 35);
                        let uid = u8_slice_to_u64(iter.next().unwrap());
                        let idx = u8_slice_to_u64(iter.next().unwrap());
                        let mut iter = v.splitn(2, |num| *num == 35);
                        let sid = u8_slice_to_u64(iter.next().unwrap());
                        let visibility = u8_slice_to_u64(iter.next().unwrap());
                        ones.push(format!(
                            "uid: {}, idx: {}, sid: {}, visibility: {}",
                            uid, idx, sid, visibility
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
                        let mut iter = k.splitn(2, |num| *num == 35);
                        let str = String::from_utf8_lossy(iter.next().unwrap());
                        let id = u8_slice_to_u64(iter.next().unwrap());
                        ones.push(format!("{}#{}", str, id));
                    }
                    "user_following" | "user_followers" | "mod_inns" | "user_inns"
                    | "inn_users" => {
                        let mut iter = k.splitn(2, |num| *num == 35);
                        let id1 = u8_slice_to_u64(iter.next().unwrap());
                        let id2 = u8_slice_to_u64(iter.next().unwrap());
                        ones.push(format!("k: {}#{}, v: {:?}", id1, id2, v));
                    }
                    _ => ones.push(format!("{} has not been supported yet", tree_name)),
                }
            }
        }
    }

    let page_data = PageData::new("Admin view", &site_config.site_name, Some(claim), false);
    let admin_view_page = AdminViewPage {
        page_data,
        tree_names,
        ones,
        tree_name: query_tree.tree_name,
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
            solo_interval: 20,
            post_interval: 60,
            comment_interval: 20,
            per_page: 30,
        }
    }
}