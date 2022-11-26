use super::{
    get_ids_by_prefix, get_one, get_range, get_site_config, into_response, u32_to_ivec,
    u8_slice_to_u32, Claim, PageData, ParamsPage, User,
};
use crate::{
    controller::{incr_id, ivec_to_u32, Feed, Item},
    error::AppError,
};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    headers::Cookie,
    response::{IntoResponse, Redirect},
    Form, TypedHeader,
};
use bincode::config::standard;
use indexmap::IndexMap;
use once_cell::sync::Lazy;
use reqwest::Client;
use serde::Deserialize;
use sled::Db;
use std::{collections::HashSet, time::Duration};
use tracing::error;
use validator::Validate;

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
}

struct OutItem {
    item_id: u32,
    title: String,
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

/// `GET /feed`
pub(crate) async fn feed(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
    Query(params): Query<ParamsFeed>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
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
    for i in db.open_tree("user_folders")?.scan_prefix(u32_to_ivec(uid)) {
        // is not public and is nonlogin
        let (k, v) = i?;
        let is_public = v[0] == 1;
        if username.is_some() && !is_public {
            continue;
        }
        let feed_id = u8_slice_to_u32(&k[(k.len() - 4)..]);
        let folder = String::from_utf8_lossy(&k[4..(k.len() - 4)]).to_string();
        let feed: Feed = get_one(&db, "feeds", feed_id)?;

        let mut is_active = false;

        match (&params.filter, &params.filter_value) {
            (Some(ref filter), Some(filter_value)) if filter == "feed" => {
                if let Ok(id) = filter_value.parse::<u32>() {
                    if id == feed_id {
                        is_active = true;
                        feed_ids.push(feed_id);
                    }
                }
            }
            (Some(ref filter), Some(filter_value)) if filter == "folder" => {
                if &folder == filter_value {
                    is_active = true;
                    feed_ids.push(feed_id);
                }
            }
            (Some(ref filter), Some(filter_value)) if filter == "star" => {
                if let Ok(id) = filter_value.parse::<u32>() {
                    if id == feed_id {
                        is_active = true;
                        if let Some(ref claim) = claim {
                            let mut star_ids =
                                get_ids_by_prefix(&db, "star", u32_to_ivec(claim.uid), None)?;
                            let ids_in_feed =
                                get_ids_by_prefix(&db, "feed_items", u32_to_ivec(feed_id), None)?;
                            star_ids.retain(|i| ids_in_feed.contains(i));
                            item_ids = star_ids;
                        }
                    }
                }
            }
            (Some(ref filter), None) if filter == "star" => {
                if let Some(ref claim) = claim {
                    item_ids = get_ids_by_prefix(&db, "star", u32_to_ivec(claim.uid), None)?;
                }
            }
            (_, _) => {
                let mut ids = get_ids_by_prefix(&db, "feed_items", u32_to_ivec(feed_id), None)?;
                item_ids.append(&mut ids);
            }
        }

        let e = map.entry(folder).or_insert(vec![]);
        let out_feed = OutFeed {
            feed_id,
            title: feed.title,
            is_active,
            is_public,
        };
        e.push(out_feed);
    }

    for id in feed_ids {
        let mut ids = get_ids_by_prefix(&db, "feed_items", u32_to_ivec(id), None)?;
        item_ids.append(&mut ids);
    }

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };
    let (start, end) = get_range(item_ids.len(), &page_params);
    item_ids = item_ids[start - 1..end].to_vec();
    if !is_desc {
        item_ids.reverse();
    }

    let mut items = Vec::with_capacity(n);
    let star_tree = db.open_tree("star")?;
    let read_tree = db.open_tree("read")?;
    for i in item_ids {
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
            updated: item.updated,
            is_starred,
            is_read,
        };
        items.push(out_item);
    }

    let page_data = PageData::new("Feed", &site_config, claim, false);
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

    Ok(into_response(&page_feed, "html"))
}

/// `GET /feed/read/:item_id`
pub(crate) async fn feed_read(
    State(db): State<Db>,
    Path(item_id): Path<u32>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let item: Item = get_one(&db, "items", item_id)?;
    if let Some(ref claim) = claim {
        let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(item_id)].concat();
        db.open_tree("read")?.insert(k, &[])?;
    }

    Ok(Redirect::to(&item.link))
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
    let site_config = get_site_config(&db)?;
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
    let page_data = PageData::new("Feed add", &site_config, Some(claim), false);
    let page_feed_add = PageFeedAdd { page_data, folders };

    Ok(into_response(&page_feed_add, "html"))
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
        .timeout(Duration::from_secs(20))
        .build()
        .unwrap()
});

/// `POST /feed/add`
pub(crate) async fn feed_add_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Form(form): Form<FormFeedAdd>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let (feed, item_ids) = update(form.url, &db).await?;
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
        ivec_to_u32(&v)
    } else {
        incr_id(&db, "feeds_count")?
    };

    let feed_items_tree = db.open_tree("feed_items")?;
    let feed_id_ivec = u32_to_ivec(feed_id);
    for id in item_ids {
        let k = [&feed_id_ivec, &u32_to_ivec(id)].concat();
        feed_items_tree.insert(k, &[])?;
    }

    feed_links_tree.insert(&feed.link, u32_to_ivec(feed_id))?;

    let feeds_tree = db.open_tree("feeds")?;
    let feed_encode = bincode::encode_to_vec(&feed, standard())?;
    feeds_tree.insert(u32_to_ivec(feed_id), feed_encode)?;

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
    let site_config = get_site_config(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    for i in db
        .open_tree("user_folders")?
        .scan_prefix(u32_to_ivec(claim.uid))
        .keys()
    {
        let i = i?;
        let feed_id = u8_slice_to_u32(&i[i.len() - 4..]);
        let feed: Feed = get_one(&db, "feeds", feed_id)?;
        update(feed.link, &db).await?;
    }

    Ok(Redirect::to(&format!("/feed/{}", claim.uid)))
}

async fn update(url: String, db: &Db) -> Result<(Feed, Vec<u32>), AppError> {
    let content = CLIENT.get(&url).send().await?.bytes().await?;

    let item_links_tree = db.open_tree("item_links")?;
    let items_tree = db.open_tree("items")?;
    let mut item_ids = vec![];
    let feed = match rss::Channel::read_from(&content[..]) {
        Ok(rss) => {
            for item in rss.items {
                let item: Item = item.into();
                let item_id = if let Some(v) = item_links_tree.get(&item.link)? {
                    ivec_to_u32(&v)
                } else {
                    incr_id(db, "items_count")?
                };
                item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                let item_encode = bincode::encode_to_vec(&item, standard())?;
                items_tree.insert(u32_to_ivec(item_id), item_encode)?;

                item_ids.push(item_id);
            }

            Feed {
                link: url,
                title: rss.title,
            }
        }
        Err(_) => match atom_syndication::Feed::read_from(&content[..]) {
            Ok(atom) => {
                for entry in atom.entries {
                    let item: Item = entry.into();
                    let item_id = if let Some(v) = item_links_tree.get(&item.link)? {
                        ivec_to_u32(&v)
                    } else {
                        incr_id(db, "items_count")?
                    };
                    item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                    let item_encode = bincode::encode_to_vec(&item, standard())?;
                    items_tree.insert(u32_to_ivec(item_id), item_encode)?;

                    item_ids.push(item_id);
                }

                Feed {
                    link: url,
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

pub(crate) async fn cron_feed(db: &Db, interval: u64) -> Result<(), AppError> {
    let sleep = tokio::time::sleep(std::time::Duration::from_secs(interval));

    for i in &db.open_tree("feed_links")? {
        let (k, _) = i?;
        let link = String::from_utf8_lossy(&k);
        if let Err(e) = update(link.to_string(), db).await {
            error!(%e);
        }
    }

    sleep.await;
    Ok(())
}

/// `GET /feed/star`
pub(crate) async fn feed_star(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(item_id): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let item_id_ivec = u32_to_ivec(item_id);
    if db.open_tree("items")?.contains_key(&item_id_ivec)? {
        let k = [&u32_to_ivec(claim.uid), &item_id_ivec].concat();
        let star_tree = db.open_tree("star")?;
        if star_tree.contains_key(&k)? {
            star_tree.remove(&k)?;
        } else {
            star_tree.insert(&k, &[])?;
        }
    }

    Ok(Redirect::to(&format!("/feed/{}?filter=star", claim.uid)))
}

/// `GET /feed/subscribe`
pub(crate) async fn feed_subscribe(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((uid, feed_id)): Path<(u32, u32)>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
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
