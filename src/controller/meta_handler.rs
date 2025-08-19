use std::{str::FromStr, sync::LazyLock};

use super::{Claim, SiteConfig, fmt::md2html};
use crate::{
    DB,
    controller::{User, filters},
    error::AppError,
    get_one,
};
use askama::Template;
use axum::{
    Form,
    extract::{FromRequest, Path, Request, rejection::FormRejection},
    http::{HeaderMap, HeaderValue, Uri},
    response::{Html, IntoResponse, Redirect, Response},
};
use axum_extra::{
    TypedHeader,
    headers::{ContentType, Cookie, Referer},
};
use http::{HeaderName, StatusCode, header};
use include_dir::{Dir, include_dir};
use serde::de::DeserializeOwned;
use tracing::error;
use validator::Validate;

#[derive(Template)]
#[template(path = "error.html", escape = "none")]
struct PageError<'a> {
    page_data: PageData<'a>,
    status: String,
    error: String,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::CaptchaError
            | AppError::NameExists
            | AppError::InnCreateLimit
            | AppError::NameInvalid
            | AppError::WrongPassword
            | AppError::ImageError(_)
            | AppError::LockedOrHidden
            | AppError::ReadOnly
            | AppError::ValidationError(_)
            | AppError::NoJoinedInn
            | AppError::Custom(_)
            | AppError::AxumFormRejection(_) => StatusCode::BAD_REQUEST,
            AppError::NotFound => StatusCode::NOT_FOUND,
            AppError::WriteInterval => StatusCode::TOO_MANY_REQUESTS,
            AppError::NonLogin => return Redirect::to("/signin").into_response(),
            AppError::Unauthorized => StatusCode::UNAUTHORIZED,
            AppError::Banned => StatusCode::FORBIDDEN,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        error!("{}, {}", status, self);
        let site_config = SiteConfig::get(&DB).unwrap_or_default();
        let page_data = PageData::new("Error", &site_config, None, false);
        let page_error = PageError {
            page_data,
            status: status.to_string(),
            error: self.to_string(),
        };

        into_response(&page_error)
    }
}

pub(crate) async fn handler_404(uri: Uri) -> impl IntoResponse {
    error!("No route for {}", uri);
    AppError::NotFound
}

pub(crate) async fn home(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let site_config = SiteConfig::get(&DB)?;
    let claim = cookie.and_then(|cookie| Claim::get(&DB, &cookie, &site_config));
    let mut home_page_code = site_config.home_page;

    if let Some(claim) = claim {
        let user: User = get_one(&DB, "users", claim.uid)?;
        home_page_code = user.home_page;
        if home_page_code == 1 {
            return Ok(Redirect::to(&format!("/feed/{}", claim.uid)));
        }
    }

    let redirect = match home_page_code {
        0 => "/inn/0",
        2 => "/inn/0?filter=joined",
        3 => "/inn/0?filter=following",
        4 => "/solo/user/0",
        5 => "/solo/user/0?filter=Following",
        6 => "/inn/list",
        _ => "/inn/0",
    };

    Ok(Redirect::to(redirect))
}

static CSS: LazyLock<String> = LazyLock::new(|| {
    let mut css = include_str!("../../static/css/bulma.min.css").to_string();
    css.push('\n');
    css.push_str(include_str!("../../static/css/bulma-list.css"));
    css.push('\n');
    css.push_str(include_str!("../../static/css/main.css"));
    css
});

pub(crate) async fn style() -> (HeaderMap, &'static str) {
    let mut headers = HeaderMap::new();

    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/css"),
    );
    headers.insert(
        HeaderName::from_static("cache-control"),
        HeaderValue::from_static("public, max-age=1209600, s-maxage=86400"),
    );

    (headers, &CSS)
}

pub(crate) async fn favicon() -> (HeaderMap, &'static str) {
    let mut headers = HeaderMap::new();

    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("image/svg+xml"),
    );
    headers.insert(
        HeaderName::from_static("cache-control"),
        HeaderValue::from_static("public, max-age=1209600, s-maxage=86400"),
    );

    let favicon = include_str!("../../static/favicon.svg");

    (headers, favicon)
}

static STATIC_JS_DIR: Dir = include_dir!("static/js");

pub(crate) async fn serve_embedded_js(Path(filename): Path<String>) -> impl IntoResponse {
    if let Some(file) = STATIC_JS_DIR.get_file(&filename) {
        let body = file.contents();

        let mut headers = HeaderMap::new();
        headers.insert("Content-Type", "application/javascript".parse().unwrap());
        headers.insert(
            HeaderName::from_static("cache-control"),
            HeaderValue::from_static("public, max-age=1209600, s-maxage=86400"),
        );

        (headers, body).into_response()
    } else {
        (StatusCode::NOT_FOUND, "File not found").into_response()
    }
}

pub(crate) async fn robots() -> &'static str {
    include_str!("../../static/robots.txt")
}

pub(super) struct PageData<'a> {
    pub(super) title: &'a str,
    pub(super) site_name: &'a str,
    pub(super) site_description: String,
    pub(super) claim: Option<Claim>,
    pub(super) has_unread: bool,
    pub(super) lang: String,
}

impl<'a> PageData<'a> {
    pub(super) fn new(
        title: &'a str,
        site_config: &'a SiteConfig,
        claim: Option<Claim>,
        has_unread: bool,
    ) -> Self {
        let site_description = md2html(&site_config.description);
        let lang = claim
            .as_ref()
            .and_then(|claim| claim.lang.as_ref())
            .map_or_else(|| site_config.lang.clone(), |lang| lang.to_owned());

        Self {
            title,
            site_name: &site_config.site_name,
            site_description,
            claim,
            has_unread,
            lang,
        }
    }
}

// TODO: https://github.com/hyperium/headers/issues/48
pub(super) fn get_referer(header: Option<TypedHeader<Referer>>) -> Option<String> {
    if let Some(TypedHeader(r)) = header {
        let referer = format!("{r:?}");
        let trimmed = referer
            .trim_start_matches("Referer(\"")
            .trim_end_matches("\")");
        Some(trimmed.to_owned())
    } else {
        None
    }
}

pub(super) struct ParamsPage {
    pub(super) anchor: usize,
    pub(super) n: usize,
    pub(super) is_desc: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct ValidatedForm<T>(pub T);

impl<T, S> FromRequest<S> for ValidatedForm<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
    Form<T>: FromRequest<S, Rejection = FormRejection>,
{
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Form(value) = Form::<T>::from_request(req, state).await?;
        value.validate()?;
        Ok(ValidatedForm(value))
    }
}

/// Render a [`Template`] into a [`Response`], or render an error page.
pub(crate) fn into_response<T: ?Sized + askama::Template>(tmpl: &T) -> Response {
    match tmpl.render() {
        Ok(body) => Html(body).into_response(),
        Err(e) => e.to_string().into_response(),
    }
}

/// Render a [`Template`] into a [`Response`] with a specific content type.
/// If the content type is invalid, it defaults to `text/html`.
pub(crate) fn into_response_with_content_type<T: ?Sized + askama::Template>(
    tmpl: &T,
    content_type: &str,
) -> Response {
    let content_type = ContentType::from_str(content_type).unwrap_or_else(|_| ContentType::html());
    let body = tmpl.render().unwrap_or_else(|e| e.to_string());
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        content_type.to_string().parse().unwrap(),
    );
    (headers, body).into_response()
}
