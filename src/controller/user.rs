//! ## [User] sign up/in/out, user profile/list controller

use super::{
    db_utils::{
        generate_nanoid_ttl, get_count, get_count_by_prefix, get_id_by_name, get_range,
        is_valid_name, ivec_to_u32, set_one, set_one_with_key, IterType,
    },
    fmt::{clean_html, ts_to_date},
    get_ids_by_prefix, get_one, incr_id, into_response,
    meta_handler::{PageData, ParamsPage, ValidatedForm},
    notification::{add_notification, NtType},
    u32_to_ivec, u8_slice_to_u32, Claim, Inn, SiteConfig, User,
};
use crate::{config::CONFIG, error::AppError, DB};
use ::rand::{thread_rng, Rng};
use askama::Template;
use axum::{
    extract::{Form, Path, Query},
    http::{header::SET_COOKIE, HeaderMap},
    response::{IntoResponse, Redirect},
};
use axum_extra::{headers::Cookie, TypedHeader};
use bincode::config::standard;
use captcha::{CaptchaName, Difficulty};
use chrono::Utc;
use data_encoding::BASE64;
use identicon::Identicon;
use ring::{
    pbkdf2,
    rand::{self, SecureRandom},
};
use serde::Deserialize;
use sled::Db;
use std::{cmp::Ordering, fmt::Display, num::NonZeroU32, time::Duration};
use tokio::time::sleep;
use validator::Validate;

/// Page data: `user.html`
#[derive(Template)]
#[template(path = "user.html")]
struct PageUser<'a> {
    user: OutUser,
    page_data: PageData<'a>,
    user_solos_count: usize,
    user_posts_count: usize,
    user_feeds_count: usize,
    user_following_count: usize,
    user_followers_count: usize,
    has_followed: Option<bool>,
    has_recovery_code: bool,
}

/// Vec data: user
struct OutUser {
    uid: u32,
    username: String,
    about: String,
    role_desc: String,
    url: String,
    created_at: String,
}

/// `GET /user/:uid`
pub(crate) async fn user(
    cookie: Option<TypedHeader<Cookie>>,
    Path(u): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let uid = match u.parse::<u32>() {
        Ok(uid) => uid,
        Err(_) => get_id_by_name(&DB, "usernames", &u)?.ok_or(AppError::NotFound)?,
    };

    let user: User = get_one(&DB, "users", uid)?;
    let out_user = OutUser {
        uid: user.uid,
        username: user.username,
        about: user.about,
        role_desc: Role::from(user.role).to_string(),
        url: user.url,
        created_at: ts_to_date(user.created_at),
    };
    let uid_ivec = u32_to_ivec(uid);

    let mut user_solos_count = 0;
    for i in DB.open_tree("user_solos")?.scan_prefix(&uid_ivec) {
        let (_, v) = i?;
        // only count public solos
        if u8_slice_to_u32(&v) == 0 {
            user_solos_count += 1;
        }
    }

    let mut user_posts_count = 0;
    for i in DB.open_tree("user_posts")?.scan_prefix(&uid_ivec) {
        let (_, v) = i?;
        // exclude private posts
        if u8_slice_to_u32(&v[4..8]) != 10 {
            user_posts_count += 1;
        }
    }

    let mut user_feeds_count = 0;
    for i in DB.open_tree("user_folders")?.scan_prefix(&uid_ivec) {
        let (_, v) = i?;
        // only count public feeds
        if v[0] == 1 {
            user_feeds_count += 1;
        }
    }

    let user_following_count = get_count_by_prefix(&DB, "user_following", &uid_ivec)?;
    let user_followers_count = get_count_by_prefix(&DB, "user_followers", &uid_ivec)?;

    let mut has_recovery_code = true;
    let has_followed = if let Some(ref claim) = claim {
        if claim.uid != uid {
            let following_k = [&u32_to_ivec(claim.uid), &uid_ivec].concat();
            Some(DB.open_tree("user_following")?.contains_key(following_k)?)
        } else {
            has_recovery_code = user.recovery_hash.is_some();
            None
        }
    } else {
        None
    };

    let title = format!("{}-{}", out_user.username, out_user.uid);

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new(&title, &site_config, claim, has_unread);
    let page_user = PageUser {
        page_data,
        user: out_user,
        user_solos_count,
        user_posts_count,
        user_feeds_count,
        user_following_count,
        user_followers_count,
        has_followed,
        has_recovery_code,
    };

    Ok(into_response(&page_user))
}

/// `GET /user/:uid/follow` follow user
pub(crate) async fn user_follow(
    cookie: Option<TypedHeader<Cookie>>,
    Path(u): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let uid = match u.parse::<u32>() {
        Ok(uid) => uid,
        Err(_) => get_id_by_name(&DB, "usernames", &u)?.ok_or(AppError::NotFound)?,
    };

    let following_k = [&u32_to_ivec(claim.uid), &u32_to_ivec(uid)].concat();
    let followers_k = [&u32_to_ivec(uid), &u32_to_ivec(claim.uid)].concat();

    let user_following_tree = DB.open_tree("user_following")?;
    let user_followers_tree = DB.open_tree("user_followers")?;

    if user_following_tree.contains_key(&following_k)? {
        user_following_tree.remove(&following_k)?;
        user_followers_tree.remove(&followers_k)?;
    } else {
        user_following_tree.insert(&following_k, &[])?;
        user_followers_tree.insert(&followers_k, &[])?;
    }

    let target = format!("/user/{u}");
    Ok(Redirect::to(&target))
}

/// Page data: `user_list.html`
#[derive(Template)]
#[template(path = "user_list.html")]
struct PageUserList<'a> {
    page_data: PageData<'a>,
    users: Vec<OutUserList>,
    anchor: usize,
    n: usize,
    is_desc: bool,
    filter: Option<String>,
    role: Option<u8>,
    info: (u32, String, bool),
    is_admin: bool,
}

/// Vec data: user list
struct OutUserList {
    uid: u32,
    username: String,
    about: String,
    role: u8,
    role_desc: String,
}

#[repr(u8)]
#[derive(Debug, PartialEq, PartialOrd)]
pub(super) enum Role {
    Banned = 0,
    Standard = 10,
    Senior = 100,
    Admin = 255,
}

impl From<u8> for Role {
    fn from(value: u8) -> Self {
        match value {
            0 => Role::Banned,
            10 => Role::Standard,
            100 => Role::Senior,
            255 => Role::Admin,
            _ => unreachable!(),
        }
    }
}

impl Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

#[derive(PartialEq, PartialOrd, Debug)]
#[repr(u8)]
pub(super) enum InnRole {
    Pending = 1,
    Deny = 2,
    Limited = 3,
    Intern = 4,
    Fellow = 5,
    Mod = 7,
    Super = 10,
}

impl InnRole {
    pub(super) fn get(db: &Db, iid: u32, uid: u32) -> Result<Option<Self>, AppError> {
        let inn_users_k = [&u32_to_ivec(iid), &u32_to_ivec(uid)].concat();
        Ok(db
            .open_tree("inn_users")?
            .get(inn_users_k)?
            .map(|role| role.to_vec()[0].into()))
    }
}

impl From<u8> for InnRole {
    fn from(value: u8) -> Self {
        match value {
            1 => InnRole::Pending,
            2 => InnRole::Deny,
            3 => InnRole::Limited,
            4 => InnRole::Intern,
            5 => InnRole::Fellow,
            7 => InnRole::Mod,
            10 => InnRole::Super,
            _ => unreachable!(),
        }
    }
}

impl Display for InnRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl OutUserList {
    const fn new(uid: u32, username: String, about: String, role: u8, role_desc: String) -> Self {
        OutUserList {
            uid,
            username,
            about,
            role,
            role_desc,
        }
    }

    fn get_from_uids(db: &Db, index: Vec<u32>, n: usize) -> Result<Vec<Self>, AppError> {
        let mut users = Vec::with_capacity(n);
        for i in index {
            let user: User = get_one(db, "users", i)?;
            let user_desc = Role::from(user.role).to_string();
            let out_user_list =
                OutUserList::new(user.uid, user.username, user.about, user.role, user_desc);
            users.push(out_user_list);
        }
        Ok(users)
    }

    fn get_inn_users(
        db: &Db,
        iid: u32,
        role: Option<u8>,
        page_params: &ParamsPage,
    ) -> Result<Vec<Self>, AppError> {
        let mut users = Vec::with_capacity(page_params.n);
        let iter = db.open_tree("inn_users")?.scan_prefix(u32_to_ivec(iid));
        let iter = if page_params.is_desc {
            IterType::Rev(iter.rev())
        } else {
            IterType::Iter(iter)
        };

        for (idx, i) in iter.enumerate() {
            if idx < page_params.anchor {
                continue;
            }

            let (k, v) = i?;
            if let Some(role) = role {
                if v[0] == role {
                    let uid = u8_slice_to_u32(&k[4..]);
                    let user: User = get_one(db, "users", uid)?;
                    let inn_role = InnRole::from(v[0]).to_string();
                    let out_user_list =
                        OutUserList::new(user.uid, user.username, user.about, v[0], inn_role);
                    users.push(out_user_list);
                }
            } else {
                let uid = u8_slice_to_u32(&k[4..]);
                let user: User = get_one(db, "users", uid)?;
                let inn_role = InnRole::from(v[0]).to_string();
                let out_user_list =
                    OutUserList::new(user.uid, user.username, user.about, v[0], inn_role);
                users.push(out_user_list);
            }

            if users.len() >= page_params.n {
                break;
            }
        }
        Ok(users)
    }
}

/// url params: `user_list.html`
#[derive(Deserialize)]
pub(crate) struct ParamsUserList {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    filter: Option<String>,
    id: Option<u32>,
    role: Option<u8>,
}

/// `GET /user/list`
pub(crate) async fn user_list(
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<ParamsUserList>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut index;
    let count;
    let info;

    let mut is_admin = false;
    if let Some(ref claim) = claim {
        is_admin = Role::from(claim.role) == Role::Admin;
    }

    let mut users = Vec::with_capacity(n);

    if let Some(id) = params.id {
        let id_ivec = u32_to_ivec(id);
        match params.filter.as_deref() {
            Some("followers") => {
                let user: User = get_one(&DB, "users", id)?;
                info = (user.uid, user.username, false);
                index = get_ids_by_prefix(&DB, "user_followers", id_ivec, Some(&page_params))?;
                users = OutUserList::get_from_uids(&DB, index, n)?;
            }
            Some("following") => {
                let user: User = get_one(&DB, "users", id)?;
                info = (user.uid, user.username, false);
                index = get_ids_by_prefix(&DB, "user_following", id_ivec, Some(&page_params))?;
                users = OutUserList::get_from_uids(&DB, index, n)?;
            }
            Some("inn") => {
                let inn: Inn = get_one(&DB, "inns", id)?;
                let need_apply = inn.inn_type != "Public";
                info = (inn.iid, inn.inn_name, need_apply);
                is_admin = false;
                if let Some(ref claim) = claim {
                    is_admin = User::is_mod(&DB, claim.uid, inn.iid)?;
                }

                if inn.inn_type == "Private" && !is_admin {
                } else {
                    users = OutUserList::get_inn_users(&DB, id, params.role, &page_params)?;
                }
            }
            _ => return Ok(Redirect::to("/user/list").into_response()),
        }
    } else {
        info = (0, "all".to_owned(), false);
        if let Some(role) = params.role {
            let iter = DB.open_tree("users")?.iter();
            let iter = if page_params.is_desc {
                IterType::Rev(iter.rev())
            } else {
                IterType::Iter(iter)
            };
            for (idx, i) in iter.enumerate() {
                if idx < page_params.anchor {
                    continue;
                }

                let (_, v) = i?;
                let (user, _): (User, usize) = bincode::decode_from_slice(&v, standard())?;
                if user.role == role {
                    let user_desc = Role::from(user.role).to_string();
                    let out_user_list =
                        OutUserList::new(user.uid, user.username, user.about, role, user_desc);
                    users.push(out_user_list);
                }

                if users.len() >= page_params.n {
                    break;
                }
            }
        } else {
            count = get_count(&DB, "default", "users_count")?;
            let (start, end) = get_range(count, &page_params);
            index = (start..=end).map(|x| x as u32).collect();
            if is_desc {
                index.reverse();
            }
            users = OutUserList::get_from_uids(&DB, index, n)?;
        }
    }

    let has_unread = if let Some(ref claim) = claim {
        User::has_unread(&DB, claim.uid)?
    } else {
        false
    };
    let page_data = PageData::new("User list", &site_config, claim, has_unread);
    let page_user_list = PageUserList {
        page_data,
        users,
        anchor,
        n,
        is_desc,
        filter: params.filter,
        role: params.role,
        info,
        is_admin,
    };

    Ok(into_response(&page_user_list))
}

/// Form data: `/role/:id/:uid`
#[derive(Deserialize)]
pub(crate) struct FormRole {
    role: String,
}

/// `POST /role/:id/:uid`
pub(crate) async fn role_post(
    cookie: Option<TypedHeader<Cookie>>,
    Path((id, uid)): Path<(u32, u32)>,
    Form(form): Form<FormRole>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let target;
    match id.cmp(&0) {
        Ordering::Greater => {
            let inn_role = InnRole::get(&DB, id, claim.uid)?.ok_or(AppError::Unauthorized)?;
            if inn_role < InnRole::Mod {
                return Err(AppError::Unauthorized);
            }

            let inn_users_k = [&u32_to_ivec(id), &u32_to_ivec(uid)].concat();

            let old_inn_role = InnRole::get(&DB, id, uid)?;

            // protect super
            if let Some(ref old) = old_inn_role {
                if old > &inn_role {
                    return Err(AppError::Unauthorized);
                }

                if old == &InnRole::Pending && form.role != "Pending" {
                    DB.open_tree("inn_apply")?.remove(&inn_users_k)?;
                }
            }

            let inn_role: u8 = match form.role.as_str() {
                "Pending" => {
                    DB.open_tree("inn_apply")?.insert(&inn_users_k, &[])?;
                    1
                }
                "Deny" => 2,
                "Limited" => 3,
                "Intern" => 4,
                "Fellow" => 5,
                "Mod" => 7,
                "Super" => {
                    // only super can lift others to super
                    if inn_role != InnRole::Super {
                        return Err(AppError::Unauthorized);
                    }
                    10
                }
                _ => unreachable!(),
            };

            if old_inn_role != Some(inn_role.into()) {
                DB.open_tree("inn_users")?
                    .insert(&inn_users_k, &[inn_role])?;

                let user_inns_k = [&u32_to_ivec(uid), &u32_to_ivec(id)].concat();
                if inn_role >= 3 {
                    DB.open_tree("user_inns")?.insert(&user_inns_k, &[])?;
                } else {
                    DB.open_tree("user_inns")?.remove(&user_inns_k)?;
                }

                if inn_role >= 7 {
                    DB.open_tree("mod_inns")?.insert(&user_inns_k, &[])?;
                } else {
                    DB.open_tree("mod_inns")?.remove(&user_inns_k)?;
                }

                add_notification(&DB, uid, NtType::InnNotification, inn_role as u32, id)?;
            }

            target = format!("/user/list?filter=inn&id={id}");
        }
        Ordering::Equal => {
            if Role::from(claim.role) != Role::Admin {
                return Err(AppError::Unauthorized);
            }

            let mut user: User = get_one(&DB, "users", uid)?;
            let role = match form.role.as_str() {
                "Admin" => 255,
                "Senior" => 100,
                "Standard" => 10,
                "Banned" => 0,
                _ => unreachable!(),
            };

            if user.role != role {
                user.role = role;
                set_one(&DB, "users", uid, &user)?;
                Claim::update_role(&DB, uid)?;

                add_notification(&DB, uid, NtType::SiteNotification, role as u32, 0)?;
            }
            target = "/user/list".to_string();
        }
        Ordering::Less => unreachable!(),
    }

    Ok(Redirect::to(&target))
}

/// Form data: `/user/setting`
#[derive(Deserialize, Validate)]
pub(crate) struct FormUser {
    #[validate(length(min = 1, max = 32))]
    username: String,
    #[validate(length(max = 1024))]
    about: String,
    #[validate(length(max = 256))]
    url: String,
    home_page: u8,
}

/// Page data: `user_setting.html`
#[derive(Template)]
#[template(path = "user_setting.html")]
struct PageUserSetting<'a> {
    page_data: PageData<'a>,
    uid: u32,
    username: String,
    url: String,
    about: String,
    sessions: Vec<String>,
    home_page: u8,
}

/// `GET /user/setting`
pub(crate) async fn user_setting(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let user: User = get_one(&DB, "users", claim.uid)?;

    let mut sessions = Vec::new();
    for i in DB.open_tree("sessions")?.iter() {
        let (_, v) = i?;
        let (claim2, _): (Claim, _) = bincode::decode_from_slice(&v, standard())?;
        if claim2.uid == claim.uid {
            sessions.push(claim2.session_id);
        }
    }

    let home_page = DB
        .open_tree("home_pages")?
        .get(u32_to_ivec(claim.uid))?
        .map_or(0, |hp| hp[0]);

    let has_unread = User::has_unread(&DB, claim.uid)?;
    let page_user_setting = PageUserSetting {
        uid: claim.uid,
        page_data: PageData::new("setting", &site_config, Some(claim), has_unread),
        username: user.username,
        about: user.about,
        url: user.url,
        sessions,
        home_page,
    };

    Ok(into_response(&page_user_setting))
}

/// Page data: `reset.html`
#[derive(Template)]
#[template(path = "reset.html")]
struct PageReset<'a> {
    page_data: PageData<'a>,
}

/// `GET /user/reset`
pub(crate) async fn reset(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;

    if let Some(cookie) = cookie {
        let claim = Claim::get(&DB, &cookie, &site_config);
        if claim.is_some() {
            return Ok(Redirect::to("/user/setting").into_response());
        }
    };

    let page_data = PageData::new("Forgot password", &site_config, None, false);
    let page_reset = PageReset { page_data };
    Ok(into_response(&page_reset))
}

/// Form data: `/user/setting`
#[derive(Deserialize, Validate)]
pub(crate) struct FormReset {
    username: String,
    recovery_code: String,
    #[validate(must_match(other = "password2", message = "Two passwords do not match"))]
    password: String,
    #[validate(length(min = 7))]
    password2: String,
}

/// `POST /user/reset`
pub(crate) async fn reset_post(
    cookie: Option<TypedHeader<Cookie>>,
    Form(input): Form<FormReset>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;

    if let Some(cookie) = cookie {
        let claim = Claim::get(&DB, &cookie, &site_config);
        if claim.is_some() {
            return Ok(Redirect::to("/user/setting").into_response());
        }
    };

    let uid = match input.username.parse::<u32>() {
        Ok(uid) => uid,
        Err(_) => get_id_by_name(&DB, "usernames", &input.username)?.ok_or(AppError::NotFound)?,
    };

    let mut user: User = get_one(&DB, "users", uid)?;
    if let Some(ref recovery_hash) = user.recovery_hash {
        if check_password(&input.recovery_code, recovery_hash) {
            user.password_hash = generate_password_hash(&input.password);
            set_one(&DB, "users", uid, &user)?;

            return Ok(Redirect::to("/signin").into_response());
        };
    }

    Err(AppError::NotFound)
}

/// `GET /user/remove/:session_id`
pub(crate) async fn remove_session(
    cookie: Option<TypedHeader<Cookie>>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    DB.open_tree("sessions")?.remove(session_id)?;
    Ok(Redirect::to("/user/setting"))
}

/// `POST /user/setting`
pub(crate) async fn user_setting_post(
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormUser>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let mut user: User = get_one(&DB, "users", claim.uid)?;

    let username = clean_html(&input.username);
    if !is_valid_name(&username) {
        return Err(AppError::NameInvalid);
    }

    let username = username.trim();
    let username_key = username.replace(' ', "_").to_lowercase();

    let username_tree = DB.open_tree("usernames")?;
    if let Some(v) = username_tree.get(&username_key)? {
        if ivec_to_u32(&v) != claim.uid {
            return Err(AppError::NameExists);
        }
    }

    if user.username != username {
        username_tree.remove(user.username)?;
        username_tree.insert(username_key, u32_to_ivec(user.uid))?;
    }

    user.username = username.to_owned();
    user.about = clean_html(&input.about);
    user.url = clean_html(&input.url);
    DB.open_tree("home_pages")?
        .insert(u32_to_ivec(user.uid), &[input.home_page])?;
    set_one(&DB, "users", claim.uid, &user)?;

    let target = format!("/user/{}", claim.uid);
    Ok(Redirect::to(&target))
}

/// Form data: `/user/setting`
#[derive(Deserialize, Validate)]
pub(crate) struct FormPassword {
    old_password: String,
    #[validate(must_match(other = "password2", message = "Two passwords do not match"))]
    password: String,
    #[validate(length(min = 7))]
    password2: String,
}

/// `POST /user/password`
pub(crate) async fn user_password_post(
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormPassword>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let mut user: User = get_one(&DB, "users", claim.uid)?;

    if check_password(&input.old_password, &user.password_hash) {
        let password_hash = generate_password_hash(&input.password);
        user.password_hash = password_hash;
        set_one(&DB, "users", claim.uid, &user)?;
        Ok(Redirect::to("/signout"))
    } else {
        sleep(Duration::from_secs(1)).await;
        Err(AppError::WrongPassword)
    }
}

pub(crate) const COOKIE_NAME: &str = "id";

/// Form data: `/signin`
#[derive(Deserialize)]
pub(crate) struct FormSignin {
    username: String,
    password: String,
    remember: String,
}

/// Page data: `signin.html`
#[derive(Template)]
#[template(path = "signin.html")]
struct PageSignin<'a> {
    page_data: PageData<'a>,
}

/// `GET /signin`
pub(crate) async fn signin(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));
    if claim.is_some() {
        let redirect = Redirect::to("/");
        return Ok(redirect.into_response());
    }
    let page_data = PageData::new("Sign in", &site_config, claim, false);

    let page_signin = PageSignin { page_data };
    Ok(into_response(&page_signin))
}

/// `POST /signin`
pub(crate) async fn signin_post(Form(input): Form<FormSignin>) -> impl IntoResponse {
    let uid = match input.username.parse::<u32>() {
        Ok(uid) => uid,
        Err(_) => {
            get_id_by_name(&DB, "usernames", &input.username)?.ok_or(AppError::WrongPassword)?
        }
    };
    let user: User = get_one(&DB, "users", uid)?;
    if check_password(&input.password, &user.password_hash) {
        let site_config = SiteConfig::get(&DB)?;
        if site_config.read_only && Role::from(user.role) != Role::Admin {
            return Err(AppError::ReadOnly);
        }

        let mut headers = HeaderMap::new();
        let cookie = Claim::generate_cookie(&DB, user, &input.remember)?;
        headers.insert(SET_COOKIE, cookie.parse().unwrap());

        if headers.is_empty() {
            return Err(AppError::WrongPassword);
        }

        Ok((headers, Redirect::to("/")))
    } else {
        sleep(Duration::from_secs(1)).await;
        Err(AppError::WrongPassword)
    }
}

/// Form data: `/signup`
#[derive(Deserialize, Validate)]
pub(crate) struct FormSignup {
    #[validate(length(min = 1, max = 32))]
    username: String,
    #[validate(must_match(other = "password2", message = "Two passwords do not match"))]
    password: String,
    #[validate(length(min = 7))]
    password2: String,
    captcha_id: String,
    captcha_value: String,
}

/// Page data: `signup.html`
#[derive(Template)]
#[template(path = "signup.html")]
struct PageSignup<'a> {
    page_data: PageData<'a>,
    captcha_id: String,
    captcha_image: String,
}

/// `GET /signup`
pub(crate) async fn signup() -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    if site_config.read_only {
        return Err(AppError::ReadOnly);
    }
    let page_data = PageData::new("Sign up", &site_config, None, false);

    let captcha_difficulty = match site_config.captcha_difficulty.as_str() {
        "Easy" => Difficulty::Easy,
        "Medium" => Difficulty::Medium,
        "Hard" => Difficulty::Hard,
        _ => unreachable!(),
    };

    let captcha_name = match site_config.captcha_name.as_str() {
        "Amelia" => CaptchaName::Amelia,
        "Lucy" => CaptchaName::Lucy,
        "Mila" => CaptchaName::Mila,
        _ => unreachable!(),
    };

    let captcha = captcha::by_name(captcha_difficulty, captcha_name);
    let captcha_id = generate_nanoid_ttl(60);
    DB.open_tree("captcha")?
        .insert(&captcha_id, &*captcha.chars_as_string())?;

    let page_signup = PageSignup {
        page_data,
        captcha_id,
        captcha_image: captcha.as_base64().unwrap(),
    };
    Ok(into_response(&page_signup))
}

/// `POST /signup`
pub(crate) async fn signup_post(
    ValidatedForm(input): ValidatedForm<FormSignup>,
) -> Result<impl IntoResponse, AppError> {
    let username = clean_html(&input.username);
    if !is_valid_name(&username) {
        return Err(AppError::NameInvalid);
    }

    let captcha_char = DB
        .open_tree("captcha")?
        .remove(&input.captcha_id)?
        .ok_or(AppError::CaptchaError)?;
    let captcha_char = String::from_utf8(captcha_char.to_vec()).unwrap();

    if captcha_char != input.captcha_value {
        return Err(AppError::CaptchaError);
    }

    let username = username.trim();
    let username_key = username.replace(' ', "_").to_lowercase();
    let usernames_tree = DB.open_tree("usernames")?;
    if usernames_tree.contains_key(&username_key)? {
        return Err(AppError::NameExists);
    }

    let password_hash = generate_password_hash(&input.password);
    let uid = incr_id(&DB, "users_count")?;

    let avatar = format!("{}/{}.png", &CONFIG.avatars_path, uid);
    Identicon::new(&generate_salt()).image().save(avatar)?;

    let created_at = Utc::now().timestamp();
    let role = if uid == 1 {
        Role::Admin
    } else if uid <= 500 {
        Role::Senior
    } else {
        Role::Standard
    };
    let user = User {
        uid,
        username: username.to_owned(),
        password_hash,
        created_at,
        role: role as u8,
        ..Default::default()
    };

    set_one(&DB, "users", uid, &user)?;
    usernames_tree.insert(username_key, u32_to_ivec(uid))?;

    let cookie = Claim::generate_cookie(&DB, user, "4h")?;
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());
    Ok((headers, Redirect::to("/")))
}

/// `GET /signout`
pub(crate) async fn signout(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(cookie) = cookie {
        let session = cookie.get(COOKIE_NAME);
        if let Some(session) = session {
            DB.open_tree("sessions")?.remove(session)?;
        }
    }

    let cookie = format!(
        "{COOKIE_NAME}=deleted; SameSite=Strict; Path=/; Secure; HttpOnly; expires=Thu, 01 Jan 1970 00:00:00 GMT"
    );
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());
    Ok((headers, Redirect::to("/")))
}

#[derive(Template)]
#[template(path = "show_recovery.html")]
struct PageShowRecovery<'a> {
    page_data: PageData<'a>,
    recovery_code: String,
}

/// Form data: `/user/recovery`
#[derive(Deserialize, Validate)]
pub(crate) struct FormRecoverySet {
    #[validate(length(min = 7))]
    password: String,
}

/// `POST /user/recovery`
pub(crate) async fn user_recovery_code(
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormRecoverySet>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let mut user: User = get_one(&DB, "users", claim.uid)?;

    if check_password(&input.password, &user.password_hash) {
        let recovery_code = gen_password();
        user.recovery_hash = Some(generate_password_hash(&recovery_code));
        set_one(&DB, "users", claim.uid, &user)?;
        let has_unread = User::has_unread(&DB, claim.uid)?;
        let page_data = PageData::new("Recovery code", &site_config, Some(claim), has_unread);
        let page_show_recovery = PageShowRecovery {
            page_data,
            recovery_code,
        };

        Ok(into_response(&page_show_recovery))
    } else {
        sleep(Duration::from_secs(1)).await;
        Err(AppError::WrongPassword)
    }
}

fn gen_password() -> String {
    let mut rng = thread_rng();
    let n: u8 = rng.gen_range(10..=24);
    const CHARSET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ\
                            abcdefghijklmnopqrstuvwxyz\
                            0123456789)(*&^%$#@!~";
    let password: String = (0..n)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect();
    password
}

/// generate salt
///
/// <https://rust-lang-nursery.github.io/rust-cookbook/cryptography/encryption.html>
fn generate_salt() -> [u8; 64] {
    let rng = rand::SystemRandom::new();
    let mut salt = [0_u8; 64];
    rng.fill(&mut salt).unwrap();
    salt
}

const N_ITER: Option<std::num::NonZeroU32> = NonZeroU32::new(100_000);

/// return hashed password and salt
fn generate_password_hash(password: &str) -> String {
    let n = N_ITER.unwrap();
    let salt = generate_salt();
    let mut pbkdf2_hash = [0_u8; 64];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA512,
        n,
        &salt,
        password.as_bytes(),
        &mut pbkdf2_hash,
    );

    BASE64.encode(&[&pbkdf2_hash[..], &salt[..]].concat())
}

/// check password
fn check_password(password: &str, password_hash: &str) -> bool {
    let n = N_ITER.unwrap();
    let decoded = BASE64.decode(password_hash.as_bytes()).unwrap();

    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA512,
        n,
        &decoded[64..],
        password.as_bytes(),
        &decoded[0..64],
    )
    .is_ok()
}

impl Claim {
    /// extract session id from request header, then query session tree to get a Claim.
    /// If session id is not found or expired, return None.
    ///
    /// ### user pageviews data
    /// Keep three day pageviews data. For privacy, the hour and minute has been striped, just date kept.
    pub(super) fn get(
        db: &Db,
        TypedHeader(cookie): &TypedHeader<Cookie>,
        site_config: &SiteConfig,
    ) -> Option<Self> {
        let session = cookie.get(COOKIE_NAME)?;
        let timestamp = session.split_once('_')?.0;
        let tree = &db.open_tree("sessions").ok()?;
        let timestamp = i64::from_str_radix(timestamp, 16).ok()?;
        let now = Utc::now();

        if timestamp < now.timestamp() {
            tree.remove(session).ok()?;
            return None;
        }

        let v = tree.get(session).ok()??;
        let (claim, _): (Claim, usize) = bincode::decode_from_slice(&v, standard()).ok()?;

        if site_config.read_only && Role::from(claim.role) != Role::Admin {
            return None;
        }

        if Role::from(claim.role) == Role::Banned {
            return None;
        }
        Some(claim)
    }

    pub(super) fn update_last_write(mut self, db: &Db) -> Result<(), AppError> {
        self.last_write = Utc::now().timestamp();
        set_one_with_key(db, "sessions", &self.session_id, &self)?;

        Ok(())
    }

    fn update_role(db: &Db, uid: u32) -> Result<(), AppError> {
        let user: User = get_one(db, "users", uid)?;

        let session_tree = db.open_tree("sessions")?;
        for i in session_tree.iter() {
            let (k, v) = i?;
            let (mut claim, _): (Claim, _) = bincode::decode_from_slice(&v, standard())?;
            if claim.uid == uid {
                claim.role = user.role;
                set_one_with_key(db, "sessions", k, &claim)?;
            }
        }

        Ok(())
    }

    /// generate a Claim from user and store it in session tree, then return a cookie with a session id.
    fn generate_cookie(db: &Db, user: User, expiry: &str) -> Result<String, AppError> {
        if user.role == 0 {
            return Err(AppError::Banned);
        }
        let seconds = expire_seconds(expiry);
        let now = Utc::now().timestamp();
        let session_id = generate_nanoid_ttl(seconds);

        let claim = Claim {
            uid: user.uid,
            username: user.username,
            role: user.role,
            last_write: now,
            session_id: session_id.clone(),
        };

        set_one_with_key(db, "sessions", &session_id, &claim)?;

        let cookie = format!(
            "{COOKIE_NAME}={session_id}; SameSite=Strict; Path=/; Secure; HttpOnly; Max-Age={seconds}"
        );
        Ok(cookie)
    }
}

/// Compute expire time in seconds from a string like "1h", "1day".
fn expire_seconds(expiry: &str) -> i64 {
    match expiry {
        "30m" => 1800,
        "1h" => 3600,
        "2h" => 2 * 3600,
        "4h" => 4 * 3600,
        "8h" => 8 * 3600,
        "12h" => 12 * 3600,
        "1day" => 24 * 3600,
        "2days" => 2 * 24 * 3600,
        "3days" => 3 * 24 * 3600,
        "4days" => 4 * 24 * 3600,
        "5days" => 5 * 24 * 3600,
        "1week" => 7 * 24 * 3600,
        "2weeks" => 2 * 7 * 24 * 3600,
        _ => 4 * 3600,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_salt_len() {
        let salt = generate_salt();
        assert_eq!(salt.len(), 64);
    }

    #[test]
    fn test_check_password() {
        let password_hash = generate_password_hash("password");
        assert!(check_password("password", &password_hash));

        let password_hash2 = generate_password_hash("password");
        assert!(check_password("password", &password_hash2));

        // must generate different password_hash and salt with the same password
        assert_ne!(password_hash, password_hash2);
    }
}
