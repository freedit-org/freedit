use freedit::controller::db_utils::{incr_id, u32_to_ivec};

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("user_uploads").unwrap();
    // old kv: uid#image_hash.ext => &[]
    // new kv: uid#img_id => image_hash.ext
    for i in tree.iter() {
        let img_id = incr_id(&db, "imgs_count").unwrap();
        let (k, _) = i.unwrap();
        println!("{:?}", &k);

        let new_k = [&k[0..4], &u32_to_ivec(img_id)].concat();
        println!("{:?}", &new_k[0..4]);
        println!("{:?}", String::from_utf8_lossy(&k[4..]));
        tree.insert(new_k, &k[4..]).unwrap();
        tree.remove(k).unwrap();
    }
}
