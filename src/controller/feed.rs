use super::{
    db_utils::{
        get_ids_by_prefix, get_one, get_range, i64_to_ivec, ivec_to_u32, set_one, u32_to_ivec,
        u8_slice_to_i64, u8_slice_to_u32,
    },
    fmt::ts_to_date,
    inn::inn_add_index,
    meta_handler::{get_referer, into_response, PageData, ParamsPage},
    Claim, Inn, Post, PostContent, PostStatus, SiteConfig, User,
};
use crate::{
    controller::{incr_id, Feed, Item},
    error::AppError,
};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    headers::{Cookie, Referer},
    response::{IntoResponse, Redirect},
    Form, TypedHeader,
};
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use sled::Db;
use std::{collections::HashSet, time::Duration};
use tracing::error;
use validator::Validate;

struct SourceItem {
    link: String,
    title: String,
    updated: i64,
    content: String,
}

impl TryFrom<rss::Item> for SourceItem {
    type Error = AppError;
    fn try_from(rss: rss::Item) -> Result<Self, Self::Error> {
        let updated = if let Some(ref pub_date) = rss.pub_date {
            if let Ok(ts) = DateTime::parse_from_rfc2822(pub_date) {
                ts.timestamp()
            } else {
                Utc::now().timestamp()
            }
        } else {
            Utc::now().timestamp()
        };

        let Some(link) = rss.link else {
            return Err(AppError::InvalidFeedLink);
        };

        Ok(Self {
            link,
            title: rss.title.unwrap_or_else(|| "No Title".to_owned()),
            updated,
            content: rss.description.unwrap_or_default(),
        })
    }
}

impl From<atom_syndication::Entry> for SourceItem {
    fn from(atom: atom_syndication::Entry) -> Self {
        let updated = if let Some(published) = atom.published {
            published.timestamp()
        } else {
            atom.updated.timestamp()
        };

        Self {
            link: atom.links[0].href.clone(),
            title: atom.title.to_string(),
            updated,
            content: atom.content.unwrap_or_default().value.unwrap_or_default(),
        }
    }
}

/// Page data: `feed.html`
#[derive(Template)]
#[template(path = "feed.html")]
struct PageFeed<'a> {
    page_data: PageData<'a>,
    folders: IndexMap<String, Vec<OutFeed>>,
    items: Vec<OutItem>,
    filter: Option<String>,
    filter_value: Option<String>,
    anchor: usize,
    n: usize,
    is_desc: bool,
    uid: u32,
    username: Option<String>,
}

struct OutFeed {
    feed_id: u32,
    title: String,
    is_active: bool,
    is_public: bool,
    err: Option<String>,
}

impl OutFeed {
    fn new(db: &Db, feed_id: u32, is_active: bool, is_public: bool) -> Result<Self, AppError> {
        let feed: Feed = get_one(db, "feeds", feed_id)?;
        let err = db
            .open_tree("feed_errs")?
            .get(u32_to_ivec(feed_id))?
            .map(|v| String::from_utf8_lossy(&v).into_owned());
        Ok(OutFeed {
            feed_id,
            title: feed.title,
            is_active,
            is_public,
            err,
        })
    }
}

struct OutItem {
    item_id: u32,
    title: String,
    feed_title: String,
    updated: String,
    is_starred: bool,
    is_read: bool,
}

/// url params: `feed.html`
#[derive(Deserialize)]
pub(crate) struct ParamsFeed {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    filter: Option<String>,
    filter_value: Option<String>,
}

struct Folder {
    folder: String,
    feed_id: u32,
    is_public: bool,
}

/// `GET /feed`
pub(crate) async fn feed(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
    Query(params): Query<ParamsFeed>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));
    let mut read = false;
    let username = match claim {
        Some(ref claim) if claim.uid == uid => None,
        _ => {
            read = true;
            let user: User = get_one(&db, "users", uid)?;
            Some(user.username)
        }
    };

    let mut map = IndexMap::new();
    let mut feed_ids = vec![];
    let mut item_ids = vec![];

    let mut folders = vec![];
    for i in db.open_tree("user_folders")?.scan_prefix(u32_to_ivec(uid)) {
        let (k, v) = i?;
        let feed_id = u8_slice_to_u32(&k[(k.len() - 4)..]);
        let folder = String::from_utf8_lossy(&k[4..(k.len() - 4)]).to_string();
        let is_public = v[0] == 1;
        folders.push(Folder {
            folder,
            feed_id,
            is_public,
        })
    }

    match (&params.filter, &params.filter_value) {
        (Some(ref filter), Some(filter_value)) if filter == "feed" => {
            if let Ok(id) = filter_value.parse::<u32>() {
                for i in folders {
                    if username.is_some() && !i.is_public {
                        continue;
                    }

                    let mut is_active = false;
                    if id == i.feed_id {
                        is_active = true;
                        feed_ids.push(i.feed_id);
                    }
                    let e: &mut Vec<OutFeed> = map.entry(i.folder).or_default();
                    let out_feed = OutFeed::new(&db, i.feed_id, is_active, i.is_public)?;
                    e.push(out_feed);
                }
            }
        }
        (Some(ref filter), Some(filter_value)) if filter == "folder" => {
            for i in folders {
                if username.is_some() && !i.is_public {
                    continue;
                }

                let mut is_active = false;
                if filter_value == &i.folder {
                    is_active = true;
                    feed_ids.push(i.feed_id);
                }
                let e = map.entry(i.folder).or_default();
                let out_feed = OutFeed::new(&db, i.feed_id, is_active, i.is_public)?;
                e.push(out_feed);
            }
        }
        (Some(ref filter), Some(filter_value)) if filter == "star" => {
            if let Ok(id) = filter_value.parse::<u32>() {
                for i in folders {
                    if username.is_some() {
                        break;
                    }

                    let mut is_active = false;
                    if id == i.feed_id {
                        is_active = true;
                        if let Some(ref claim) = claim {
                            let mut star_ids = get_item_ids_and_ts(&db, "star", claim.uid)?;
                            let ids_in_feed =
                                get_ids_by_prefix(&db, "feed_items", u32_to_ivec(i.feed_id), None)?;
                            star_ids.retain(|(i, _)| ids_in_feed.contains(i));
                            item_ids = star_ids;
                        }
                    }
                    let e = map.entry(i.folder).or_default();
                    let out_feed = OutFeed::new(&db, i.feed_id, is_active, i.is_public)?;
                    e.push(out_feed);
                }
            }
        }
        (Some(ref filter), None) if filter == "star" => {
            if let Some(ref claim) = claim {
                item_ids = get_item_ids_and_ts(&db, "star", claim.uid)?;
            }
        }
        (Some(ref filter), Some(filter_value)) if filter == "unread" => {
            if let Ok(id) = filter_value.parse::<u32>() {
                for i in folders {
                    if username.is_some() {
                        break;
                    }

                    let mut is_active = false;
                    if id == i.feed_id {
                        is_active = true;
                        if let Some(ref claim) = claim {
                            let read_ids =
                                get_ids_by_prefix(&db, "read", u32_to_ivec(claim.uid), None)?;
                            let mut ids_in_feed = get_item_ids_and_ts(&db, "feed_items", id)?;
                            ids_in_feed.retain(|(i, _)| !read_ids.contains(i));
                            item_ids = ids_in_feed;
                        }
                    }
                    let e = map.entry(i.folder).or_default();
                    let out_feed = OutFeed::new(&db, i.feed_id, is_active, i.is_public)?;
                    e.push(out_feed);
                }
            }
        }
        (Some(ref filter), None) if filter == "unread" => {
            if let Some(ref claim) = claim {
                let read_ids = get_ids_by_prefix(&db, "read", u32_to_ivec(claim.uid), None)?;
                for i in folders {
                    let is_active = false;
                    let mut ids_in_feed = get_item_ids_and_ts(&db, "feed_items", i.feed_id)?;
                    ids_in_feed.retain(|(i, _)| !read_ids.contains(i));
                    item_ids.append(&mut ids_in_feed);

                    let e = map.entry(i.folder).or_default();
                    let out_feed = OutFeed::new(&db, i.feed_id, is_active, i.is_public)?;
                    e.push(out_feed);
                }
            }
        }
        (_, _) => {
            for i in folders {
                if username.is_some() && !i.is_public {
                    continue;
                }

                let mut ids = get_item_ids_and_ts(&db, "feed_items", i.feed_id)?;
                item_ids.append(&mut ids);

                let is_active = false;
                let e = map.entry(i.folder).or_default();
                let out_feed = OutFeed::new(&db, i.feed_id, is_active, i.is_public)?;
                e.push(out_feed);
            }
        }
    }

    for id in feed_ids {
        let mut ids = get_item_ids_and_ts(&db, "feed_items", id)?;
        item_ids.append(&mut ids);
    }
    item_ids.sort_unstable_by(|a, b| a.1.cmp(&b.1));
    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };
    let (start, end) = get_range(item_ids.len(), &page_params);
    item_ids = item_ids[start - 1..end].to_vec();
    if is_desc {
        item_ids.reverse();
    }
    let mut items = Vec::with_capacity(n);
    let star_tree = db.open_tree("star")?;
    let read_tree = db.open_tree("read")?;
    for (i, _) in item_ids {
        let item: Item = get_one(&db, "items", i)?;
        let mut is_read = read;
        let is_starred = if let Some(ref claim) = claim {
            let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(i)].concat();
            if read_tree.contains_key(&k)? {
                is_read = true;
            }
            star_tree.contains_key(k)?
        } else {
            false
        };

        let out_item = OutItem {
            item_id: i,
            title: item.title,
            feed_title: item.feed_title,
            updated: ts_to_date(item.updated),
            is_starred,
            is_read,
        };
        items.push(out_item);
    }

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&db, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("Feed", &site_config, claim, has_unread);
    let page_feed = PageFeed {
        page_data,
        folders: map,
        items,
        filter: params.filter,
        filter_value: params.filter_value,
        n,
        anchor,
        is_desc,
        uid,
        username,
    };

    Ok(into_response(&page_feed))
}

fn get_item_ids_and_ts(db: &Db, tree: &str, id: u32) -> Result<Vec<(u32, i64)>, AppError> {
    let mut res = vec![];
    for i in db.open_tree(tree)?.scan_prefix(u32_to_ivec(id)) {
        let (k, v) = i?;
        let item_id = u8_slice_to_u32(&k[4..8]);
        let ts = u8_slice_to_i64(&v);
        res.push((item_id, ts))
    }
    Ok(res)
}

struct OutItemRead {
    item_id: u32,
    title: String,
    link: String,
    feed_title: String,
    updated: String,
    content: String,
    is_starred: bool,
}

/// Page data: `feed.html`
#[derive(Template)]
#[template(path = "feed_read.html", escape = "none")]
struct PageFeedRead<'a> {
    page_data: PageData<'a>,
    item: OutItemRead,
    allow_img: bool,
}

/// url params: `feed_read.html`
#[derive(Deserialize)]
pub(crate) struct ParamsFeedRead {
    allow_img: Option<bool>,
}

/// `GET /feed/read/:item_id`
pub(crate) async fn feed_read(
    State(db): State<Db>,
    Path(item_id): Path<u32>,
    Query(params): Query<ParamsFeedRead>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let item: Item = get_one(&db, "items", item_id)?;
    let is_starred = if let Some(ref claim) = claim {
        let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(item_id)].concat();
        db.open_tree("star")?.contains_key(k)?
    } else {
        false
    };

    let out_item_read = OutItemRead {
        item_id,
        title: item.title,
        link: item.link,
        feed_title: item.feed_title,
        updated: ts_to_date(item.updated),
        content: item.content,
        is_starred,
    };
    if let Some(ref claim) = claim {
        let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(item_id)].concat();
        db.open_tree("read")?.insert(k, &[])?;
    }

    let allow_img = params.allow_img.unwrap_or_default();
    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&db, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("Feed", &site_config, claim, has_unread);
    let page_feed_read = PageFeedRead {
        page_data,
        item: out_item_read,
        allow_img,
    };

    Ok(into_response(&page_feed_read))
}

/// Page data: `feed_add.html`
#[derive(Template)]
#[template(path = "feed_add.html")]
struct PageFeedAdd<'a> {
    page_data: PageData<'a>,
    folders: HashSet<String>,
}

/// `GET /feed/add`
pub(crate) async fn feed_add(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let mut folders = HashSet::new();
    for i in db
        .open_tree("user_folders")?
        .scan_prefix(u32_to_ivec(claim.uid))
        .keys()
    {
        let i = i?;
        let folder = String::from_utf8_lossy(&i[4..(i.len() - 4)]).to_string();
        folders.insert(folder);
    }

    if folders.is_empty() {
        folders.insert("Default".to_owned());
    }
    let has_unread = User::has_unread(&db, claim.uid)?;
    let page_data = PageData::new("Feed add", &site_config, Some(claim), has_unread);
    let page_feed_add = PageFeedAdd { page_data, folders };

    Ok(into_response(&page_feed_add))
}

/// Form data: `/feed/add`
#[derive(Deserialize, Validate)]
pub(crate) struct FormFeedAdd {
    #[validate(length(max = 256))]
    url: String,
    #[validate(length(max = 256))]
    folder: String,
    #[validate(length(max = 256))]
    new_folder: String,
    is_public: bool,
}

static CLIENT: Lazy<Client> = Lazy::new(|| {
    reqwest::Client::builder()
        // timeout shoule be less than global timeout
        .timeout(Duration::from_secs(6))
        .build()
        .unwrap()
});

/// `POST /feed/add`
pub(crate) async fn feed_add_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Form(form): Form<FormFeedAdd>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let (feed, item_ids) = update(&form.url, &db, 20).await?;
    let feed_links_tree = db.open_tree("feed_links")?;
    let user_folders_tree = db.open_tree("user_folders")?;
    let feed_id = if let Some(v) = feed_links_tree.get(&feed.link)? {
        let id = ivec_to_u32(&v);
        // change folder(remove the old record)
        for i in user_folders_tree.scan_prefix(u32_to_ivec(claim.uid)) {
            let (k, _) = i?;
            if u8_slice_to_u32(&k[k.len() - 4..]) == id {
                user_folders_tree.remove(k)?;
            }
        }
        id
    } else {
        incr_id(&db, "feeds_count")?
    };

    let feed_items_tree = db.open_tree("feed_items")?;
    let feed_id_ivec = u32_to_ivec(feed_id);
    for (id, ts) in item_ids {
        let k = [&feed_id_ivec, &u32_to_ivec(id)].concat();
        feed_items_tree.insert(k, i64_to_ivec(ts))?;
    }

    feed_links_tree.insert(&feed.link, u32_to_ivec(feed_id))?;

    set_one(&db, "feeds", feed_id, &feed)?;

    let folder = if form.folder.as_str() != "New" {
        form.folder
    } else if !form.new_folder.is_empty() {
        form.new_folder
    } else {
        "Default".to_string()
    };
    let k = [
        &u32_to_ivec(claim.uid),
        folder.as_bytes(),
        &u32_to_ivec(feed_id),
    ]
    .concat();

    let v = if form.is_public { &[1] } else { &[0] };
    user_folders_tree.insert(k, v)?;

    Ok(Redirect::to(&format!("/feed/{}", claim.uid)))
}

/// `GET /feed/update`
pub(crate) async fn feed_update(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let feed_items_tree = db.open_tree("feed_items")?;
    let mut handers = vec![];
    for i in db
        .open_tree("user_folders")?
        .scan_prefix(u32_to_ivec(claim.uid))
        .keys()
    {
        let i = i?;
        let feed_id = u8_slice_to_u32(&i[i.len() - 4..]);
        let db = db.clone();
        let feed: Feed = get_one(&db, "feeds", feed_id)?;
        let feed_items_tree = feed_items_tree.clone();

        let h = tokio::spawn(async move {
            match update(&feed.link, &db, 20).await {
                Ok((_, item_ids)) => {
                    for (item_id, ts) in item_ids {
                        let k = [&u32_to_ivec(feed_id), &u32_to_ivec(item_id)].concat();
                        if let Err(e) = feed_items_tree.insert(k, i64_to_ivec(ts)) {
                            error!(?e);
                        };
                        if let Ok(tree) = db.open_tree("feed_errs") {
                            let _ = tree.remove(u32_to_ivec(feed_id));
                        }
                    }
                }
                Err(e) => {
                    error!("update {} failed, error: {e}", feed.title);
                    if let Err(e) = db
                        .open_tree("feed_errs")
                        .and_then(|t| t.insert(u32_to_ivec(feed_id), &*e.to_string()))
                    {
                        error!(?e);
                    };
                }
            };
        });

        handers.push(h);
    }

    for i in handers {
        if let Err(e) = i.await {
            error!(?e);
        }
    }

    Ok(Redirect::to(&format!("/feed/{}", claim.uid)))
}

pub(super) async fn update(
    url: &str,
    db: &Db,
    n: usize,
) -> Result<(Feed, Vec<(u32, i64)>), AppError> {
    let content = CLIENT.get(url).send().await?.bytes().await?;

    let item_links_tree = db.open_tree("item_links")?;
    let mut item_ids = vec![];
    let feed = match rss::Channel::read_from(&content[..]) {
        Ok(rss) => {
            for item in rss.items.into_iter().take(n) {
                let source_item: SourceItem = item.try_into()?;
                let item_id = if let Some(v) = item_links_tree.get(&source_item.link)? {
                    ivec_to_u32(&v)
                } else {
                    incr_id(db, "items_count")?
                };

                let item = Item {
                    link: source_item.link,
                    title: source_item.title,
                    feed_title: rss.title.clone(),
                    updated: source_item.updated,
                    content: source_item.content,
                };

                item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                set_one(db, "items", item_id, &item)?;

                item_ids.push((item_id, item.updated));
            }

            Feed {
                link: url.to_owned(),
                title: rss.title,
            }
        }
        Err(_) => match atom_syndication::Feed::read_from(&content[..]) {
            Ok(atom) => {
                for entry in atom.entries.into_iter().take(n) {
                    let source_item: SourceItem = entry.into();
                    let item_id = if let Some(v) = item_links_tree.get(&source_item.link)? {
                        ivec_to_u32(&v)
                    } else {
                        incr_id(db, "items_count")?
                    };
                    let item = Item {
                        link: source_item.link,
                        title: source_item.title,
                        feed_title: atom.title.to_string(),
                        updated: source_item.updated,
                        content: source_item.content,
                    };
                    item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                    set_one(db, "items", item_id, &item)?;

                    item_ids.push((item_id, item.updated));
                }

                Feed {
                    link: url.to_owned(),
                    title: atom.title.to_string(),
                }
            }
            Err(_) => {
                return Err(AppError::InvalidFeedLink);
            }
        },
    };

    Ok((feed, item_ids))
}

pub async fn cron_feed(db: &Db) -> Result<(), AppError> {
    let mut set = HashSet::new();
    for i in &db.open_tree("user_folders")? {
        let (k, _) = i?;
        let feed_id = u8_slice_to_u32(&k[(k.len() - 4)..]);
        set.insert(feed_id);
    }

    let mut inn_feeds = Vec::new();
    for i in &db.open_tree("inn_feeds")? {
        let (k, v) = i?;
        let iid = u8_slice_to_u32(&k[0..4]);
        let feed_id = u8_slice_to_u32(&k[4..8]);
        let uid = u8_slice_to_u32(&v);
        set.insert(feed_id);
        inn_feeds.push((iid, feed_id, uid));
    }

    let feed_items_tree = db.open_tree("feed_items")?;
    let feed_errs_tree = db.open_tree("feed_errs")?;
    for id in set {
        if let Ok(feed) = get_one::<Feed>(db, "feeds", id) {
            match update(&feed.link, db, 5).await {
                Ok((_, item_ids)) => {
                    for (item_id, ts) in item_ids {
                        let k = [&u32_to_ivec(id), &u32_to_ivec(item_id)].concat();
                        feed_items_tree.insert(k, i64_to_ivec(ts))?;
                        for (iid, feed_id, uid) in &inn_feeds {
                            if *feed_id == id {
                                inn_feed_to_post(db, *iid, item_id, *uid, ts)?;
                            }
                        }
                    }
                    let _ = feed_errs_tree.remove(u32_to_ivec(id));
                }
                Err(e) => {
                    error!("update {} failed, error: {e}", feed.title);
                    feed_errs_tree.insert(u32_to_ivec(id), &*e.to_string())?;
                }
            };
        };
    }

    Ok(())
}

/// convert inn feed items to post
pub(super) fn inn_feed_to_post(
    db: &Db,
    iid: u32,
    item_id: u32,
    uid: u32,
    ts: i64,
) -> Result<(), AppError> {
    let inn_item_k = &[u32_to_ivec(iid), u32_to_ivec(item_id)].concat();
    let inn_items_tree = db.open_tree("inn_items")?;
    if !inn_items_tree.contains_key(inn_item_k)? {
        let inn: Inn = get_one(db, "inns", iid)?;
        let tag = format!("{}_feed", inn.inn_name);
        let visibility = if inn.inn_type.as_str() == "Private" {
            10
        } else {
            0
        };
        let pid = incr_id(db, "posts_count")?;
        let item: Item = get_one(db, "items", item_id)?;
        let post = Post {
            pid,
            uid,
            iid,
            title: item.title,
            tags: vec![tag.clone()],
            content: PostContent::FeedItemId(item_id),
            created_at: ts,
            status: PostStatus::Normal,
        };

        set_one(db, "posts", pid, &post)?;

        let tag_k = [tag.as_bytes(), &u32_to_ivec(pid)].concat();
        db.open_tree("tags")?.insert(tag_k, &[])?;

        let k = [&u32_to_ivec(iid), &u32_to_ivec(pid)].concat();
        db.open_tree("inn_posts")?.insert(&k, &[])?;

        inn_add_index(db, iid, pid, ts as u32, visibility)?;

        let k = [&u32_to_ivec(post.uid), &u32_to_ivec(pid)].concat();
        let v = [&u32_to_ivec(iid), &u32_to_ivec(visibility)].concat();
        db.open_tree("user_posts")?.insert(k, v)?;

        inn_items_tree.insert(inn_item_k, &[])?;
    }

    Ok(())
}

/// `GET /feed/star`
pub(crate) async fn feed_star(
    State(db): State<Db>,
    referer: Option<TypedHeader<Referer>>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(item_id): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let item_id_ivec = u32_to_ivec(item_id);
    if db.open_tree("items")?.contains_key(&item_id_ivec)? {
        let k = [&u32_to_ivec(claim.uid), &item_id_ivec].concat();
        let star_tree = db.open_tree("star")?;
        if star_tree.contains_key(&k)? {
            star_tree.remove(&k)?;
        } else {
            let now = Utc::now().timestamp();
            star_tree.insert(&k, i64_to_ivec(now))?;
        }
    }

    let target = if let Some(referer) = get_referer(referer) {
        referer
    } else {
        format!("/feed/{}?filter=star", claim.uid)
    };
    Ok(Redirect::to(&target))
}

/// `GET /feed/subscribe`
pub(crate) async fn feed_subscribe(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((uid, feed_id)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let user_folder_tree = db.open_tree("user_folders")?;

    for k in user_folder_tree.scan_prefix(u32_to_ivec(uid)).keys() {
        let k = k?;
        let feed_id_ivec = &k[(k.len() - 4)..];
        if u8_slice_to_u32(feed_id_ivec) == feed_id {
            if uid == claim.uid {
                // user unsubsribe
                user_folder_tree.remove(k)?;
            } else {
                // add other's feed
                let folder_ivec = &k[4..(k.len() - 4)];
                let new_key = [&u32_to_ivec(claim.uid), folder_ivec, feed_id_ivec].concat();
                user_folder_tree.insert(new_key, &[1])?;
            }
            break;
        };
    }

    Ok(Redirect::to(&format!("/feed/{}", claim.uid)))
}
