use super::{
    db_utils::{
        extract_element, get_count_by_prefix, get_id_by_name, get_ids_by_tag, get_range, set_one,
        IterType,
    },
    fmt::{md2html, ts_to_date},
    get_ids_by_prefix, get_one, incr_id, into_response, ivec_to_u32,
    meta_handler::{get_referer, PageData, ParamsPage, ValidatedForm},
    notification::{add_notification, mark_read, NtType},
    u32_to_ivec, u8_slice_to_u32, Claim, SiteConfig, Solo, User,
};
use crate::{error::AppError, DB};
use askama::Template;
use axum::{
    extract::{Path, Query, TypedHeader},
    headers::{Cookie, Referer},
    response::{IntoResponse, Redirect},
};
use chrono::Utc;
use serde::Deserialize;
use sled::Db;
use validator::Validate;

/// Form data: `/solo/user/:uid` solo create.
#[derive(Deserialize, Validate)]
pub(crate) struct FormSolo {
    #[validate(length(min = 1, max = 1000))]
    content: String,
    visibility: String,
    reply_to: u32,
}

/// Page data: `solo_list.html`
#[derive(Template)]
#[template(path = "solo_list.html", escape = "none")]
struct PageSoloList<'a> {
    page_data: PageData<'a>,
    solos: Vec<OutSolo>,
    uid: u32,
    username: String,
    anchor: usize,
    n: usize,
    is_desc: bool,
    is_following: bool,
    filter: Option<String>,
    hashtag: Option<String>,
}

/// Vec data: solo
struct OutSolo {
    uid: u32,
    sid: u32,
    username: String,
    content: String,
    created_at: String,
    visibility: u32,
    like: bool,
    like_count: usize,
    reply_to: Option<u32>,
    replies: Vec<u32>,
}

impl OutSolo {
    fn get(db: &Db, sid: u32, current_uid: Option<u32>) -> Result<Option<Self>, AppError> {
        let solo: Solo = get_one(db, "solos", sid)?;
        let user: User = get_one(db, "users", solo.uid)?;
        let date = ts_to_date(solo.created_at);

        if let Some(uid) = current_uid {
            if solo.visibility == 20 {
                if uid != solo.uid {
                    return Ok(None);
                }
            } else if solo.visibility == 10 && uid != solo.uid {
                let k = [&u32_to_ivec(solo.uid), &u32_to_ivec(uid)].concat();
                if !db.open_tree("user_followers")?.contains_key(k)? {
                    return Ok(None);
                }
            }
        } else if solo.visibility > 0 {
            return Ok(None);
        }

        let mut like = false;
        if let Some(uid) = current_uid {
            let k = [&u32_to_ivec(sid), &u32_to_ivec(uid)].concat();
            if db.open_tree("solo_users_like")?.contains_key(k)? {
                like = true;
            }
        }

        let like_count =
            get_count_by_prefix(db, "solo_users_like", &u32_to_ivec(sid)).unwrap_or_default();

        let out_solo = Self {
            uid: solo.uid,
            sid: solo.sid,
            username: user.username,
            content: solo.content,
            created_at: date,
            visibility: solo.visibility,
            like,
            like_count,
            reply_to: solo.reply_to,
            replies: solo.replies,
        };

        Ok(Some(out_solo))
    }
}

fn can_visit_solo(visibility: u32, followers: &[u32], solo_uid: u32, current_uid: u32) -> bool {
    visibility == 0
        || (visibility == 10 && followers.contains(&solo_uid))
        || (visibility == 20 && solo_uid == current_uid)
}

/// url params: `solo.html`
#[derive(Deserialize)]
pub(crate) struct ParamsSolo {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    filter: Option<String>,
    hashtag: Option<String>,
    nid: Option<u32>,
}

/// `GET /solo/user/:uid` solo page
pub(crate) async fn solo_list(
    cookie: Option<TypedHeader<Cookie>>,
    Path(u): Path<String>,
    Query(params): Query<ParamsSolo>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let uid = match u.parse::<u32>() {
        Ok(uid) => uid,
        Err(_) => get_id_by_name(&DB, "usernames", &u)?.ok_or(AppError::NotFound)?,
    };

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut is_following = false;
    let mut index = Vec::with_capacity(n);
    let mut followers = Vec::new();
    let mut current_uid = 0;
    if let Some(ref claim) = claim {
        let following_k = [&u32_to_ivec(claim.uid), &u32_to_ivec(uid)].concat();
        if DB.open_tree("user_following")?.contains_key(following_k)? {
            is_following = true;
        }

        if let Ok(v) = get_ids_by_prefix(&DB, "user_followers", u32_to_ivec(claim.uid), None) {
            followers = v;
        }
        current_uid = claim.uid;
        followers.push(claim.uid);
    }

    match params.filter.as_deref() {
        Some("Following") => {
            if let Some(ref claim) = claim {
                if let Ok(uids) =
                    get_ids_by_prefix(&DB, "user_following", u32_to_ivec(claim.uid), None)
                {
                    index = get_solos_by_uids(&DB, &uids, &followers, current_uid, &page_params)?;
                };
            }
        }
        Some("Like") => {
            if let Some(ref claim) = claim {
                if let Ok(sids) =
                    get_ids_by_prefix(&DB, "user_solos_like", u32_to_ivec(claim.uid), None)
                {
                    let (start, end) = get_range(sids.len(), &page_params);
                    index = sids[start - 1..end].to_vec();
                    if is_desc {
                        index.reverse();
                    }
                };
            }
        }
        _ => {
            if let Some(ref hashtag) = params.hashtag {
                index = get_ids_by_tag(&DB, "hashtags", hashtag, Some(&page_params))?;
            } else if uid == 0 {
                index = get_all_solos(&DB, "solo_timeline", &followers, current_uid, &page_params)?;
            } else {
                index = get_solos_by_uids(&DB, &[uid], &followers, current_uid, &page_params)?;
            }
        }
    }

    let mut out_solos = Vec::with_capacity(index.len());
    for sid in index {
        if let Some(out_solo) = OutSolo::get(&DB, sid, claim.as_ref().map(|c| c.uid))? {
            out_solos.push(out_solo);
        }
    }

    let filter = if claim.is_none() { None } else { params.filter };

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };

    let username = if uid > 0 {
        let user: User = get_one(&DB, "users", uid)?;
        user.username
    } else {
        "All".to_owned()
    };
    let page_data = PageData::new("Solo", &site_config, claim, has_unread);

    let page_solo_list = PageSoloList {
        page_data,
        solos: out_solos,
        uid,
        username,
        anchor,
        n,
        is_desc,
        is_following,
        filter,
        hashtag: params.hashtag,
    };
    Ok(into_response(&page_solo_list))
}

/// Page data: `solo.html`
#[derive(Template)]
#[template(path = "solo.html", escape = "none")]
struct PageSolo<'a> {
    page_data: PageData<'a>,
    solo: OutSolo,
    reply_solos: Vec<OutSolo>,
}

/// `GET /solo/:sid`
pub(crate) async fn solo(
    cookie: Option<TypedHeader<Cookie>>,
    Path(sid): Path<u32>,
    Query(params): Query<ParamsSolo>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let out_solo =
        OutSolo::get(&DB, sid, claim.as_ref().map(|c| c.uid))?.ok_or(AppError::NotFound)?;

    // TODO: Reply solos should be paginated
    let mut reply_solos = Vec::with_capacity(out_solo.replies.len());
    for i in &out_solo.replies {
        if let Ok(Some(out_solo)) = OutSolo::get(&DB, *i, claim.as_ref().map(|c| c.uid)) {
            reply_solos.push(out_solo);
        }
    }

    if let Some(nid) = params.nid {
        if let Some(ref claim) = claim {
            let prefix = [&u32_to_ivec(claim.uid), &u32_to_ivec(nid)].concat();
            let tree = DB.open_tree("notifications")?;
            for i in tree.scan_prefix(prefix) {
                let (k, _) = i?;
                tree.update_and_fetch(k, mark_read)?;
            }
        }
    }

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("Solo", &site_config, claim, has_unread);
    let page_solo = PageSolo {
        page_data,
        solo: out_solo,
        reply_solos,
    };

    Ok(into_response(&page_solo))
}

fn get_all_solos(
    db: &Db,
    timeline_tree: &str,
    followers: &[u32],
    current_uid: u32,
    page_params: &ParamsPage,
) -> Result<Vec<u32>, AppError> {
    let tree = db.open_tree(timeline_tree)?;
    let mut count: usize = 0;
    let mut result = Vec::with_capacity(page_params.n);

    let iter = if page_params.is_desc {
        IterType::Rev(tree.iter().rev())
    } else {
        IterType::Iter(tree.iter())
    };
    for i in iter {
        // kv_pair: sid = uid#visibility
        let (k, v) = i?;
        let solo_uid = u8_slice_to_u32(&v[0..4]);
        let visibility = u8_slice_to_u32(&v[4..8]);
        if can_visit_solo(visibility, followers, solo_uid, current_uid) {
            if count < page_params.anchor {
                count += 1;
                continue;
            }
            result.push(ivec_to_u32(&k));
        }

        if result.len() == page_params.n {
            break;
        }
    }
    Ok(result)
}

fn get_solos_by_uids(
    db: &Db,
    uids: &[u32],
    followers: &[u32],
    current_uid: u32,
    page_params: &ParamsPage,
) -> Result<Vec<u32>, AppError> {
    let mut sids = Vec::with_capacity(page_params.n);
    let user_solos_tree = db.open_tree("user_solos")?;
    for uid in uids {
        let prefix = u32_to_ivec(*uid);
        // kv_pair: uid#sid = visibility
        for i in user_solos_tree.scan_prefix(prefix) {
            let (k, v) = i?;
            let sid = u8_slice_to_u32(&k[4..8]);
            let visibility = u8_slice_to_u32(&v);
            if can_visit_solo(visibility, followers, *uid, current_uid) {
                sids.push(sid);
            }
        }
    }
    let (start, end) = get_range(sids.len(), page_params);
    sids = sids[start - 1..end].to_vec();
    if page_params.is_desc {
        sids.reverse();
    }
    Ok(sids)
}

/// `POST /solo/user/:uid` solo page
pub(crate) async fn solo_post(
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormSolo>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let created_at = Utc::now().timestamp();
    if created_at - claim.last_write < site_config.solo_interval {
        return Err(AppError::WriteInterval);
    }

    let visibility = match input.visibility.as_str() {
        "Everyone" => 0,
        "Following" => 10,
        "Just me" => 20,
        _ => unreachable!(),
    };

    let uid = claim.uid;

    let sid = incr_id(&DB, "solos_count")?;
    let sid_ivec = u32_to_ivec(sid);
    let mut content = input.content;
    let mut hashtags = Vec::new();

    let replied_uesr;
    let reply_to;
    if input.reply_to == 0 {
        replied_uesr = None;
        reply_to = None;
    } else {
        let mut solo_replied: Solo = get_one(&DB, "solos", input.reply_to)?;
        solo_replied.replies.push(sid);
        replied_uesr = Some(solo_replied.uid);
        set_one(&DB, "solos", input.reply_to, &solo_replied)?;

        if solo_replied.uid != uid {
            add_notification(
                &DB,
                solo_replied.uid,
                NtType::SoloComment,
                input.reply_to,
                sid,
            )?;
        }

        reply_to = Some(input.reply_to)
    };

    if visibility == 0 {
        hashtags = extract_element(&content, 5, '#');
        if !hashtags.is_empty() {
            let hashtags_tree = DB.open_tree("hashtags")?;
            for hashtag in &hashtags {
                let k = [hashtag.as_bytes(), &sid_ivec].concat();
                hashtags_tree.insert(k, &[])?;
            }
        }
        for tag in &hashtags {
            let tag_link = format!("#[{tag}](/solo/user/0?hashtag={tag})");
            content = content.replace(&format!("#{tag}"), &tag_link);
        }

        // extract @username or @uid notificaiton
        let notifications = extract_element(&content, 5, '@');
        for notification in &notifications {
            let (uid, username) = match notification.parse::<u32>() {
                Ok(uid) => {
                    if let Ok(user) = get_one::<User>(&DB, "users", uid) {
                        (uid, user.username)
                    } else {
                        continue;
                    }
                }
                Err(_) => {
                    if let Some(uid) = get_id_by_name(&DB, "usernames", notification)? {
                        (uid, notification.to_string())
                    } else {
                        continue;
                    }
                }
            };
            let notification_link = format!("[{username}](/user/{uid})");
            let from = format!("@{notification}");
            let to = format!("@{notification_link}");
            content = content.replace(&from, &to);

            // notify user to be mentioned in comment
            if uid != claim.uid && replied_uesr != Some(uid) {
                add_notification(&DB, uid, NtType::SoloMention, sid, 0)?;
            }
        }
    }

    let solo = Solo {
        sid,
        uid,
        visibility,
        content: md2html(&content),
        hashtags,
        created_at,
        reply_to,
        replies: vec![],
    };

    set_one(&DB, "solos", sid, &solo)?;
    let k = [&u32_to_ivec(claim.uid), &sid_ivec].concat();
    DB.open_tree("user_solos")?
        .insert(k, &u32_to_ivec(visibility))?;

    // kv_pair: sid = uid#visibility
    let v = [&u32_to_ivec(claim.uid), &u32_to_ivec(visibility)].concat();
    DB.open_tree("solo_timeline")?.insert(&sid_ivec, v)?;

    User::update_stats(&DB, claim.uid, "solo")?;
    claim.update_last_write(&DB)?;

    if visibility == 0 {
        DB.open_tree("tan")?.insert(format!("solo{}", sid), &[])?;
    }

    let target = if input.reply_to > 0 {
        format!("/solo/{}", input.reply_to)
    } else {
        "/solo/user/0".to_string()
    };
    Ok(Redirect::to(&target))
}

/// `GET /solo/:sid/like` solo like
pub(crate) async fn solo_like(
    referer: Option<TypedHeader<Referer>>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(sid): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let solo: Solo = get_one(&DB, "solos", sid)?;

    let user_solos_like_k = [&u32_to_ivec(claim.uid), &u32_to_ivec(sid)].concat();
    let solo_users_like_k = [&u32_to_ivec(sid), &u32_to_ivec(claim.uid)].concat();
    let user_solos_like_tree = DB.open_tree("user_solos_like")?;
    let solo_users_like_tree = DB.open_tree("solo_users_like")?;

    match solo_users_like_tree.get(&solo_users_like_k)? {
        None => {
            user_solos_like_tree.insert(&user_solos_like_k, &[])?;
            solo_users_like_tree.insert(&solo_users_like_k, &[])?;
        }
        Some(_) => {
            user_solos_like_tree.remove(&user_solos_like_k)?;
            solo_users_like_tree.remove(&solo_users_like_k)?;
        }
    }
    let target = if let Some(referer) = get_referer(referer) {
        referer
    } else {
        format!("/solo/user/{}", solo.uid)
    };

    Ok(Redirect::to(&target))
}

/// `GET /solo/:sid/delete` solo delete
pub(crate) async fn solo_delete(
    cookie: Option<TypedHeader<Cookie>>,
    Path(sid): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let solo: Solo = get_one(&DB, "solos", sid)?;
    if solo.uid != claim.uid {
        return Err(AppError::Unauthorized);
    }

    let sid_ivec = u32_to_ivec(sid);

    DB.open_tree("solos")?.remove(&sid_ivec)?;
    DB.open_tree("solo_timeline")?.remove(&sid_ivec)?;

    let solo_users_like_tree = DB.open_tree("solo_users_like")?;
    let user_solos_like_tree = DB.open_tree("user_solos_like")?;
    for i in solo_users_like_tree.scan_prefix(&sid_ivec) {
        let (k, _) = i?;
        let uid = &k[4..8];
        let user_solos_like_k = [uid, &sid_ivec].concat();
        user_solos_like_tree.remove(user_solos_like_k)?;
        solo_users_like_tree.remove(&k)?;
    }

    let hashtags_tree = DB.open_tree("hashtags")?;
    for hashtag in solo.hashtags {
        let k = [hashtag.as_bytes(), &sid_ivec].concat();
        hashtags_tree.remove(k)?;
    }

    let k = [&u32_to_ivec(claim.uid), &sid_ivec].concat();
    DB.open_tree("user_solos")?.remove(k)?;

    DB.open_tree("tan")?.remove(format!("solo{}", sid))?;

    let target = format!("/solo/user/{}", solo.uid);
    Ok(Redirect::to(&target))
}
