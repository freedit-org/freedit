use std::collections::HashMap;

use bincode::{Decode, Encode};
use freedit::{get_one, ivec_to_u32, set_one};
use serde::Serialize;

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url);
    let db = config.open().unwrap();

    let tree = db.open_tree("users").unwrap();
    for i in tree.iter() {
        let (k, _) = i.unwrap();
        let id = ivec_to_u32(&k);
        let old_user: OldUser = get_one(&db, "users", id).unwrap();

        let lang = db
            .open_tree("lang")
            .unwrap()
            .get(&k)
            .unwrap()
            .map(|s| String::from_utf8_lossy(&s).to_string());

        let home_page = db
            .open_tree("home_pages")
            .unwrap()
            .get(&k)
            .unwrap()
            .map_or(0, |hp| hp[0]);

        let pub_key = db
            .open_tree("pub_keys")
            .unwrap()
            .get(&k)
            .unwrap()
            .map(|s| String::from_utf8_lossy(&s).to_string());

        let new_user = User {
            uid: old_user.uid,
            username: old_user.username,
            password_hash: old_user.password_hash,
            recovery_hash: old_user.recovery_hash,
            created_at: old_user.created_at,
            role: old_user.role,
            url: old_user.url,
            about: old_user.about,
            lang,
            home_page,
            pub_key,
        };
        set_one(&db, "users", id, &new_user).unwrap();
    }

    db.drop_tree("inns_private").unwrap();
    db.drop_tree("lang").unwrap();
    db.drop_tree("home_pages").unwrap();
    db.drop_tree("pub_keys").unwrap();

    let tree = db.open_tree("items").unwrap();
    for i in tree.iter() {
        let (k, _) = i.unwrap();
        let id = ivec_to_u32(&k);
        let old_item: OldItem = get_one(&db, "items", id).unwrap();

        let new_item = Item {
            link: old_item.link,
            title: old_item.title,
            feed_title: old_item.feed_title,
            updated: old_item.updated,
            content: old_item.content,
            podcast: None,
        };
        set_one(&db, "items", id, &new_item).unwrap();
    }

    // let export = db.export();
    // let new = sled::open("new_db").unwrap();
    // new.import(export);
}

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
    lang: Option<String>,
    home_page: u8,
    pub_key: Option<String>,
}

#[derive(Default, Encode, Decode, Serialize)]
struct OldUser {
    uid: u32,
    username: String,
    password_hash: String,
    recovery_hash: Option<String>,
    created_at: i64,
    role: u8,
    url: String,
    about: String,
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

#[derive(Encode, Decode, Debug)]
struct OldItem {
    link: String,
    title: String,
    feed_title: String,
    updated: i64,
    content: String,
}
