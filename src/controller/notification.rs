use super::{
    get_ids_by_prefix, get_one, incr_id,
    meta_handler::PageData,
    u32_to_ivec, u8_slice_to_u32,
    user::{InnRole, Role},
    Claim, Comment, Inn, Post, SiteConfig, Solo, User,
};
use crate::{controller::filters, error::AppError, DB};
use axum::{extract::Query, response::IntoResponse};
use axum_extra::{headers::Cookie, TypedHeader};
use bincode::config::standard;
use rinja_axum::{into_response, Template};
use serde::Deserialize;
use sled::{Db, IVec};
use snailquote::unescape;

/// notification.html
#[derive(Template)]
#[template(path = "notification.html", escape = "none")]
struct NotificationPage<'a> {
    page_data: PageData<'a>,
    notifications: Vec<Notification>,
    inn_notifications: Vec<InnNotification>,
    anchor: usize,
    n: usize,
}

#[derive(Deserialize)]
pub(crate) struct NotifyParams {
    op_type: Option<String>,
    nid: Option<u32>,
    anchor: Option<usize>,
}

#[repr(u8)]
pub(super) enum NtType {
    PostComment = 1,
    PostMention = 2,
    SoloComment = 3,
    SoloMention = 4,
    InnNotification = 5,
    SiteNotification = 6,
    Message = 7,
    SoloDelete = 8,
    ImageDelete = 9,
    PostLock = 10,
    PostHide = 11,
    CommentHide = 12,
    CommentMention = 13,
}

impl From<u8> for NtType {
    fn from(value: u8) -> Self {
        match value {
            1 => Self::PostComment,
            2 => Self::PostMention,
            3 => Self::SoloComment,
            4 => Self::SoloMention,
            5 => Self::InnNotification,
            6 => Self::SiteNotification,
            7 => Self::Message,
            8 => Self::SoloDelete,
            9 => Self::ImageDelete,
            10 => Self::PostLock,
            11 => Self::PostHide,
            12 => Self::CommentHide,
            13 => Self::CommentMention,
            _ => unreachable!(),
        }
    }
}

struct Notification {
    nid: u32,
    uid: u32,
    content1: String,
    content2: String,
    is_read: bool,
}

/// work for [set_merge_operator](https://docs.rs/sled/latest/sled/struct.Db.html#method.set_merge_operator):
/// update notification code to read.
pub(super) fn mark_read(old: Option<&[u8]>) -> Option<Vec<u8>> {
    old.map(|bytes| [&bytes[0..8], &[1u8]].concat())
}

/// `GET /notification`
///
/// Batch mode:
///
/// 30 notifications in a batch and batch delete only if they has been marked read
pub(crate) async fn notification(
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<NotifyParams>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&DB, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let prefix = u32_to_ivec(claim.uid);
    let tree = DB.open_tree("notifications")?;

    let anchor = params.anchor.unwrap_or(0);
    let n = site_config.per_page;
    if let Some(op_type) = params.op_type {
        match op_type.as_str() {
            "mark_batch" => {
                for (idx, i) in tree.scan_prefix(&prefix).enumerate() {
                    if idx < anchor {
                        continue;
                    }
                    if idx >= n + anchor {
                        break;
                    }
                    let (key, _) = i?;
                    tree.update_and_fetch(key, mark_read)?;
                }
            }
            "delete_batch" => {
                for (idx, i) in tree.scan_prefix(&prefix).enumerate() {
                    if idx < anchor {
                        continue;
                    }
                    if idx >= n + anchor {
                        break;
                    }
                    let (key, value) = i?;
                    // Delete notification if it is read
                    if value[8] == 1 {
                        tree.remove(key)?;
                    }
                }
            }
            "mark" => {
                if let Some(nid) = params.nid {
                    let prefix = [&u32_to_ivec(claim.uid), &u32_to_ivec(nid)].concat();
                    for i in tree.scan_prefix(prefix) {
                        let (k, _) = i?;
                        tree.update_and_fetch(k, mark_read)?;
                    }
                }
            }
            "delete" => {
                if let Some(nid) = params.nid {
                    let prefix = [&u32_to_ivec(claim.uid), &u32_to_ivec(nid)].concat();
                    for i in tree.scan_prefix(prefix) {
                        let (k, _) = i?;
                        tree.remove(k)?;
                    }
                }
            }
            _ => {}
        }
    }

    let mut notifications = Vec::with_capacity(n);
    for (idx, i) in tree.scan_prefix(&prefix).rev().enumerate() {
        if idx < anchor {
            continue;
        }
        if idx >= n + anchor {
            break;
        }

        // uid#nid#nt_type = id1#id2#is_read
        let (key, value) = i?;
        let nid = u8_slice_to_u32(&key[4..8]);
        let is_read = value[8] == 1;

        let nt_type: NtType = key[8].into();
        match nt_type {
            NtType::PostComment => {
                if let Some(v) = &DB.open_tree("post_comments")?.get(&value[0..8])? {
                    let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
                    let post: Post = get_one(&DB, "posts", comment.pid)?;
                    let user: User = get_one(&DB, "users", comment.uid)?;
                    let content1 = format!(
                        "{} commented on your post <a href='/post/{}/{}?nid={}#{}'>{}</a>",
                        user.username, post.iid, comment.pid, nid, comment.cid, post.title
                    );
                    let notification = Notification {
                        nid,
                        uid: comment.uid,
                        content1,
                        content2: unescape(&comment.content).unwrap(),
                        is_read,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::PostMention => {
                let pid = u8_slice_to_u32(&value[0..4]);
                let Ok(post) = get_one::<Post>(&DB, "posts", pid) else {
                    tree.remove(&key)?;
                    continue;
                };
                let user: User = get_one(&DB, "users", post.uid)?;
                let content2 = format!(
                    "{} mentioned you on post <a href='/post/{}/{}?nid={}'>{}</a>",
                    user.username, post.iid, pid, nid, post.title
                );
                let notification = Notification {
                    nid,
                    uid: post.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
            NtType::CommentMention => {
                if let Some(v) = &DB.open_tree("post_comments")?.get(&value[0..8])? {
                    let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
                    let post: Post = get_one(&DB, "posts", comment.pid)?;
                    let user: User = get_one(&DB, "users", comment.uid)?;
                    let content1 = format!(
                        "{} mentioned you on post <a href='/post/{}/{}?nid={}#{}'>{}</a>",
                        user.username, post.iid, comment.pid, nid, comment.cid, post.title
                    );
                    let notification = Notification {
                        nid,
                        uid: comment.uid,
                        content1,
                        content2: unescape(&comment.content).unwrap(),
                        is_read,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::PostHide => {
                let uid = u8_slice_to_u32(&value[0..4]);
                let user: User = get_one(&DB, "users", uid)?;
                let pid = u8_slice_to_u32(&value[4..8]);
                let post: Post = get_one(&DB, "posts", pid)?;
                let content2 = format!(
                    "{} has hidden your post <a href='/post/{}/{}?nid={}'>{}</a>",
                    user.username, post.iid, pid, nid, post.title
                );
                let notification = Notification {
                    nid,
                    uid: post.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
            NtType::PostLock => {
                let uid = u8_slice_to_u32(&value[0..4]);
                let user: User = get_one(&DB, "users", uid)?;
                let pid = u8_slice_to_u32(&value[4..8]);
                let post: Post = get_one(&DB, "posts", pid)?;
                let content2 = format!(
                    "{} has locked your post <a href='/post/{}/{}?nid={}'>{}</a>",
                    user.username, post.iid, pid, nid, post.title
                );
                let notification = Notification {
                    nid,
                    uid: post.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
            NtType::CommentHide => {
                if let Some(v) = &DB.open_tree("post_comments")?.get(&value[0..8])? {
                    let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
                    let post: Post = get_one(&DB, "posts", comment.pid)?;
                    let content1 = format!(
                        "Your comment on <a href='/post/{}/{}?nid={}#{}'>{}</a> has been hidden",
                        post.iid, comment.pid, nid, comment.cid, post.title
                    );
                    let notification = Notification {
                        nid,
                        uid: comment.uid,
                        content1,
                        content2: unescape(&comment.content).unwrap(),
                        is_read,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::SoloComment => {
                let sid1 = u8_slice_to_u32(&value[0..4]);
                let sid2 = u8_slice_to_u32(&value[4..8]);
                if let Ok(solo) = get_one::<Solo>(&DB, "solos", sid2) {
                    let user: User = get_one(&DB, "users", solo.uid)?;
                    let content1 = format!(
                        "{} commented your <a href='/solo/{}?nid={}'>Solo</a>",
                        &user.username, sid1, nid
                    );
                    let notification = Notification {
                        nid,
                        uid: solo.uid,
                        content1,
                        content2: unescape(&solo.content).unwrap(),
                        is_read,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::SoloMention => {
                let sid1 = u8_slice_to_u32(&value[0..4]);
                if let Ok(solo) = get_one::<Solo>(&DB, "solos", sid1) {
                    let user: User = get_one(&DB, "users", solo.uid)?;
                    let content1 = format!(
                        "{} mentioned you on <a href='/solo/{}?nid={}'>Solo</a>",
                        &user.username, sid1, nid
                    );
                    let notification = Notification {
                        nid,
                        uid: solo.uid,
                        content1,
                        content2: unescape(&solo.content).unwrap(),
                        is_read,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::SoloDelete => {
                let uid = u8_slice_to_u32(&value[0..4]);
                let user: User = get_one(&DB, "users", uid)?;
                let sid = u8_slice_to_u32(&value[4..8]);
                let content2 = format!("{} has deleted your solo(id={})", user.username, sid);
                let notification = Notification {
                    nid,
                    uid: user.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
            NtType::InnNotification => {
                let role = u8_slice_to_u32(&value[0..4]);
                let role_desc = InnRole::from(role as u8).to_string();
                let iid = u8_slice_to_u32(&value[4..8]);
                let inn: Inn = get_one(&DB, "inns", iid)?;
                let content2 = format!(
                    "Your role in {} (id:{}) has been changed to {role_desc}",
                    inn.inn_name, iid
                );
                let notification = Notification {
                    nid,
                    uid: claim.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
            NtType::SiteNotification => {
                let role = u8_slice_to_u32(&value[0..4]);
                let role_desc = Role::from(role as u8).to_string();
                let content2 = format!("Your site role has been changed to {role_desc}");
                let notification = Notification {
                    nid,
                    uid: claim.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
            NtType::Message => {
                let sender_id = u8_slice_to_u32(&value[0..4]);
                let sender: User = get_one(&DB, "users", sender_id)?;
                let mid = u8_slice_to_u32(&value[4..8]);
                let content2 = format!(
                    "{} send a <a href='/inbox/{mid}?nid={nid}'>e2ee message</a> to you.",
                    sender.username
                );
                let notification = Notification {
                    nid,
                    uid: sender.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
            NtType::ImageDelete => {
                let uid = u8_slice_to_u32(&value[0..4]);
                let user: User = get_one(&DB, "users", uid)?;
                let img_id = u8_slice_to_u32(&value[4..8]);
                let content2 = format!("{} has deleted your image(id={})", &user.username, img_id);
                let notification = Notification {
                    nid,
                    uid: user.uid,
                    content1: String::new(),
                    content2,
                    is_read,
                };
                notifications.push(notification);
            }
        }
    }

    let mut inn_notifications = Vec::new();
    let mod_inns = get_ids_by_prefix(&DB, "mod_inns", prefix, None)?;
    for i in mod_inns {
        for i in DB.open_tree("inn_apply")?.scan_prefix(u32_to_ivec(i)) {
            let (k, _) = i?;
            let inn_notification = InnNotification {
                iid: u8_slice_to_u32(&k[0..4]),
                uid: u8_slice_to_u32(&k[4..]),
            };
            inn_notifications.push(inn_notification);
        }

        if inn_notifications.len() >= n + anchor {
            break;
        }
    }

    let has_unread = User::has_unread(&DB, claim.uid)?;
    let page_data = PageData::new("notification", &site_config, Some(claim), has_unread);
    let notification_page = NotificationPage {
        page_data,
        notifications,
        inn_notifications,
        anchor,
        n,
    };

    Ok(into_response(&notification_page))
}

struct InnNotification {
    iid: u32,
    uid: u32,
}

pub(super) fn add_notification(
    db: &Db,
    uid: u32,
    nt_type: NtType,
    id1: u32,
    id2: u32,
) -> Result<(), AppError> {
    let nid = incr_id(db, "notifications_count")?;
    let k = [
        &u32_to_ivec(uid),
        &u32_to_ivec(nid),
        &IVec::from(&[nt_type as u8]),
    ]
    .concat();
    let v = [&u32_to_ivec(id1), &u32_to_ivec(id2), &IVec::from(&[0])].concat();
    db.open_tree("notifications")?.insert(k, v)?;

    Ok(())
}
