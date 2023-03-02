use bincode::{config::standard, Decode, Encode};
use freedit::controller::{
    db_utils::{incr_id, set_one, u32_to_ivec, u8_slice_to_u32},
    notification::{add_notification, NtType},
    Post, PostContent, PostStatus,
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
        let (_, v) = i.unwrap();
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

        set_one(&db, "posts", new.pid, &new).unwrap();
    }

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

    let tree = db.open_tree("notifications").unwrap();
    // old kv: uid#pid#cid => notification_code
    // new kv: uid#nid#nt_type => id1#id2#is_read
    for i in tree.iter() {
        let (k, _) = i.unwrap();
        let uid = u8_slice_to_u32(&k[0..4]);
        let pid = u8_slice_to_u32(&k[4..8]);
        let cid = u8_slice_to_u32(&k[8..12]);

        if pid > 0 {
            add_notification(&db, uid, NtType::PostComment, pid, cid).unwrap();
        } else {
            add_notification(&db, uid, NtType::SoloComment, cid, pid).unwrap();
        }

        tree.remove(k).unwrap();
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
