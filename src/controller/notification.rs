use super::{
    get_ids_by_prefix, get_one, incr_id, into_response,
    meta_handler::PageData,
    u32_to_ivec, u8_slice_to_u32,
    user::{InnRole, Role},
    Claim, Comment, Inn, Post, SiteConfig, Solo, User,
};
use crate::error::AppError;
use askama::Template;
use axum::{
    extract::{Query, State},
    headers::Cookie,
    response::IntoResponse,
    TypedHeader,
};
use bincode::config::standard;
use serde::Deserialize;
use sled::{Db, IVec};
use snailquote::unescape;
use std::fmt::Display;

/// notification.html
#[derive(Template)]
#[template(path = "notification.html", escape = "none")]
struct NotificationPage<'a> {
    page_data: PageData<'a>,
    notifications: Vec<Notification>,
    inn_notifications: Vec<InnNotification>,
}

#[derive(Deserialize)]
pub(crate) struct NotifyParams {
    op_type: Option<String>,
    nid: Option<u32>,
}

#[repr(u8)]
pub enum NtType {
    PostComment = 1,
    PostMention = 2,
    SoloComment = 3,
    SoloMention = 4,
    InnNotification = 5,
    SiteNotification = 6,
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
            _ => unreachable!(),
        }
    }
}

impl Display for NtType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PostComment => write!(f, "PostComment"),
            Self::PostMention => write!(f, "PostMention"),
            Self::SoloComment => write!(f, "SoloComment"),
            Self::SoloMention => write!(f, "SoloMention"),
            Self::InnNotification => write!(f, "InnNotification"),
            Self::SiteNotification => write!(f, "SiteNotification"),
        }
    }
}

struct Notification {
    nid: u32,
    nt_type: String,
    uid: u32,
    username: String,
    id1: u32,
    id2: u32,
    id3: u32,
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
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<NotifyParams>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&db)?;
    let claim = cookie
        .and_then(|cookie| Claim::get(&db, &cookie, &site_config))
        .ok_or(AppError::NonLogin)?;

    let prefix = u32_to_ivec(claim.uid);
    let tree = db.open_tree("notifications")?;

    if let Some(op_type) = params.op_type {
        match op_type.as_str() {
            "mark_batch" => {
                for (n, i) in tree.scan_prefix(&prefix).enumerate() {
                    let (key, _) = i?;
                    tree.update_and_fetch(key, mark_read)?;
                    if n >= 30 {
                        break;
                    }
                }
            }
            "delete_batch" => {
                for (n, i) in tree.scan_prefix(&prefix).enumerate() {
                    let (key, value) = i?;
                    // Delete notification if it is read
                    if value[8] == 1 {
                        tree.remove(key)?;
                    }
                    if n >= 30 {
                        break;
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

    let mut notifications = Vec::with_capacity(30);
    for (n, i) in tree.scan_prefix(&prefix).enumerate() {
        // uid#nid#nt_type = id1#id2#is_read
        let (key, value) = i?;
        let nid = u8_slice_to_u32(&key[4..8]);

        let nt_type: NtType = key[8].into();
        match nt_type {
            NtType::PostComment | NtType::PostMention => {
                if let Some(v) = &db.open_tree("post_comments")?.get(&value[0..8])? {
                    let (comment, _): (Comment, usize) = bincode::decode_from_slice(v, standard())?;
                    let post: Post = get_one(&db, "posts", comment.pid)?;
                    let user: User = get_one(&db, "users", comment.uid)?;
                    let notification = Notification {
                        nid,
                        nt_type: nt_type.to_string(),
                        uid: comment.uid,
                        username: user.username,
                        id1: post.iid,
                        id2: comment.pid,
                        id3: comment.cid,
                        content1: post.title,
                        content2: unescape(&comment.content).unwrap(),
                        is_read: value[8] == 1,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::SoloComment => {
                let sid1 = u8_slice_to_u32(&value[0..4]);
                let sid2 = u8_slice_to_u32(&value[4..8]);
                if let Ok(solo) = get_one::<Solo>(&db, "solos", sid2) {
                    let user: User = get_one(&db, "users", solo.uid)?;
                    let notification = Notification {
                        nid,
                        nt_type: nt_type.to_string(),
                        uid: solo.uid,
                        username: user.username,
                        id1: sid1,
                        id2: sid2,
                        id3: 0,
                        content1: "".into(),
                        content2: unescape(&solo.content).unwrap(),
                        is_read: value[8] == 1,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::SoloMention => {
                let sid1 = u8_slice_to_u32(&value[0..4]);
                if let Ok(solo) = get_one::<Solo>(&db, "solos", sid1) {
                    let user: User = get_one(&db, "users", solo.uid)?;
                    let notification = Notification {
                        nid,
                        nt_type: nt_type.to_string(),
                        uid: solo.uid,
                        username: user.username,
                        id1: sid1,
                        id2: 0,
                        id3: 0,
                        content1: "".into(),
                        content2: unescape(&solo.content).unwrap(),
                        is_read: value[8] == 1,
                    };
                    notifications.push(notification);
                } else {
                    tree.remove(&key)?;
                };
            }
            NtType::InnNotification => {
                let role = u8_slice_to_u32(&value[0..4]);
                let role_desc = InnRole::from(role as u8).to_string();
                let iid = u8_slice_to_u32(&value[4..8]);
                let inn: Inn = get_one(&db, "inns", iid)?;
                let notification = Notification {
                    nid,
                    nt_type: nt_type.to_string(),
                    uid: claim.uid,
                    username: claim.username.clone(),
                    id1: 0,
                    id2: 0,
                    id3: 0,
                    content1: "".into(),
                    content2: format!(
                        "Your role in {} (id:{}) has been changed to {role_desc}",
                        inn.inn_name, iid
                    ),
                    is_read: value[8] == 1,
                };
                notifications.push(notification);
            }
            NtType::SiteNotification => {
                let role = u8_slice_to_u32(&value[0..4]);
                let role_desc = Role::from(role as u8).to_string();
                let notification = Notification {
                    nid,
                    nt_type: nt_type.to_string(),
                    uid: claim.uid,
                    username: claim.username.clone(),
                    id1: 0,
                    id2: 0,
                    id3: 0,
                    content1: "".into(),
                    content2: format!("Your site role has been changed to {role_desc}"),
                    is_read: value[8] == 1,
                };
                notifications.push(notification);
            }
        }

        if n >= 30 {
            break;
        }
    }
    notifications.reverse();

    let mut inn_notifications = Vec::new();
    let mod_inns = get_ids_by_prefix(&db, "mod_inns", prefix, None)?;
    for i in mod_inns {
        for i in db.open_tree("inn_apply")?.scan_prefix(u32_to_ivec(i)) {
            let (k, _) = i?;
            let inn_notification = InnNotification {
                iid: u8_slice_to_u32(&k[0..4]),
                uid: u8_slice_to_u32(&k[4..]),
            };
            inn_notifications.push(inn_notification);
        }

        if inn_notifications.len() >= 30 {
            break;
        }
    }

    let has_unread = User::has_unread(&db, claim.uid)?;
    let page_data = PageData::new("notification", &site_config, Some(claim), has_unread);
    let notification_page = NotificationPage {
        page_data,
        notifications,
        inn_notifications,
    };

    Ok(into_response(&notification_page))
}

struct InnNotification {
    iid: u32,
    uid: u32,
}

pub fn add_notification(
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
