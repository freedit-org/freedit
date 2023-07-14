use bincode::{Decode, Encode};
use freedit::controller::db_utils::{get_one, ivec_to_u32, set_one};
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

        let new_inn = Inn {
            iid: old_inn.iid,
            inn_name: old_inn.inn_name,
            about: old_inn.about,
            description: old_inn.description,
            topics: old_inn.topics,
            inn_type: old_inn.inn_type,
            early_birds: old_inn.early_birds,
            created_at: old_inn.created_at,
            limit_edit_seconds: 60 * 30,
        };

        set_one(&db, "inns", id, &new_inn).unwrap();
    }
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Inn {
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
}
