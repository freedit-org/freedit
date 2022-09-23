use thiserror::Error;
use tracing::error;

#[derive(Error, Debug)]
pub(super) enum AppError {
    // 5XX
    #[error("Sled db error: {}", .0)]
    SledError(#[from] sled::Error),
    #[error("save avatar to png error: {}", .0)]
    GenerateAvatarError(&'static str),
    #[error("Bincode encode error: {}", .0)]
    BincodeEnError(#[from] bincode::error::EncodeError),
    #[error("Bincode decode error: {}", .0)]
    BincodeDeError(#[from] bincode::error::DecodeError),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),

    // 4XX
    #[error("Captcha Error")]
    CaptchaError,
    #[error("Name already exists")]
    NameExists,
    #[error("Too many inns you are managing")]
    InnCreateLimit,
    #[error("Username should not start with a number, should not contain '@' or '#'")]
    UsernameInvalid,
    #[error("Not found")]
    NotFound,
    #[error("wrong password")]
    WrongPassword,
    #[error("Too many attempts please try again later")]
    WriteInterval,
    #[error("unauthorized")]
    Unauthorized,
    #[error("Please login first")]
    NonLogin,
    #[error("You have been banned")]
    Banned,
    #[error("The post has been locked by mod")]
    Locked,
    #[error("The post has been hidden by mod")]
    Hidden,
    #[error(transparent)]
    ImageError(#[from] image::ImageError),
    #[error("The site is under maintenance. It is read only at the moment")]
    ReadOnly,
    #[error(transparent)]
    ValidationError(#[from] validator::ValidationErrors),
    #[error(transparent)]
    AxumFormRejection(#[from] axum::extract::rejection::FormRejection),
}
