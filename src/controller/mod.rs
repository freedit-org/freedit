//! ## model
//!
//! Any changes in these tables should be handled with care and usually
//! involve data migration, otherwise data might be lost.
//!
//! ### user
//! | tree             | key                  | value            |
//! |------------------|----------------------|------------------|
//! | default          | "users_count"        | N                |
//! | "users"          | `uid`                | [`User`]         |
//! | "usernames"      | `username`           | `uid`            |
//! | "user_following" | `uid#uid`            | `&[]`            |
//! | "user_followers" | `uid#uid`            | `&[]`            |
//! | "user_stats"     | `timestamp#uid#type` | N                |
//! | "user_uploads"   | `uid#img_id`         | `image_hash.ext` |
//! | default          | "imgs_count"         | N                |
//! | "home_pages"     | `uid`                | `u8`             |
//! | "tan"            | `ctype#id`           | `&[]`            |
//! | "lang"           | `uid`                | `lang`           |
//!
//! ### notification
//! | tree            | key                   | value             |
//! |-----------------|-----------------------|-------------------|
//! | default         | "notifications_count" | N                 |
//! | "notifications" | `uid#nid#nt_type`     | `id1#id2#is_read` |
//!
//! ### captcha
//!
//! | tree        | key                | value         |
//! |-------------|--------------------|---------------|
//! | "captcha"   | `timestamp_nanoid` | captcha_chars |
//!
//! ### solo
//! | tree               | key           | value            |
//! |--------------------|---------------|------------------|
//! | default            | "solos_count" | N                |
//! | "solos"            | `sid`         | [`Solo`]         |
//! | "user_solos"       | `uid#sid`     | `solo_type`      |
//! | "user_solos_like"  | `uid#sid`     | `&[]`            |
//! | "solo_users_like"  | `sid#uid`     | `&[]`            |
//! | "solo_timeline"    | `sid`         | `uid#solo_type`  |
//! | "hashtags"         | `hashtag#sid` | `&[]`            |
//!
//! ### session
//! | tree       | key                | value     |
//! |------------|--------------------|-----------|
//! | "sessions" | `timestamp_nanoid` | [`Claim`] |
//!
//! ### site config
//! | tree      | key           | value          |
//! |-----------|---------------|----------------|
//! | default   | "site_config" | [`SiteConfig`] |
//!
//! ### inn
//! | tree            | key           | value               |
//! |-----------------|---------------|---------------------|
//! | default         | "inns_count"  | N                   |
//! | "inns"          | `iid`         | [`Inn`]             |
//! | "inn_names"     | `inn_name`    | `iid`               |
//! | "topics"        | `topic#iid`   | `&[]`               |
//! | "mod_inns"      | `uid#iid`     | `&[]`               |
//! | "user_inns"     | `uid#iid`     | `&[]`               |
//! | "inn_apply"     | `iid#uid`     | `&[]`               |
//! | "inn_users"     | `iid#uid`     | `&[1/2/3/4/5/8/10]` |
//! | "drafts"        | `uid`         | [`FormPost`]        |
//! | "inn_feeds"     | `iid#feed_id` | `uid`               |
//! | "inn_items"     | `iid#item_id` | `&[]`               |
//!
//! ### post
//! | tree                | key                 | value                |
//! |-------------------- |---------------------|----------------------|
//! | default             | "posts_count"       | N                    |
//! | "posts"             | `pid`               | [`Post`]             |
//! | "inn_posts"         | `iid#pid`           | `&[]`                |
//! | "user_posts"        | `uid#pid`           | `iid#inn_type`       |
//! | "tags"              | `tag#pid`           | `&[]`                |
//! | "post_upvotes"      | `pid#uid`           | `&[]`                |
//! | "post_downvotes"    | `pid#uid`           | `&[]`                |
//! | "post_timeline_idx" | `iid#pid`           | `timestamp#inn_type` |
//! | "post_timeline"     | `timestamp#iid#pid` | `inn_type`           |
//! | "post_pageviews"    | `pid`               | N                    |
//! | "post_pins"         | `iid#pid`           | `&[]`                |
//!
//! ### comment
//! | tree                  | key                  | value       |
//! |-----------------------|----------------------|-------------|
//! | "post_comments_count" | `pid`                | N           |
//! | "post_comments"       | `pid#cid`            | [`Comment`] |
//! | "user_comments"       | `uid#pid#cid`        | `&[]`       |
//! | "comment_upvotes"     | `pid#cid#uid`        | `&[]`       |
//! | "comment_downvotes"   | `pid#cid#uid`        | `&[]`       |
//!
//! ### rss
//! | tree                  | key                  | value       |
//! |-----------------------|----------------------|-------------|
//! | default               | "feeds_count"        | N           |
//! | default               | "items_count"        | N           |
//! | "user_folders"        | `uid#folder#feed_id` | `&[0/1]`    |
//! | "feeds"               | `feed_id`            | [`Feed`]    |
//! | "feed_errs"           | `feed_id`            | "error msg" |
//! | "feed_items"          | `feed_id#item_id`    | `timestamp` |
//! | "feed_links"          | `feed_link`          | `feed_id`   |
//! | "item_links"          | `item_link`          | `item_id`   |
//! | "items"               | `item_id`            | [`Item`]    |
//! | "read"                | `uid#item_id`        | `&[]`       |
//! | "star"                | `uid#item_id`        | `timestamp` |
//!
//! ### e2ee message
//! | tree                  | key              | value              |
//! |-----------------------|------------------|--------------------|
//! | default               | "messages_count" | N                  |
//! | "messages"            | `mid`            | `#uid#uid#message` |
//! | "pub_keys"            | `uid`            | `pub_key`          |
//! | "user_messages"       | `uid#mid`        | `&[]`              |

pub(super) mod db_utils;
pub(super) mod feed;
pub(super) mod meta_handler;
pub(super) mod notification;
pub(super) mod tantivy;

pub(super) mod admin;
pub(super) mod inn;
pub(super) mod message;
pub(super) mod solo;
pub(super) mod upload;
pub(super) mod user;

mod fmt;

use self::db_utils::{
    get_ids_by_prefix, get_one, incr_id, ivec_to_u32, u8_slice_to_u32, u32_to_ivec,
};
use self::fmt::md2html;
use self::tantivy::{FIELDS, ToDoc};
use self::user::Role;
use crate::error::AppError;
use ::tantivy::TantivyDocument;
use bincode::config::standard;
use bincode::{Decode, Encode};
use jiff::{Timestamp, ToSpan};
use serde::{Deserialize, Serialize};
use sled::Db;
use std::collections::HashMap;
use std::fmt::Display;
use validator::Validate;

/// user
///
/// ### Permissions
/// | role     | post/solo | creat inn | site admin |
/// |----------|:---------:|:---------:|:----------:|
/// | Banned   |           |           |            |
/// | Standard | ✅        |           |            |
/// | Senior   | ✅        | ✅        |            |
/// | Admin    | ✅        | ✅        | ✅         |

#[derive(Default, Encode, Decode, Serialize)]
struct User {
    uid: u32,
    username: String,
    password_hash: String,
    recovery_hash: Option<String>,
    created_at: i64,
    role: u8,
    url: String,
    about: String,
}

impl std::fmt::Debug for User {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "uid: {}, username: {}, password_hash: ******, recovery_hash is set: {}, created_at: {}, role: {}, url: {}, about: {} }}",
            self.uid,
            self.username,
            self.recovery_hash.is_some(),
            self.created_at,
            self.role,
            self.url,
            self.about
        )
    }
}

impl User {
    fn is_mod(db: &Db, uid: u32, iid: u32) -> Result<bool, AppError> {
        let k = [&u32_to_ivec(uid), &u32_to_ivec(iid)].concat();
        Ok(db.open_tree("mod_inns")?.contains_key(k)?)
    }

    fn is_admin(db: &Db, uid: u32) -> Result<bool, AppError> {
        let user: User = get_one(db, "users", uid)?;
        Ok(Role::from(user.role) == Role::Admin)
    }

    /// check if the user has unread notifications
    fn has_unread(db: &Db, uid: u32) -> Result<bool, AppError> {
        let prefix = u32_to_ivec(uid);
        let iter = db.open_tree("notifications")?.scan_prefix(&prefix);
        for i in iter {
            let (_, v) = i?;
            if v[8] == 0 {
                return Ok(true);
            }
        }

        let mod_inns = get_ids_by_prefix(db, "mod_inns", &prefix, None)?;
        for i in mod_inns {
            if db
                .open_tree("inn_apply")?
                .scan_prefix(u32_to_ivec(i))
                .next()
                .is_some()
            {
                return Ok(true);
            }
        }

        Ok(false)
    }

    fn update_stats(db: &Db, uid: u32, stat_type: &str) -> Result<(), AppError> {
        let expire = Timestamp::now()
            .checked_add(72.hours())
            .unwrap()
            .as_second();
        let key = format!("{expire:x}_{uid}_{stat_type}");
        incr_id(&db.open_tree("user_stats")?, key)?;
        Ok(())
    }
}

#[derive(PartialEq, Clone, Copy)]
enum SoloType {
    Public = 0,
    Following = 10,
    Private = 20,
}

impl From<u32> for SoloType {
    fn from(value: u32) -> Self {
        match value {
            0 => SoloType::Public,
            10 => SoloType::Following,
            20 => SoloType::Private,
            _ => SoloType::Private,
        }
    }
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Solo {
    sid: u32,
    uid: u32,
    solo_type: u32,
    content: String,
    hashtags: Vec<String>,
    created_at: i64,
    reply_to: Option<u32>,
    replies: Vec<u32>,
}

impl ToDoc for Solo {
    fn to_doc(&self, _id: Option<u32>) -> TantivyDocument {
        let mut doc = TantivyDocument::default();
        doc.add_text(FIELDS.id, format!("solo{}", self.sid));
        doc.add_text(FIELDS.title, &self.content);
        doc.add_u64(FIELDS.uid, self.uid as u64);
        doc.add_text(FIELDS.ctype, "solo");
        doc
    }
}

#[derive(PartialEq, Clone, Copy)]
enum InnType {
    Public = 0,
    Apply = 5,
    Private = 10,
    Hidden = 20,
    PrivateHidden = 30,
}

impl From<u8> for InnType {
    fn from(value: u8) -> Self {
        match value {
            0 => InnType::Public,
            5 => InnType::Apply,
            10 => InnType::Private,
            20 => InnType::Hidden,
            30 => InnType::PrivateHidden,
            _ => InnType::Hidden,
        }
    }
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Inn {
    iid: u32,
    inn_name: String,
    about: String,
    description: String,
    topics: Vec<String>,
    inn_type: u8,
    early_birds: u32,
    created_at: i64,
    limit_edit_seconds: u32,
}

impl Inn {
    fn is_open_access(&self) -> bool {
        InnType::from(self.inn_type) == InnType::Public
            || InnType::from(self.inn_type) == InnType::Apply
    }

    fn is_closed(&self) -> bool {
        InnType::from(self.inn_type) == InnType::Hidden
            || InnType::from(self.inn_type) == InnType::PrivateHidden
    }

    fn is_private(&self) -> bool {
        InnType::from(self.inn_type) == InnType::Private
            || InnType::from(self.inn_type) == InnType::PrivateHidden
    }
}

#[derive(Encode, Decode, Serialize, PartialEq, PartialOrd, Debug, Clone)]
pub(super) enum PostStatus {
    Normal,
    LockedByUser,
    HiddenByUser,
    LockedByMod,
    HiddenByMod,
}

impl Display for PostStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Encode, Decode, Serialize, Debug)]
pub(super) enum PostContent {
    Markdown(String),
    FeedItemId(u32),
}

impl PostContent {
    fn to_html(&self, db: &Db) -> Result<String, AppError> {
        match self {
            PostContent::Markdown(md) => Ok(md2html(md)),
            PostContent::FeedItemId(id) => {
                let item: Item = get_one(db, "items", *id)?;
                let mut content = format!(
                    r#"
                    <article class="message is-info">
                        <div class="message-header">
                            <p>Info</p>
                        </div>
                        <div class="message-body">
                        This post is auto-generated from RSS feed <b>{}</b>. Source: <a href="{}">{}</a>
                        </div>
                    </article>
                    "#,
                    item.feed_title, item.link, item.title
                );
                content.push_str(&item.content);
                Ok(content)
            }
        }
    }
}

impl Display for PostContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostContent::Markdown(c) => write!(f, "{c}"),
            PostContent::FeedItemId(id) => write!(f, "From item_id: {id}"),
        }
    }
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Post {
    pid: u32,
    uid: u32,
    iid: u32,
    title: String,
    tags: Vec<String>,
    content: PostContent,
    created_at: i64,
    status: PostStatus,
}

impl ToDoc for Post {
    fn to_doc(&self, _id: Option<u32>) -> TantivyDocument {
        let mut doc = TantivyDocument::default();
        doc.add_text(FIELDS.id, format!("post{}", self.pid));
        doc.add_text(FIELDS.title, &self.title);
        doc.add_u64(FIELDS.uid, self.uid as u64);
        doc.add_text(FIELDS.content, self.content.to_string());
        doc.add_text(FIELDS.ctype, "post");
        doc
    }
}

/// Form data: `/inn/:iid/post/:pid` post create/edit page
#[derive(Debug, Default, Deserialize, Validate, Encode, Decode)]
pub(super) struct FormPost {
    #[validate(skip)]
    iid: u32,
    #[validate(length(min = 1, max = 256))]
    title: String,
    #[validate(length(min = 1, max = 128))]
    tags: String,
    #[validate(length(min = 1, max = 65535))]
    content: String,
    #[validate(skip)]
    is_draft: Option<bool>,
    #[validate(skip)]
    delete_draft: Option<bool>,
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Comment {
    cid: u32,
    pid: u32,
    uid: u32,
    reply_to: Option<u32>,
    content: String,
    created_at: i64,
    is_hidden: bool,
}

impl ToDoc for Comment {
    fn to_doc(&self, _id: Option<u32>) -> TantivyDocument {
        let mut doc = TantivyDocument::default();
        doc.add_text(FIELDS.id, format!("comt{}/{}", self.pid, self.cid));
        doc.add_text(FIELDS.title, &self.content);
        doc.add_u64(FIELDS.uid, self.uid as u64);
        doc.add_text(FIELDS.ctype, "comt");
        doc
    }
}

#[derive(Encode, Decode, Debug)]
struct Feed {
    link: String,
    title: String,
}

#[derive(Encode, Decode, Debug)]
struct Item {
    link: String,
    title: String,
    feed_title: String,
    updated: i64,
    content: String,
    podcast: Option<Podcast>,
}

#[derive(Encode, Decode, Debug)]
struct Podcast {
    enclosure_url: String,
    enclosure_length: String,
    enclosure_mime_type: String,
    audio_downloaded: bool,
    exts: HashMap<String, String>,
}

impl ToDoc for Item {
    fn to_doc(&self, id: Option<u32>) -> TantivyDocument {
        let mut doc = TantivyDocument::default();
        doc.add_text(FIELDS.id, format!("item{}", id.unwrap()));
        doc.add_text(FIELDS.title, &self.title);
        doc.add_text(FIELDS.content, &self.content);
        doc.add_text(FIELDS.ctype, "item");
        doc
    }
}

/// Go to source code to see default value: [SiteConfig::default()]
#[derive(Serialize, Deserialize, Encode, Decode, Validate, Debug)]
pub(super) struct SiteConfig {
    #[validate(length(max = 64))]
    site_name: String,
    // domain only used for inn feed
    #[validate(skip)]
    domain: String,
    #[validate(length(max = 5120))]
    description: String,
    #[validate(skip)]
    read_only: bool,
    #[validate(range(max = 32))]
    inn_mod_max: usize,
    #[validate(range(max = 256))]
    title_max_length: usize,
    #[validate(range(max = 65535))]
    article_max_length: usize,
    #[validate(range(max = 65535))]
    comment_max_length: usize,
    #[validate(range(max = 3600))]
    solo_interval: i64,
    #[validate(range(max = 3600))]
    post_interval: i64,
    #[validate(range(max = 3600))]
    comment_interval: i64,
    #[validate(range(max = 100))]
    per_page: usize,
    #[validate(skip)]
    captcha_difficulty: String,
    #[validate(skip)]
    captcha_name: String,
    #[validate(skip)]
    home_page: u8,
    #[validate(skip)]
    spam_regex: Option<String>,
    #[validate(length(max = 16))]
    lang: String,
    tos_link: String,
}

impl SiteConfig {
    /// get [SiteConfig]
    fn get(db: &Db) -> Result<SiteConfig, AppError> {
        let site_config = &db.get("site_config")?.unwrap_or_default();
        let (site_config, _): (SiteConfig, usize) =
            bincode::decode_from_slice(site_config, standard()).unwrap_or_default();
        Ok(site_config)
    }
}

#[derive(Encode, Decode)]
struct Claim {
    uid: u32,
    username: String,
    role: u8,
    last_write: i64,
    session_id: String,
    lang: Option<String>,
}

mod filters {
    use std::{collections::HashMap, sync::LazyLock};
    use tracing::error;

    static I18N: LazyLock<HashMap<(&str, &str), &str>> = LazyLock::new(|| {
        let mut i18n = HashMap::new();
        let en = include_str!("../../i18n/en.toml");
        let en = basic_toml::from_str::<HashMap<&str, &str>>(en).unwrap();
        for (k, v) in en.iter() {
            i18n.insert(("en", *k), *v);
        }

        let zh_cn = include_str!("../../i18n/zh_cn.toml");
        let zh_cn = basic_toml::from_str::<HashMap<&str, &str>>(zh_cn).unwrap();
        for (k, v) in zh_cn.iter() {
            i18n.insert(("zh_cn", *k), *v);
        }

        let ja = include_str!("../../i18n/ja.toml");
        let ja = basic_toml::from_str::<HashMap<&str, &str>>(ja).unwrap();
        for (k, v) in ja.iter() {
            i18n.insert(("ja", *k), *v);
        }

        let fr = include_str!("../../i18n/fr.toml");
        let fr = basic_toml::from_str::<HashMap<&str, &str>>(fr).unwrap();
        for (k, v) in fr.iter() {
            i18n.insert(("fr", *k), *v);
        }
        i18n
    });

    pub(super) fn l10n(
        s: &str,
        _: &dyn askama::Values,
        lang: &str,
    ) -> ::askama::Result<&'static str> {
        if let Some(v) = I18N.get(&(lang, s)) {
            Ok(v)
        } else {
            let Some(en) = I18N.get(&("en", s)) else {
                panic!("No translation for {s} in en");
            };
            error!("No translation for {s} in {lang}");
            Ok(en)
        }
    }
}
