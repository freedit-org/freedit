/// cargo run --example v0_7-v_0_8 -- <path to v0.7 db>
/// If no path provided, will use "freedit.db" in current directory
/// This will create a new file "freedit_v0_8.db" in current directory
use bincode::{Decode, Encode, config::standard};
use serde::Serialize;
use std::collections::HashMap;

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url);
    let db = config.open().unwrap();

    let export = db.export();
    let new = sled::open("freedit_v0_8.db").unwrap();
    new.import(export);

    let tree = new.open_tree("users").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();

        let (old_user, _): (OldUser, usize) = bincode::decode_from_slice(&v, standard()).unwrap();

        let lang = new
            .open_tree("lang")
            .unwrap()
            .get(&k)
            .unwrap()
            .map(|s| String::from_utf8_lossy(&s).to_string());

        let home_page = new
            .open_tree("home_pages")
            .unwrap()
            .get(&k)
            .unwrap()
            .map_or(0, |hp| hp[0]);

        let pub_key = new
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
        let encoded = bincode::encode_to_vec(new_user, standard()).unwrap();
        tree.insert(k, encoded).unwrap();
    }

    new.drop_tree("inns_private").unwrap();
    new.drop_tree("lang").unwrap();
    new.drop_tree("home_pages").unwrap();
    new.drop_tree("pub_keys").unwrap();

    println!("Migrated users");

    let tree = new.open_tree("items").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let (old_item, _): (OldItem, usize) = bincode::decode_from_slice(&v, standard()).unwrap();

        let new_item = Item {
            link: old_item.link,
            title: old_item.title,
            feed_title: old_item.feed_title,
            updated: old_item.updated,
            content: old_item.content,
            podcast: None,
        };
        let encoded = bincode::encode_to_vec(new_item, standard()).unwrap();
        tree.insert(k, encoded).unwrap();
    }

    println!("Migrated items");
    println!("Migration completed! New database at freedit_v0_8.db");
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
