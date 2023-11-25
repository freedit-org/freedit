use askama::Template;
use axum::{
    extract::{Path, Query},
    headers::Cookie,
    response::{IntoResponse, Redirect},
    Form, TypedHeader,
};
use serde::Deserialize;

use crate::{controller::fmt::clean_html, error::AppError, DB};

use super::{
    db_utils::{get_one, incr_id, u32_to_ivec, u8_slice_to_u32},
    meta_handler::{into_response, PageData},
    notification::{add_notification, mark_read, NtType},
    Claim, SiteConfig, User,
};

/// Page data: `message.html`
#[derive(Template)]
#[template(path = "message.html", escape = "none")]
struct PageMessage<'a> {
    page_data: PageData<'a>,
    pub_key: String,
    receiver_id: u32,
}

/// `GET /message/:uid`
pub(crate) async fn message(
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    if DB
        .open_tree("pub_keys")?
        .get(u32_to_ivec(claim.uid))?
        .is_none()
    {
        return Err(AppError::Custom(
            "You have not generated key pairs. In order to receive msg, please <a href= '/key'>generate one</a>.".to_string(),
        ));
    }

    let Some(pub_key) = DB.open_tree("pub_keys")?.get(u32_to_ivec(uid))? else {
        return Err(AppError::Custom(
            "User has not generated key pairs".to_string(),
        ));
    };

    let title = format!("Sending e2ee Message to {}", uid);

    let page_message = PageMessage {
        receiver_id: uid,
        page_data: PageData::new(&title, &site_config, Some(claim), false),
        pub_key: String::from_utf8_lossy(&pub_key).to_string(),
    };

    Ok(into_response(&page_message))
}

/// Form data: `/message/:uid`
#[derive(Deserialize)]
pub(crate) struct FormMessage {
    message: String,
}

/// `GET /message/:uid`
pub(crate) async fn message_post(
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
    Form(input): Form<FormMessage>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let mid = incr_id(&DB, "messages_count")?;
    let message = clean_html(&input.message);
    let v = [
        &u32_to_ivec(uid),
        &u32_to_ivec(claim.uid),
        message.as_bytes(),
    ]
    .concat();

    DB.open_tree("messages")?.insert(&u32_to_ivec(mid), v)?;
    add_notification(&DB, uid, NtType::Message, claim.uid, mid)?;

    let redirect = format!("/user/{}", uid);
    Ok(Redirect::to(&redirect))
}

/// Page data: `key.html`
#[derive(Template)]
#[template(path = "key.html", escape = "none")]
struct PageKey<'a> {
    page_data: PageData<'a>,
    pub_key: String,
}

/// `GET /key`
pub(crate) async fn key(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let pub_key = DB
        .open_tree("pub_keys")?
        .get(u32_to_ivec(claim.uid))?
        .map(|r| String::from_utf8_lossy(&r).to_string())
        .unwrap_or_default();

    let page_key = PageKey {
        page_data: PageData::new("Generate Key Pairs", &site_config, Some(claim), false),
        pub_key,
    };

    Ok(into_response(&page_key))
}

/// Form data: `/key`
#[derive(Deserialize)]
pub(crate) struct FormKey {
    pub_key: String,
}

/// `POST /key`
pub(crate) async fn key_post(
    cookie: Option<TypedHeader<Cookie>>,
    Form(input): Form<FormKey>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let pub_key = clean_html(&input.pub_key);

    DB.open_tree("pub_keys")?
        .insert(u32_to_ivec(claim.uid), pub_key.as_str())?;

    Ok(Redirect::to("/key"))
}

/// Page data: `inbox.html`
#[derive(Template)]
#[template(path = "inbox.html", escape = "none")]
struct PageInbox<'a> {
    page_data: PageData<'a>,
    message: String,
    sender_id: u32,
    sender_name: String,
}

/// url params: `inbox.html`
#[derive(Deserialize)]
pub(crate) struct ParamsInbox {
    nid: Option<u32>,
}

/// `GET /inbox`
pub(crate) async fn inbox(
    cookie: Option<TypedHeader<Cookie>>,
    Path(mid): Path<u32>,
    Query(params): Query<ParamsInbox>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let v = DB
        .open_tree("messages")?
        .get(u32_to_ivec(mid))?
        .ok_or(AppError::NotFound)?;

    let receiver = u8_slice_to_u32(&v[0..4]);
    if receiver != claim.uid {
        return Err(AppError::NotFound);
    }
    let sender_id = u8_slice_to_u32(&v[4..8]);
    let sender: User = get_one(&DB, "users", sender_id)?;
    let message = String::from_utf8_lossy(&v[8..]).to_string();

    if let Some(nid) = params.nid {
        let tree = DB.open_tree("notifications")?;
        let prefix = [&u32_to_ivec(claim.uid), &u32_to_ivec(nid)].concat();
        for i in tree.scan_prefix(prefix) {
            let (k, _) = i?;
            tree.update_and_fetch(k, mark_read)?;
        }
    }

    let page_inbox = PageInbox {
        page_data: PageData::new("Inbox", &site_config, Some(claim), false),
        message,
        sender_id,
        sender_name: sender.username,
    };

    Ok(into_response(&page_inbox))
}
