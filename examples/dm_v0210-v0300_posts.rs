use bincode::{config::standard, Decode, Encode};
use freedit::controller::{Post, PostContent, PostStatus};
use serde::Serialize;

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("posts").unwrap();
    for i in tree.iter() {
        let (k, v) = i.unwrap();
        let (old, _): (OldPost, usize) = bincode::decode_from_slice(&v, standard()).unwrap();

        let status = if old.is_locked {
            PostStatus::LockedByMod
        } else if old.is_hidden {
            PostStatus::HiddenByMod
        } else {
            PostStatus::Normal
        };

        let new = Post {
            pid: old.pid,
            uid: old.uid,
            iid: old.iid,
            title: old.title,
            tags: old.tags,
            content: PostContent::Markdown(old.content),
            created_at: old.created_at,
            status,
        };

        let post_encoded = bincode::encode_to_vec(&new, standard()).unwrap();
        tree.insert(k, post_encoded).unwrap();
    }
}

#[derive(Encode, Decode, Serialize, Debug)]
struct OldPost {
    pid: u32,
    uid: u32,
    iid: u32,
    title: String,
    tags: Vec<String>,
    content: String,
    created_at: i64,
    is_locked: bool,
    is_hidden: bool,
}
