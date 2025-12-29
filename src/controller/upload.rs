use super::{
    Claim, SiteConfig, User,
    db_utils::{IterType, u8_slice_to_u32},
    filters, incr_id,
    inn::ParamsTag,
    meta_handler::{PageData, get_referer, into_response},
    notification::{NtType, add_notification},
    u32_to_ivec,
    user::{InnRole, Role},
};
use crate::{DB, config::CONFIG, error::AppError};
use askama::Template;
use axum::{
    extract::{Multipart, Path, Query},
    response::{IntoResponse, Redirect},
};
use axum_extra::{
    TypedHeader,
    headers::{Cookie, Referer},
};
use data_encoding::HEXLOWER;
use image::{ImageEncoder, ImageFormat, ImageReader, codecs::jpeg::JpegEncoder};
use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY};
use serde::Deserialize;
use std::io::Cursor;
use tokio::fs::{self, remove_file};
use tracing::error;

#[derive(Deserialize)]
pub(crate) struct UploadPicParams {
    page_type: String,
    iid: Option<u32>,
}

/// `POST /mod/inn_icon` && `/user/avatar`
pub(crate) async fn upload_pic_post(
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<UploadPicParams>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let target;
    let fname = match params.page_type.as_str() {
        "inn" => {
            if let Some(iid) = params.iid {
                let inn_role = InnRole::get(&DB, iid, claim.uid)?.ok_or(AppError::Unauthorized)?;
                if inn_role < InnRole::Mod {
                    return Err(AppError::Unauthorized);
                }
                target = format!("/mod/{iid}");
                format!("{}/{}.png", &CONFIG.inn_icons_path, iid)
            } else {
                return Err(AppError::NotFound);
            }
        }
        "user" => {
            target = "/user/setting".to_string();
            format!("{}/{}.png", &CONFIG.avatars_path, claim.uid)
        }
        _ => return Err(AppError::NotFound),
    };

    if let Some(field) = multipart.next_field().await.unwrap() {
        let data = match field.bytes().await {
            Ok(data) => data,
            Err(e) => {
                error!("{:?}", e);
                return Ok(e.into_response());
            }
        };
        let image_format_detected = image::guess_format(&data)?;
        image::load_from_memory_with_format(&data, image_format_detected)?;
        fs::write(fname, &data).await.unwrap();
    }

    Ok(Redirect::to(&target).into_response())
}

/// Page data: `gallery.html`
#[derive(Template)]
#[template(path = "gallery.html")]
struct PageGallery<'a> {
    page_data: PageData<'a>,
    imgs: Vec<(u32, String)>,
    anchor: usize,
    is_desc: bool,
    n: usize,
    uid: u32,
}

/// `GET /gallery/:uid`
pub(crate) async fn gallery(
    cookie: Option<TypedHeader<Cookie>>,
    Path(uid): Path<u32>,
    Query(params): Query<ParamsTag>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    if claim.uid != uid && Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    let has_unread = User::has_unread(&DB, claim.uid)?;

    let anchor = params.anchor.unwrap_or(0);
    let is_desc = params.is_desc.unwrap_or(true);
    let n = 12;

    let mut imgs = Vec::with_capacity(n);
    let ks = DB.open_partition("user_uploads", Default::default())?;
    let iter = ks.inner().prefix(u32_to_ivec(uid));
    let iter = if is_desc {
        IterType::Rev(iter.rev())
    } else {
        IterType::Fwd(iter)
    };

    for (idx, i) in iter.enumerate() {
        if idx < anchor {
            continue;
        }

        let (k, v) = i?;
        let img_id = u8_slice_to_u32(&k[4..]);
        let img = String::from_utf8_lossy(&v).to_string();
        imgs.push((img_id, img));

        if imgs.len() >= n {
            break;
        }
    }

    let page_data = PageData::new("gallery", &site_config, Some(claim), has_unread);
    let page_gallery = PageGallery {
        page_data,
        imgs,
        anchor,
        is_desc,
        n,
        uid,
    };

    Ok(into_response(&page_gallery))
}

/// `GET /image/delete/:uid/:img_id`
pub(crate) async fn image_delete(
    cookie: Option<TypedHeader<Cookie>>,
    Path((uid, img_id)): Path<(u32, u32)>,
    referer: Option<TypedHeader<Referer>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    if claim.uid != uid && Role::from(claim.role) != Role::Admin {
        return Err(AppError::Unauthorized);
    }

    let k = [u32_to_ivec(uid), u32_to_ivec(img_id)].concat();
    let tree = DB.open_partition("user_uploads", Default::default())?;
    if let Some(v1) = tree.take(&k)? {
        // When the same pictures uploaded, only one will be saved. So when deleting, we must check that.
        let mut count = 0;
        for i in tree.inner().iter() {
            let (_, v2) = i?;
            if v1 == v2 {
                count += 1;
                break;
            }
        }

        if count == 0 {
            let img = String::from_utf8_lossy(&v1);
            let path = format!("{}/{}", CONFIG.upload_path, img);
            remove_file(path).await?;
        }
    } else {
        return Err(AppError::NotFound);
    }

    if uid != claim.uid {
        add_notification(&DB, uid, NtType::ImageDelete, claim.uid, img_id)?;
    }

    let target = if let Some(referer) = get_referer(referer) {
        referer
    } else {
        format!("/gallery/{uid}")
    };
    Ok(Redirect::to(&target))
}

/// Page data: `upload.html`
#[derive(Template)]
#[template(path = "upload.html")]
struct PageUpload<'a> {
    page_data: PageData<'a>,
    imgs: Vec<String>,
    uid: u32,
}

/// `GET /upload`
pub(crate) async fn upload(
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let has_unread = User::has_unread(&DB, claim.uid)?;
    let uid = claim.uid;
    let page_data = PageData::new("upload images", &site_config, Some(claim), has_unread);
    let page_upload = PageUpload {
        page_data,
        imgs: vec![],
        uid,
    };

    Ok(into_response(&page_upload))
}

/// `POST /upload`
pub(crate) async fn upload_post(
    cookie: Option<TypedHeader<Cookie>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = SiteConfig::get(&DB)?;
    let claim = Claim::get(&DB, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let mut imgs = Vec::with_capacity(10);
    let mut batch = DB.inner().batch();
    let user_uploads = DB
        .inner()
        .open_partition("user_uploads", Default::default())?;
    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| AppError::Custom(e.to_string()))?
    {
        if imgs.len() > 10 {
            break;
        }

        let data = match field.bytes().await {
            Ok(data) => data,
            Err(e) => {
                error!("{:?}", e);
                return Ok(e.into_response());
            }
        };

        let image_format_detected = image::guess_format(&data)?;
        let img_data = match image_format_detected {
            ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Png => {
                // Re-encode JPEG/WEBP/PNG if it contains EXIF metadata or size is large
                let mut re_encode = false;

                let exifreader = exif::Reader::new();
                if let Ok(exif) = exifreader.read_from_container(&mut Cursor::new(&data))
                    && !exif.buf().is_empty()
                {
                    re_encode = true;
                }

                let quality = Quality::get(data.len());
                if quality < 100 {
                    re_encode = true;
                }

                if re_encode {
                    let dyn_img = ImageReader::new(std::io::Cursor::new(&data))
                        .with_guessed_format()?
                        .decode()?;
                    let mut writer = Vec::new();
                    let mut encoder = JpegEncoder::new_with_quality(&mut writer, quality);
                    encoder.set_exif_metadata(vec![]).unwrap();
                    dyn_img.write_with_encoder(encoder)?;
                    writer
                } else {
                    data.to_vec()
                }
            }

            ImageFormat::Gif => data.to_vec(),
            _ => {
                continue;
            }
        };

        let mut context = Context::new(&SHA1_FOR_LEGACY_USE_ONLY);
        context.update(&img_data);
        let digest = context.finish();
        let sha1 = HEXLOWER.encode(digest.as_ref());
        let ext = *image_format_detected
            .extensions_str()
            .first()
            .unwrap_or(&"jpg");
        let fname = format!("{}.{}", &sha1[0..20], ext);
        let location = format!("{}/{}", &CONFIG.upload_path, fname);

        fs::write(location, &img_data).await.unwrap();
        let img_id = incr_id(&DB, "imgs_count")?;
        let k = [u32_to_ivec(claim.uid), u32_to_ivec(img_id)].concat();
        batch.insert(&user_uploads, k, fname.as_bytes());

        imgs.push(fname);
    }

    batch.commit()?;

    let has_unread = User::has_unread(&DB, claim.uid)?;
    let uid = claim.uid;
    let page_data = PageData::new("upload images", &site_config, Some(claim), has_unread);
    let page_upload = PageUpload {
        page_data,
        imgs,
        uid,
    };

    Ok(into_response(&page_upload))
}

#[derive(Copy, Clone)]
struct Quality;

impl Quality {
    fn get(file_size: usize) -> u8 {
        match file_size {
            file_size if file_size > 5000000 => 70,
            file_size if file_size > 1500000 => 75,
            file_size if file_size > 1000000 => 80,
            file_size if file_size > 800000 => 85,
            file_size if file_size > 600000 => 90,
            file_size if file_size > 400000 => 95,
            _ => 100,
        }
    }
}
