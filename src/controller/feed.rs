use super::{
    Claim, Inn, Post, PostContent, PostStatus, SiteConfig, User,
    db_utils::{
        get_ids_by_prefix, get_one, get_range, i64_to_ivec, ivec_to_u32, set_one, u8_slice_to_i64,
        u8_slice_to_u32, u32_to_ivec,
    },
    fmt::{clean_html, ts_to_date},
    inn::inn_add_index,
    meta_handler::{PageData, ParamsPage, get_referer, into_response},
};
use crate::{
    DB,
    config::CONFIG,
    controller::{Feed, Item, Podcast, filters, incr_id},
    error::AppError,
};
use askama::Template;
use axum::{
    Form,
    extract::{Path, Query},
    response::{IntoResponse, Redirect},
};
use axum_extra::{
    TypedHeader,
    headers::{Cookie, Referer},
};
use cached::proc_macro::cached;
use infer::is_audio;
use jiff::{Timestamp, fmt::rfc2822};
use reqwest::Client;
use serde::Deserialize;
use sled::Db;
use std::{collections::HashSet, time::Duration};
use std::{
    collections::{BTreeMap, HashMap},
    sync::LazyLock,
};
use tracing::{error, info, warn};
use validator::Validate;

struct SourceItem {
    link: String,
    title: String,
    updated: i64,
    content: String,
    podcast: Option<Podcast>,
}

static P: rfc2822::DateTimeParser = rfc2822::DateTimeParser::new().relaxed_weekday(true);

impl TryFrom<rss::Item> for SourceItem {
    type Error = AppError;
    fn try_from(rss: rss::Item) -> Result<Self, Self::Error> {
        let updated = if let Some(ref pub_date) = rss.pub_date {
            if let Ok(ts) = P.parse_timestamp(pub_date) {
                ts.as_second()
            } else {
                warn!("invalid pub_date: {}, rss: {:?}", pub_date, rss.link);
                Timestamp::now().as_second()
            }
        } else {
            Timestamp::now().as_second()
        };

        let Some(link) = rss.link else {
            return Err(AppError::InvalidFeedLink);
        };

        let content = rss.content.unwrap_or_else(|| {
            rss.description
                .unwrap_or_else(|| rss.itunes_ext.and_then(|e| e.summary).unwrap_or_default())
        });
        let title = if let Some(title) = rss.title {
            title
        } else if content.len() > 100 {
            let mut real_len = 100;
            while !content.is_char_boundary(real_len) {
                real_len -= 1;
            }
            format!("{}...", &content[0..real_len])
        } else {
            content.clone()
        };

        let mut podcast: Option<Podcast> = None;
        if let Some(closure) = rss.enclosure {
            let enclosure_url = closure.url;
            let enclosure_length = closure.length;
            let enclosure_mime_type = closure.mime_type;
            let pod = Podcast {
                enclosure_url,
                enclosure_length,
                enclosure_mime_type,
                audio_downloaded: false,
                exts: HashMap::new(),
            };
            podcast = Some(pod);
        }

        Ok(Self {
            link,
            title,
            updated,
            content,
            podcast,
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

        let content = if let Some(content) = atom.content {
            content.value.unwrap_or_default()
        } else if let Some(summary) = atom.summary {
            summary.value
        } else {
            let mut desc = String::new();
            // for YouTube podcast xml
            for ext in atom.extensions().values() {
                for extv in ext.values() {
                    for m in extv {
                        let c = m.children();
                        if let Some(description) = c.get("description")
                            && let Some(fd) = description.first()
                            && let Some(d) = fd.value()
                        {
                            desc = d.to_owned();
                        }
                    }
                }
            }
            desc
        };

        Self {
            link: atom.links[0].href.clone(),
            title: atom.title.to_string(),
            updated,
            content,
            podcast: None,
        }
    }
}

/// Page data: `feed.html`
#[derive(Template)]
#[template(path = "feed.html")]
struct PageFeed<'a> {
    page_data: PageData<'a>,
    folders: BTreeMap<String, Vec<OutFeed>>,
    items: Vec<OutItem>,
    filter: Option<String>,
    anchor: usize,
    n: usize,
    is_desc: bool,
    uid: u32,
    username: Option<String>,
    active_folder: String,
    active_feed: u32,
}

struct OutFeed {
    feed_id: u32,
    title: String,
    is_public: bool,
    err: Option<String>,
}

impl OutFeed {
    fn new(db: &Db, feed_id: u32, is_public: bool) -> Result<Self, AppError> {
        let feed: Feed = get_one(db, "feeds", feed_id)?;
        let err = db
            .open_tree("feed_errs")?
            .get(u32_to_ivec(feed_id))?
            .map(|v| String::from_utf8_lossy(&v).into_owned());
        Ok(OutFeed {
            feed_id,
            title: feed.title,
            is_public,
            err,
        })
    }
}

struct OutItem {
    item_id: u32,
    title: String,
    folder: String,
    feed_id: u32,
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
    active_folder: Option<String>,
    active_feed: Option<u32>,
}

struct Folder {
    folder: String,
    feed_id: u32,
    is_public: bool,
}

/// `GET /feed`
pub(crate) async fn feed(
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
    Query(params): Query<ParamsFeed>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));
    let mut read = false;
    let username = match claim {
        Some(ref claim) if claim.uid == uid => None,
        _ => {
            read = true;
            let user: User = get_one(&DB, "users", uid)?;
            Some(user.username)
        }
    };

    let mut map = BTreeMap::new();
    let mut feed_ids = vec![];

    let mut folders = vec![];
    let mut feed_id_folder = HashMap::new();
    for i in DB.open_tree("user_folders")?.scan_prefix(u32_to_ivec(uid)) {
        let (k, v) = i?;
        let feed_id = u8_slice_to_u32(&k[(k.len() - 4)..]);
        let folder = String::from_utf8_lossy(&k[4..(k.len() - 4)]).to_string();
        feed_id_folder.insert(feed_id, folder.clone());
        let is_public = v[0] == 1;
        folders.push(Folder {
            folder,
            feed_id,
            is_public,
        })
    }

    let mut active_folder = params.active_folder;

    for feed in folders {
        if username.is_some() && !feed.is_public {
            continue;
        }

        let e: &mut Vec<OutFeed> = map.entry(feed.folder.clone()).or_default();
        let out_feed = OutFeed::new(&DB, feed.feed_id, feed.is_public)?;
        e.push(out_feed);

        if let Some(ref active_folder) = active_folder
            && active_folder != &feed.folder
            && !active_folder.is_empty()
        {
            continue;
        }

        if let Some(active_feed) = params.active_feed
            && active_feed != 0
        {
            if active_feed != feed.feed_id {
                continue;
            }
            active_folder = Some(feed.folder)
        }

        feed_ids.push(feed.feed_id);
    }

    let mut item_ids = vec![];
    for id in feed_ids {
        let mut ids = get_item_ids_and_ts(&DB, "feed_items", id)?;
        item_ids.append(&mut ids);
    }

    let mut read_ids = HashSet::new();
    let mut star_ids = vec![];
    let mut star_ids_set = HashSet::new();
    if let Some(ref claim) = claim {
        star_ids = get_item_ids_and_ts(&DB, "star", claim.uid)?;
        star_ids_set = star_ids.iter().map(|(i, _)| *i).collect();

        read_ids = get_ids_by_prefix(&DB, "read", u32_to_ivec(claim.uid), None)?
            .into_iter()
            .collect();
    }

    if let Some(filter) = &params.filter {
        if filter == "star" {
            if active_folder.is_some() {
                item_ids.retain(|(i, _)| star_ids_set.contains(i));
            } else {
                item_ids = star_ids;
            }
        } else if filter == "unread" {
            item_ids.retain(|(i, _)| !read_ids.contains(i));
        }
    }

    item_ids.sort_unstable_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)));
    item_ids.dedup_by(|a, b| a.0 == b.0);

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
    for (i, _) in item_ids {
        let item: Item = get_one(&DB, "items", i)?;
        let mut is_read = read;
        if read_ids.contains(&i) {
            is_read = true;
        }

        let is_starred = star_ids_set.contains(&i);
        let feed_id = get_feed_id(i)?;
        let folder = if let Some(r) = feed_id_folder.get(&feed_id) {
            r.to_owned()
        } else {
            active_folder
                .clone()
                .unwrap_or_else(|| "Default".to_owned())
        };
        let out_item = OutItem {
            item_id: i,
            title: item.title,
            folder,
            feed_id,
            feed_title: item.feed_title,
            updated: ts_to_date(item.updated),
            is_starred,
            is_read,
        };
        items.push(out_item);
    }

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("Feed", &site_config, claim, has_unread);
    let page_feed = PageFeed {
        page_data,
        folders: map,
        items,
        filter: params.filter,
        n,
        anchor,
        is_desc,
        uid,
        username,
        active_feed: params.active_feed.unwrap_or_default(),
        active_folder: active_folder.unwrap_or_default(),
    };

    Ok(into_response(&page_feed))
}

#[cached(result = true)]
fn get_feed_id(item_id: u32) -> Result<u32, AppError> {
    for i in DB.open_tree("feed_items")?.iter() {
        let (k, _) = i?;
        let item_id2 = u8_slice_to_u32(&k[4..8]);
        if item_id == item_id2 {
            let feed_id = u8_slice_to_u32(&k[0..4]);
            return Ok(feed_id);
        }
    }
    Err(AppError::NotFound)
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
    podcast: Option<Podcast>,
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
    Path(item_id): Path<u32>,
    Query(params): Query<ParamsFeedRead>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let item: Item = get_one(&DB, "items", item_id)?;
    let is_starred = if let Some(ref claim) = claim {
        let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(item_id)].concat();
        DB.open_tree("star")?.contains_key(k)?
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
        podcast: item.podcast,
    };
    if let Some(ref claim) = claim {
        let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(item_id)].concat();
        DB.open_tree("read")?.insert(k, &[])?;
    }

    let allow_img = params.allow_img.unwrap_or_default();
    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };

    let title = out_item_read.title.clone();
    let page_data = PageData::new(&title, &site_config, claim, has_unread);
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
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let mut folders = HashSet::new();
    for i in DB
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
    let has_unread = User::has_unread(&DB, claim.uid)?;
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
    #[validate(skip)]
    is_public: bool,
}

static APP_USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"),);

static CLIENT: LazyLock<Client> = LazyLock::new(|| {
    let mut client = reqwest::Client::builder()
        .user_agent(APP_USER_AGENT)
        .timeout(Duration::from_secs(60));
    if !CONFIG.proxy.is_empty() {
        let proxy = reqwest::Proxy::all(&CONFIG.proxy).unwrap();
        client = client.proxy(proxy);
    }
    client.build().unwrap()
});

/// `POST /feed/add`
pub(crate) async fn feed_add_post(
    cookie: Option<TypedHeader<Cookie>>,
    Form(form): Form<FormFeedAdd>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let (feed, item_ids) = update(&form.url, &DB, 20).await?;
    let feed_links_tree = DB.open_tree("feed_links")?;
    let user_folders_tree = DB.open_tree("user_folders")?;
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
        incr_id(&DB, "feeds_count")?
    };

    let feed_items_tree = DB.open_tree("feed_items")?;
    let feed_id_ivec = u32_to_ivec(feed_id);
    for (id, ts) in item_ids {
        let k = [&feed_id_ivec, &u32_to_ivec(id)].concat();
        feed_items_tree.insert(k, i64_to_ivec(ts))?;
    }

    feed_links_tree.insert(&feed.link, u32_to_ivec(feed_id))?;

    set_one(&DB, "feeds", feed_id, &feed)?;

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
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let feed_items_tree = DB.open_tree("feed_items")?;
    let mut handers = vec![];
    for i in DB
        .open_tree("user_folders")?
        .scan_prefix(u32_to_ivec(claim.uid))
        .keys()
    {
        let i = i?;
        let feed_id = u8_slice_to_u32(&i[i.len() - 4..]);
        let feed: Feed = get_one(&DB, "feeds", feed_id)?;
        let feed_items_tree = feed_items_tree.clone();

        let h = tokio::spawn(async move {
            match update(&feed.link, &DB, 20).await {
                Ok((_, item_ids)) => {
                    for (item_id, ts) in item_ids {
                        let k = [&u32_to_ivec(feed_id), &u32_to_ivec(item_id)].concat();
                        if let Err(e) = feed_items_tree.insert(k, i64_to_ivec(ts)) {
                            error!(?e);
                        };
                        if let Ok(tree) = DB.open_tree("feed_errs") {
                            let _ = tree.remove(u32_to_ivec(feed_id));
                        }
                    }
                }
                Err(e) => {
                    error!("update {} failed, error: {e}", feed.title);
                    if let Err(e) = DB
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
    let tan_tree = db.open_tree("tan")?;
    let mut item_ids = vec![];
    let feed = match rss::Channel::read_from(&content[..]) {
        Ok(rss) => {
            for item in rss.items.into_iter().take(n) {
                let source_item: SourceItem = item.try_into()?;
                let item_id;
                if let Some(v) = item_links_tree.get(&source_item.link)? {
                    item_id = ivec_to_u32(&v);
                    let item: Item = get_one(db, "items", item_id)?;
                    item_ids.push((item_id, item.updated));
                } else {
                    item_id = incr_id(db, "items_count")?;
                    let item = Item {
                        link: source_item.link,
                        title: clean_html(&source_item.title),
                        feed_title: clean_html(&rss.title),
                        updated: source_item.updated,
                        content: clean_html(&source_item.content),
                        podcast: source_item.podcast,
                    };
                    item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                    set_one(db, "items", item_id, &item)?;
                    tan_tree.insert(format!("item{item_id}"), &[])?;
                    item_ids.push((item_id, source_item.updated));
                };
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
                    let item_id;
                    if let Some(v) = item_links_tree.get(&source_item.link)? {
                        item_id = ivec_to_u32(&v);
                        let item: Item = get_one(db, "items", item_id)?;
                        item_ids.push((item_id, item.updated));
                    } else {
                        item_id = incr_id(db, "items_count")?;
                        let item = Item {
                            link: source_item.link,
                            title: clean_html(&source_item.title),
                            feed_title: clean_html(&atom.title),
                            updated: source_item.updated,
                            content: clean_html(&source_item.content),
                            podcast: source_item.podcast,
                        };
                        item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                        set_one(db, "items", item_id, &item)?;
                        tan_tree.insert(format!("item{item_id}"), &[])?;
                        item_ids.push((item_id, source_item.updated));
                    };
                }

                Feed {
                    link: url.to_owned(),
                    title: atom.title.to_string(),
                }
            }
            Err(e) => {
                error!(?e);
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

    for i in &db.open_tree("inn_feeds")? {
        let (k, _) = i?;
        let feed_id = u8_slice_to_u32(&k[4..8]);
        set.insert(feed_id);
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

    for i in &db.open_tree("inn_feeds")? {
        let (k, v) = i?;
        let iid = u8_slice_to_u32(&k[0..4]);
        let feed_id = u8_slice_to_u32(&k[4..8]);
        let uid = u8_slice_to_u32(&v);

        inn_feed_to_post(db, iid, feed_id, uid)?;
    }

    Ok(())
}

pub async fn cron_download_audio(db: &Db) -> Result<(), AppError> {
    const MAX_FILE_SIZE: u64 = 300 * 1024 * 1024; // 300 MB
    for i in db.open_tree("items")?.iter().rev() {
        let (k, _) = i?;
        let item_id = u8_slice_to_u32(&k);
        let mut item: Item = get_one(db, "items", item_id)?;
        if let Some(ref podcast) = item.podcast
            && !podcast.audio_downloaded
            && !podcast.enclosure_url.is_empty()
        {
            if !podcast.enclosure_length.is_empty()
                && let Ok(size) = podcast.enclosure_length.parse::<u64>()
                && size > MAX_FILE_SIZE
            {
                warn!("Skipping item {item_id}: file too large ({size} bytes)");
                continue;
            }
            match CLIENT
                .get(&podcast.enclosure_url)
                .timeout(Duration::from_secs(600))
                .send()
                .await
            {
                Ok(audio) if audio.status().is_success() => {
                    let audio_bytes = match audio.bytes().await {
                        Ok(audio_bytes) => audio_bytes,
                        Err(e) => {
                            warn!("failed to download audio for item {item_id}, error: {e}");
                            continue;
                        }
                    };
                    if is_audio(&audio_bytes)
                        && let Some(file_type) = infer::get(&audio_bytes)
                    {
                        let path = std::path::PathBuf::from(&CONFIG.podcast_path);
                        let filename = format!("{item_id}.{}", file_type.extension());
                        let audio_path = path.join(&filename);

                        std::fs::write(&audio_path, &audio_bytes)?;
                        item.podcast.as_mut().unwrap().audio_downloaded = true;
                        item.podcast.as_mut().unwrap().enclosure_url = filename;

                        set_one(db, "items", item_id, &item)?;
                        info!("downloaded audio for item {item_id}, saved to {audio_path:?}");
                    } else {
                        item.podcast = None;

                        set_one(db, "items", item_id, &item)?;
                        warn!("The enclosure_url is not audio file.");
                    }
                }
                Err(e) => {
                    warn!("failed to download audio for item {item_id}, error: {e}");
                }
                Ok(resp) => {
                    warn!(
                        "failed to download audio for item {item_id}, status: {}",
                        resp.status()
                    );
                }
            }
        }
    }

    Ok(())
}

/// convert inn feed items to post
pub(super) fn inn_feed_to_post(db: &Db, iid: u32, feed_id: u32, uid: u32) -> Result<(), AppError> {
    let inn_items_tree = db.open_tree("inn_items")?;
    let item_ids = get_item_ids_and_ts(db, "feed_items", feed_id)?;
    for (item_id, ts) in item_ids.into_iter().rev().take(5) {
        let inn_item_k = &[u32_to_ivec(iid), u32_to_ivec(item_id)].concat();
        if !inn_items_tree.contains_key(inn_item_k)? {
            let inn: Inn = get_one(db, "inns", iid)?;
            let tag = format!("{}_feed", inn.inn_name);
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
            db.open_tree("inn_posts")?.insert(k, &[])?;

            inn_add_index(db, iid, pid, ts as u32, inn.inn_type)?;

            let k = [&u32_to_ivec(post.uid), &u32_to_ivec(pid)].concat();
            let mut v = iid.to_be_bytes().to_vec();
            v.push(inn.inn_type);
            db.open_tree("user_posts")?.insert(k, v)?;

            inn_items_tree.insert(inn_item_k, &[])?;
        }
    }

    Ok(())
}

/// `GET /feed/star`
pub(crate) async fn feed_star(
    referer: Option<TypedHeader<Referer>>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(item_id): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let item_id_ivec = u32_to_ivec(item_id);
    if DB.open_tree("items")?.contains_key(&item_id_ivec)? {
        let k = [&u32_to_ivec(claim.uid), &item_id_ivec].concat();
        let star_tree = DB.open_tree("star")?;
        if star_tree.contains_key(&k)? {
            star_tree.remove(&k)?;
        } else {
            let now = Timestamp::now().as_second();
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
    cookie: Option<TypedHeader<Cookie>>,
    Path((uid, feed_id)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let user_folder_tree = DB.open_tree("user_folders")?;

    for k in user_folder_tree.scan_prefix(u32_to_ivec(uid)).keys() {
        let k = k?;
        let feed_id_ivec = &k[(k.len() - 4)..];
        if u8_slice_to_u32(feed_id_ivec) == feed_id {
            if uid == claim.uid {
                // user unsubscribe
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
