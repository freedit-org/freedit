use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    // 5XX
    #[error("Sled db error: {}", .0)]
    SledError(#[from] sled::Error),
    #[error("Bincode encode error: {}", .0)]
    BincodeEnError(#[from] bincode::error::EncodeError),
    #[error("Bincode decode error: {}", .0)]
    BincodeDeError(#[from] bincode::error::DecodeError),
    #[error(transparent)]
    Utf8Error(#[from] std::str::Utf8Error),
    #[error(transparent)]
    IoError(#[from] std::io::Error),
    #[error("You must join inn first")]
    NoJoinedInn,
    #[error(transparent)]
    TantivyError(#[from] tantivy::TantivyError),

    // 4XX
    #[error("Captcha Error")]
    CaptchaError,
    #[error("Name already exists")]
    NameExists,
    #[error("Too many inns you are managing")]
    InnCreateLimit,
    #[error("Name should not start with a number, should be <a href='https://doc.rust-lang.org/std/primitive.char.html#method.is_alphanumeric'>alphanumeric</a> or '_' or ' '")]
    NameInvalid,
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
    #[error("The post has been locked or hidden")]
    LockedOrHidden,
    #[error(transparent)]
    ImageError(#[from] image::ImageError),
    #[error("The site is under maintenance. It is read only at the moment")]
    ReadOnly,
    #[error(transparent)]
    ValidationError(#[from] validator::ValidationErrors),
    #[error(transparent)]
    AxumFormRejection(#[from] axum::extract::rejection::FormRejection),
    #[error("Invalid feed link")]
    InvalidFeedLink,
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),
    #[error("{0}")]
    Custom(String),
}
