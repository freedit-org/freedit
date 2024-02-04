use bincode::{Decode, Encode};
use freedit::{get_one, ivec_to_u32, set_one, u8_slice_to_u32};
use serde::Serialize;

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url);
    let db = config.open().unwrap();

    let tree = db.open_tree("inns").unwrap();
    for i in tree.iter() {
        let (k, _) = i.unwrap();
        let id = ivec_to_u32(&k);
        let old_inn: OldInn = get_one(&db, "inns", id).unwrap();

        let inn_type = match old_inn.inn_type.as_str() {
            "Public" => 0,
            "Apply" => 5,
            "Private" => 10,
            "Hidden" => 20,
            _ => 20,
        };

        let new_inn = Inn {
            iid: old_inn.iid,
            inn_name: old_inn.inn_name,
            about: old_inn.about,
            description: old_inn.description,
            topics: old_inn.topics,
            inn_type,
            early_birds: old_inn.early_birds,
            created_at: old_inn.created_at,
            limit_edit_seconds: 60 * 30,
        };

        set_one(&db, "inns", id, &new_inn).unwrap();
    }

    let tree = db.open_tree("user_posts").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let iid = u8_slice_to_u32(&v[0..4]);
        let inn: Inn = get_one(&db, "inns", iid).unwrap();
        let mut v = iid.to_be_bytes().to_vec();
        v.push(inn.inn_type);
        tree.insert(k, v).unwrap();
    }

    let tree = db.open_tree("post_timeline_idx").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let iid = u8_slice_to_u32(&k[0..4]);
        let inn: Inn = get_one(&db, "inns", iid).unwrap();
        let mut v = v.to_vec();
        v.push(inn.inn_type);
        tree.insert(k, v).unwrap();
    }

    let tree = db.open_tree("post_timeline").unwrap();
    for i in tree.iter() {
        let (k, _) = i.unwrap();
        let iid = u8_slice_to_u32(&k[4..8]);
        let inn: Inn = get_one(&db, "inns", iid).unwrap();
        tree.insert(k, &[inn.inn_type]).unwrap();
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

#[derive(Encode, Decode, Serialize, Debug)]
struct OldInn {
    iid: u32,
    inn_name: String,
    about: String,
    description: String,
    topics: Vec<String>,
    inn_type: String,
    early_birds: u32,
    created_at: i64,
    limit_edit_seconds: u32,
}
