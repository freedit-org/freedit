use super::{
    db_utils::{u8_slice_to_u32, IterType},
    incr_id,
    inn::ParamsTag,
    meta_handler::{get_referer, into_response, PageData},
    notification::{add_notification, NtType},
    u32_to_ivec,
    user::{InnRole, Role},
    Claim, SiteConfig, User,
};
use crate::{config::CONFIG, controller::filters, error::AppError, DB};
use axum::{
    extract::{Multipart, Path, Query},
    response::{IntoResponse, Redirect},
};
use axum_extra::{
    headers::{Cookie, Referer},
    TypedHeader,
};
use data_encoding::HEXLOWER;
use image::{imageops::FilterType, ImageFormat};
use img_parts::{DynImage, ImageEXIF};
use mozjpeg::{ColorSpace, Compress, ScanMode};
use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY};
use rinja::Template;
use serde::Deserialize;
use sled::Batch;
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
    let iter = DB.open_tree("user_uploads")?.scan_prefix(u32_to_ivec(uid));
    let iter = if is_desc {
        IterType::Rev(iter.rev())
    } else {
        IterType::Iter(iter)
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

    let k = [&u32_to_ivec(uid), &u32_to_ivec(img_id)].concat();
    let tree = DB.open_tree("user_uploads")?;
    if let Some(v1) = tree.remove(&k)? {
        // When the same pictures uploaded, only one will be saved. So when deleting, we must check that.
        let mut count = 0;
        for i in tree.iter() {
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
        format!("/gallery/{}", uid)
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
    let mut batch = Batch::default();
    while let Some(field) = multipart.next_field().await.unwrap() {
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
        let ext;
        let img_data = match image_format_detected {
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP => {
                if let Ok(Some(mut img)) = DynImage::from_bytes(data) {
                    img.set_exif(None);
                    let img_noexif = img.encoder().bytes();

                    // author: "Kim tae hyeon <kimth0734@gmail.com>"
                    // https://github.com/altair823/image_compressor/blob/main/src/compressor.rs
                    // license = "MIT"
                    let dyn_img =
                        image::load_from_memory_with_format(&img_noexif, image_format_detected)?;
                    let factor = Factor::get(img_noexif.len());

                    // resize
                    let width = (dyn_img.width() as f32 * factor.size_ratio) as u32;
                    let height = (dyn_img.width() as f32 * factor.size_ratio) as u32;
                    let resized_img = dyn_img.resize(width, height, FilterType::Lanczos3);

                    // compress
                    let mut comp = Compress::new(ColorSpace::JCS_RGB);
                    comp.set_scan_optimization_mode(ScanMode::Auto);
                    comp.set_quality(factor.quality);

                    let target_width = resized_img.width() as usize;
                    let target_height = resized_img.height() as usize;
                    comp.set_size(target_width, target_height);

                    comp.set_optimize_scans(true);
                    let mut comp = comp.start_compress(Vec::new()).unwrap();

                    let mut line: usize = 0;
                    let resized_img_data = resized_img.into_rgb8().into_vec();
                    loop {
                        if line > target_height - 1 {
                            break;
                        }
                        let idx = line * target_width * 3..(line + 1) * target_width * 3;
                        comp.write_scanlines(&resized_img_data[idx]).unwrap();
                        line += 1;
                    }

                    if let Ok(comp) = comp.finish() {
                        ext = "jpeg";
                        comp
                    } else {
                        continue;
                    }
                } else {
                    continue;
                }
            }
            ImageFormat::Gif => {
                ext = "gif";
                data.to_vec()
            }
            _ => {
                continue;
            }
        };

        let mut context = Context::new(&SHA1_FOR_LEGACY_USE_ONLY);
        context.update(&img_data);
        let digest = context.finish();
        let sha1 = HEXLOWER.encode(digest.as_ref());
        let fname = format!("{}.{}", &sha1[0..20], ext);
        let location = format!("{}/{}", &CONFIG.upload_path, fname);

        fs::write(location, &img_data).await.unwrap();
        let img_id = incr_id(&DB, "imgs_count")?;
        let k = [&u32_to_ivec(claim.uid), &u32_to_ivec(img_id)].concat();
        batch.insert(k, &*fname);

        imgs.push(fname);
    }
    DB.open_tree("user_uploads")?.apply_batch(batch)?;

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
struct Factor {
    /// Quality of the new compressed image.
    /// Values range from 0 to 100 in float.
    quality: f32,

    /// Ratio for resize the new compressed image.
    /// Values range from 0 to 1 in float.
    size_ratio: f32,
}

impl Factor {
    /// Create a new `Factor` instance.
    /// The `quality` range from 0 to 100 in float,
    /// and `size_ratio` range from 0 to 1 in float.
    ///
    /// # Panics
    ///
    /// - If the quality value is 0 or less.
    /// - If the quality value exceeds 100.
    /// - If the size ratio value is 0 or less.
    /// - If the size ratio value exceeds 1.
    fn new(quality: f32, size_ratio: f32) -> Self {
        if (quality > 0. && quality <= 100.) && (size_ratio > 0. && size_ratio <= 1.) {
            Self {
                quality,
                size_ratio,
            }
        } else {
            panic!("Wrong Factor argument!");
        }
    }

    fn get(file_size: usize) -> Factor {
        match file_size {
            file_size if file_size > 5000000 => Factor::new(70., 0.75),
            file_size if file_size > 1000000 => Factor::new(75., 0.8),
            file_size if file_size > 600000 => Factor::new(80., 0.85),
            file_size if file_size > 400000 => Factor::new(85., 0.9),
            file_size if file_size > 200000 => Factor::new(90., 0.95),
            _ => Factor::new(100., 1.0),
        }
    }
}
