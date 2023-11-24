use super::{db_utils::u32_to_ivec, fmt::md2html, Claim, SiteConfig};
use crate::{error::AppError, DB};
use askama::Template;
use axum::{
    async_trait,
    body::BoxBody,
    extract::{rejection::FormRejection, FromRequest},
    headers::{Cookie, HeaderName, Referer},
    http::{self, HeaderMap, HeaderValue, Request, Uri},
    response::{IntoResponse, Redirect, Response},
    Form, TypedHeader,
};
use once_cell::sync::Lazy;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tokio::signal;
use tracing::error;
use validator::Validate;

pub(super) fn into_response<T: Template>(t: &T) -> Response<BoxBody> {
    match t.render() {
        Ok(body) => {
            let headers = [(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static(T::MIME_TYPE),
            )];

            (headers, body).into_response()
        }
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

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

pub(crate) struct ValidatedForm<T>(pub(super) T);

#[async_trait]
impl<T, S, B> FromRequest<S, B> for ValidatedForm<T>
where
    T: DeserializeOwned + Validate,
    S: Send + Sync,
    Form<T>: FromRequest<S, B, Rejection = FormRejection>,
    B: Send + 'static,
{
    type Rejection = AppError;

    async fn from_request(req: Request<B>, state: &S) -> Result<Self, Self::Rejection> {
        let Form(value) = Form::<T>::from_request(req, state).await?;
        value.validate()?;
        Ok(ValidatedForm(value))
    }
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

static CSS: Lazy<String> = Lazy::new(|| {
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

pub(crate) async fn encryption_js() -> &'static str {
    include_str!("../../static/js/encryption-helper.js")
}

pub(crate) async fn encoding_js() -> &'static str {
    include_str!("../../static/js/encoding-helper.js")
}

pub(crate) async fn robots() -> &'static str {
    include_str!("../../static/robots.txt")
}

pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    println!("signal received, starting graceful shutdown");
}

pub(super) struct PageData<'a> {
    pub(super) title: &'a str,
    pub(super) site_name: &'a str,
    pub(super) site_description: String,
    pub(super) claim: Option<Claim>,
    pub(super) has_unread: bool,
}

impl<'a> PageData<'a> {
    pub(super) fn new(
        title: &'a str,
        site_config: &'a SiteConfig,
        claim: Option<Claim>,
        has_unread: bool,
    ) -> Self {
        let site_description = md2html(&site_config.description);
        Self {
            title,
            site_name: &site_config.site_name,
            site_description,
            claim,
            has_unread,
        }
    }
}

// TODO: https://github.com/hyperium/headers/issues/48
pub(super) fn get_referer(header: Option<TypedHeader<Referer>>) -> Option<String> {
    if let Some(TypedHeader(r)) = header {
        let referer = format!("{r:?}");
        let trimed = referer
            .trim_start_matches("Referer(\"")
            .trim_end_matches("\")");
        Some(trimed.to_owned())
    } else {
        None
    }
}

pub(super) struct ParamsPage {
    pub(super) anchor: usize,
    pub(super) n: usize,
    pub(super) is_desc: bool,
}
