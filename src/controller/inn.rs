//! ## [Inn] / [Post] / [Comment] read / create / update controller

use super::{
    extract_element, get_batch, get_count_by_prefix, get_ids_by_prefix, get_one, get_range,
    get_site_config, get_uid_by_name, has_unread, incr_id, into_response, ivec_to_u64, mark_read,
    md2html, timestamp_to_date, u64_to_ivec, u8_slice_to_u64, user_stats, Claim, Comment, Inn,
    PageData, ParamsPage, Post, User, ValidatedForm,
};
use crate::{
    config::CONFIG,
    controller::{get_count, set_index, IterType},
    error::AppError,
};
use ::time::OffsetDateTime;
use askama::Template;
use axum::{
    extract::{Path, Query, State, TypedHeader},
    headers::Cookie,
    response::{IntoResponse, Redirect},
};
use bincode::config::standard;
use flate2::{write::GzEncoder, Compression};
use serde::Deserialize;
use sled::{Batch, Db};
use std::io::Write;
use tokio::{
    fs::{create_dir_all, File},
    io::AsyncWriteExt,
    time,
};
use tracing::debug;
use validator::Validate;

/// Page data: `inn_create.html`
#[derive(Template)]
#[template(path = "inn_create.html")]
struct PageInnCreate<'a> {
    page_data: PageData<'a>,
}

/// Page data: `inn_edit.html`
#[derive(Template)]
#[template(path = "inn_edit.html")]
struct PageInnEdit<'a> {
    page_data: PageData<'a>,
    inn: Inn,
}

/// `GET /mod/:iid` inn create/edit page, only moderators or admin can do this
///
/// if iid is 0, then create a new inn
pub(crate) async fn mod_inn(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(iid): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    if claim.role < 10 {
        return Err(AppError::Unauthorized);
    }

    // create new inn
    if iid == 0 {
        let page_data = PageData::new("create new inn", &site_config.site_name, Some(claim), false);
        let page_inn_create = PageInnCreate { page_data };
        Ok(into_response(&page_inn_create, "html"))
    } else {
        let inn: Inn = get_one(&db, "inns", iid)?;

        if !inn.mods.contains(&claim.uid) && (claim.role < u8::MAX) {
            return Err(AppError::Unauthorized);
        }
        let page_data = PageData::new("edit inn", &site_config.site_name, Some(claim), false);
        let page_inn_edit = PageInnEdit { page_data, inn };
        Ok(into_response(&page_inn_edit, "html"))
    }
}

/// Form data: `/mod/:iid` inn create/edit page
#[derive(Deserialize, Validate)]
pub(crate) struct FormInn {
    #[validate(length(min = 1, max = 64))]
    inn_name: String,
    #[validate(length(min = 1, max = 1024))]
    about: String,
    #[validate(length(min = 1, max = 65535))]
    description: String,
    #[validate(length(min = 1, max = 128))]
    topics: String,
    mods: Option<String>,
    inn_type: String,
}

/// `POST /mod/:iid` inn create/edit page, only moderators or admin can do this
///
/// if iid is 0, then create a new inn
pub(crate) async fn mod_inn_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(mut iid): Path<u64>,
    ValidatedForm(input): ValidatedForm<FormInn>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if claim.role < 10 {
        return Err(AppError::Unauthorized);
    }

    // get inn moderators
    let mut uids: Vec<u64> = vec![];
    if let Some(mods) = input.mods {
        for uid in mods.split('#') {
            if let Ok(uid) = uid.parse::<u64>() {
                uids.push(uid);
            }
        }
    };
    let users_tree = db.open_tree("users")?;
    uids.retain(|uid| {
        users_tree
            .contains_key(u64_to_ivec(*uid))
            .ok()
            .unwrap_or(false)
    });
    uids.truncate(10);
    if !uids.contains(&claim.uid) {
        uids.push(claim.uid);
    }

    // get inn topics
    let mut topics: Vec<String> = input
        .topics
        .split('#')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    topics.truncate(5);

    let inn_names_tree = db.open_tree("inn_names")?;

    let mut batch_mods = Batch::default();
    let mut batch_topics = Batch::default();
    // create new inn
    if iid == 0 {
        // check if inn name exists
        if inn_names_tree.contains_key(&input.inn_name)? {
            return Err(AppError::NameExists);
        }
        iid = incr_id(&db, "inns_count")?;
    } else {
        // edit inn

        // check if this name is used by other inn
        let search_iid = inn_names_tree.get(&input.inn_name)?;
        if search_iid.is_some() && search_iid != Some(u64_to_ivec(iid)) {
            return Err(AppError::NameExists);
        }

        let inn: Inn = get_one(&db, "inns", iid)?;
        if !inn.mods.contains(&claim.uid) || (claim.role < u8::MAX) {
            return Err(AppError::Unauthorized);
        }

        // remove the old inn name
        if input.inn_name != inn.inn_name {
            inn_names_tree.remove(&inn.inn_name)?;
        }

        // remove the old inn topics
        for topic in inn.topics {
            let k = [topic.as_bytes(), &u64_to_ivec(iid)].concat();
            batch_topics.remove(&*k);
        }

        // remove the old inn moderators
        for uid in inn.mods {
            let k = [&u64_to_ivec(uid), &u64_to_ivec(iid)].concat();
            batch_mods.remove(k);
        }
    }

    let iid_ivec = u64_to_ivec(iid);

    // set topic index for inns
    for topic in &topics {
        let k = [topic.as_bytes(), &u64_to_ivec(iid)].concat();
        batch_topics.insert(&*k, &[]);
    }
    db.open_tree("topics")?.apply_batch(batch_topics)?;

    // set index for inn moderators
    for uid in &uids {
        let k = [&u64_to_ivec(*uid), &iid_ivec].concat();
        batch_mods.insert(k, &[]);
    }
    db.open_tree("mod_inns")?.apply_batch(batch_mods)?;

    let description_html = md2html(&input.description)?;
    let inn = Inn {
        iid,
        inn_name: input.inn_name,
        about: input.about,
        description: input.description,
        description_html,
        topics,
        mods: uids,
        inn_type: input.inn_type,
        created_at: OffsetDateTime::now_utc().unix_timestamp(),
    };

    if inn.inn_type.as_str() == "Private" {
        db.open_tree("inns_private")?.insert(&iid_ivec, &[])?;
    }

    let inn_encoded = bincode::encode_to_vec(&inn, standard())?;

    db.open_tree("inns")?.insert(&iid_ivec, inn_encoded)?;
    inn_names_tree.insert(inn.inn_name, iid_ivec)?;

    static_inn_list_update(&db).await?;

    let target = format!("/inn/{}", iid);
    Ok(Redirect::to(&target))
}

/// url params: `inn_list.html`
#[derive(Deserialize)]
pub(crate) struct ParamsInnList {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    topic: Option<String>,
    filter: Option<String>,
}

/// Vec data: inn
struct OutInnList {
    iid: u64,
    inn_name: String,
    about: String,
    topics: Vec<String>,
}

/// Page data: `inn_list.html`
#[derive(Template)]
#[template(path = "inn_list.html")]
struct PageInnList<'a> {
    page_data: PageData<'a>,
    inns: Vec<OutInnList>,
    anchor: usize,
    n: usize,
    is_desc: bool,
    filter: Option<String>,
    topic: Option<String>,
}

/// `GET /inn/list` inns list page
pub(crate) async fn inn_list(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<ParamsInnList>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));
    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut inns: Vec<Inn> = Vec::with_capacity(n);

    if let Some(topic) = &params.topic {
        for i in get_ids_by_prefix(&db, "topics", topic, Some(&page_params))? {
            if let Ok(inn) = get_one(&db, "inns", i) {
                inns.push(inn);
            }
        }
    } else if let Some(claim) = &claim {
        let uid_ivec = u64_to_ivec(claim.uid);
        if params.filter.as_deref() == Some("mod") {
            for i in get_ids_by_prefix(&db, "mod_inns", uid_ivec, Some(&page_params))? {
                if let Ok(inn) = get_one(&db, "inns", i) {
                    inns.push(inn);
                }
            }
        } else if params.filter.as_deref() == Some("joined") {
            for i in get_ids_by_prefix(&db, "user_inns", uid_ivec, Some(&page_params))? {
                if let Ok(inn) = get_one(&db, "inns", i) {
                    inns.push(inn);
                }
            }
        } else {
            inns = get_batch(&db, "default", "inns_count", "inns", &page_params)?;
        }
    } else {
        inns = get_batch(&db, "default", "inns_count", "inns", &page_params)?;
    }

    let mut out_inns = Vec::with_capacity(inns.len());
    for i in inns {
        let out_inn = OutInnList {
            iid: i.iid,
            inn_name: i.inn_name,
            about: i.about,
            topics: i.topics,
        };
        out_inns.push(out_inn);
    }

    let filter = if claim.is_none() { None } else { params.filter };
    let has_unread = if let Some(ref claim) = claim {
        has_unread(&db, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("inns", &site_config.site_name, claim, has_unread);
    let page_inn_list = PageInnList {
        page_data,
        inns: out_inns,
        anchor,
        n,
        is_desc,
        topic: params.topic,
        filter,
    };

    Ok(into_response(&page_inn_list, "html"))
}

/// `/static/inn/list/:page.html`
async fn static_inn_list_update(db: &Db) -> Result<(), AppError> {
    let site_config = get_site_config(db)?;
    let n = 30;
    let mut anchor = 0;
    let is_desc = true;

    let mut inns_count = get_count(db, "default", "inns_count")?;
    let mut page = 0;
    while inns_count > 0 {
        page += 1;
        let page_params = ParamsPage { anchor, n, is_desc };
        let inns: Vec<Inn> = get_batch(db, "default", "inns_count", "inns", &page_params)?;
        let mut out_inns = Vec::with_capacity(inns.len());
        for i in inns {
            let out_inn = OutInnList {
                iid: i.iid,
                inn_name: i.inn_name,
                about: i.about,
                topics: i.topics,
            };
            out_inns.push(out_inn);
        }
        let page_data = PageData::new("inns", &site_config.site_name, None, false);
        let page_inn_list = PageInnList {
            page_data,
            inns: out_inns,
            anchor,
            n,
            is_desc,
            topic: None,
            filter: None,
        };

        let res = page_inn_list.render().unwrap();
        let mut e = GzEncoder::new(Vec::with_capacity(8192), Compression::default());
        e.write_all(res.as_bytes()).unwrap();
        let compressed_bytes = e.finish().unwrap();

        let target_dir = format!("{}/inn/list/", &CONFIG.html_path);
        let target_dir = std::path::Path::new(&target_dir);
        if !target_dir.exists() {
            create_dir_all(target_dir).await?;
        }

        let target = target_dir.join(format!("{}.html.gz", page));
        let mut file = File::create(&target).await?;
        file.write_all(&compressed_bytes).await?;
        debug!("target {}", target.display());

        let is_last = inns_count <= n;
        if is_last {
            break;
        }
        anchor += n;
        inns_count -= n;

        if page >= site_config.static_page {
            break;
        }
    }

    Ok(())
}

/// Page data: `post_create.html`
#[derive(Template)]
#[template(path = "post_create.html")]
struct PagePostCreate<'a> {
    page_data: PageData<'a>,
    iid: u64,
}

/// Page data: `post_edit.html`
#[derive(Template)]
#[template(path = "post_edit.html")]
struct PagePostEdit<'a> {
    page_data: PageData<'a>,
    post: Post,
}

/// `GET /post/:iid/edit/:pid` post create/edit page
///
/// if pid is 0, then create a new post
pub(crate) async fn edit_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let inn_role = get_inn_role(&db, iid, claim.uid)?.ok_or(AppError::Unauthorized)?;
    if inn_role <= 3 {
        return Err(AppError::Unauthorized);
    }

    if !db.open_tree("inns")?.contains_key(u64_to_ivec(iid))? {
        return Err(AppError::NotFound);
    }

    if pid == 0 {
        let page_data = PageData::new("new post", &site_config.site_name, Some(claim), false);
        let page_post_create = PagePostCreate { page_data, iid };

        Ok(into_response(&page_post_create, "html"))
    } else {
        let post: Post = get_one(&db, "posts", pid)?;

        if post.is_locked {
            return Err(AppError::Locked);
        }

        if post.uid != claim.uid {
            return Err(AppError::Unauthorized);
        }
        let page_data = PageData::new("edit post", &site_config.site_name, Some(claim), false);
        let page_post_edit = PagePostEdit { page_data, post };

        Ok(into_response(&page_post_edit, "html"))
    }
}

/// Form data: `/inn/:iid/post/:pid` post create/edit page
#[derive(Deserialize, Validate)]
pub(crate) struct FormPost {
    #[validate(length(min = 1, max = 256))]
    title: String,
    #[validate(length(min = 1, max = 128))]
    tags: String,
    #[validate(length(min = 1, max = 65535))]
    content: String,
}

/// `POST /post/:iid/edit/:pid` post create/edit page
///
/// if pid is 0, then create a new post
pub(crate) async fn edit_post_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, old_pid)): Path<(u64, u64)>,
    ValidatedForm(input): ValidatedForm<FormPost>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let inn_role = get_inn_role(&db, iid, claim.uid)?.ok_or(AppError::Unauthorized)?;
    if inn_role <= 3 {
        return Err(AppError::Unauthorized);
    }

    let created_at = OffsetDateTime::now_utc().unix_timestamp();
    if created_at - claim.last_write < site_config.post_interval {
        return Err(AppError::WriteInterval);
    }

    let inn: Inn = get_one(&db, "inns", iid)?;

    let pid = if old_pid == 0 {
        incr_id(&db, "posts_count")?
    } else {
        old_pid
    };
    let pid_ivec = u64_to_ivec(pid);

    let mut tags = vec![];
    let mut visibility = 0;
    if inn.inn_type.as_str() == "Private" {
        visibility = 10
    } else {
        tags = input
            .tags
            .split('#')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        tags.truncate(5);

        let mut batch = Batch::default();
        if old_pid > 0 {
            let post: Post = get_one(&db, "posts", old_pid)?;
            if post.uid != claim.uid {
                return Err(AppError::Unauthorized);
            }

            if post.is_locked {
                return Err(AppError::Locked);
            }

            for old_tag in post.tags.iter() {
                let k = [old_tag.as_bytes(), &u64_to_ivec(old_pid)].concat();
                batch.remove(k);
            }
        }

        for tag in tags.iter() {
            let k = [tag.as_bytes(), &pid_ivec].concat();
            batch.insert(k, &[]);
        }
        db.open_tree("tags")?.apply_batch(batch)?;
    }

    let post = Post {
        pid,
        uid: claim.uid,
        iid,
        title: input.title,
        tags,
        content_html: md2html(&input.content)?,
        content: input.content,
        created_at,
        is_locked: false,
    };

    let post_encoded = bincode::encode_to_vec(&post, standard())?;
    db.open_tree("posts")?.insert(&pid_ivec, post_encoded)?;

    let iid_ivec = u64_to_ivec(iid);
    let visibility_ivec = u64_to_ivec(visibility);
    if old_pid == 0 {
        set_index(&db, "inn_posts_count", iid, "inn_posts_idx", &pid_ivec)?;
        let target = [&iid_ivec, &pid_ivec, &visibility_ivec].concat();
        set_index(&db, "user_posts_count", claim.uid, "user_posts_idx", target)?;
    }

    if visibility < 10 {
        db.open_tree("static_user_post")?
            .insert(u64_to_ivec(claim.uid), &[])?;
        db.open_tree("static_inn_post")?.insert(&iid_ivec, &[])?;
    }

    let created_at_ivec = u64_to_ivec(created_at as u64);
    let k = [&iid_ivec, &pid_ivec].concat();

    if old_pid > 0 {
        let old_timestamp = db.open_tree("post_timeline_idx")?.get(&k)?;
        if let Some(v) = old_timestamp {
            let k = [&v, &iid_ivec, &pid_ivec].concat();
            db.open_tree("post_timeline")?.remove(&k)?;
        }
    }
    // kv_pair: iid#pid = timestamp
    db.open_tree("post_timeline_idx")?
        .insert(k, &created_at_ivec)?;

    let k = [&created_at_ivec, &iid_ivec, &pid_ivec].concat();
    // kv_pair: timestamp#iid#pid = visibility
    db.open_tree("post_timeline")?.insert(k, visibility_ivec)?;
    static_post(&db, pid).await?;

    user_stats(&db, claim.uid, "post")?;
    claim.update_last_write(&db)?;

    let target = format!("/post/{}/{}", iid, pid);
    Ok(Redirect::to(&target))
}

/// Vec data: post list
struct OutPostList {
    pid: u64,
    iid: u64,
    inn_name: String,
    uid: u64,
    username: String,
    title: String,
    created_at: String,
    comment_count: u64,
}

/// Page data: `tag.html`
#[derive(Template)]
#[template(path = "tag.html")]
struct PageTag<'a> {
    page_data: PageData<'a>,
    posts: Vec<OutPostList>,
    anchor: usize,
    n: usize,
    is_desc: bool,
    tag: String,
}

/// url params: `tag.html`
#[derive(Deserialize)]
pub(crate) struct ParamsTag {
    anchor: Option<usize>,
    is_desc: Option<bool>,
}

/// `GET /inn/tag/:tag` tag page
pub(crate) async fn tag(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(tag): Path<String>,
    Query(params): Query<ParamsTag>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let index = get_ids_by_prefix(&db, "tags", &tag, Some(&page_params))?;
    let out_post_list = get_out_post_list(&db, &index)?;

    let page_data = PageData::new("inn", &site_config.site_name, claim, false);
    let page_tag = PageTag {
        page_data,
        posts: out_post_list,
        anchor,
        n,
        is_desc,
        tag,
    };

    Ok(into_response(&page_tag, "html"))
}

/// Page data: `inn.html`
#[derive(Template)]
#[template(path = "inn.html")]
struct PageInn<'a> {
    page_data: PageData<'a>,
    posts: Vec<OutPostList>,
    iid: u64,
    inn_name: String,
    anchor: usize,
    n: usize,
    is_desc: bool,
    inn_role: u8,
    filter: Option<String>,
    username: Option<String>,
    inn_users_count: usize,
    is_mod: bool,
    has_apply: bool,
}

/// url params: `inn.html`
#[derive(Deserialize)]
pub(crate) struct ParamsInn {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    filter: Option<String>,
}

/// `GET /inn/:iid` inn page
pub(crate) async fn inn(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(iid): Path<u64>,
    Query(params): Query<ParamsInn>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut index = Vec::with_capacity(n);
    let mut joined_inns = &Vec::new();
    let mut user_iins: Result<Vec<u64>, AppError> = Err(AppError::NotFound);
    let mut username: Option<String> = None;
    let mut is_mod = false;
    let mut has_apply = false;
    if let Some(ref claim) = claim {
        let k = [&u64_to_ivec(claim.uid), &u64_to_ivec(iid)].concat();
        if db.open_tree("mod_inns")?.contains_key(&k)? {
            is_mod = true;
            for i in &db.open_tree("inn_users")? {
                let (_, v) = i?;
                if v == [1] {
                    has_apply = true;
                    break;
                }
            }
        }

        user_iins = get_ids_by_prefix(&db, "user_inns", u64_to_ivec(claim.uid), None);
        if let Ok(ref user_iins) = user_iins {
            joined_inns = user_iins;
        }
    }

    match params.filter.as_deref() {
        Some("joined") if claim.is_some() => {
            if let Ok(ref iids) = user_iins {
                index = get_pids_by_iids(&db, iids, &page_params)?;
            };
        }
        Some("following") => {
            if let Some(ref claim) = claim {
                let user_following: Vec<u64> =
                    get_ids_by_prefix(&db, "user_following", u64_to_ivec(claim.uid), None)
                        .unwrap_or_default();
                index = get_pids_by_uids(&db, &user_following, joined_inns, &page_params)?;
            }
        }
        Some(uid) => {
            if let Ok(uid) = uid.parse::<u64>() {
                let user: User = get_one(&db, "users", uid)?;
                username = Some(user.username);
                index = get_pids_by_uids(&db, &[uid], joined_inns, &page_params)?;
            };
        }
        _ => {
            if iid == 0 {
                index = get_pids_all(&db, joined_inns, &page_params)?
            } else {
                index = get_pids_by_iids(&db, &[iid], &page_params)?;
            }
        }
    }

    let out_post_list = get_out_post_list(&db, &index)?;
    let mut inn_role = 0;
    if let Some(ref claim) = claim {
        if iid > 0 {
            if let Ok(Some(role)) = get_inn_role(&db, iid, claim.uid) {
                inn_role = role
            }
        }
    }

    let inn_users_count = if iid > 0 {
        get_count_by_prefix(&db, "inn_users", &u64_to_ivec(iid))?
    } else {
        0
    };

    let filter = if claim.is_none() { None } else { params.filter };
    let has_unread = if let Some(ref claim) = claim {
        has_unread(&db, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("inn", &site_config.site_name, claim, has_unread);
    let inn_name = if iid > 0 && !out_post_list.is_empty() {
        &out_post_list[0].inn_name
    } else {
        "No Post"
    };
    let page_inn = PageInn {
        page_data,
        inn_name: inn_name.to_string(),
        posts: out_post_list,
        anchor,
        iid,
        n,
        is_desc,
        inn_role,
        filter,
        username,
        inn_users_count,
        is_mod,
        has_apply,
    };

    Ok(into_response(&page_inn, "html"))
}

/// Page data: `inn_static.html`
#[derive(Template)]
#[template(path = "inn_static.html")]
struct PageInnStatic<'a> {
    page_data: &'a PageData<'a>,
    posts: Vec<OutPostList>,
    id: u64,
    name: String,
    page: usize,
    is_last: bool,
    is_user: bool,
}

/// render static inn page
async fn render_post_list(
    db: &Db,
    id: u64,
    page: usize,
    is_last: bool,
    pids: &Vec<u64>,
    page_data: &PageData<'_>,
    is_user: bool,
) -> Result<(), AppError> {
    let out_post_list = get_out_post_list(db, pids)?;
    let name = if is_user {
        let user: User = get_one(db, "users", id)?;
        user.username
    } else if id > 0 && !out_post_list.is_empty() {
        out_post_list[0].inn_name.clone()
    } else {
        "No Post".to_owned()
    };

    let page_inn_static = PageInnStatic {
        page_data,
        id,
        name,
        posts: out_post_list,
        page,
        is_last,
        is_user,
    };

    let res = &page_inn_static.render().unwrap();
    let mut e = GzEncoder::new(Vec::with_capacity(8192), Compression::default());
    e.write_all(res.as_bytes()).unwrap();
    let compressed_bytes = e.finish().unwrap();

    let target_dir = if is_user {
        format!("{}/inn/user/{}", &CONFIG.html_path, id)
    } else {
        format!("{}/inn/{}", &CONFIG.html_path, id)
    };
    let target_dir = std::path::Path::new(&target_dir);
    if !target_dir.exists() {
        create_dir_all(target_dir).await?;
    }

    let target = target_dir.join(format!("{}.html.gz", page));
    let target = std::path::Path::new(&target);
    let mut file = File::create(target).await?;
    file.write_all(&compressed_bytes).await?;
    debug!("target {} : pids: {:?}", target.display(), pids);

    Ok(())
}

/// Cron job: generate static page `/static/inn` tab `All`
///
/// `/static/inn/0/:page.html`
pub(crate) async fn static_inn_all(db: &Db, interval: u64) -> Result<(), AppError> {
    let sleep = time::sleep(time::Duration::from_secs(interval));
    if let Some((k, _)) = db.open_tree("post_timeline")?.last()? {
        let timestamp = u8_slice_to_u64(&k[0..8]);
        let last_check = OffsetDateTime::now_utc().unix_timestamp() as u64 - interval;
        if last_check - 3 > timestamp {
            sleep.await;
            return Ok(());
        }
    };

    let site_config = get_site_config(db)?;
    let n = 30;
    let is_desc = true;
    let mut anchor = 0;
    let page_data = &PageData::new("inn", &site_config.site_name, None, false);

    let mut posts_count = get_count(db, "default", "posts_count")?;
    for i in db.open_tree("inns_private")?.iter() {
        let (k, _) = i?;
        let count = get_count(db, "inn_posts_count", &k)?;
        posts_count -= count;
    }

    let mut page: usize = 0;
    while posts_count > 0 {
        let iid = 0;
        page += 1;
        let page_params = ParamsPage { anchor, n, is_desc };
        let index = get_pids_all(db, &[], &page_params)?;
        let is_last = posts_count <= n;
        render_post_list(db, iid, page, is_last, &index, page_data, false).await?;
        if is_last {
            break;
        }
        anchor += n;
        posts_count -= n;

        if page >= site_config.static_page {
            break;
        }
    }

    sleep.await;
    Ok(())
}

/// Cron job: generate static page`/static/inn` tab `:Inn` and `:User`
///
/// `/static/inn/:iid/:page.html`
///
/// `/static/inn/user/:uid/:page.html`
pub(crate) async fn static_inn_update(db: &Db, interval: u64) -> Result<(), AppError> {
    let site_config = get_site_config(db)?;

    let n = 30;
    let is_desc = true;
    let mut anchor = 0;
    let page_data = &PageData::new("inn", &site_config.site_name, None, false);

    // update inn post list
    let inns_private_tree = db.open_tree("inns_private")?;
    let tree = db.open_tree("static_inn_post")?;
    for i in tree.iter() {
        let (k, _) = i?;
        let iid = ivec_to_u64(&k);
        if inns_private_tree.contains_key(&k)? {
            debug!("inn {} is private", iid);
            continue;
        }

        let mut page = 0;
        let mut posts_count = get_count(db, "inn_posts_count", &k)?;
        while posts_count > 0 {
            page += 1;
            let page_params = ParamsPage { anchor, n, is_desc };
            let index = get_pids_by_iids(db, &[iid], &page_params)?;
            let is_last = posts_count <= n;
            render_post_list(db, iid, page, is_last, &index, page_data, false).await?;

            if is_last {
                break;
            }
            anchor += n;
            posts_count -= n;

            if page >= site_config.static_page {
                break;
            }
        }
        tree.remove(&k)?;
    }

    // update user post list
    let tree = db.open_tree("static_user_post")?;
    for i in tree.iter() {
        let (k, _) = i?;
        let uid = ivec_to_u64(&k);
        let mut page = 0;
        let page_params = ParamsPage { anchor, n, is_desc };
        let mut posts_count = get_count(db, "user_posts_count", &k)?;
        while posts_count > 0 {
            page += 1;
            let index = get_pids_by_uids(db, &[uid], &[], &page_params)?;
            let is_last = posts_count <= n;
            render_post_list(db, uid, page, is_last, &index, page_data, true).await?;
            if is_last {
                break;
            }
            anchor += n;
            posts_count -= n;

            if page >= 10 {
                break;
            }
        }
        tree.remove(&k)?;
    }

    time::sleep(time::Duration::from_secs(interval)).await;
    Ok(())
}

/// get [OutPostList] from pids
fn get_out_post_list(db: &Db, index: &[u64]) -> Result<Vec<OutPostList>, AppError> {
    let mut post_lists = Vec::with_capacity(index.len());
    if !index.is_empty() {
        for pid in index {
            let post: Post = get_one(db, "posts", *pid)?;
            let user: User = get_one(db, "users", post.uid)?;
            let date = timestamp_to_date(post.created_at)?;
            let inn: Inn = get_one(db, "inns", post.iid)?;
            let comment_count = get_count(db, "post_comments_count", u64_to_ivec(*pid))? as u64;

            let post_list = OutPostList {
                pid: post.pid,
                iid: post.iid,
                inn_name: inn.inn_name,
                uid: post.uid,
                username: user.username,
                title: post.title,
                created_at: date,
                comment_count,
            };
            post_lists.push(post_list);
        }
    }
    Ok(post_lists)
}

/// get pids all, controlled by `visibility`, sorted by timestamp
fn get_pids_all(
    db: &Db,
    joined_inns: &[u64],
    page_params: &ParamsPage,
) -> Result<Vec<u64>, AppError> {
    let tree = db.open_tree("post_timeline")?;
    let mut count: usize = 0;
    let mut result = Vec::with_capacity(page_params.n);
    let iter = if page_params.is_desc {
        IterType::Rev(tree.iter().rev())
    } else {
        IterType::Iter(tree.iter())
    };

    // kvpaire: timestamp#iid#pid = visibility
    for i in iter {
        let (k, v) = i?;
        let id = u8_slice_to_u64(&k[8..16]);
        let out_id = u8_slice_to_u64(&k[16..24]);

        let visibility = ivec_to_u64(&v);
        if visibility == 0 || (visibility == 10 && joined_inns.contains(&id)) {
            if count < page_params.anchor {
                count += 1;
                continue;
            } else {
                result.push(out_id);
            }
        }

        if result.len() == page_params.n {
            break;
        }
    }

    Ok(result)
}

/// get pids by multi iids, sorted by timestamp
fn get_pids_by_iids(db: &Db, iids: &[u64], page_params: &ParamsPage) -> Result<Vec<u64>, AppError> {
    let mut pids = Vec::with_capacity(page_params.n);
    let mut pairs = Vec::new();
    for iid in iids {
        let prefix = u64_to_ivec(*iid);
        // kv_pair: iid#pid = timestamp#visibility
        for i in db.open_tree("post_timeline_idx")?.scan_prefix(prefix) {
            let (k, v) = i?;
            let pid = u8_slice_to_u64(&k[8..16]);
            let timestamp = ivec_to_u64(&v);
            pairs.push((pid, timestamp));
        }
    }
    pairs.sort_unstable_by_key(|pair| pair.1);
    pairs.iter().for_each(|pair| pids.push(pair.0));
    let (start, end) = get_range(pids.len(), page_params);
    pids = pids[start - 1..end].to_vec();
    if page_params.is_desc {
        pids.reverse();
    }
    Ok(pids)
}

/// get pids by multi uids, controlled by `visibility`, sorted by timestamp
fn get_pids_by_uids(
    db: &Db,
    uids: &[u64],
    joined_inns: &[u64],
    page_params: &ParamsPage,
) -> Result<Vec<u64>, AppError> {
    let mut pids = Vec::with_capacity(page_params.n);
    for uid in uids {
        let prefix = u64_to_ivec(*uid);
        // kv_pair: uid#idx = iid#pid#visibility
        for i in db.open_tree("user_posts_idx")?.scan_prefix(prefix) {
            let (_, v) = i?;
            let iid = u8_slice_to_u64(&v[0..8]);
            let pid = u8_slice_to_u64(&v[8..16]);
            let visibility = u8_slice_to_u64(&v[16..24]);
            if visibility == 0 || (visibility == 10 && joined_inns.contains(&iid)) {
                pids.push(pid);
            }
        }
    }
    let (start, end) = get_range(pids.len(), page_params);
    pids = pids[start - 1..end].to_vec();
    if page_params.is_desc {
        pids.reverse();
    }
    Ok(pids)
}

/// `GET /inn/:iid/join` join inn
pub(crate) async fn inn_join(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(iid): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let inn: Inn = get_one(&db, "inns", iid)?;

    let user_inns_k = [&u64_to_ivec(claim.uid), &u64_to_ivec(iid)].concat();
    let inn_users_k = [&u64_to_ivec(iid), &u64_to_ivec(claim.uid)].concat();
    let user_inns_tree = db.open_tree("user_inns")?;
    let inn_users_tree = db.open_tree("inn_users")?;

    match inn_users_tree.get(&inn_users_k)? {
        None => {
            if inn.inn_type.as_str() != "Public" {
                // 1: applied, but pending
                inn_users_tree.insert(&inn_users_k, &[1])?;
            } else {
                user_inns_tree.insert(&user_inns_k, &[])?;
                // 4: Public, default Intern
                inn_users_tree.insert(&inn_users_k, &[4])?;
            }
        }
        Some(_) => {
            user_inns_tree.remove(&user_inns_k)?;
            inn_users_tree.remove(&inn_users_k)?;
        }
    }

    let target = format!("/inn/{}", iid);
    Ok(Redirect::to(&target))
}

/// Vec data: post
struct OutPost {
    pid: u64,
    iid: u64,
    inn_name: String,
    uid: u64,
    username: String,
    title: String,
    tags: Vec<String>,
    content_html: String,
    created_at: String,
    upvotes: usize,
    downvotes: usize,
    is_locked: bool,
    is_upvoted: bool,
    is_downvoted: bool,
}

/// Page data: `post.html`
#[derive(Template)]
#[template(path = "post.html", escape = "none")]
struct PagePost<'a> {
    page_data: PageData<'a>,
    post: OutPost,
    comments: Vec<OutComment>,
    pageview: u64,
    anchor: usize,
    n: usize,
    is_desc: bool,
    has_joined: bool,
    is_mod: bool,
}

/// Vec data: Comment
struct OutComment {
    cid: u64,
    uid: u64,
    username: String,
    content: String,
    created_at: String,
    upvotes: usize,
    downvotes: usize,
    is_upvoted: bool,
    is_downvoted: bool,
}

/// url params: `post.html`
#[derive(Deserialize)]
pub(crate) struct ParamsPost {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    notification_cid: Option<u64>,
}

/// `GET /inn/:iid/:pid` post page
pub(crate) async fn post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u64, u64)>,
    Query(params): Query<ParamsPost>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let post: Post = get_one(&db, "posts", pid)?;
    let user: User = get_one(&db, "users", post.uid)?;
    let date = timestamp_to_date(post.created_at)?;
    let inn: Inn = get_one(&db, "inns", post.iid)?;

    let mut has_joined = false;
    let mut is_upvoted = false;
    let mut is_downvoted = false;
    let mut is_mod = false;
    let upvotes = get_count_by_prefix(&db, "post_upvotes", &u64_to_ivec(pid)).unwrap_or_default();
    let downvotes =
        get_count_by_prefix(&db, "post_downvotes", &u64_to_ivec(pid)).unwrap_or_default();
    if let Some(ref claim) = claim {
        let k = [&u64_to_ivec(pid), &u64_to_ivec(claim.uid)].concat();
        if db.open_tree("post_upvotes")?.contains_key(&k)? {
            is_upvoted = true;
        }
        if db.open_tree("post_downvotes")?.contains_key(&k)? {
            is_downvoted = true;
        }

        let k = [&u64_to_ivec(claim.uid), &u64_to_ivec(iid)].concat();
        if db.open_tree("user_inns")?.contains_key(&k)? {
            has_joined = true;
        }

        if db.open_tree("mod_inns")?.contains_key(&k)? {
            is_mod = true;
        }

        if let Some(notification_cid) = params.notification_cid {
            let k = [
                &u64_to_ivec(claim.uid),
                &u64_to_ivec(pid),
                &u64_to_ivec(notification_cid),
            ]
            .concat();
            db.open_tree("notifications")?
                .update_and_fetch(k, mark_read)?;
        }
    }

    let out_post = OutPost {
        pid: post.pid,
        uid: post.uid,
        username: user.username,
        iid: post.iid,
        inn_name: inn.inn_name,
        title: post.title,
        tags: post.tags,
        is_locked: post.is_locked,
        content_html: post.content_html,
        created_at: date,
        upvotes,
        downvotes,
        is_upvoted,
        is_downvoted,
    };

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(false);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut out_comments = Vec::with_capacity(n);
    let count = get_count(&db, "post_comments_count", u64_to_ivec(pid))?;
    if count > 0 {
        let (start, end) = get_range(count, &page_params);
        let post_comments_tree = db.open_tree("post_comments")?;
        let comment_upvotes_tree = db.open_tree("comment_upvotes")?;
        let comment_downvotes_tree = db.open_tree("comment_downvotes")?;
        for i in start..=end {
            let k = [&u64_to_ivec(pid), &u64_to_ivec(i as u64)].concat();
            let v = &post_comments_tree.get(k)?;
            if let Some(v) = v {
                let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
                let user: User = get_one(&db, "users", comment.uid)?;
                let date = timestamp_to_date(comment.created_at)?;

                let mut is_upvoted = false;
                let mut is_downvoted = false;

                if let Some(ref claim) = claim {
                    let k = [
                        &u64_to_ivec(pid),
                        &u64_to_ivec(comment.cid),
                        &u64_to_ivec(claim.uid),
                    ]
                    .concat();
                    is_upvoted = comment_upvotes_tree.contains_key(&k)?;
                    is_downvoted = comment_downvotes_tree.contains_key(&k)?;
                }

                let prefix = [&u64_to_ivec(pid), &u64_to_ivec(comment.cid)].concat();
                let upvotes =
                    get_count_by_prefix(&db, "comment_upvotes", &prefix).unwrap_or_default();
                let downvotes =
                    get_count_by_prefix(&db, "comment_downvotes", &prefix).unwrap_or_default();

                let out_comment = OutComment {
                    cid: comment.cid,
                    uid: comment.uid,
                    username: user.username,
                    content: comment.content,
                    created_at: date,
                    upvotes,
                    downvotes,
                    is_upvoted,
                    is_downvoted,
                };
                out_comments.push(out_comment);
            }
        }
        if is_desc {
            out_comments.reverse();
        }
    }

    let pageview = incr_id(&db.open_tree("post_pageviews")?, u64_to_ivec(pid))?;
    let has_unread = if let Some(ref claim) = claim {
        has_unread(&db, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("post", &site_config.site_name, claim, has_unread);
    let page_post = PagePost {
        page_data,
        post: out_post,
        comments: out_comments,
        pageview,
        anchor,
        n,
        is_desc,
        has_joined,
        is_mod,
    };

    Ok(into_response(&page_post, "html"))
}

/// generate static page `/static/post/:pid`
async fn static_post(db: &Db, pid: u64) -> Result<(), AppError> {
    let pid_ivec = u64_to_ivec(pid);
    let site_config = get_site_config(db)?;
    let post: Post = get_one(db, "posts", pid)?;
    let user: User = get_one(db, "users", post.uid)?;
    let date = timestamp_to_date(post.created_at)?;
    let inn: Inn = get_one(db, "inns", post.iid)?;
    let upvotes = get_count_by_prefix(db, "post_upvotes", &u64_to_ivec(pid)).unwrap_or_default();
    let downvotes =
        get_count_by_prefix(db, "post_downvotes", &u64_to_ivec(pid)).unwrap_or_default();

    let out_post = OutPost {
        pid: post.pid,
        uid: post.uid,
        username: user.username,
        iid: post.iid,
        inn_name: inn.inn_name,
        title: post.title,
        tags: post.tags,
        content_html: post.content_html,
        created_at: date,
        is_locked: post.is_locked,
        upvotes,
        downvotes,
        is_upvoted: false,
        is_downvoted: false,
    };

    let n = 50;
    let anchor = 0;
    let is_desc = true;

    let mut out_comments = Vec::with_capacity(n);
    let count = get_count(db, "post_comments_count", &pid_ivec)?;
    if count > 0 {
        // TODO: comments pagination
        let post_comments_tree = db.open_tree("post_comments")?;
        for i in 1..=count {
            let k = [&u64_to_ivec(pid), &u64_to_ivec(i as u64)].concat();
            let v = &post_comments_tree.get(k)?;
            if let Some(v) = v {
                let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
                let user: User = get_one(db, "users", comment.uid)?;
                let date = timestamp_to_date(comment.created_at)?;

                let prefix = [&u64_to_ivec(pid), &u64_to_ivec(comment.cid)].concat();
                let upvotes =
                    get_count_by_prefix(db, "comment_upvotes", &prefix).unwrap_or_default();
                let downvotes =
                    get_count_by_prefix(db, "comment_downvotes", &prefix).unwrap_or_default();

                let out_comment = OutComment {
                    cid: comment.cid,
                    uid: comment.uid,
                    username: user.username,
                    content: comment.content,
                    created_at: date,
                    upvotes,
                    downvotes,
                    is_upvoted: false,
                    is_downvoted: false,
                };
                out_comments.push(out_comment);
            }
        }
    }

    let pageview = if let Some(v) = &db.open_tree("post_pageviews")?.get(&pid_ivec)? {
        ivec_to_u64(v)
    } else {
        0
    };
    let page_data = PageData::new("post", &site_config.site_name, None, false);
    let page_post = PagePost {
        page_data,
        post: out_post,
        comments: out_comments,
        pageview,
        anchor,
        n,
        is_desc,
        has_joined: false,
        is_mod: false,
    };

    let res = &page_post.render().unwrap();

    let target_dir = format!("{}/post", &CONFIG.html_path);
    let target_dir = std::path::Path::new(&target_dir);
    if !target_dir.exists() {
        create_dir_all(target_dir).await?;
    }

    let mut e = GzEncoder::new(Vec::with_capacity(8192), Compression::default());
    e.write_all(res.as_bytes()).unwrap();
    let compressed_bytes = e.finish().unwrap();

    let target = target_dir.join(format!("{}.html.gz", pid));
    let target = std::path::Path::new(&target);
    let mut file = File::create(target).await?;
    file.write_all(&compressed_bytes).await?;
    debug!("target {} : pid: {:?}", target.display(), pid);

    Ok(())
}

/// Form data: `/inn/:iid/:pid/` comment create
#[derive(Deserialize, Validate)]
pub(crate) struct FormComment {
    #[validate(length(min = 1, max = 10000))]
    comment: String,
}

/// `POST /post/:iid/:pid/` comment create
pub(crate) async fn comment_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u64, u64)>,
    ValidatedForm(input): ValidatedForm<FormComment>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let inn_role = get_inn_role(&db, iid, claim.uid)?.ok_or(AppError::Unauthorized)?;
    if inn_role < 3 {
        return Err(AppError::Unauthorized);
    }

    if !db.open_tree("inns")?.contains_key(u64_to_ivec(iid))? {
        return Err(AppError::NotFound);
    }

    let created_at = OffsetDateTime::now_utc().unix_timestamp();
    if created_at - claim.last_write < site_config.comment_interval {
        return Err(AppError::WriteInterval);
    }

    let post: Post = get_one(&db, "posts", pid)?;
    if post.is_locked {
        return Err(AppError::Locked);
    }

    let pid_ivec = u64_to_ivec(pid);
    let cid = incr_id(&db.open_tree("post_comments_count")?, &pid_ivec)?;

    let mut content = input.comment;

    // extract @username or @uid notificaiton
    let notifications = extract_element(&content, 5, '@');
    let notification_tree = db.open_tree("notifications")?;
    for notification in &notifications {
        let (uid, username) = match notification.parse::<u64>() {
            Ok(uid) => {
                if let Ok(user) = get_one::<User>(&db, "users", uid) {
                    (uid, user.username)
                } else {
                    continue;
                }
            }
            Err(_) => {
                if let Some(uid) = get_uid_by_name(&db, notification)? {
                    (uid, notification.to_string())
                } else {
                    continue;
                }
            }
        };
        let notification_link = format!("[{}](/user/{})", username, uid);
        let from = format!("@{}", notification);
        let to = format!("@{}", notification_link);
        content = content.replace(&from, &to);

        // notify user to be mentioned in comment
        let notify_key = [&u64_to_ivec(uid), &pid_ivec, &u64_to_ivec(cid)].concat();
        notification_tree.insert(notify_key, vec![0])?;
    }

    let reply_to = extract_element(&content, 1, '#');
    let mut reply_to_cid = None;
    if !reply_to.is_empty() {
        if let Ok(reply_cid) = reply_to[0].parse::<u64>() {
            if reply_cid < cid {
                let reply_link = format!("[{}](/inn/{}/{}#{})", reply_to[0], iid, pid, reply_cid);
                let from = format!("#{}", reply_cid);
                let to = format!("#{}", reply_link);
                content = content.replace(&from, &to);
                reply_to_cid = Some(reply_cid);
            }
        }
    }

    let content = md2html(&content)?;
    let comment = Comment {
        cid,
        pid,
        uid: claim.uid,
        reply_to: reply_to_cid,
        content,
        created_at,
        // TODO: comment is collapsed
        is_collapsed: false,
    };
    let comment_encoded = bincode::encode_to_vec(&comment, standard())?;
    let k = [&pid_ivec, &u64_to_ivec(cid)].concat();
    db.open_tree("post_comments")?.insert(&k, comment_encoded)?;

    set_index(
        &db,
        "user_comments_count",
        claim.uid,
        "user_comments_idx",
        k,
    )?;

    let created_at_ivec = u64_to_ivec(created_at as u64);
    let iid_ivec = u64_to_ivec(iid);
    let k = [&iid_ivec, &pid_ivec].concat();

    let old_timestamp = db.open_tree("post_timeline_idx")?.get(&k)?;
    let mut visibility = 0;
    if let Some(v) = old_timestamp {
        let k = [&v, &iid_ivec, &pid_ivec].concat();
        if let Some(v) = db.open_tree("post_timeline")?.remove(&k)? {
            visibility = ivec_to_u64(&v);
        };
    }

    // only the fellow could update the timeline by adding comment
    if inn_role >= 5 {
        // kv_pair: iid#pid = timestamp
        db.open_tree("post_timeline_idx")?
            .insert(k, &created_at_ivec)?;

        let k = [&created_at_ivec, &iid_ivec, &pid_ivec].concat();
        // kv_pair: timestamp#iid#pid = visibility
        db.open_tree("post_timeline")?
            .insert(k, u64_to_ivec(visibility))?;
    }

    // notify post author
    if post.uid != claim.uid {
        let notify_key = [&u64_to_ivec(post.uid), &pid_ivec, &u64_to_ivec(cid)].concat();
        notification_tree.insert(notify_key, vec![1])?;
    }

    user_stats(&db, claim.uid, "comment")?;
    claim.update_last_write(&db)?;

    static_post(&db, pid).await?;
    if visibility < 10 {
        db.open_tree("static_inn_post")?.insert(&iid_ivec, &[])?;
    }
    let target = format!("/post/{}/{}", iid, pid);
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/upvote` post upvote
pub(crate) async fn post_upvote(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let post_upvotes_tree = db.open_tree("post_upvotes")?;
    let k = [&u64_to_ivec(pid), &u64_to_ivec(claim.uid)].concat();
    if post_upvotes_tree.contains_key(&k)? {
        post_upvotes_tree.remove(&k)?;
    } else {
        post_upvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{}/{}", iid, pid);
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/:cid/upvote` comment upvote
pub(crate) async fn comment_upvote(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid, cid)): Path<(u64, u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;
    let k = [
        &u64_to_ivec(pid),
        &u64_to_ivec(cid),
        &u64_to_ivec(claim.uid),
    ]
    .concat();

    let comment_upvotes_tree = db.open_tree("comment_upvotes")?;
    if comment_upvotes_tree.contains_key(&k)? {
        comment_upvotes_tree.remove(&k)?;
    } else {
        comment_upvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{}/{}", iid, pid);
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/post_lock` post lock
pub(crate) async fn post_lock(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    // only mod can lock post
    let mod_inns_k = [&u64_to_ivec(claim.uid), &u64_to_ivec(iid)].concat();
    if !db.open_tree("mod_inns")?.contains_key(mod_inns_k)? {
        return Err(AppError::Unauthorized);
    }

    let mut post: Post = get_one(&db, "posts", pid)?;
    post.is_locked = !post.is_locked;

    let post_encoded = bincode::encode_to_vec(&post, standard())?;
    db.open_tree("posts")?
        .insert(u64_to_ivec(pid), post_encoded)?;

    let target = format!("/post/{}/{}", iid, pid);
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/downvote` post downvote
pub(crate) async fn post_downvote(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let post_downvotes_tree = db.open_tree("post_downvotes")?;
    let k = [&u64_to_ivec(pid), &u64_to_ivec(claim.uid)].concat();
    if post_downvotes_tree.contains_key(&k)? {
        post_downvotes_tree.remove(&k)?;
    } else {
        post_downvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{}/{}", iid, pid);
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/:cid/downvote` comment downvote
pub(crate) async fn comment_downvote(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid, cid)): Path<(u64, u64, u64)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;
    let k = [
        &u64_to_ivec(pid),
        &u64_to_ivec(cid),
        &u64_to_ivec(claim.uid),
    ]
    .concat();

    let comment_downvotes_tree = db.open_tree("comment_downvotes")?;
    if comment_downvotes_tree.contains_key(&k)? {
        comment_downvotes_tree.remove(&k)?;
    } else {
        comment_downvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{}/{}", iid, pid);
    Ok(Redirect::to(&target))
}

fn get_inn_role(db: &Db, iid: u64, uid: u64) -> Result<Option<u8>, AppError> {
    let inn_users_k = [&u64_to_ivec(iid), &u64_to_ivec(uid)].concat();
    Ok(db
        .open_tree("inn_users")?
        .get(&inn_users_k)?
        .map(|role| role.to_vec()[0]))
}
