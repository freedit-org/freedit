use bincode::{config::standard, Decode, Encode};
use data_encoding::BASE64;

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("users").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let (old, _): (OldUser, usize) = bincode::decode_from_slice(&v, standard()).unwrap();
        let salt = BASE64.decode(old.salt.as_bytes()).unwrap();
        let pass = BASE64.decode(old.password_hash.as_bytes()).unwrap();
        let password_hash = BASE64.encode(&[&pass[..], &salt[..]].concat());

        let new = User {
            uid: old.uid,
            username: old.username,
            about: old.about,
            role: old.role,
            url: old.url,
            password_hash,
            created_at: old.created_at,
        };

        let new_encoded = bincode::encode_to_vec(&new, standard()).unwrap();
        tree.insert(k, new_encoded).unwrap();
    }

    let tree = db.open_tree("inns").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let (old, _): (OldInn, usize) = bincode::decode_from_slice(&v, standard()).unwrap();
        let new = Inn {
            iid: old.iid,
            inn_name: old.inn_name,
            about: old.about,
            description: old.description,
            topics: old.topics,
            inn_type: old.inn_type,
            created_at: old.created_at,
            early_birds: 0,
        };

        let new_encoded = bincode::encode_to_vec(&new, standard()).unwrap();
        tree.insert(k, new_encoded).unwrap();
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
    // add new field, breaking the api
    early_birds: u32,
    created_at: i64,
}

#[derive(Encode, Decode)]
struct OldUser {
    uid: u32,
    username: String,
    salt: String,
    password_hash: String,
    created_at: i64,
    role: u8,
    url: String,
    about: String,
}

#[derive(Encode, Decode)]
struct User {
    uid: u32,
    username: String,
    password_hash: String,
    created_at: i64,
    role: u8,
    url: String,
    about: String,
}
