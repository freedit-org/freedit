//! ## [User] sign up/in/out, user profile/list controller

use super::{
    generate_nanoid_expire, get_count_by_prefix, get_ids_by_prefix, get_inn_role, get_one,
    get_range, get_site_config, get_uid_by_name, incr_id, into_response, is_mod, timestamp_to_date,
    u32_to_ivec, u8_slice_to_u32, user_stats, Claim, Inn, IterType, PageData, ParamsPage,
    SiteConfig, User, ValidatedForm,
};
use crate::{config::CONFIG, controller::get_count, error::AppError};
use askama::Template;
use axum::{
    extract::{Form, Path, Query, State},
    headers::Cookie,
    http::{header::SET_COOKIE, HeaderMap},
    response::{IntoResponse, Redirect},
    TypedHeader,
};
use bincode::config::standard;
use captcha::{CaptchaName, Difficulty};
use data_encoding::BASE64;
use hash_avatar::Generator;
use ring::{
    pbkdf2,
    rand::{self, SecureRandom},
};
use serde::Deserialize;
use sled::Db;
use std::{cmp::Ordering, num::NonZeroU32, time::Duration};
use time::OffsetDateTime;
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
    user_comments_count: usize,
    user_following_count: usize,
    user_followers_count: usize,
    has_followed: Option<bool>,
}

/// Vec data: user
struct OutUser {
    uid: u32,
    username: String,
    about: String,
    role: u8,
    url: String,
    created_at: String,
}

/// `GET /user/:uid`
pub(crate) async fn user(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));
    let user: User = get_one(&db, "users", uid)?;
    let out_user = OutUser {
        uid: user.uid,
        username: user.username,
        about: user.about,
        role: user.role,
        url: user.url,
        created_at: timestamp_to_date(user.created_at)?,
    };
    let uid_ivec = u32_to_ivec(uid);
    let user_solos_count = get_count_by_prefix(&db, "user_solos", &uid_ivec)?;
    let user_posts_count = get_count_by_prefix(&db, "user_posts", &uid_ivec)?;
    let user_comments_count = get_count_by_prefix(&db, "user_comments", &uid_ivec)?;
    let user_following_count = get_count_by_prefix(&db, "user_following", &u32_to_ivec(uid))?;
    let user_followers_count = get_count_by_prefix(&db, "user_followers", &u32_to_ivec(uid))?;

    let has_followed = if let Some(ref claim) = claim {
        if claim.uid != uid {
            let following_k = [&u32_to_ivec(claim.uid), &u32_to_ivec(uid)].concat();
            Some(db.open_tree("user_following")?.contains_key(following_k)?)
        } else {
            None
        }
    } else {
        None
    };

    let title = format!("{}-{}", out_user.username, out_user.uid);
    let page_data = PageData::new(&title, &site_config.site_name, claim, false);
    let page_user = PageUser {
        page_data,
        user: out_user,
        user_solos_count,
        user_posts_count,
        user_comments_count,
        user_following_count,
        user_followers_count,
        has_followed,
    };

    Ok(into_response(&page_user, "html"))
}

/// `GET /user/:uid/follow` follow user
pub(crate) async fn user_follow(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let following_k = [&u32_to_ivec(claim.uid), &u32_to_ivec(uid)].concat();
    let followers_k = [&u32_to_ivec(uid), &u32_to_ivec(claim.uid)].concat();

    let user_following_tree = db.open_tree("user_following")?;
    let user_followers_tree = db.open_tree("user_followers")?;

    if user_following_tree.contains_key(&following_k)? {
        user_following_tree.remove(&following_k)?;
        user_followers_tree.remove(&followers_k)?;
    } else {
        user_following_tree.insert(&following_k, &[])?;
        user_followers_tree.insert(&followers_k, &[])?;
    }

    let target = format!("/user/{}", uid);
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
}

impl OutUserList {
    const fn new(uid: u32, username: String, about: String, role: u8) -> Self {
        OutUserList {
            uid,
            username,
            about,
            role,
        }
    }

    fn get_from_uids(db: &Db, index: Vec<u32>, n: usize) -> Result<Vec<Self>, AppError> {
        let mut users = Vec::with_capacity(n);
        for i in index {
            let user: User = get_one(db, "users", i)?;
            let out_user_list = OutUserList::new(user.uid, user.username, user.about, user.role);
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
            if idx >= page_params.anchor + page_params.n {
                break;
            }
            let (k, v) = i?;
            if let Some(role) = role {
                if v[0] == role {
                    let uid = u8_slice_to_u32(&k[8..]);
                    let user: User = get_one(db, "users", uid)?;
                    let out_user_list = OutUserList::new(user.uid, user.username, user.about, v[0]);
                    users.push(out_user_list);
                }
            } else {
                let uid = u8_slice_to_u32(&k[8..]);
                let user: User = get_one(db, "users", uid)?;
                let out_user_list = OutUserList::new(user.uid, user.username, user.about, v[0]);
                users.push(out_user_list);
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
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<ParamsUserList>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));

    let n = site_config.per_page;
    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let page_params = ParamsPage { anchor, n, is_desc };

    let mut index;
    let count;
    let info;

    let mut is_admin = false;
    if let Some(ref claim) = claim {
        is_admin = claim.role == u8::MAX;
    }

    let mut users = Vec::with_capacity(n);

    if let Some(id) = params.id {
        let id_ivec = u32_to_ivec(id);
        match params.filter.as_deref() {
            Some("followers") => {
                let user: User = get_one(&db, "users", id)?;
                info = (user.uid, user.username, false);
                index = get_ids_by_prefix(&db, "user_followers", id_ivec, Some(&page_params))?;
                users = OutUserList::get_from_uids(&db, index, n)?;
            }
            Some("following") => {
                let user: User = get_one(&db, "users", id)?;
                info = (user.uid, user.username, false);
                index = get_ids_by_prefix(&db, "user_following", id_ivec, Some(&page_params))?;
                users = OutUserList::get_from_uids(&db, index, n)?;
            }
            Some("inn") => {
                let inn: Inn = get_one(&db, "inns", id)?;
                let need_apply = inn.inn_type != "Public";
                info = (inn.iid, inn.inn_name, need_apply);
                is_admin = false;
                if let Some(ref claim) = claim {
                    is_admin = is_mod(&db, claim.uid, inn.iid)?;
                }

                if inn.inn_type == "Private" && !is_admin {
                } else {
                    users = OutUserList::get_inn_users(&db, id, params.role, &page_params)?;
                }
            }
            _ => return Ok(Redirect::to("/user/list").into_response()),
        }
    } else {
        info = (0, "all".to_owned(), false);
        if let Some(role) = params.role {
            let iter = db.open_tree("users")?.iter();
            let iter = if page_params.is_desc {
                IterType::Rev(iter.rev())
            } else {
                IterType::Iter(iter)
            };
            for (idx, i) in iter.enumerate() {
                if idx < page_params.anchor {
                    continue;
                }
                if idx >= page_params.anchor + page_params.n {
                    break;
                }
                let (_, v) = i?;
                let (user, _): (User, usize) = bincode::decode_from_slice(&v, standard())?;
                if user.role == role {
                    let out_user_list = OutUserList::new(user.uid, user.username, user.about, role);
                    users.push(out_user_list);
                }
            }
        } else {
            count = get_count(&db, "default", "users_count")?;
            let (start, end) = get_range(count, &page_params);
            index = (start..=end).map(|x| x as u32).collect();
            if is_desc {
                index.reverse();
            }
            users = OutUserList::get_from_uids(&db, index, n)?;
        }
    }

    let page_data = PageData::new("User list", &site_config.site_name, claim, false);
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

    Ok(into_response(&page_user_list, "html"))
}

/// Form data: `/role/:id/:uid`
#[derive(Deserialize)]
pub struct FormRole {
    role: String,
}

/// `POST /role/:id/:uid`
pub(crate) async fn role_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((id, uid)): Path<(u32, u32)>,
    Form(form): Form<FormRole>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let target;
    match id.cmp(&0) {
        Ordering::Greater => {
            let inn_role = get_inn_role(&db, id, claim.uid)?.ok_or(AppError::Unauthorized)?;
            if inn_role < 8 {
                return Err(AppError::Unauthorized);
            }

            let inn_users_k = [&u32_to_ivec(id), &u32_to_ivec(uid)].concat();

            // protect super
            if let Some(user_inn_role) = get_inn_role(&db, id, uid)? {
                if user_inn_role > inn_role {
                    return Err(AppError::Unauthorized);
                }

                if user_inn_role == 1 && form.role != "Pending" {
                    db.open_tree("inn_apply")?.remove(&inn_users_k)?;
                }
            }

            let inn_role: u8 = match form.role.as_str() {
                "Pending" => {
                    db.open_tree("inn_apply")?.insert(&inn_users_k, &[])?;
                    1
                }
                "Deny" => 2,
                "Limited" => 3,
                "Intern" => 4,
                "Fellow" => 5,
                "Mod" => 8,
                "Super" => {
                    // only super can lift others to super
                    if inn_role != 10 {
                        return Err(AppError::Unauthorized);
                    }
                    10
                }
                _ => unreachable!(),
            };

            db.open_tree("inn_users")?
                .insert(&inn_users_k, &[inn_role])?;

            let user_inns_k = [&u32_to_ivec(uid), &u32_to_ivec(id)].concat();
            if inn_role >= 3 {
                db.open_tree("user_inns")?.insert(&user_inns_k, &[])?;
            } else {
                db.open_tree("user_inns")?.remove(&user_inns_k)?;
            }

            if inn_role >= 8 {
                db.open_tree("mod_inns")?.insert(&user_inns_k, &[])?;
            } else {
                db.open_tree("mod_inns")?.remove(&user_inns_k)?;
            }

            target = format!("/user/list?filter=inn&id={}", id);
        }
        Ordering::Equal => {
            if !claim.role == u8::MAX {
                return Err(AppError::Unauthorized);
            }

            let mut user: User = get_one(&db, "users", uid)?;
            user.role = match form.role.as_str() {
                "Admin" => 255,
                "Normal" => 10,
                "Banned" => 0,
                _ => unreachable!(),
            };
            let user_encode = bincode::encode_to_vec(&user, standard())?;
            db.open_tree("users")?
                .insert(&u32_to_ivec(uid), user_encode)?;

            Claim::update_role(&db, uid)?;
            target = "/user/list".to_string();
        }
        Ordering::Less => unreachable!(),
    }

    Ok(Redirect::to(&target))
}

/// Form data: `/user/setting`
#[derive(Deserialize, Validate)]
pub(crate) struct FormUser {
    #[validate(length(min = 1, max = 64))]
    username: String,
    #[validate(length(max = 1024))]
    about: String,
    #[validate(length(max = 256))]
    url: String,
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
}

/// `GET /user/setting`
pub(crate) async fn user_setting(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let user: User = get_one(&db, "users", claim.uid)?;

    let mut sessions = Vec::new();
    for i in db.open_tree("sessions")?.iter() {
        let (_, v) = i?;
        let (claim2, _): (Claim, _) = bincode::decode_from_slice(&v, standard())?;
        if claim2.uid == claim.uid {
            sessions.push(claim2.session_id);
        }
    }

    let page_user_setting = PageUserSetting {
        uid: claim.uid,
        page_data: PageData::new("setting", &site_config.site_name, Some(claim), false),
        username: user.username,
        about: user.about,
        url: user.url,
        sessions,
    };

    Ok(into_response(&page_user_setting, "html"))
}

/// `GET /user/remove/:session_id`
pub(crate) async fn remove_session(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(session_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    db.open_tree("sessions")?.remove(&session_id)?;
    Ok(Redirect::to("/user/setting"))
}

/// `POST /user/setting`
pub(crate) async fn user_setting_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormUser>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let mut user: User = get_one(&db, "users", claim.uid)?;

    if input.username.chars().next().unwrap().is_numeric() {
        return Err(AppError::UsernameInvalid);
    }
    if input.username.chars().any(char::is_control) {
        return Err(AppError::UsernameInvalid);
    }
    if input.username.contains(['@', '#']) {
        return Err(AppError::UsernameInvalid);
    }

    let tree = db.open_tree("usernames")?;
    if user.username != input.username {
        tree.remove(&user.username)?;
        tree.insert(&input.username, u32_to_ivec(user.uid))?;
    }

    user.username = input.username;
    user.about = input.about;
    user.url = input.url;
    let user_encode = bincode::encode_to_vec(&user, standard())?;
    db.open_tree("users")?
        .insert(u32_to_ivec(claim.uid), &*user_encode)?;

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
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormPassword>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let mut user: User = get_one(&db, "users", claim.uid)?;

    if check_password(&input.old_password, &user.salt, &user.password_hash) {
        let (password_hash, salt) = generate_password_hash(&input.password);
        user.password_hash = password_hash;
        user.salt = salt;
        let user_encode = bincode::encode_to_vec(&user, standard())?;
        db.open_tree("users")?
            .insert(u32_to_ivec(claim.uid), &*user_encode)?;
        Ok(Redirect::to("/signout"))
    } else {
        sleep(Duration::from_secs(1)).await;
        Err(AppError::WrongPassword)
    }
}

pub(crate) const COOKIE_NAME: &str = "__Host-id";

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
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    let claim = cookie.and_then(|cookie| Claim::get(&db, &cookie, &site_config));
    if claim.is_some() {
        let redirect = Redirect::to("/");
        return Ok(redirect.into_response());
    }
    let page_data = PageData::new("Sign in", &site_config.site_name, claim, false);

    let page_signin = PageSignin { page_data };
    Ok(into_response(&page_signin, "html"))
}

/// `POST /signin`
pub(crate) async fn signin_post(
    State(db): State<Db>,
    Form(input): Form<FormSignin>,
) -> impl IntoResponse {
    let uid = match input.username.parse::<u32>() {
        Ok(uid) => uid,
        Err(_) => get_uid_by_name(&db, &input.username)?.ok_or(AppError::WrongPassword)?,
    };
    let user: User = get_one(&db, "users", uid)?;
    if check_password(&input.password, &user.salt, &user.password_hash) {
        let site_config = get_site_config(&db)?;
        if site_config.read_only && user.role != u8::MAX {
            return Err(AppError::ReadOnly);
        }

        let mut headers = HeaderMap::new();
        let cookie = Claim::generate_cookie(&db, user, &input.remember)?;
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
pub struct FormSignup {
    #[validate(length(min = 1, max = 64))]
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
pub(crate) async fn signup(State(db): State<Db>) -> Result<impl IntoResponse, AppError> {
    let site_config = get_site_config(&db)?;
    if site_config.read_only {
        return Err(AppError::ReadOnly);
    }
    let page_data = PageData::new("Sign up", &site_config.site_name, None, false);

    let captcha = captcha::by_name(Difficulty::Easy, CaptchaName::Amelia);
    let captcha_id = generate_nanoid_expire(60);
    db.open_tree("captcha")?
        .insert(&captcha_id, &*captcha.chars_as_string())?;

    let page_signup = PageSignup {
        page_data,
        captcha_id,
        captcha_image: captcha.as_base64().unwrap(),
    };
    Ok(into_response(&page_signup, "html"))
}

/// `POST /signup`
pub(crate) async fn signup_post(
    State(db): State<Db>,
    ValidatedForm(input): ValidatedForm<FormSignup>,
) -> Result<impl IntoResponse, AppError> {
    if input.username.chars().next().unwrap().is_numeric() {
        return Err(AppError::UsernameInvalid);
    }
    if input.username.chars().any(char::is_control) {
        return Err(AppError::UsernameInvalid);
    }
    if input.username.contains(['@', '#']) {
        return Err(AppError::UsernameInvalid);
    }

    let captcha_char = db
        .open_tree("captcha")?
        .remove(&input.captcha_id)?
        .ok_or(AppError::CaptchaError)?;
    let captcha_char = String::from_utf8(captcha_char.to_vec()).unwrap();

    if captcha_char != input.captcha_value {
        return Err(AppError::CaptchaError);
    }

    let tree = db.open_tree("usernames")?;
    if tree.contains_key(&input.username)? {
        return Err(AppError::NameExists);
    }

    let (password_hash, salt) = generate_password_hash(&input.password);
    let uid = incr_id(&db, "users_count")?;

    let avatar = format!("{}/{}.png", &CONFIG.avatars_path, uid);
    match Generator::new().create().save_to_png(&avatar) {
        Ok(_) => (),
        Err(e) => {
            return Err(AppError::GenerateAvatarError(e));
        }
    }

    let created_at = OffsetDateTime::now_utc().unix_timestamp();
    let role = if uid == 1 { u8::MAX } else { 10 };
    let user = User {
        uid,
        username: input.username,
        salt,
        password_hash,
        created_at,
        role,
        ..Default::default()
    };

    let user_encode = bincode::encode_to_vec(&user, standard())?;
    let uid_ivec = u32_to_ivec(uid);
    db.open_tree("users")?.insert(&uid_ivec, user_encode)?;
    db.open_tree("usernames")?
        .insert(&user.username, &uid_ivec)?;

    let cookie = Claim::generate_cookie(&db, user, "4h")?;
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());
    Ok((headers, Redirect::to("/")))
}

/// `GET /signout`
pub(crate) async fn signout(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    if let Some(cookie) = cookie {
        let session = cookie.get(COOKIE_NAME);
        if let Some(session) = session {
            db.open_tree("sessions")?.remove(session)?;
        }
    }

    let cookie = format!(
        "{}=deleted; SameSite=Strict; Path=/; Secure; HttpOnly; expires=Thu, 01 Jan 1970 00:00:00 GMT",
        COOKIE_NAME
    );
    let mut headers = HeaderMap::new();
    headers.insert(SET_COOKIE, cookie.parse().unwrap());
    Ok((headers, Redirect::to("/")))
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
fn generate_password_hash(password: &str) -> (String, String) {
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
    let password_hash = BASE64.encode(&pbkdf2_hash);
    let salt = BASE64.encode(&salt);

    (password_hash, salt)
}

/// check password
fn check_password(password: &str, salt: &str, password_hash: &str) -> bool {
    let n = N_ITER.unwrap();

    pbkdf2::verify(
        pbkdf2::PBKDF2_HMAC_SHA512,
        n,
        &BASE64.decode(salt.as_bytes()).unwrap(),
        password.as_bytes(),
        &BASE64.decode(password_hash.as_bytes()).unwrap(),
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
        let now = OffsetDateTime::now_utc();

        if timestamp < now.unix_timestamp() {
            tree.remove(session).ok()?;
            return None;
        }

        let v = tree.get(session).ok()??;
        let (claim, _): (Claim, usize) = bincode::decode_from_slice(&v, standard()).ok()?;

        if site_config.read_only && claim.role != u8::MAX {
            return None;
        }

        if claim.role == 0 {
            return None;
        }
        user_stats(db, claim.uid, "view").ok()?;
        Some(claim)
    }

    pub(super) fn update_last_write(mut self, db: &Db) -> Result<(), AppError> {
        self.last_write = OffsetDateTime::now_utc().unix_timestamp();
        let claim_encode = bincode::encode_to_vec(&self, standard())?;
        db.open_tree("sessions")?
            .insert(&self.session_id, claim_encode)?;
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
                let claim_encode = bincode::encode_to_vec(&claim, standard())?;
                session_tree.insert(&k, claim_encode)?;
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
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let session_id = generate_nanoid_expire(seconds);

        let claim = Claim {
            uid: user.uid,
            username: user.username,
            role: user.role,
            last_write: now,
            session_id: session_id.clone(),
        };

        let claim_encode = bincode::encode_to_vec(&claim, standard())?;

        db.open_tree("sessions")?
            .insert(&session_id, claim_encode)?;

        let cookie = format!(
            "{}={}; SameSite=Strict; Path=/; Secure; HttpOnly; Max-Age={}",
            COOKIE_NAME, session_id, seconds
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
        _ => unreachable!(),
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
        let (password_hash, salt) = generate_password_hash("password");
        assert!(check_password("password", &salt, &password_hash));

        let (password_hash2, salt2) = generate_password_hash("password");
        assert!(check_password("password", &salt2, &password_hash2));

        // must generate different password_hash and salt with the same password
        assert_ne!(password_hash, password_hash2);
        assert_ne!(salt, salt2);
    }
}
