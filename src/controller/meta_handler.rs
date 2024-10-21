use std::sync::LazyLock;

use super::{db_utils::u32_to_ivec, fmt::md2html, Claim, SiteConfig};
use crate::{controller::filters, error::AppError, DB};
use axum::{
    http::{HeaderMap, HeaderValue, Uri},
    response::{IntoResponse, Redirect, Response},
};
use axum_extra::{
    headers::{Cookie, Referer},
    TypedHeader,
};
use http::{HeaderName, StatusCode};
use rinja_axum::{into_response, Template};
use tracing::error;

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
        if let Some(home_page) = DB.open_tree("home_pages")?.get(u32_to_ivec(claim.uid))? {
            if let Some(code) = home_page.first() {
                home_page_code = *code;
                if home_page_code == 1 {
                    return Ok(Redirect::to(&format!("/feed/{}", claim.uid)));
                }
            };
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
    // TODO: CSS minification
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

pub(crate) async fn encryption_js() -> (HeaderMap, &'static str) {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/javascript"),
    );
    headers.insert(
        HeaderName::from_static("cache-control"),
        HeaderValue::from_static("public, max-age=1209600, s-maxage=86400"),
    );
    let js = include_str!("../../static/js/encryption-helper.js");

    (headers, js)
}

pub(crate) async fn encoding_js() -> (HeaderMap, &'static str) {
    let mut headers = HeaderMap::new();
    headers.insert(
        HeaderName::from_static("content-type"),
        HeaderValue::from_static("text/javascript"),
    );
    headers.insert(
        HeaderName::from_static("cache-control"),
        HeaderValue::from_static("public, max-age=1209600, s-maxage=86400"),
    );
    let js = include_str!("../../static/js/encoding-helper.js");

    (headers, js)
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
