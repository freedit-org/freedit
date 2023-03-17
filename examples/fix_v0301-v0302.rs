use freedit::controller::{
    db_utils::{get_one, u32_to_ivec, u8_slice_to_u32},
    Post,
};

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    for i in &db.open_tree("post_timeline").unwrap() {
        let (k, v) = i.unwrap();
        let iid = u8_slice_to_u32(&k[4..8]);
        let pid = u8_slice_to_u32(&k[8..12]);
        let visibility = u8_slice_to_u32(&v);

        let post: Post = get_one(&db, "posts", pid).unwrap();

        let k1 = [&u32_to_ivec(post.uid), &u32_to_ivec(pid)].concat();
        let v = [&u32_to_ivec(iid), &u32_to_ivec(visibility)].concat();
        db.open_tree("user_posts").unwrap().insert(k1, v).unwrap();
    }
}
