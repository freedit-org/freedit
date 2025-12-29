use super::{
    Claim, Comment, Feed, FormPost, Inn, Item, Post, SiteConfig, Solo, User,
    db_utils::{IterType, get_range, ivec_to_u32, set_one_with_key, u8_slice_to_u32},
    filters,
    fmt::{clean_html, ts_to_date},
    inn::ParamsTag,
    meta_handler::{PageData, ParamsPage, ValidatedForm, into_response},
    user::Role,
};
use crate::{DB, error::AppError};
use askama::Template;
use axum::{
    extract::Query,
    response::{IntoResponse, Redirect},
};
use axum_extra::{TypedHeader, headers::Cookie};
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
    for i in DB.list_partitions() {
        let name = String::from_utf8_lossy(i.as_bytes());
        tree_names.push(name.to_string());
    }
    tree_names.sort_unstable();

    let mut ones = Vec::with_capacity(n);
    let tree_name = params.tree_name.unwrap_or_else(|| "default".to_owned());

    if tree_names.contains(&tree_name) {
        let tree = DB.open_partition(&tree_name, Default::default())?;
        let iter = tree.inner().iter();
        let iter = if is_desc {
            IterType::Rev(iter.rev())
        } else {
            IterType::Fwd(iter)
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
                "default" => {
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
                    let (one, _): (User, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{one:?}")).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "solos" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Solo, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{one:?}")).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "inns" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Inn, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{one:?}")).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "posts" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Post, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{one:?}")).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "post_comments" => {
                    let pid = u8_slice_to_u32(&k[0..4]);
                    let cid = u8_slice_to_u32(&k[4..8]);
                    let (one, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{one:?}")).unwrap();
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
                    let timestamp = u8_slice_to_u32(&v[0..4]);
                    let inn_type = v[4];
                    ones.push(format!(
                        "id: {id}, idx: {idx}, timestamp: {timestamp}, inn_type: {inn_type}"
                    ));
                }
                "user_posts" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let pid = u8_slice_to_u32(&k[4..8]);
                    let iid = u8_slice_to_u32(&v[0..4]);
                    let inn_type = v[4];
                    ones.push(format!(
                        "uid: {uid},  iid: {iid}, pid: {pid}, inn_type: {inn_type}"
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
                "user_following" | "user_followers" | "user_message" | "mod_inns" | "user_inns"
                | "inn_users" | "inn_apply" | "post_upvotes" | "post_downvotes"
                | "user_solos_like" | "inn_posts" | "solo_users_like" | "feed_items" | "read"
                | "star" | "inn_feeds" | "inn_items" | "post_pins" => {
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
                "user_solos" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let sid = u8_slice_to_u32(&k[4..8]);
                    let solo_type = u8_slice_to_u32(&v);
                    ones.push(format!("uid: {uid}, sid: {sid}, solo_type: {solo_type}"));
                }
                "solo_timeline" => {
                    let sid = u8_slice_to_u32(&k[0..4]);
                    let uid = u8_slice_to_u32(&v[0..4]);
                    let solo_type = u8_slice_to_u32(&v[4..8]);
                    ones.push(format!("sid: {sid}, uid: {uid}, solo_type: {solo_type}"));
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
                    let inn_type = v[0];
                    ones.push(format!("{date} - {iid} - {pid} - {inn_type}"));
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
                    let one_fmt = unescape(&format!("{one:?}")).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "feed_errs" => {
                    let id = ivec_to_u32(&k);
                    let msg = String::from_utf8_lossy(&v);
                    ones.push(format!("{id}: {msg}"));
                }
                "drafts" => {
                    let uid = u8_slice_to_u32(&k[0..4]);
                    let (draft, _): (FormPost, usize) = bincode::decode_from_slice(&v, standard())?;
                    ones.push(format!("{uid}: {draft:?}"));
                }
                "items" => {
                    let key = ivec_to_u32(&k);
                    let (one, _): (Item, usize) = bincode::decode_from_slice(&v, standard())?;
                    let one_fmt = unescape(&format!("{one:?}")).unwrap();
                    ones.push(format!("{key}: {one_fmt}"));
                }
                "tan" => {
                    let id = String::from_utf8_lossy(&k);
                    ones.push(format!("{id}: {v:?}"));
                }
                "messages" => {
                    let mid = u8_slice_to_u32(&k);
                    let receiver_id = u8_slice_to_u32(&v[0..4]);
                    let sender_id = u8_slice_to_u32(&v[4..8]);
                    let msg = String::from_utf8_lossy(&v[8..]);
                    ones.push(format!("{mid} - {receiver_id} - {sender_id} - {msg}"));
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

    let mut site_config = input;
    site_config.spam_regex = match site_config.spam_regex {
        Some(s) if !s.is_empty() => {
            if let Err(e) = regex::Regex::new(&s) {
                return Err(AppError::Custom(e.to_string()));
            }
            Some(s)
        }
        _ => None,
    };

    site_config.site_name = clean_html(&site_config.site_name);
    site_config.domain = clean_html(&site_config.domain);
    site_config.description = clean_html(&site_config.description);
    site_config.captcha_difficulty = clean_html(&site_config.captcha_difficulty);
    site_config.captcha_name = clean_html(&site_config.captcha_name);
    site_config.tos_link = clean_html(&site_config.tos_link);

    set_one_with_key(&DB, "default", "site_config", &site_config)?;
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
            spam_regex: None,
            lang: "en".into(),
            tos_link: "https://en.wikipedia.org/wiki/WTFPL".into(),
            custom_footer_code: None,
            login_captcha: false,
        }
    }
}

/// Page data: `admin_gallery.html`
#[derive(Template)]
#[template(path = "admin_gallery.html")]
struct PageAdminGallery<'a> {
    page_data: PageData<'a>,
    imgs: Vec<(u32, u32, String)>,
    anchor: usize,
    is_desc: bool,
    n: usize,
}

/// `GET /admin/gallery`
pub(crate) async fn admin_gallery(
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<ParamsTag>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    let has_unread = User::has_unread(&DB, claim.uid)?;

    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let n = 12;

    let mut imgs = Vec::new();
    let ks = DB.open_partition("user_uploads", Default::default())?;
    for i in ks.inner().iter() {
        let (k, v) = i?;
        let uid = u8_slice_to_u32(&k[0..4]);
        let img_id = u8_slice_to_u32(&k[4..8]);
        let img = String::from_utf8_lossy(&v).to_string();
        imgs.push((uid, img_id, img));
    }

    imgs.sort_unstable_by(|a, b| a.1.cmp(&b.1));

    let page_params = ParamsPage { anchor, n, is_desc };
    let count = imgs.len();
    let (start, end) = get_range(count, &page_params);

    let mut imgs = imgs[(start - 1)..end].to_vec();
    if is_desc {
        imgs.reverse();
    }

    let page_data = PageData::new("Admin gallery", &site_config, Some(claim), has_unread);
    let page_gallery = PageAdminGallery {
        page_data,
        imgs,
        anchor,
        is_desc,
        n,
    };

    Ok(into_response(&page_gallery))
}
