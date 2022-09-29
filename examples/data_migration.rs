use bincode::{config::standard, Decode, Encode};

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("inns").unwrap();

    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let (old_inn, _): (OldInn, usize) = bincode::decode_from_slice(&v, standard()).unwrap();
        let inn = Inn {
            iid: old_inn.iid,
            inn_name: old_inn.inn_name,
            about: old_inn.about,
            description: old_inn.description,
            topics: old_inn.topics,
            inn_type: old_inn.inn_type,
            created_at: old_inn.created_at,
            early_birds: 0,
        };

        let inn_encoded = bincode::encode_to_vec(&inn, standard()).unwrap();
        tree.insert(k, inn_encoded).unwrap();
    }
}

#[derive(Encode, Decode)]
struct OldInn {
    iid: u32,
    inn_name: String,
    about: String,
    description: String,
    topics: Vec<String>,
    inn_type: String,
    created_at: i64,
}

#[derive(Encode, Decode)]
struct Inn {
    iid: u32,
    inn_name: String,
    about: String,
    description: String,
    topics: Vec<String>,
    inn_type: String,
    created_at: i64,
    // add new field, breaking the api
    early_birds: u32,
}
