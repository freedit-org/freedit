//! ## Inn
//!
//! ### Permissions
//! | role    | comment | post | update timeline | lock post | inn admin | protected | Note             |
//! |---------|:-------:|:----:|:---------------:|:---------:|:---------:|:---------:|------------------|
//! | Pending |         |      |                 |           |           |           | Apply or Private |
//! | Deny    |         |      |                 |           |           |           | Apply or Private |
//! | Limited | ✅      |      |                 |           |           |           |                  |
//! | Intern  | ✅      | ✅   |                 |           |           |           |                  |
//! | Fellow  | ✅      | ✅   | ✅              |           |           |           |                  |
//! | Mod     | ✅      | ✅   | ✅              | ✅        | ✅        |           |                  |
//! | Super   | ✅      | ✅   | ✅              | ✅        | ✅        | ✅        |                  |

use super::{
    db_utils::{
        extract_element, get_batch, get_count, get_count_by_prefix, get_id_by_name,
        get_ids_by_prefix, get_ids_by_tag, get_one, get_range, is_valid_name, ivec_to_u32, set_one,
        set_one_with_key, u32_to_ivec, u8_slice_to_u32, IterType,
    },
    feed::{inn_feed_to_post, update},
    fmt::{clean_html, md2html, ts_to_date},
    incr_id,
    meta_handler::{PageData, ParamsPage},
    notification::{add_notification, mark_read, NtType},
    user::{InnRole, Role},
    Claim, Comment, Feed, FormPost, Inn, InnType, Post, PostContent, PostStatus, SiteConfig, User,
};
use crate::{error::AppError, DB};
use askama::{filters::escape, Html, Template};
use askama_axum::into_response;
use atom_syndication::{
    CategoryBuilder, ContentBuilder, EntryBuilder, FeedBuilder, LinkBuilder, PersonBuilder, Text,
    WriteConfig,
};
use axum::{
    extract::{Path, Query},
    response::{IntoResponse, Redirect},
    Form,
};
use axum_extra::{headers::Cookie, TypedHeader};
use axum_garde::WithValidation;
use bincode::config::standard;
use cached::proc_macro::cached;
use chrono::{DateTime, Utc};
use garde::Validate;
use once_cell::sync::Lazy;
use serde::Deserialize;
use sled::{transaction::ConflictableTransactionError, Transactional};
use sled::{Batch, Db};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    path::PathBuf,
};

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
    inn_feeds: Vec<Feed>,
}

/// `GET /mod/:iid` inn create/edit page
///
/// if iid is 0, then create a new inn
pub(crate) async fn mod_inn(
    cookie: Option<TypedHeader<Cookie>>,
    Path(iid): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    if Role::from(claim.role) < Role::Senior && Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    if iid == 0 && site_config.inn_mod_max > 0 {
        let mod_counts = get_count_by_prefix(&DB, "mod_inns", &u32_to_ivec(claim.uid))?;
        if mod_counts >= site_config.inn_mod_max {
            return Err(AppError::InnCreateLimit);
        }
    }

    let has_unread = User::has_unread(&DB, claim.uid)?;
    // create new inn
    if iid == 0 {
        let page_data = PageData::new("create new inn", &site_config, Some(claim), has_unread);
        let page_inn_create = PageInnCreate { page_data };
        Ok(into_response(&page_inn_create))
    } else {
        if !User::is_mod(&DB, claim.uid, iid)? && Role::from(claim.role) != Role::Admin {
            return Err(AppError::Unauthorized);
        }

        let page_data = PageData::new("edit inn", &site_config, Some(claim), has_unread);
        let inn: Inn = get_one(&DB, "inns", iid)?;
        let mut inn_feeds = Vec::new();
        for i in DB.open_tree("inn_feeds")?.scan_prefix(u32_to_ivec(iid)) {
            let (k, _) = i?;
            let feed_id = u8_slice_to_u32(&k[4..8]);
            let feed: Feed = get_one(&DB, "feeds", feed_id)?;
            inn_feeds.push(feed);
        }
        let page_inn_edit = PageInnEdit {
            page_data,
            inn,
            inn_feeds,
        };
        Ok(into_response(&page_inn_edit))
    }
}

/// Form data: `/mod/:iid` inn create/edit page
#[derive(Deserialize, Validate)]
pub(crate) struct FormInn {
    #[garde(length(min = 1, max = 64))]
    inn_name: String,
    #[garde(length(min = 1, max = 512))]
    about: String,
    #[garde(length(min = 1, max = 65535))]
    description: String,
    #[garde(length(min = 1, max = 128))]
    topics: String,
    #[garde(skip)]
    inn_type: u8,
    #[garde(skip)]
    early_birds: u32,
    #[garde(skip)]
    limit_edit_seconds: u32,
}

/// `POST /mod/:iid` inn create/edit page
///
/// if iid is 0, then create a new inn
pub(crate) async fn mod_inn_post(
    cookie: Option<TypedHeader<Cookie>>,
    Path(mut iid): Path<u32>,
    WithValidation(input): WithValidation<Form<FormInn>>,
) -> Result<impl IntoResponse, AppError> {
    let inn_name = clean_html(&input.inn_name);
    if !is_valid_name(&inn_name) {
        return Err(AppError::NameInvalid);
    }

    let inn_name_key = inn_name.replace(' ', "_").to_lowercase();

    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if Role::from(claim.role) < Role::Senior && Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    if iid == 0 && site_config.inn_mod_max > 0 {
        let mod_counts = get_count_by_prefix(&DB, "mod_inns", &u32_to_ivec(claim.uid))?;
        if mod_counts >= site_config.inn_mod_max {
            return Err(AppError::InnCreateLimit);
        }
    }

    // get inn topics
    let mut topics: BTreeSet<String> = input
        .topics
        .split('#')
        .map(clean_html)
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();

    let inn_type = InnType::from(input.inn_type);
    if inn_type == InnType::Private {
        topics.insert("private".into());
    } else {
        topics.retain(|t| t != "private")
    }

    let mut topics: Vec<_> = topics.into_iter().collect();
    topics.truncate(5);

    let inn_names_tree = DB.open_tree("inn_names")?;

    let mut batch_topics = Batch::default();
    // create new inn
    if iid == 0 {
        // check if inn name exists
        if inn_names_tree.contains_key(&inn_name_key)? {
            return Err(AppError::NameExists);
        }
        iid = incr_id(&DB, "inns_count")?;
    } else {
        // edit inn

        // check if this name is used by other inn
        let search_iid = inn_names_tree.get(&inn_name_key)?;
        if search_iid.is_some() && search_iid != Some(u32_to_ivec(iid)) {
            return Err(AppError::NameExists);
        }

        if !User::is_mod(&DB, claim.uid, iid)? && Role::from(claim.role) != Role::Admin {
            return Err(AppError::Unauthorized);
        }

        let inn: Inn = get_one(&DB, "inns", iid)?;
        let old_inn_type = InnType::from(inn.inn_type);

        if old_inn_type != inn_type {
            match old_inn_type {
                InnType::Apply => {
                    if inn_type != InnType::Hidden && inn_type != InnType::Public {
                        return Err(AppError::Custom("Inn type err".into()));
                    }
                }
                InnType::Public => {
                    if inn_type != InnType::Hidden && inn_type != InnType::Apply {
                        return Err(AppError::Custom("Inn type err".into()));
                    }
                }
                InnType::Hidden => {
                    if inn_type != InnType::Public && inn_type != InnType::Apply {
                        return Err(AppError::Custom("Inn type err".into()));
                    }
                }
                InnType::Private => {
                    if inn_type != InnType::PrivateHidden {
                        return Err(AppError::Custom("Inn type err".into()));
                    }
                }
                InnType::PrivateHidden => {
                    if inn_type != InnType::Private {
                        return Err(AppError::Custom("Inn type err".into()));
                    }
                }
            }

            let tree = DB.open_tree("user_posts")?;
            for i in tree.iter() {
                let (k, mut v) = i?;
                let iid = u8_slice_to_u32(&v[0..4]);
                if iid == inn.iid {
                    v[4] = inn_type as u8;
                    tree.insert(k, v)?;
                }
            }

            let tree = DB.open_tree("post_timeline_idx")?;
            for i in tree.scan_prefix(u32_to_ivec(iid)) {
                let (k, mut v) = i?;
                v[4] = inn_type as u8;
                tree.insert(k, v)?;
            }

            let tree = DB.open_tree("post_timeline")?;
            for i in tree.iter() {
                let (k, _) = i?;
                let iid = u8_slice_to_u32(&k[4..8]);
                if iid == inn.iid {
                    let v = inn_type as u8;
                    tree.insert(k, &[v])?;
                }
            }
        }

        // remove the old inn name
        if inn_name != inn.inn_name {
            let old_inn_name_key = inn.inn_name.replace(' ', "_").to_lowercase();
            inn_names_tree.remove(old_inn_name_key)?;
        }

        // remove the old inn topics
        for topic in inn.topics {
            let k = [topic.as_bytes(), &u32_to_ivec(iid)].concat();
            batch_topics.remove(&*k);
        }
    }

    let iid_ivec = u32_to_ivec(iid);

    // set topic index for inns
    for topic in &topics {
        let k = [topic.as_bytes(), &u32_to_ivec(iid)].concat();
        batch_topics.insert(&*k, &[]);
    }
    DB.open_tree("topics")?.apply_batch(batch_topics)?;

    // set index for user mods and user inns
    let k = [&u32_to_ivec(claim.uid), &iid_ivec].concat();
    DB.open_tree("mod_inns")?.insert(&k, &[])?;
    DB.open_tree("user_inns")?.insert(&k, &[])?;

    // set index for inn users
    let k = [&iid_ivec, &u32_to_ivec(claim.uid)].concat();
    DB.open_tree("inn_users")?.insert(k, &[10])?;

    let inn = Inn {
        iid,
        inn_name,
        about: clean_html(&input.about),
        description: clean_html(&input.description),
        topics,
        inn_type: inn_type as u8,
        early_birds: input.early_birds,
        created_at: Utc::now().timestamp(),
        limit_edit_seconds: input.limit_edit_seconds,
    };

    if InnType::from(inn.inn_type) == InnType::Private {
        DB.open_tree("inns_private")?.insert(&iid_ivec, &[])?;
    }

    set_one(&DB, "inns", iid, &inn)?;
    inn_names_tree.insert(inn_name_key, iid_ivec)?;

    let target = format!("/inn/{iid}");
    Ok(Redirect::to(&target))
}

/// Form data: `/mod/feed/:iid` inn feed page
#[derive(Deserialize)]
pub(crate) struct FormInnFeed {
    url: String,
}

/// `POST /mod/feed/:iid` inn feed page
pub(crate) async fn mod_feed_post(
    cookie: Option<TypedHeader<Cookie>>,
    Path(iid): Path<u32>,
    Form(input): Form<FormInnFeed>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if Role::from(claim.role) < Role::Senior {
        return Err(AppError::Unauthorized);
    }

    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    if input.url.contains(&format!("inn/{iid}/feed")) {
        return Err(AppError::Custom("You can not feed yourself".into()));
    }

    let (feed, _) = update(&clean_html(&input.url), &DB, 5).await?;

    let feed_links_tree = DB.open_tree("feed_links")?;
    let feed_id = if let Some(v) = feed_links_tree.get(&feed.link)? {
        ivec_to_u32(&v)
    } else {
        let id = incr_id(&DB, "feeds_count")?;
        feed_links_tree.insert(&feed.link, u32_to_ivec(id))?;
        id
    };

    set_one(&DB, "feeds", feed_id, &feed)?;

    let k = &[u32_to_ivec(iid), u32_to_ivec(feed_id)].concat();
    let inn_feeds_tree = DB.open_tree("inn_feeds")?;
    if inn_feeds_tree.contains_key(k)? {
        inn_feeds_tree.remove(k)?;
    } else {
        inn_feeds_tree.insert(k, u32_to_ivec(claim.uid))?;
        inn_feed_to_post(&DB, iid, feed_id, claim.uid)?;
    }

    let target = format!("/mod/{iid}");
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
    iid: u32,
    inn_name: String,
    about: String,
    topics: Vec<String>,
}

/// Page data: `inn_list.html`
#[derive(Template)]
#[template(path = "inn_list.html", escape = "none")]
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
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<ParamsInnList>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));
    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut inns: Vec<Inn> = Vec::with_capacity(n);

    if let Some(topic) = &params.topic {
        for i in get_ids_by_tag(&DB, "topics", topic, Some(&page_params))? {
            if let Ok(inn) = get_one::<Inn>(&DB, "inns", i) {
                if !inn.is_closed() {
                    inns.push(inn);
                }
            }
        }
    } else if let Some(claim) = &claim {
        let uid_ivec = u32_to_ivec(claim.uid);
        if params.filter.as_deref() == Some("mod") {
            for i in get_ids_by_prefix(&DB, "mod_inns", uid_ivec, Some(&page_params))? {
                if let Ok(inn) = get_one(&DB, "inns", i) {
                    inns.push(inn);
                }
            }
        } else if params.filter.as_deref() == Some("joined") {
            for i in get_ids_by_prefix(&DB, "user_inns", uid_ivec, Some(&page_params))? {
                if let Ok(inn) = get_one::<Inn>(&DB, "inns", i) {
                    if !inn.is_closed() {
                        inns.push(inn);
                    }
                }
            }
        } else {
            inns = get_batch(&DB, "default", "inns_count", "inns", &page_params)?;
        }
    } else {
        inns = get_batch(&DB, "default", "inns_count", "inns", &page_params)?;
    }

    let mut out_inns = Vec::with_capacity(inns.len());
    for i in inns {
        let icon = match InnType::from(i.inn_type) {
            InnType::Public => "",
            InnType::Private => "🔒 ",
            InnType::Apply => "🙋 ",
            InnType::Hidden | InnType::PrivateHidden => "💀 ",
        };
        let inn_name = format!("{} {}", icon, i.inn_name);
        let out_inn = OutInnList {
            iid: i.iid,
            inn_name,
            about: i.about,
            topics: i.topics,
        };
        out_inns.push(out_inn);
    }

    let filter = if claim.is_none() { None } else { params.filter };
    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("inn list", &site_config, claim, has_unread);
    let page_inn_list = PageInnList {
        page_data,
        inns: out_inns,
        anchor,
        n,
        is_desc,
        topic: params.topic,
        filter,
    };

    Ok(into_response(&page_inn_list))
}

/// Page data: `post_create.html`
#[derive(Template)]
#[template(path = "post_create.html")]
struct PagePostCreate<'a> {
    page_data: PageData<'a>,
    joined: Vec<(String, u32)>,
    selected_iid: u32,
    draft: FormPost,
    draft_titles: Vec<String>,
}

/// Page data: `post_edit.html`
#[derive(Template)]
#[template(path = "post_edit.html")]
struct PagePostEdit<'a> {
    page_data: PageData<'a>,
    post: Post,
}

/// url params: `post_create.html`
#[derive(Deserialize)]
pub(crate) struct ParamsPostCreate {
    iid: Option<u32>,
    from_draft: Option<String>,
}

/// `GET /post/edit/:pid` post create/edit page
///
/// if pid is 0, then create a new post
pub(crate) async fn edit_post(
    cookie: Option<TypedHeader<Cookie>>,
    Path(pid): Path<u32>,
    Query(params): Query<ParamsPostCreate>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let joined_ids = get_ids_by_prefix(&DB, "user_inns", u32_to_ivec(claim.uid), None)?;
    let mut joined = Vec::with_capacity(joined_ids.len());
    for id in joined_ids {
        let inn: Inn = get_one(&DB, "inns", id)?;
        if !inn.is_closed() {
            let inn_role = InnRole::get(&DB, inn.iid, claim.uid)?;
            if let Some(role) = inn_role {
                if role >= InnRole::Intern {
                    joined.push((inn.inn_name, inn.iid));
                }
            }
        }
    }

    if joined.is_empty() {
        return Err(AppError::NoJoinedInn);
    }

    let mut selected_iid = params.iid.unwrap_or_default();
    let has_unread = User::has_unread(&DB, claim.uid)?;
    if pid == 0 {
        let mut draft = FormPost::default();
        let mut draft_titles = vec![];
        for i in DB.open_tree("drafts")?.scan_prefix(u32_to_ivec(claim.uid)) {
            let (k, _) = i?;
            let draft_title = String::from_utf8_lossy(&k[4..]).to_string();
            draft_titles.push(draft_title);
        }

        if let Some(from_draft) = params.from_draft {
            let k: Vec<u8> = [&u32_to_ivec(claim.uid), from_draft.as_bytes()].concat();
            if let Some(v) = DB.open_tree("drafts")?.get(k)? {
                (draft, _) = bincode::decode_from_slice(&v, standard())?;
            };
            selected_iid = draft.iid;
        };

        let page_data = PageData::new("new post", &site_config, Some(claim), has_unread);
        let page_post_create = PagePostCreate {
            page_data,
            joined,
            draft,
            selected_iid,
            draft_titles,
        };

        Ok(into_response(&page_post_create))
    } else {
        let mut post: Post = get_one(&DB, "posts", pid)?;
        let inn: Inn = get_one(&DB, "inns", post.iid)?;

        if (post.created_at + (inn.limit_edit_seconds as i64) < Utc::now().timestamp())
            && inn.limit_edit_seconds != 0
        {
            return Err(AppError::Unauthorized);
        }

        if post.uid != claim.uid {
            return Err(AppError::Unauthorized);
        }

        if post.status != PostStatus::Normal {
            return Err(AppError::LockedOrHidden);
        }

        if InnType::from(inn.inn_type) == InnType::Private {
            post.tags.push("private".into());
        }

        let page_data = PageData::new("edit post", &site_config, Some(claim), has_unread);
        let page_post_edit = PagePostEdit { page_data, post };

        Ok(into_response(&page_post_edit))
    }
}

pub(super) fn inn_add_index(
    db: &Db,
    iid: u32,
    pid: u32,
    timestamp: u32,
    inn_type: u8,
) -> Result<(), AppError> {
    let tl_idx_tree = db.open_tree("post_timeline_idx")?;
    let tl_tree = db.open_tree("post_timeline")?;

    (&tl_idx_tree, &tl_tree)
        .transaction(|(tl_idx, tl)| {
            let k = [&u32_to_ivec(iid), &u32_to_ivec(pid)].concat();
            let created_at_ivec = u32_to_ivec(timestamp);
            let mut v = timestamp.to_be_bytes().to_vec();
            v.push(inn_type);
            tl_idx.insert(&*k, v)?;

            let k = [&created_at_ivec, &u32_to_ivec(iid), &u32_to_ivec(pid)].concat();
            tl.insert(k, &[inn_type])?;

            Ok::<(), ConflictableTransactionError<AppError>>(())
        })
        .map_err(|e| AppError::Custom(format!("transaction error: {e}")))
}

fn inn_rm_index(db: &Db, iid: u32, pid: u32) -> Result<u8, AppError> {
    let tl_idx_tree = db.open_tree("post_timeline_idx")?;
    let tl_tree = db.open_tree("post_timeline")?;

    (&tl_idx_tree, &tl_tree)
        .transaction(|(tl_idx, tl)| {
            let mut inn_type = 0;

            let k = [&u32_to_ivec(iid), &u32_to_ivec(pid)].concat();
            if let Some(v) = tl_idx.remove(&*k)? {
                let k = [&v[0..4], &u32_to_ivec(iid), &u32_to_ivec(pid)].concat();
                if let Some(v) = tl.remove(k)? {
                    inn_type = v[0];
                }
            }

            Ok::<u8, ConflictableTransactionError<AppError>>(inn_type)
        })
        .map_err(|e| AppError::Custom(format!("transaction error: {e}")))
}

/// `POST /post/edit/:pid` post create/edit page
///
/// if pid is 0, then create a new post
pub(crate) async fn edit_post_post(
    cookie: Option<TypedHeader<Cookie>>,
    Path(old_pid): Path<u32>,
    WithValidation(input): WithValidation<Form<FormPost>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let input = input.into_inner();

    let is_draft = input.is_draft.unwrap_or_default();
    let delete_draft = input.delete_draft.unwrap_or_default();

    let k: Vec<u8> = [&u32_to_ivec(claim.uid), input.title.as_bytes()].concat();

    if let Some(spam_regex) = &site_config.spam_regex {
        let re = regex::Regex::new(spam_regex).unwrap();
        if re.is_match(&input.title) || re.is_match(&input.content) || re.is_match(&input.tags) {
            return Err(AppError::Custom("Spam detected".into()));
        }
    }

    if delete_draft {
        DB.open_tree("drafts")?.remove(&k)?;
        return Ok(Redirect::to("/post/edit/0"));
    }
    if is_draft {
        set_one_with_key(&DB, "drafts", k, &input)?;
        return Ok(Redirect::to("/post/edit/0"));
    }

    let iid = input.iid;
    let inn_role = InnRole::get(&DB, iid, claim.uid)?.ok_or(AppError::Unauthorized)?;
    if inn_role <= InnRole::Limited {
        return Err(AppError::Unauthorized);
    }

    let mut created_at = Utc::now().timestamp();
    if created_at - claim.last_write < site_config.post_interval {
        return Err(AppError::WriteInterval);
    }

    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    let pid = if old_pid == 0 {
        incr_id(&DB, "posts_count")?
    } else {
        old_pid
    };
    let pid_ivec = u32_to_ivec(pid);

    let mut tags = vec![];
    if inn.is_open_access() {
        let tags_set: BTreeSet<String> = input
            .tags
            .split('#')
            .map(clean_html)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        tags = tags_set.into_iter().collect();
        tags.truncate(5);

        let mut batch = Batch::default();
        if old_pid > 0 {
            let post: Post = get_one(&DB, "posts", old_pid)?;
            if post.uid != claim.uid {
                return Err(AppError::Unauthorized);
            }

            if post.status != PostStatus::Normal {
                return Err(AppError::LockedOrHidden);
            }

            if post.iid != iid {
                return Err(AppError::NotFound);
            }

            created_at = post.created_at;
            for old_tag in &post.tags {
                let k = [old_tag.as_bytes(), &u32_to_ivec(old_pid)].concat();
                batch.remove(k);
            }
        }

        for tag in &tags {
            let k = [tag.as_bytes(), &pid_ivec].concat();
            batch.insert(k, &[]);
        }
        DB.open_tree("tags")?.apply_batch(batch)?;
    }

    let mut content = input.content;
    // extract @username or @uid notification
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
        let notification_link = format!(
            "<span class='replytag'>[![](/static/avatars/{uid}.png){username}](/user/{uid})</span>"
        );
        let from = format!("@{notification}");
        let to = format!("@{notification_link}");
        content = content.replace(&from, &to);

        // notify user to be mentioned
        // prevent duplicate notifications
        if uid != claim.uid {
            add_notification(&DB, uid, NtType::PostMention, pid, 0)?;
        }
    }

    let post = Post {
        pid,
        uid: claim.uid,
        iid,
        title: clean_html(&input.title),
        tags,
        content: PostContent::Markdown(clean_html(&content)),
        created_at,
        status: PostStatus::Normal,
    };

    set_one(&DB, "posts", pid, &post)?;

    let iid_ivec = u32_to_ivec(iid);
    if old_pid == 0 {
        let k = [&iid_ivec, &pid_ivec].concat();
        DB.open_tree("inn_posts")?.insert(k, &[])?;

        let k = [&u32_to_ivec(claim.uid), &pid_ivec].concat();
        let mut v = iid.to_be_bytes().to_vec();
        v.push(inn.inn_type);
        DB.open_tree("user_posts")?.insert(k, v)?;
    }

    if old_pid > 0 {
        inn_rm_index(&DB, iid, pid)?;
    }

    inn_add_index(&DB, iid, pid, created_at as u32, inn.inn_type)?;
    User::update_stats(&DB, claim.uid, "post")?;
    claim.update_last_write(&DB)?;

    if inn.is_open_access() {
        DB.open_tree("tan")?.insert(format!("post{}", pid), &[])?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// Vec data: post list
struct OutPostList {
    pid: u32,
    iid: u32,
    inn_name: String,
    uid: u32,
    username: String,
    title: String,
    created_at: String,
    comment_count: u32,
    last_reply: Option<(u32, String)>,
    is_pinned: bool,
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
    pub(crate) anchor: Option<usize>,
    pub(crate) is_desc: Option<bool>,
}

/// `GET /inn/tag/:tag` tag page
pub(crate) async fn tag(
    cookie: Option<TypedHeader<Cookie>>,
    Path(tag): Path<String>,
    Query(params): Query<ParamsTag>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let index = get_ids_by_tag(&DB, "tags", &tag, Some(&page_params))?;
    let out_post_list = get_out_post_list(&DB, &index)?;

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("inn", &site_config, claim, has_unread);
    let page_tag = PageTag {
        page_data,
        posts: out_post_list,
        anchor,
        n,
        is_desc,
        tag,
    };

    Ok(into_response(&page_tag))
}

/// Page data: `inn.html`
#[derive(Template)]
#[template(path = "inn.html", escape = "none")]
struct PageInn<'a> {
    page_data: PageData<'a>,
    posts: Vec<OutPostList>,
    iid: u32,
    inn_name: String,
    about: String,
    description: String,
    anchor: usize,
    n: usize,
    is_desc: bool,
    inn_role: u8,
    filter: Option<String>,
    username: Option<String>,
    inn_users_count: usize,
    is_mod: bool,
    inns: Vec<(u32, String, bool)>,
    recommend_users: Vec<(u32, String)>,
    counts: usize,
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
    cookie: Option<TypedHeader<Cookie>>,
    Path(i): Path<String>,
    Query(params): Query<ParamsInn>,
) -> Result<impl IntoResponse, AppError> {
    let iid = match i.parse::<u32>() {
        Ok(iid) => iid,
        Err(_) => get_id_by_name(&DB, "inn_names", &i)?.ok_or(AppError::NotFound)?,
    };

    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut index = Vec::with_capacity(n);
    let mut joined_inns = &Vec::new();
    let mut user_iins: Result<Vec<u32>, AppError> = Err(AppError::NotFound);
    let mut username: Option<String> = None;
    let mut is_mod = false;
    let mut is_site_admin = false;
    if let Some(ref claim) = claim {
        is_mod = User::is_mod(&DB, claim.uid, iid)?;
        is_site_admin = Role::from(claim.role) == Role::Admin;
        user_iins = get_ids_by_prefix(&DB, "user_inns", u32_to_ivec(claim.uid), None);
        if let Ok(ref user_iins) = user_iins {
            joined_inns = user_iins;
        }
    }

    match params.filter.as_deref() {
        Some("joined") if claim.is_some() => {
            if let Ok(ref iids) = user_iins {
                index = get_pids_by_iids(&DB, iids, &page_params)?;
            };
        }
        Some("following") => {
            if let Some(ref claim) = claim {
                let user_following: Vec<u32> =
                    get_ids_by_prefix(&DB, "user_following", u32_to_ivec(claim.uid), None)
                        .unwrap_or_default();
                index = get_pids_by_uids(
                    &DB,
                    &user_following,
                    joined_inns,
                    &page_params,
                    is_site_admin,
                )?;
            }
        }
        Some(uid) => {
            if let Ok(uid) = uid.parse::<u32>() {
                let user: User = get_one(&DB, "users", uid)?;
                username = Some(user.username);
                index = get_pids_by_uids(&DB, &[uid], joined_inns, &page_params, is_site_admin)?;
            };
        }
        _ => {
            if iid == 0 {
                index = get_pids_all(&DB, joined_inns, &page_params, is_site_admin)?;
            } else {
                if DB
                    .open_tree("inns_private")?
                    .contains_key(u32_to_ivec(iid))?
                {
                    if joined_inns.contains(&iid) || is_site_admin {
                        index = get_pids_by_iids(&DB, &[iid], &page_params)?;
                    }
                } else {
                    index = get_pids_by_iids(&DB, &[iid], &page_params)?;
                }

                // add pinned posts
                let pinned_pids = get_ids_by_prefix(&DB, "post_pins", u32_to_ivec(iid), None)?;
                index.retain(|r| !pinned_pids.contains(r));
                index = pinned_pids.into_iter().chain(index).collect();
            }
        }
    }

    let out_post_list = get_out_post_list(&DB, &index)?;
    let mut inn_role = 0;
    if let Some(ref claim) = claim {
        if iid > 0 {
            if let Ok(Some(role)) = InnRole::get(&DB, iid, claim.uid) {
                inn_role = role as u8;
            }
        }
    }

    let inn_users_count = if iid > 0 {
        get_count_by_prefix(&DB, "inn_users", &u32_to_ivec(iid))?
    } else {
        0
    };

    let filter = if claim.is_none() { None } else { params.filter };
    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };

    let recommend_inns = recommend_inns()?;

    let mut inns = Vec::new();
    for (id, inn_name) in recommend_inns {
        if id != iid {
            let joined = joined_inns.contains(&id);
            inns.push((id, inn_name, joined))
        }
    }

    let page_data = PageData::new("inn", &site_config, claim, has_unread);

    let inn_name;
    let about;
    let description;

    if iid > 0 {
        let inn: Inn = get_one(&DB, "inns", iid)?;
        let icon = match InnType::from(inn.inn_type) {
            InnType::Public => "",
            InnType::Private => "🔒 ",
            InnType::Apply => "🙋 ",
            InnType::Hidden | InnType::PrivateHidden => "💀 ",
        };
        inn_name = format!("{} {}", icon, inn.inn_name);
        about = inn.about;
        description = md2html(&inn.description);
    } else {
        inn_name = "No post".into();
        about = "".into();
        description = "".into();
    };

    let recommend_users = recommend_users()?;
    let counts = get_count_by_prefix(&DB, "inn_posts", &u32_to_ivec(iid)).unwrap_or_default();
    let page_inn = PageInn {
        page_data,
        inn_name,
        about,
        description,
        posts: out_post_list,
        anchor,
        iid,
        n,
        is_desc,
        inn_role,
        filter,
        username,
        inn_users_count,
        inns,
        is_mod: is_mod || is_site_admin,
        recommend_users,
        counts,
    };

    Ok(into_response(&page_inn))
}

#[cached(time = 120, result = true)]
fn recommend_inns() -> Result<Vec<(u32, String)>, AppError> {
    let mut maps = HashMap::new();
    for i in &DB.open_tree("inn_posts")? {
        let (k, _) = i?;
        let iid = u8_slice_to_u32(&k[0..4]);
        maps.entry(iid).and_modify(|e| *e += 1).or_insert(1);
    }

    let mut maps = maps.into_iter().collect::<Vec<_>>();
    maps.sort_by(|a, b| b.1.cmp(&a.1));

    let mut recommend_inns = Vec::new();
    for (iid, _) in maps.into_iter() {
        let inn: Inn = get_one(&DB, "inns", iid)?;
        if inn.is_open_access() {
            recommend_inns.push((iid, inn.inn_name));
        }
        if recommend_inns.len() >= 3 {
            break;
        }
    }

    Ok(recommend_inns)
}

#[cached(time = 120, result = true)]
fn recommend_users() -> Result<Vec<(u32, String)>, AppError> {
    const NUM: usize = 15;
    let mut uids = HashSet::with_capacity(NUM);
    let mut users = vec![];
    for i in &DB.open_tree("user_posts")? {
        let (k, _) = i?;
        let uid = u8_slice_to_u32(&k[0..4]);
        uids.insert(uid);
        if uids.len() >= NUM {
            break;
        }
    }

    for i in &DB.open_tree("user_comments")? {
        if uids.len() >= NUM {
            break;
        }
        let (k, _) = i?;
        let uid = u8_slice_to_u32(&k[0..4]);
        uids.insert(uid);
    }

    for i in &DB.open_tree("user_solos")? {
        if uids.len() >= NUM {
            break;
        }
        let (k, _) = i?;
        let uid = u8_slice_to_u32(&k[0..4]);
        uids.insert(uid);
    }

    for i in uids {
        let user: User = get_one(&DB, "users", i)?;
        users.push((i, user.username));
    }

    Ok(users)
}

static FEED_CONFIG: Lazy<WriteConfig> = Lazy::new(|| WriteConfig {
    write_document_declaration: false,
    indent_size: Some(2),
});

/// `GET /inn/:iid/atom.xml` inn page
pub(crate) async fn inn_feed(Path(i): Path<String>) -> Result<impl IntoResponse, AppError> {
    let page_params = ParamsPage {
        anchor: 0,
        n: 20,
        is_desc: true,
    };

    let site_config = SiteConfig::get(&DB)?;
    let iid = match i.parse::<u32>() {
        Ok(iid) => iid,
        Err(_) => get_id_by_name(&DB, "inn_names", &i)?.ok_or(AppError::NotFound)?,
    };

    let mut index = Vec::with_capacity(page_params.n);
    let title;
    let description;
    let alternate_link = PathBuf::from(&site_config.domain)
        .join("inn")
        .join(iid.to_string())
        .display()
        .to_string();
    let self_link = format!("{alternate_link}/atom.xml");
    let mut categories = Vec::new();

    if iid == 0 {
        index = get_pids_all(&DB, &[], &page_params, false)?;
        title = site_config.site_name;
        description = site_config.description;
    } else {
        let inn: Inn = get_one(&DB, "inns", iid)?;
        description = md2html(&inn.about);
        if inn.is_open_access() {
            index = get_pids_by_iids(&DB, &[iid], &page_params)?;
        }
        title = inn.inn_name;
        for i in &inn.topics {
            categories.push(CategoryBuilder::default().term(i).build());
        }
    }

    let mut entries = Vec::with_capacity(index.len());
    for i in index.into_iter() {
        let post: Post = get_one(&DB, "posts", i)?;
        let user: User = get_one(&DB, "users", post.uid)?;
        let content = post.content.to_html(&DB)?;
        let post_link = format!("/post/{}/{}", post.iid, post.pid);
        let dt: DateTime<Utc> = DateTime::<Utc>::from_timestamp(post.created_at, 0).unwrap();
        let entry = EntryBuilder::default()
            .authors(vec![PersonBuilder::default()
                .name(user.username)
                .uri(Some(format!("{}/user/{}", &site_config.domain, user.uid)))
                .build()])
            .id(&post_link)
            .updated(dt)
            .links(vec![LinkBuilder::default()
                .rel("alternate".to_owned())
                .mime_type(Some("text/html".to_owned()))
                .href(post_link)
                .build()])
            .title(Text::plain(post.title))
            .content(Some(ContentBuilder::default().value(Some(content)).build()))
            .build();
        entries.push(entry);
    }

    let mut feed = FeedBuilder::default()
        .title(title)
        .subtitle(Some(Text::plain(description)))
        .id(&self_link)
        .link(
            LinkBuilder::default()
                .rel("self".to_owned())
                .mime_type(Some("application/atom+xml".to_owned()))
                .href(self_link)
                .build(),
        )
        .link(
            LinkBuilder::default()
                .rel("alternate".to_owned())
                .mime_type(Some("text/html".to_owned()))
                .href(alternate_link)
                .build(),
        )
        .build();

    feed.set_categories(categories);
    feed.set_entries(entries);

    let mut output = Vec::new();
    feed.write_with_config(&mut output, *FEED_CONFIG).unwrap();

    let headers = [(
        http::header::CONTENT_TYPE,
        http::HeaderValue::from_static("application/xml"),
    )];

    Ok((headers, output))
}

/// get [OutPostList] from pids
fn get_out_post_list(db: &Db, index: &[u32]) -> Result<Vec<OutPostList>, AppError> {
    let mut post_lists = Vec::with_capacity(index.len());
    if !index.is_empty() {
        for pid in index {
            let post: Post = get_one(db, "posts", *pid)?;
            let user: User = get_one(db, "users", post.uid)?;
            let date = ts_to_date(post.created_at);
            let inn: Inn = get_one(db, "inns", post.iid)?;
            let comment_count =
                get_count_by_prefix(db, "post_comments", &u32_to_ivec(*pid))? as u32;

            let last_reply = if let Some(i) = db
                .open_tree("post_comments")?
                .scan_prefix(u32_to_ivec(*pid))
                .last()
            {
                let (_, v) = i?;
                let (one, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
                let user: User = get_one(db, "users", one.uid)?;
                Some((user.uid, user.username))
            } else {
                None
            };

            let k = [&u32_to_ivec(post.iid), &u32_to_ivec(post.pid)].concat();
            let is_pinned = db.open_tree("post_pins")?.contains_key(k)?;

            let post_list = OutPostList {
                pid: post.pid,
                iid: post.iid,
                inn_name: inn.inn_name,
                uid: post.uid,
                username: user.username,
                title: post.title,
                created_at: date,
                comment_count,
                last_reply,
                is_pinned,
            };
            post_lists.push(post_list);
        }
    }
    Ok(post_lists)
}

/// get pids all, controlled by `inn_type`, sorted by timestamp
fn get_pids_all(
    db: &Db,
    joined_inns: &[u32],
    page_params: &ParamsPage,
    is_site_admin: bool,
) -> Result<Vec<u32>, AppError> {
    let tree = db.open_tree("post_timeline")?;
    let mut count: usize = 0;
    let mut result = Vec::with_capacity(page_params.n);
    let iter = if page_params.is_desc {
        IterType::Rev(tree.iter().rev())
    } else {
        IterType::Iter(tree.iter())
    };

    // kvpaire: timestamp#iid#pid = inn_type
    for i in iter {
        let (k, v) = i?;
        let id = u8_slice_to_u32(&k[4..8]);
        let out_id = u8_slice_to_u32(&k[8..12]);
        let inn_type = InnType::from(v[0]);
        if inn_type == InnType::Public
            || inn_type == InnType::Apply
            || (inn_type == InnType::Private && (joined_inns.contains(&id) || is_site_admin))
        {
            if count < page_params.anchor {
                count += 1;
                continue;
            }
            result.push(out_id);
        }

        if result.len() == page_params.n {
            break;
        }
    }

    Ok(result)
}

/// get pids by multi iids, sorted by timestamp
fn get_pids_by_iids(db: &Db, iids: &[u32], page_params: &ParamsPage) -> Result<Vec<u32>, AppError> {
    let mut pids = Vec::with_capacity(page_params.n);
    let mut pairs = Vec::new();
    for iid in iids {
        let prefix = u32_to_ivec(*iid);
        // kv_pair: iid#pid = timestamp
        for i in db.open_tree("post_timeline_idx")?.scan_prefix(prefix) {
            let (k, v) = i?;
            let pid = u8_slice_to_u32(&k[4..8]);
            let timestamp = u8_slice_to_u32(&v[0..4]);
            let inn_type = InnType::from(v[4]);
            if inn_type != InnType::Hidden && inn_type != InnType::PrivateHidden {
                pairs.push((pid, timestamp));
            }
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

/// get pids by multi uids, controlled by `inn_type`, sorted by timestamp
fn get_pids_by_uids(
    db: &Db,
    uids: &[u32],
    joined_inns: &[u32],
    page_params: &ParamsPage,
    is_site_admin: bool,
) -> Result<Vec<u32>, AppError> {
    let mut pids = Vec::with_capacity(page_params.n);
    for uid in uids {
        let prefix = u32_to_ivec(*uid);
        // kv_pair: uid#pid = iid#inn_type
        for i in db.open_tree("user_posts")?.scan_prefix(prefix) {
            let (k, v) = i?;
            let pid = u8_slice_to_u32(&k[4..8]);
            let iid = u8_slice_to_u32(&v[0..4]);
            let inn_type = InnType::from(v[4]);
            if inn_type == InnType::Public
                || inn_type == InnType::Apply
                || (inn_type == InnType::Private && (joined_inns.contains(&iid) || is_site_admin))
            {
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
    cookie: Option<TypedHeader<Cookie>>,
    Path(i): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let iid = match i.parse::<u32>() {
        Ok(iid) => iid,
        Err(_) => get_id_by_name(&DB, "inn_names", &i)?.ok_or(AppError::NotFound)?,
    };

    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    let user_inns_k = [&u32_to_ivec(claim.uid), &u32_to_ivec(iid)].concat();
    let inn_users_k = [&u32_to_ivec(iid), &u32_to_ivec(claim.uid)].concat();
    let user_inns_tree = DB.open_tree("user_inns")?;
    let inn_users_tree = DB.open_tree("inn_users")?;
    let inn_apply_tree = DB.open_tree("inn_apply")?;

    match inn_users_tree.get(&inn_users_k)? {
        None => {
            if InnType::from(inn.inn_type) == InnType::Apply
                || InnType::from(inn.inn_type) == InnType::Private
            {
                // 1: applied, but pending
                inn_users_tree.insert(&inn_users_k, &[1])?;
                inn_apply_tree.insert(&inn_users_k, &[])?;
            } else {
                user_inns_tree.insert(&user_inns_k, &[])?;
                let count = get_count_by_prefix(&DB, "inn_users", &u32_to_ivec(iid))? as u32;
                if inn.early_birds > 0 && count <= inn.early_birds {
                    inn_users_tree.insert(&inn_users_k, &[5])?;
                } else {
                    inn_users_tree.insert(&inn_users_k, &[4])?;
                }
            }
        }
        Some(_) => {
            user_inns_tree.remove(&user_inns_k)?;
            inn_users_tree.remove(&inn_users_k)?;
            inn_apply_tree.remove(&inn_users_k)?;
        }
    }

    let target = format!("/inn/{i}");
    Ok(Redirect::to(&target))
}

/// Vec data: post
struct OutPost {
    pid: u32,
    iid: u32,
    inn_name: String,
    uid: u32,
    username: String,
    title: String,
    tags: Vec<String>,
    content_html: String,
    og_content: String,
    created_at: String,
    upvotes: usize,
    downvotes: usize,
    status: String,
    is_upvoted: bool,
    is_downvoted: bool,
    can_edit: bool,
    is_pinned: bool,
}

/// Page data: `post.html`
#[derive(Template)]
#[template(path = "post.html", escape = "none")]
struct PagePost<'a> {
    page_data: PageData<'a>,
    post: OutPost,
    comments: Vec<OutComment>,
    pageview: u32,
    anchor: usize,
    n: usize,
    is_desc: bool,
    has_joined: bool,
    is_mod: bool,
    is_author: bool,
    can_delete: bool,
}

/// Vec data: Comment
struct OutComment {
    cid: u32,
    uid: u32,
    username: String,
    content: String,
    created_at: String,
    upvotes: usize,
    downvotes: usize,
    is_upvoted: bool,
    is_downvoted: bool,
    is_hidden: bool,
}

/// url params: `post.html`
#[derive(Deserialize)]
pub(crate) struct ParamsPost {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    nid: Option<u32>,
}

/// `GET /inn/:iid/:pid` post page
pub(crate) async fn post(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
    Query(params): Query<ParamsPost>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let post: Post = get_one(&DB, "posts", pid)?;
    let user: User = get_one(&DB, "users", post.uid)?;
    let date = ts_to_date(post.created_at);
    let inn: Inn = get_one(&DB, "inns", post.iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    if InnType::from(inn.inn_type) == InnType::Private {
        match claim.as_ref() {
            Some(claim) => {
                let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(iid)].concat();
                if !DB.open_tree("user_inns")?.contains_key(k)?
                    && Role::from(claim.role) != Role::Admin
                {
                    return Err(AppError::NotFound);
                }
            }
            None => return Err(AppError::NotFound),
        }
    }

    if post.iid != iid {
        return Err(AppError::NotFound);
    }

    let mut has_joined = false;
    let mut is_upvoted = false;
    let mut is_downvoted = false;
    let mut is_mod = false;
    let mut is_author = false;
    let mut can_edit = false;
    let mut can_delete = false;
    let upvotes = get_count_by_prefix(&DB, "post_upvotes", &u32_to_ivec(pid)).unwrap_or_default();
    let downvotes =
        get_count_by_prefix(&DB, "post_downvotes", &u32_to_ivec(pid)).unwrap_or_default();
    if let Some(ref claim) = claim {
        if post.uid == claim.uid {
            is_author = true;
        }

        let k = [&u32_to_ivec(pid), &u32_to_ivec(claim.uid)].concat();
        if DB.open_tree("post_upvotes")?.contains_key(&k)? {
            is_upvoted = true;
        }
        if DB.open_tree("post_downvotes")?.contains_key(&k)? {
            is_downvoted = true;
        }

        if is_author
            && (inn.limit_edit_seconds == 0
                || post.created_at + (inn.limit_edit_seconds as i64) >= Utc::now().timestamp())
        {
            can_edit = true;
        }

        if post.status != PostStatus::Normal {
            can_edit = false;
        }

        let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(iid)].concat();
        if DB.open_tree("user_inns")?.contains_key(&k)? {
            has_joined = true;
        }
        if DB.open_tree("mod_inns")?.contains_key(&k)? || Role::from(claim.role) == Role::Admin {
            is_mod = true;
        }

        if let Some(nid) = params.nid {
            let prefix = [&u32_to_ivec(claim.uid), &u32_to_ivec(nid)].concat();
            let tree = DB.open_tree("notifications")?;
            for i in tree.scan_prefix(prefix) {
                let (k, _) = i?;
                tree.update_and_fetch(k, mark_read)?;
            }
        }
    }

    let og_content: String = match post.status {
        PostStatus::HiddenByMod => "Hidden by mod.".into(),
        PostStatus::HiddenByUser => "Hidden by user.".into(),
        _ => match post.content {
            PostContent::Markdown(ref content) => escape(Html, content).unwrap().to_string(),
            PostContent::FeedItemId(_) => "This post is auto-generated from RSS feed".into(),
        },
    };
    let content = match post.status {
        PostStatus::HiddenByMod => "<p><i>Hidden by mod.</i></p>".into(),
        PostStatus::HiddenByUser => "<p><i>Hidden by user.</i></p>".into(),
        _ => {
            let diff = (Utc::now().timestamp() - post.created_at) / 24 / 3600;
            if diff > 30 {
                let mut content = format!(
                    r#"
                    <article class="message is-warning">
                        <div class="message-header">
                            <p>Warning</p>
                        </div>
                        <div class="message-body">
                            This post was published <b>{} days ago</b>. The information described in this article may have changed.
                        </div>
                    </article>
                    "#,
                    diff
                );
                content.push_str(&post.content.to_html(&DB)?);
                content
            } else {
                post.content.to_html(&DB)?
            }
        }
    };

    let k = [&u32_to_ivec(iid), &u32_to_ivec(pid)].concat();
    let is_pinned = DB.open_tree("post_pins")?.contains_key(k)?;

    let out_post = OutPost {
        pid: post.pid,
        uid: post.uid,
        username: user.username,
        iid: post.iid,
        inn_name: inn.inn_name,
        title: post.title,
        tags: post.tags,
        status: post.status.to_string(),
        content_html: content,
        og_content,
        created_at: date,
        upvotes,
        downvotes,
        is_upvoted,
        is_downvoted,
        can_edit,
        is_pinned,
    };

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(false);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut out_comments = Vec::with_capacity(n);
    let max_id = get_count(&DB, "post_comments_count", u32_to_ivec(pid))?;
    if max_id > 0 {
        let (start, _) = get_range(max_id, &page_params);
        let post_comments_tree = DB.open_tree("post_comments")?;
        let comment_upvotes_tree = DB.open_tree("comment_upvotes")?;
        let comment_downvotes_tree = DB.open_tree("comment_downvotes")?;
        for i in start..=max_id {
            let k = [&u32_to_ivec(pid), &u32_to_ivec(i as u32)].concat();
            let v = &post_comments_tree.get(k)?;
            if let Some(v) = v {
                let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
                let user: User = get_one(&DB, "users", comment.uid)?;
                let date = ts_to_date(comment.created_at);

                let mut is_upvoted = false;
                let mut is_downvoted = false;

                if let Some(ref claim) = claim {
                    let k = [
                        &u32_to_ivec(pid),
                        &u32_to_ivec(comment.cid),
                        &u32_to_ivec(claim.uid),
                    ]
                    .concat();
                    is_upvoted = comment_upvotes_tree.contains_key(&k)?;
                    is_downvoted = comment_downvotes_tree.contains_key(&k)?;
                }

                let prefix = [&u32_to_ivec(pid), &u32_to_ivec(comment.cid)].concat();
                let upvotes =
                    get_count_by_prefix(&DB, "comment_upvotes", &prefix).unwrap_or_default();
                let downvotes =
                    get_count_by_prefix(&DB, "comment_downvotes", &prefix).unwrap_or_default();

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
                    is_hidden: comment.is_hidden,
                };
                out_comments.push(out_comment);
                if out_comments.len() >= n {
                    break;
                }
            }
        }
        if is_desc {
            out_comments.reverse();
        }
    }

    let count = get_count_by_prefix(&DB, "post_comments", &u32_to_ivec(pid))?;
    if count == 0 && is_author {
        can_delete = true;
    }

    let pageview = incr_id(&DB.open_tree("post_pageviews")?, u32_to_ivec(pid))?;
    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };

    let title = out_post.title.clone();
    let page_data = PageData::new(&title, &site_config, claim, has_unread);
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
        is_author,
        can_delete,
    };

    Ok(into_response(&page_post))
}

/// Form data: `/inn/:iid/:pid/` comment create
#[derive(Deserialize, Validate)]
pub(crate) struct FormComment {
    #[garde(length(min = 1, max = 10000))]
    content: String,
}

/// `POST /post/:iid/:pid/` comment create
pub(crate) async fn comment_post(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
    WithValidation(input): WithValidation<Form<FormComment>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;
    let input = input.into_inner();

    if let Some(spam_regex) = &site_config.spam_regex {
        let re = regex::Regex::new(spam_regex).unwrap();
        if re.is_match(&input.content) {
            return Err(AppError::Custom("Spam detected".into()));
        }
    }

    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    let inn_role = InnRole::get(&DB, iid, claim.uid)?.ok_or(AppError::Unauthorized)?;
    if inn_role < InnRole::Limited {
        return Err(AppError::Unauthorized);
    }

    if !DB.open_tree("inns")?.contains_key(u32_to_ivec(iid))? {
        return Err(AppError::NotFound);
    }

    let created_at = Utc::now().timestamp();
    if created_at - claim.last_write < site_config.comment_interval {
        return Err(AppError::WriteInterval);
    }

    let post: Post = get_one(&DB, "posts", pid)?;
    if post.iid != iid {
        return Err(AppError::NotFound);
    }
    if post.status != PostStatus::Normal {
        return Err(AppError::LockedOrHidden);
    }

    let pid_ivec = u32_to_ivec(pid);
    let cid = incr_id(&DB.open_tree("post_comments_count")?, &pid_ivec)?;

    let mut content = input.content;

    // extract @username or @uid notification
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
        let notification_link = format!(
            "<span class='replytag'>[![](/static/avatars/{uid}.png){username}](/user/{uid})</span>"
        );
        let from = format!("@{notification}");
        let to = format!("@{notification_link}");
        content = content.replace(&from, &to);

        // notify user to be mentioned in comment
        // prevent duplicate notifications
        if uid != post.uid {
            add_notification(&DB, uid, NtType::CommentMention, pid, cid)?;
        }
    }

    let reply_to = extract_element(&content, 5, '#');
    let mut reply_to_cid = None;
    if !reply_to.is_empty() {
        if let Ok(reply_cid) = reply_to[0].parse::<u32>() {
            if reply_cid < cid {
                let reply_link = format!("[{}](/post/{}/{}#{})", reply_to[0], iid, pid, reply_cid);
                let from = format!("#{reply_cid}");
                let to = format!("#{reply_link}");
                content = content.replace(&from, &to);
                reply_to_cid = Some(reply_cid);
            }
        }
    }

    let comment = Comment {
        cid,
        pid,
        uid: claim.uid,
        reply_to: reply_to_cid,
        content: md2html(&content),
        created_at,
        is_hidden: false,
    };
    let k = [&pid_ivec, &u32_to_ivec(cid)].concat();
    set_one_with_key(&DB, "post_comments", k, &comment)?;

    let k = [&u32_to_ivec(claim.uid), &pid_ivec, &u32_to_ivec(cid)].concat();
    DB.open_tree("user_comments")?.insert(k, &[])?;

    // only the fellow could update the timeline by adding comment
    if inn_role >= InnRole::Fellow {
        let inn_type = inn_rm_index(&DB, iid, pid)?;
        inn_add_index(&DB, iid, pid, created_at as u32, inn_type)?;
    }

    // notify post author
    if post.uid != claim.uid {
        add_notification(&DB, post.uid, NtType::PostComment, pid, cid)?;
    }

    User::update_stats(&DB, claim.uid, "comment")?;
    claim.update_last_write(&DB)?;

    if inn.is_open_access() {
        DB.open_tree("tan")?
            .insert(format!("comt{}/{}", pid, cid), &[])?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// Page data: `preview.html`
#[derive(Template)]
#[template(path = "preview.html", escape = "none")]
struct PagePreview<'a> {
    page_data: PageData<'a>,
    content: String,
}

/// `POST /preview`
pub(crate) async fn preview(
    WithValidation(input): WithValidation<Form<FormComment>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let page_data = PageData::new("preview", &site_config, None, false);

    let page_preview = PagePreview {
        page_data,
        content: md2html(&input.content),
    };

    Ok(into_response(&page_preview))
}

/// `GET /post/:iid/:pid/:cid/delete` comment delete
pub(crate) async fn comment_delete(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid, cid)): Path<(u32, u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let k = [
        &u32_to_ivec(claim.uid),
        &u32_to_ivec(pid),
        &u32_to_ivec(cid),
    ]
    .concat();
    if !DB.open_tree("user_comments")?.contains_key(k)? {
        return Err(AppError::Unauthorized);
    }

    let k = [&u32_to_ivec(pid), &u32_to_ivec(cid)].concat();
    DB.open_tree("post_comments")?.remove(k)?;

    let inn_type = inn_rm_index(&DB, iid, pid)?;
    let latest_id = DB
        .open_tree("post_comments")?
        .scan_prefix(u32_to_ivec(pid))
        .last();

    let timestamp = if let Some(Ok((_, v))) = latest_id {
        let (comment, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
        comment.created_at
    } else {
        let post: Post = get_one(&DB, "posts", pid)?;
        post.created_at
    };

    inn_add_index(&DB, iid, pid, timestamp as u32, inn_type)?;

    DB.open_tree("tan")?
        .remove(format!("comt{}/{}", pid, cid))?;

    let target = format!("/post/{pid}/{cid}");
    Ok(Redirect::to(&target))
}

/// `GET /post/:iid/:pid/:cid/hide` comment hide
pub(crate) async fn comment_hide(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid, cid)): Path<(u32, u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(iid)].concat();
    if !DB.open_tree("mod_inns")?.contains_key(k)? && Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    let k = [&u32_to_ivec(pid), &u32_to_ivec(cid)].concat();
    let v = DB
        .open_tree("post_comments")?
        .get(&k)?
        .ok_or(AppError::NotFound)?;
    let (mut comment, _): (Comment, usize) = bincode::decode_from_slice(&v, standard())?;
    comment.is_hidden = !comment.is_hidden;

    set_one_with_key(&DB, "post_comments", k, &comment)?;
    add_notification(
        &DB,
        comment.uid,
        NtType::CommentHide,
        comment.pid,
        comment.cid,
    )?;

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/upvote` post upvote
pub(crate) async fn post_upvote(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    let post_upvotes_tree = DB.open_tree("post_upvotes")?;
    let k = [&u32_to_ivec(pid), &u32_to_ivec(claim.uid)].concat();
    if post_upvotes_tree.contains_key(&k)? {
        post_upvotes_tree.remove(&k)?;
    } else {
        post_upvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/:cid/upvote` comment upvote
pub(crate) async fn comment_upvote(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid, cid)): Path<(u32, u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;
    let k = [
        &u32_to_ivec(pid),
        &u32_to_ivec(cid),
        &u32_to_ivec(claim.uid),
    ]
    .concat();

    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    let comment_upvotes_tree = DB.open_tree("comment_upvotes")?;
    if comment_upvotes_tree.contains_key(&k)? {
        comment_upvotes_tree.remove(&k)?;
    } else {
        comment_upvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/downvote` post downvote
pub(crate) async fn post_downvote(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;
    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    let post_downvotes_tree = DB.open_tree("post_downvotes")?;
    let k = [&u32_to_ivec(pid), &u32_to_ivec(claim.uid)].concat();
    if post_downvotes_tree.contains_key(&k)? {
        post_downvotes_tree.remove(&k)?;
    } else {
        post_downvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/delete` post delete
pub(crate) async fn post_delete(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;
    let mut post: Post = get_one(&DB, "posts", pid)?;
    let count = get_count_by_prefix(&DB, "post_comments", &u32_to_ivec(pid))?;

    if count == 0 && post.uid == claim.uid {
        post.content = PostContent::Markdown("*Post deleted by author.*".into());
        set_one(&DB, "posts", pid, &post)?;

        // remove this post from inn timeline
        inn_rm_index(&DB, iid, pid)?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/:cid/downvote` comment downvote
pub(crate) async fn comment_downvote(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid, cid)): Path<(u32, u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;
    let k = [
        &u32_to_ivec(pid),
        &u32_to_ivec(cid),
        &u32_to_ivec(claim.uid),
    ]
    .concat();

    let inn: Inn = get_one(&DB, "inns", iid)?;
    if inn.is_closed() {
        return Err(AppError::LockedOrHidden);
    }

    let comment_downvotes_tree = DB.open_tree("comment_downvotes")?;
    if comment_downvotes_tree.contains_key(&k)? {
        comment_downvotes_tree.remove(&k)?;
    } else {
        comment_downvotes_tree.insert(&k, &[])?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/lock` post lock
pub(crate) async fn post_lock(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let mut post: Post = get_one(&DB, "posts", pid)?;

    if User::is_mod(&DB, claim.uid, iid)? || Role::from(claim.role) == Role::Admin {
        if post.status != PostStatus::LockedByMod {
            add_notification(&DB, post.uid, NtType::PostLock, claim.uid, post.pid)?;
            post.status = PostStatus::LockedByMod
        } else if post.status == PostStatus::LockedByMod {
            post.status = PostStatus::Normal
        }
    } else if post.uid == claim.uid {
        if post.status == PostStatus::Normal {
            post.status = PostStatus::LockedByUser
        } else if post.status == PostStatus::LockedByUser {
            post.status = PostStatus::Normal
        }
    }

    set_one(&DB, "posts", pid, &post)?;

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/hide` post hide
pub(crate) async fn post_hide(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let mut post: Post = get_one(&DB, "posts", pid)?;
    let old_status = post.status.clone();

    if User::is_mod(&DB, claim.uid, iid)? || Role::from(claim.role) == Role::Admin {
        if post.status != PostStatus::HiddenByMod {
            add_notification(&DB, post.uid, NtType::PostHide, claim.uid, post.pid)?;
            post.status = PostStatus::HiddenByMod
        } else if post.status == PostStatus::HiddenByMod {
            post.status = PostStatus::Normal
        }
    } else if post.uid == claim.uid {
        if post.status < PostStatus::HiddenByUser {
            post.status = PostStatus::HiddenByUser
        } else if post.status == PostStatus::HiddenByUser {
            post.status = PostStatus::Normal
        }
    }

    set_one(&DB, "posts", pid, &post)?;

    if (old_status <= PostStatus::HiddenByUser && post.status == PostStatus::HiddenByUser)
        || (old_status <= PostStatus::HiddenByMod && post.status == PostStatus::HiddenByMod)
    {
        //remove from inn timeline
        inn_rm_index(&DB, iid, pid)?;
    } else if (old_status == PostStatus::HiddenByUser && post.status < PostStatus::HiddenByUser)
        || (old_status == PostStatus::HiddenByMod && post.status < PostStatus::HiddenByMod)
    {
        let k0 = [&u32_to_ivec(post.uid), &u32_to_ivec(pid)].concat();
        if let Some(v) = DB.open_tree("user_posts")?.get(k0)? {
            inn_add_index(&DB, iid, pid, post.created_at as u32, v[4])?;
        }
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}

/// `GET /inn/:iid/:pid/pin` post pin
pub(crate) async fn post_pin(
    cookie: Option<TypedHeader<Cookie>>,
    Path((iid, pid)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    if !User::is_mod(&DB, claim.uid, iid)? && Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    let k = [&u32_to_ivec(iid), &u32_to_ivec(pid)].concat();
    let tree = DB.open_tree("post_pins")?;
    if tree.contains_key(&k)? {
        tree.remove(&k)?;
    } else {
        tree.insert(&k, &[])?;
    }

    let target = format!("/post/{iid}/{pid}");
    Ok(Redirect::to(&target))
}
