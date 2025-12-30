/// cargo run --example v0_8-v_0_9
/// This will migrate the v0.8 sled database located at "data/freedit.db"
/// to a v0.9 fjall database located at "data/fjall.db"
fn main() {
    // v0.8 db path: data/freedit.db
    println!("Starting migration from v0.8 to v0.9...");
    println!("Reading v0.8 database from data/freedit.db");
    let db_url = "data/freedit.db";
    // check if file exists
    if !std::path::Path::new(db_url).exists() {
        panic!("Database file {} does not exist", db_url);
    }
    let config = sled::Config::default().path(db_url);
    let sled_db = config.open().unwrap();

    // v0.9 db path: data/fjall.db
    println!("Creating v0.9 database at data/fjall.db");
    let fjall_url = "data/fjall.db";
    let db = fjall::Config::new(fjall_url).open_transactional().unwrap();

    for i in sled_db.tree_names() {
        let mut name = String::from_utf8_lossy(&i);
        if name == "__sled__default" {
            name = "default".into();
        }
        let sled_tree = sled_db.open_tree(&i).unwrap();
        let fjall_tree = db
            .open_partition(name.as_ref(), Default::default())
            .unwrap();
        for item in sled_tree.iter() {
            let (k, v) = item.unwrap();
            fjall_tree.insert(k.to_vec(), v.to_vec()).unwrap();
        }
        println!("Migrated tree: {}", name);
    }
    println!("Migration completed! New database at data/fjall.db");
}
