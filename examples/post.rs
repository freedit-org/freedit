use bincode::{Decode, Encode};
use freedit::controller::{
    db_utils::{get_one, ivec_to_u32, set_one},
    PostContent,
};
use serde::Serialize;

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

    let tree = db.open_tree("posts").unwrap();
    for i in tree.iter() {
        let (k, _) = i.unwrap();
        let id = ivec_to_u32(&k);
        let post: Post = get_one(&db, "posts", id).unwrap();
        let new_status = match post.status {
            PostStatus::Normal => NewPostStatus::Normal,
            PostStatus::LockedByUser => NewPostStatus::LockedByUser,
            PostStatus::HiddenByUser => NewPostStatus::HiddenByUser,
            PostStatus::LockedByMod => NewPostStatus::LockedByMod,
            PostStatus::HiddenByMod => NewPostStatus::HiddenByMod,
        };

        let new_post = NewPost {
            pid: post.pid,
            uid: post.uid,
            iid: post.iid,
            title: post.title,
            tags: post.tags,
            content: post.content,
            created_at: post.created_at,
            status: new_status,
        };

        set_one(&db, "posts", id, &new_post).unwrap();
    }
}

#[derive(Encode, Decode, Serialize, PartialEq, PartialOrd, Debug, Clone)]
#[repr(u8)]
enum PostStatus {
    Normal = 0,
    LockedByUser = 4,
    HiddenByUser = 8,
    LockedByMod = 12,
    HiddenByMod = 16,
}

#[derive(Encode, Decode, Serialize, Debug)]
struct Post {
    pid: u32,
    uid: u32,
    iid: u32,
    title: String,
    tags: Vec<String>,
    content: PostContent,
    created_at: i64,
    status: PostStatus,
}

#[derive(Encode, Decode, Serialize, PartialEq, PartialOrd, Debug, Clone)]
enum NewPostStatus {
    Normal,
    LockedByUser,
    HiddenByUser,
    LockedByMod,
    HiddenByMod,
}

#[derive(Encode, Decode, Serialize, Debug)]
struct NewPost {
    pid: u32,
    uid: u32,
    iid: u32,
    title: String,
    tags: Vec<String>,
    content: PostContent,
    created_at: i64,
    status: NewPostStatus,
}
