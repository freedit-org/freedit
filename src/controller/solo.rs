use super::{
    extract_element, get_count_by_prefix, get_ids_by_prefix, get_one, get_range, get_site_config,
    has_unread, incr_id, into_response, ivec_to_u64, timestamp_to_date, u64_to_ivec,
    u8_slice_to_u64, user_stats, utils::md2html, Claim, IterType, PageData, ParamsPage, Solo, User,
    ValidatedForm,
};
use crate::error::AppError;
use askama::Template;
use axum::{
    extract::{Path, Query, State, TypedHeader},
    headers::Cookie,
    response::{IntoResponse, Redirect},
};
use bincode::config::standard;
use serde::Deserialize;
use sled::Db;
use time::OffsetDateTime;
use validator::Validate;

/// Form data: `/solo/user/:uid` solo create.
#[derive(Deserialize, Validate)]
pub(crate) struct FormSolo {
    #[validate(length(min = 1, max = 1000))]
    content: String,
    visibility: String,
}

/// Page data: `solo.html`
#[derive(Template)]
#[template(path = "solo.html", escape = "none")]
struct PageSolo<'a> {
    page_data: PageData<'a>,
    solos: Vec<OutSolo>,
    uid: u64,
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
    uid: u64,
    sid: u64,
    username: String,
    content: String,
    created_at: String,
    visibility: u64,
    like: bool,
    like_count: usize,
}

fn can_visit_solo(visibility: u64, followers: &[u64], solo_uid: u64, current_uid: u64) -> bool {
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
}

/// `GET /solo/user/:uid` solo page
pub(crate) async fn solo(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u64>,
    Query(params): Query<ParamsSolo>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut is_following = false;
    let mut index = Vec::with_capacity(n);
    let mut followers = Vec::new();
    let mut current_uid = 0;
    if let Some(ref claim) = claim {
        let following_k = [&u64_to_ivec(claim.uid), &u64_to_ivec(uid)].concat();
        if db.open_tree("user_following")?.contains_key(&following_k)? {
            is_following = true;
        }

        if let Ok(v) = get_ids_by_prefix(&db, "user_followers", u64_to_ivec(claim.uid), None) {
            followers = v;
        }
        current_uid = claim.uid;
        followers.push(claim.uid);
    }

    match params.filter.as_deref() {
        Some("Following") => {
            if let Some(ref claim) = claim {
                if let Ok(uids) =
                    get_ids_by_prefix(&db, "user_following", u64_to_ivec(claim.uid), None)
                {
                    index = get_solos_by_uids(&db, &uids, &followers, current_uid, &page_params)?;
                };
            }
        }
        Some("Like") => {
            if let Some(ref claim) = claim {
                if let Ok(sids) =
                    get_ids_by_prefix(&db, "user_solos_like", u64_to_ivec(claim.uid), None)
                {
                    let (start, end) = get_range(sids.len(), &page_params);
                    index = sids[start - 1..end].to_vec();
                };
            }
        }
        _ => {
            if let Some(ref hashtag) = params.hashtag {
                index = get_ids_by_prefix(&db, "hashtags", hashtag, Some(&page_params))?;
            } else if uid == 0 {
                index = get_all_solos(&db, "solo_timeline", &followers, current_uid, &page_params)?
            } else {
                index = get_solos_by_uids(&db, &[uid], &followers, current_uid, &page_params)?;
            }
        }
    }

    let mut out_solos = Vec::with_capacity(index.len());
    if !index.is_empty() {
        for sid in index {
            let solo: Solo = get_one(&db, "solos", sid)?;
            let user: User = get_one(&db, "users", solo.uid)?;
            let date = timestamp_to_date(solo.created_at)?;

            let mut like = false;
            if let Some(ref claim) = claim {
                let k = [&u64_to_ivec(sid), &u64_to_ivec(claim.uid)].concat();
                if db.open_tree("solo_users_like")?.contains_key(&k)? {
                    like = true;
                }
            }

            let like_count =
                get_count_by_prefix(&db, "solo_users_like", &u64_to_ivec(sid)).unwrap_or_default();

            let out_solo = OutSolo {
                uid: solo.uid,
                sid: solo.sid,
                username: user.username,
                content: solo.content,
                created_at: date,
                visibility: solo.visibility,
                like,
                like_count,
            };

            out_solos.push(out_solo);
        }
    }

    let filter = if claim.is_none() { None } else { params.filter };

    let has_unread = if let Some(ref claim) = claim {
        has_unread(&db, claim.uid)?
    } else {
        false
    };

    let username = if uid > 0 {
        let user: User = get_one(&db, "users", uid)?;
        user.username
    } else {
        "All".to_owned()
    };
    let page_data = PageData::new("Solo", &site_config.site_name, claim, has_unread);

    let page_solo = PageSolo {
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
    Ok(into_response(&page_solo, "html"))
}

fn get_all_solos(
    db: &Db,
    timeline_tree: &str,
    followers: &[u64],
    current_uid: u64,
    page_params: &ParamsPage,
) -> Result<Vec<u64>, AppError> {
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
        let solo_uid = u8_slice_to_u64(&v[0..8]);
        let visibility = u8_slice_to_u64(&v[8..16]);
        if can_visit_solo(visibility, followers, solo_uid, current_uid) {
            if count < page_params.anchor {
                count += 1;
                continue;
            } else {
                result.push(ivec_to_u64(&k));
            }
        }

        if result.len() == page_params.n {
            break;
        }
    }
    Ok(result)
}

fn get_solos_by_uids(
    db: &Db,
    uids: &[u64],
    followers: &[u64],
    current_uid: u64,
    page_params: &ParamsPage,
) -> Result<Vec<u64>, AppError> {
    let mut sids = Vec::with_capacity(page_params.n);
    for uid in uids {
        let prefix = u64_to_ivec(*uid);
        // kv_pair: uid#sid = visibility
        for i in db.open_tree("user_solos")?.scan_prefix(prefix) {
            let (k, v) = i?;
            let sid = u8_slice_to_u64(&k[8..16]);
            let visibility = u8_slice_to_u64(&v);
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
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormSolo>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let created_at = OffsetDateTime::now_utc().unix_timestamp();
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

    let sid = incr_id(&db, "solos_count")?;
    let sid_ivec = u64_to_ivec(sid);
    let mut content = input.content;
    let mut hashtags = Vec::new();

    // TODO: hashtag per user, note-taking
    if visibility == 0 {
        hashtags = extract_element(&content, 5, '#');
        if !hashtags.is_empty() {
            let hashtags_tree = db.open_tree("hashtags")?;
            for hashtag in &hashtags {
                let k = [hashtag.as_bytes(), &sid_ivec].concat();
                hashtags_tree.insert(k, &[])?;
            }
        }
        for tag in &hashtags {
            let tag_link = format!("[{}](/solo/user/0?hashtag={})", tag, tag);
            content = content.replace(tag, &tag_link);
        }
    }

    let solo = Solo {
        sid,
        uid,
        visibility,
        content: md2html(&content),
        hashtags,
        created_at,
    };

    let solo_encode = bincode::encode_to_vec(&solo, standard())?;

    db.open_tree("solos")?.insert(&sid_ivec, solo_encode)?;
    let k = [&u64_to_ivec(claim.uid), &sid_ivec].concat();
    db.open_tree("user_solos")?
        .insert(k, &u64_to_ivec(visibility))?;

    // kv_pair: sid = uid#visibility
    let v = [&u64_to_ivec(claim.uid), &u64_to_ivec(visibility)].concat();
    db.open_tree("solo_timeline")?.insert(&sid_ivec, v)?;

    user_stats(&db, claim.uid, "solo")?;
    claim.update_last_write(&db)?;

    let target = format!("/solo/user/{}", uid);
    Ok(Redirect::to(&target))
}

/// `GET /solo/:sid/like` solo like
pub(crate) async fn solo_like(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(sid): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let solo: Solo = get_one(&db, "solos", sid)?;

    let user_solos_like_k = [&u64_to_ivec(claim.uid), &u64_to_ivec(sid)].concat();
    let solo_users_like_k = [&u64_to_ivec(sid), &u64_to_ivec(claim.uid)].concat();
    let user_solos_like_tree = db.open_tree("user_solos_like")?;
    let solo_users_like_tree = db.open_tree("solo_users_like")?;

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

    let target = format!("/solo/user/{}", solo.uid);
    Ok(Redirect::to(&target))
}

/// `GET /solo/:sid/delete` solo delete
pub(crate) async fn solo_delete(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(sid): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let solo: Solo = get_one(&db, "solos", sid)?;
    if solo.uid != claim.uid {
        return Err(AppError::Unauthorized);
    }

    let sid_ivec = u64_to_ivec(sid);

    db.open_tree("solos")?.remove(&sid_ivec)?;
    db.open_tree("solo_timeline")?.remove(&sid_ivec)?;

    let solo_users_like_tree = db.open_tree("solo_users_like")?;
    let user_solos_like_tree = db.open_tree("user_solos_like")?;
    for i in solo_users_like_tree.scan_prefix(&sid_ivec) {
        let (k, _) = i?;
        let uid = &k[8..16];
        let user_solos_like_k = [uid, &sid_ivec].concat();
        user_solos_like_tree.remove(&user_solos_like_k)?;
        solo_users_like_tree.remove(&k)?;
    }

    let hashtags_tree = db.open_tree("hashtags")?;
    for hashtag in solo.hashtags {
        let k = [hashtag.as_bytes(), &sid_ivec].concat();
        hashtags_tree.remove(&k)?;
    }

    let k = [&u64_to_ivec(claim.uid), &sid_ivec].concat();
    db.open_tree("user_solos")?.remove(k)?;

    let target = format!("/solo/user/{}", solo.uid);
    Ok(Redirect::to(&target))
}
