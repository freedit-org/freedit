fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("usernames").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let name = String::from_utf8_lossy(&k);
        tree.remove(&k).unwrap();
        tree.insert(name.to_lowercase(), v).unwrap();
    }

    let tree = db.open_tree("inn_names").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let name = String::from_utf8_lossy(&k);
        tree.remove(&k).unwrap();
        tree.insert(name.to_lowercase(), v).unwrap();
    }
}
