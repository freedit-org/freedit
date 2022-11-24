use super::{
    get_one, get_range, get_site_config, into_response, u32_to_ivec, u8_slice_to_u32, Claim,
    PageData, ParamsPage,
};
use crate::{
    controller::{incr_id, ivec_to_u32, Feed, Item},
    error::AppError,
};
use askama::Template;
use axum::{
    extract::{Query, State},
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

/// Page data: `feed.html`
#[derive(Template)]
#[template(path = "feed.html")]
struct PageFeed<'a> {
    page_data: PageData<'a>,
    feeds: IndexMap<String, Vec<(u32, String, bool)>>,
    items: Vec<Item>,
    filter: Option<String>,
    filter_value: Option<String>,
    anchor: usize,
    n: usize,
    is_desc: bool,
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
    Query(params): Query<ParamsFeed>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let mut map = IndexMap::new();
    let mut item_ids = vec![];

    for i in db
        .open_tree("user_folders")?
        .scan_prefix(u32_to_ivec(claim.uid))
        .keys()
    {
        let i = i?;
        let feed_id = u8_slice_to_u32(&i[(i.len() - 4)..]);
        let folder = String::from_utf8_lossy(&i[4..(i.len() - 4)]).to_string();
        let mut feed: Feed = get_one(&db, "feeds", feed_id)?;

        let mut is_active_feed = false;
        if let (Some(filter), Some(filter_value)) = (&params.filter, &params.filter_value) {
            match filter.as_ref() {
                "item" => {
                    if let Ok(id) = filter_value.parse::<u32>() {
                        if id == feed_id {
                            item_ids.append(&mut feed.item_ids);
                            is_active_feed = true;
                        }
                    }
                }
                "folder" => {
                    if &folder == filter_value {
                        is_active_feed = true;
                        item_ids.append(&mut feed.item_ids);
                    }
                }
                _ => {
                    item_ids.append(&mut feed.item_ids);
                }
            }
        } else {
            item_ids.append(&mut feed.item_ids);
        }

        let e = map.entry(folder).or_insert(vec![]);
        e.push((feed_id, feed.title, is_active_feed));
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
    for i in item_ids {
        let item: Item = get_one(&db, "items", i)?;
        items.push(item);
    }

    let page_data = PageData::new("Feed", &site_config, Some(claim), false);
    let page_feed = PageFeed {
        page_data,
        feeds: map,
        items,
        filter: params.filter,
        filter_value: params.filter_value,
        n,
        anchor,
        is_desc,
    };

    Ok(into_response(&page_feed, "html"))
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
#[derive(Deserialize, Debug)]
pub(crate) struct FormFeedAdd {
    url: String,
    folder: String,
    new_folder: String,
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

    let content = CLIENT.get(&form.url).send().await?.bytes().await?;

    let item_links_tree = db.open_tree("item_links")?;
    let items_tree = db.open_tree("items")?;
    let feed = match rss::Channel::read_from(&content[..]) {
        Ok(rss) => {
            let mut item_ids = Vec::with_capacity(rss.items.len());
            for item in rss.items {
                let item: Item = item.into();
                let item_id = if let Some(v) = item_links_tree.get(&item.link)? {
                    ivec_to_u32(&v)
                } else {
                    incr_id(&db, "items_count")?
                };
                item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                let item_encode = bincode::encode_to_vec(&item, standard())?;
                items_tree.insert(u32_to_ivec(item_id), item_encode)?;

                item_ids.push(item_id);
            }

            Feed {
                link: rss.link,
                title: rss.title,
                item_ids,
            }
        }
        Err(_) => match atom_syndication::Feed::read_from(&content[..]) {
            Ok(atom) => {
                let mut item_ids = Vec::with_capacity(atom.entries.len());
                for entry in atom.entries {
                    let item: Item = entry.into();
                    let item_id = if let Some(v) = item_links_tree.get(&item.link)? {
                        ivec_to_u32(&v)
                    } else {
                        incr_id(&db, "items_count")?
                    };
                    item_links_tree.insert(&item.link, u32_to_ivec(item_id))?;
                    let item_encode = bincode::encode_to_vec(&item, standard())?;
                    items_tree.insert(u32_to_ivec(item_id), item_encode)?;

                    item_ids.push(item_id);
                }

                Feed {
                    link: atom.links[0].href.clone(),
                    title: atom.title.to_string(),
                    item_ids,
                }
            }
            Err(_) => {
                return Err(AppError::InvalidFeedLink);
            }
        },
    };

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

    user_folders_tree.insert(k, &[])?;

    Ok(Redirect::to("/feed"))
}
