use fjall::KeyspaceCreateOptions;

fn main() {
    let db_url = "data/freedit.db";
    let config = sled::Config::default().path(db_url);
    let sled_db = config.open().unwrap();

    let fjall_url = "data/fjall.db";
    let db = fjall::SingleWriterTxDatabase::builder(fjall_url)
        .open()
        .unwrap();

    for i in sled_db.tree_names() {
        let mut name = String::from_utf8_lossy(&i);
        if name == "__sled__default" {
            name = "default".into();
        }
        let sled_tree = sled_db.open_tree(&i).unwrap();
        let fjall_tree = db
            .keyspace(name.as_ref(), KeyspaceCreateOptions::default)
            .unwrap();
        for item in sled_tree.iter() {
            let (k, v) = item.unwrap();
            fjall_tree.insert(k.to_vec(), v.to_vec()).unwrap();
        }
        println!("Migrated tree: {}", name);
    }
    println!("Database migration completed.");
}
