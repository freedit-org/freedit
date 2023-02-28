use freedit::controller::{
    db_utils::u8_slice_to_u32,
    notification::{add_notification, NtType},
};

fn main() {
    let db_url = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "freedit.db".to_owned());
    let config = sled::Config::default().path(db_url).use_compression(true);
    let db = config.open().unwrap();

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
