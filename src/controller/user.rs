//! ## [User] sign up/in/out, user profile/list controller

use super::{
    generate_nanoid_expire, get_count_by_prefix, get_ids_by_prefix, get_inn_status_by_prefix,
    get_one, get_range, get_site_config, get_uid_by_name, incr_id, into_response,
    timestamp_to_date, u64_to_ivec, Claim, Inn, PageData, ParamsPage, SiteConfig, User,
    ValidatedForm, SEP,
};
use crate::{controller::get_count, error::AppError};
use ::pbkdf2::{
    password_hash::{Ident, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Params, Pbkdf2,
};
use ::rand::rngs::OsRng;
use askama::Template;
use axum::{
    extract::{Form, Path, Query},
    headers::Cookie,
    http::{header::SET_COOKIE, HeaderMap},
    response::{IntoResponse, Redirect},
    Extension, TypedHeader,
};
use bincode::config::standard;
use captcha::{CaptchaName, Difficulty};
use hash_avatar::Generator;
use serde::Deserialize;
use sled::Db;
use std::{cmp::Ordering, time::Duration};
use time::{OffsetDateTime, Time};
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
    uid: u64,
    username: String,
    about: String,
    role: u8,
    url: String,
    created_at: String,
}

/// `GET /user/:uid`
pub(crate) async fn user(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u64>,
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
    let uid_ivec = u64_to_ivec(uid);
    let user_solos_count = get_count(&db, "user_solos_count", &uid_ivec)?;
    let user_posts_count = get_count(&db, "user_posts_count", &uid_ivec)?;
    let user_comments_count = get_count(&db, "user_comments_count", &uid_ivec)?;
    let user_following_count = get_count_by_prefix(&db, "user_following", &u64_to_ivec(uid))?;
    let user_followers_count = get_count_by_prefix(&db, "user_followers", &u64_to_ivec(uid))?;

    let has_followed = if let Some(ref claim) = claim {
        if claim.uid != uid {
            let following_k = [&u64_to_ivec(claim.uid), &SEP, &u64_to_ivec(uid)].concat();
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
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u64>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let following_k = [&u64_to_ivec(claim.uid), &SEP, &u64_to_ivec(uid)].concat();
    let followers_k = [&u64_to_ivec(uid), &SEP, &u64_to_ivec(claim.uid)].concat();

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
    info: (u64, String),
    is_admin: bool,
}

/// Vec data: user list
struct OutUserList {
    uid: u64,
    username: String,
    about: String,
    role: u8,
}

/// url params: `user_list.html`
#[derive(Deserialize)]
pub(crate) struct ParamsUserList {
    anchor: Option<usize>,
    is_desc: Option<bool>,
    filter: Option<String>,
    id: Option<u64>,
}

/// `GET /user/list`
pub(crate) async fn user_list(
    Extension(db): Extension<Db>,
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
    let filter;
    let info;
    let mut status = vec![];
    let mut is_admin = false;
    if let Some(ref claim) = claim {
        is_admin = claim.role == u8::MAX;
    }

    if let Some(id) = params.id {
        let id_ivec = u64_to_ivec(id);
        match params.filter.as_deref() {
            Some("followers") => {
                let user: User = get_one(&db, "users", id)?;
                info = (user.uid, user.username);
                index = get_ids_by_prefix(&db, "user_followers", id_ivec, Some(&page_params))?;
                filter = Some("followers".to_owned());
            }
            Some("following") => {
                let user: User = get_one(&db, "users", id)?;
                info = (user.uid, user.username);
                index = get_ids_by_prefix(&db, "user_following", id_ivec, Some(&page_params))?;
                filter = Some("following".to_owned());
            }
            Some("inn") => {
                let inn: Inn = get_one(&db, "inns", id)?;
                info = (inn.iid, inn.inn_name);
                (index, status) =
                    get_inn_status_by_prefix(&db, "inn_users", id_ivec, Some(&page_params))?;
                filter = Some("inn".to_owned());
                is_admin = false;
                if let Some(ref claim) = claim {
                    is_admin = inn.mods.contains(&claim.uid);
                }
                if inn.inn_type == "Public" {
                    is_admin = false;
                }
            }
            _ => {
                info = (0, "all".to_owned());
                count = get_count(&db, "default", "users_count")?;
                let (start, end) = get_range(count, &page_params);
                index = (start..=end).map(|x| x as u64).collect();
                if is_desc {
                    index.reverse();
                }
                filter = None;
            }
        }
    } else {
        info = (0, "all".to_owned());
        count = get_count(&db, "default", "users_count")?;
        let (start, end) = get_range(count, &page_params);
        index = (start..=end).map(|x| x as u64).collect();
        if is_desc {
            index.reverse();
        }
        filter = None;
    }

    let mut users = Vec::with_capacity(n);
    for (idx, i) in index.into_iter().enumerate() {
        let user: User = get_one(&db, "users", i)?;
        let role = if params.filter.as_deref() == Some("inn") {
            status[idx]
        } else {
            user.role
        };
        let out_user_list = OutUserList {
            uid: user.uid,
            username: user.username,
            about: user.about,
            role,
        };
        users.push(out_user_list);
    }

    let page_data = PageData::new("User list", &site_config.site_name, claim, false);
    let page_user_list = PageUserList {
        page_data,
        users,
        anchor,
        n,
        is_desc,
        filter,
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
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Path((id, uid)): Path<(u64, u64)>,
    Form(form): Form<FormRole>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let target;
    match id.cmp(&0) {
        Ordering::Greater => {
            let iin: Inn = get_one(&db, "inns", id)?;
            if !iin.mods.contains(&claim.uid) {
                return Err(AppError::Unauthorized);
            }

            let inn_user_v: u8 = match form.role.as_str() {
                "Pending" => 1,
                "Deny" => 2,
                "Accept" => 3,
                _ => unreachable!(),
            };
            let inn_users_k = [&u64_to_ivec(id), &SEP, &u64_to_ivec(uid)].concat();
            db.open_tree("inn_users")?
                .insert(&inn_users_k, &[inn_user_v])?;

            let user_inns_k = [&u64_to_ivec(uid), &SEP, &u64_to_ivec(id)].concat();
            if inn_user_v == 3 {
                db.open_tree("user_inns")?.insert(&user_inns_k, &[])?;
            } else {
                db.open_tree("user_inns")?.remove(&user_inns_k)?;
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
                .insert(&u64_to_ivec(uid), user_encode)?;
            target = "/user/list".to_string();
        }
        _ => unreachable!(),
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
    uid: u64,
    username: String,
    url: String,
    about: String,
}

/// `GET /user/setting`
pub(crate) async fn user_setting(
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let user: User = get_one(&db, "users", claim.uid)?;

    let page_user_setting = PageUserSetting {
        uid: claim.uid,
        page_data: PageData::new("setting", &site_config.site_name, Some(claim), false),
        username: user.username,
        about: user.about,
        url: user.url,
    };

    Ok(into_response(&page_user_setting, "html"))
}

/// `POST /user/setting`
pub(crate) async fn user_setting_post(
    Extension(db): Extension<Db>,
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
    if input.username.chars().any(|c| c.is_control()) {
        return Err(AppError::UsernameInvalid);
    }
    if input.username.contains(['@', '#']) {
        return Err(AppError::UsernameInvalid);
    }

    let tree = db.open_tree("usernames")?;
    if user.username != input.username {
        tree.remove(&user.username)?;
        tree.insert(&input.username, u64_to_ivec(user.uid))?;
    }

    user.username = input.username;
    user.about = input.about;
    user.url = input.url;
    let user_encode = bincode::encode_to_vec(&user, standard())?;
    db.open_tree("users")?
        .insert(u64_to_ivec(claim.uid), &*user_encode)?;

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
    Extension(db): Extension<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    ValidatedForm(input): ValidatedForm<FormPassword>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let mut user: User = get_one(&db, "users", claim.uid)?;

    if check_password(&input.old_password, &user.password_hash) {
        let password_hash = generate_password_hash(&input.password);
        user.password_hash = password_hash;
        let user_encode = bincode::encode_to_vec(&user, standard())?;
        db.open_tree("users")?
            .insert(u64_to_ivec(claim.uid), &*user_encode)?;
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
    Extension(db): Extension<Db>,
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
    Extension(db): Extension<Db>,
    Form(input): Form<FormSignin>,
) -> impl IntoResponse {
    let uid = match input.username.parse::<u64>() {
        Ok(uid) => uid,
        Err(_) => get_uid_by_name(&db, &input.username)?.ok_or(AppError::WrongPassword)?,
    };
    let user: User = get_one(&db, "users", uid)?;
    if check_password(&input.password, &user.password_hash) {
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
pub(crate) async fn signup(Extension(db): Extension<Db>) -> Result<impl IntoResponse, AppError> {
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
    Extension(db): Extension<Db>,
    ValidatedForm(input): ValidatedForm<FormSignup>,
) -> Result<impl IntoResponse, AppError> {
    if input.username.chars().next().unwrap().is_numeric() {
        return Err(AppError::UsernameInvalid);
    }
    if input.username.chars().any(|c| c.is_control()) {
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

    let password_hash = generate_password_hash(&input.password);
    let uid = incr_id(&db, "users_count")?;

    let avatar = format!("./static/avatars/{}.png", uid);
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
        password_hash,
        created_at,
        role,
        ..Default::default()
    };

    let user_encode = bincode::encode_to_vec(&user, standard())?;
    let uid_ivec = u64_to_ivec(uid);
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
    Extension(db): Extension<Db>,
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

const PARAMS: pbkdf2::Params = Params {
    rounds: 100_000,
    output_length: 64,
};

/// return hashed password and salt
fn generate_password_hash(password: &str) -> String {
    let salt = SaltString::generate(&mut OsRng);
    let password_hash = Pbkdf2
        .hash_password_customized(
            password.as_bytes(),
            Some(Ident::new_unwrap("pbkdf2-sha512")),
            None,
            PARAMS,
            &salt,
        )
        .unwrap()
        .to_string();

    password_hash
}

/// check password
fn check_password(password: &str, password_hash: &str) -> bool {
    let parsed_hash = PasswordHash::new(password_hash).unwrap();
    Pbkdf2
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok()
}

impl Claim {
    /// extract session id from request header, then query session tree to get a Claim.
    /// If session id is not found or expired, return None.
    ///
    /// ### user pageviews data
    /// Keep three day pageviews data. For pravacy, the hour and minute has been striped, just date kept.
    pub(super) fn get(
        db: &Db,
        TypedHeader(cookie): &TypedHeader<Cookie>,
        site_config: &SiteConfig,
    ) -> Option<Self> {
        let session = cookie.get(COOKIE_NAME)?;
        let timestamp = session.split('#').next();
        let tree = &db.open_tree("sessions").ok()?;
        let timestamp = i64::from_str_radix(timestamp?, 16).ok()?;
        let now = OffsetDateTime::now_utc();

        if timestamp < now.unix_timestamp() {
            tree.remove(&session).ok()?;
            return None;
        }

        let v = tree.get(&session).ok()??;
        let (claim, _): (Claim, usize) = bincode::decode_from_slice(&v, standard()).ok()?;

        if site_config.read_only && claim.role != u8::MAX {
            return None;
        }

        if claim.role == 0 {
            return None;
        }

        let expire = now
            .replace_time(Time::MIDNIGHT)
            .saturating_add(time::Duration::days(3))
            .unix_timestamp();
        let key = format!("{:x}#{}", expire, claim.uid);
        incr_id(&db.open_tree("user_pageviews").ok()?, key).ok()?;
        Some(claim)
    }

    /// generate a Claim from user and store it in session tree, then return a cookie with a session id.
    fn generate_cookie(db: &Db, user: User, expiry: &str) -> Result<String, AppError> {
        if user.role == 0 {
            return Err(AppError::Banned);
        }
        let seconds = expire_seconds(expiry);
        let exp = OffsetDateTime::now_utc().unix_timestamp() + seconds;
        let session_id = generate_nanoid_expire(seconds);

        let claim = Claim {
            uid: user.uid,
            username: user.username,
            role: user.role,
            exp,
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
    fn test_check_password() {
        let password_hash = generate_password_hash("password");
        assert!(check_password("password", &password_hash));

        let password_hash2 = generate_password_hash("password");
        assert!(check_password("password", &password_hash2));

        // must generate different password_hash and salt with the same password
        assert_ne!(password_hash, password_hash2);
    }
}
