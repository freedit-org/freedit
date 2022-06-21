use axum::{
    http::{uri::InvalidUri, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub(super) enum AppError {
    // 5XX
    #[error("Sled db error: {}", .0)]
    SledError(#[from] sled::Error),
    #[error("Sled transaction error: {}", .0)]
    SledTransactionError(#[from] sled::transaction::TransactionError),
    #[error("save avatar to png error: {}", .0)]
    GenerateAvatarError(&'static str),
    #[error("Bincode encode error: {}", .0)]
    BincodeEnError(#[from] bincode::error::EncodeError),
    #[error("Bincode decode error: {}", .0)]
    BincodeDeError(#[from] bincode::error::DecodeError),
    #[error("time error: {}", .0)]
    TimeError(String),
    #[error("Invalid Uri: {}", .0)]
    InvalidUri(#[from] InvalidUri),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    // 4XX
    #[error("Captcha Error")]
    CaptchaError,
    #[error("Name already exists")]
    NameExists,
    #[error("Username should not start with a number, should not contain '@' or '#'")]
    UsernameInvalid,
    #[error("Not found")]
    NotFound,
    #[error("wrong password")]
    WrongPassword,
    #[error("unauthorized")]
    Unauthorized,
    #[error("Please login first")]
    NonLogin,
    #[error("You have been banned")]
    Banned,
    #[error(transparent)]
    ImageError(#[from] image::ImageError),
    #[error("The site is under maintenance. It is read only at the moment")]
    ReadOnly,
    #[error("Latex to mathml error: {}", .0)]
    LatexError(#[from] latex2mathml::LatexError),
    #[error(transparent)]
    ValidationError(#[from] validator::ValidationErrors),
    #[error(transparent)]
    AxumFormRejection(#[from] axum::extract::rejection::FormRejection),
}

// TODO: CSS Better style
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match self {
            AppError::CaptchaError
            | AppError::NameExists
            | AppError::UsernameInvalid
            | AppError::NotFound
            | AppError::WrongPassword
            | AppError::Unauthorized
            | AppError::Banned
            | AppError::ImageError(_)
            | AppError::ReadOnly
            | AppError::ValidationError(_)
            | AppError::AxumFormRejection(_)
            | AppError::LatexError(_) => StatusCode::BAD_REQUEST,
            AppError::NonLogin => return Redirect::to("/signin").into_response(),
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        error!("{}, {}", status, self);

        let html = format!(
            r#"<strong>Error: {}</strong>
            <p>{}</p>
            <p><a href="/">Home</p>"#,
            status, self
        );
        let body = Html(html);

        (status, body).into_response()
    }
}
