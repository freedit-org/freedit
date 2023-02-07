use askama::Template;
use axum::{
    extract::{Multipart, Query, State},
    headers::Cookie,
    response::{IntoResponse, Redirect},
    TypedHeader,
};
use data_encoding::HEXLOWER;
use image::{imageops::FilterType, ImageFormat};
use img_parts::{DynImage, ImageEXIF};
use mozjpeg::{ColorSpace, Compress, ScanMode};
use ring::digest::{Context, SHA1_FOR_LEGACY_USE_ONLY};
use serde::Deserialize;
use sled::{Batch, Db};
use tokio::fs;

use crate::{config::CONFIG, error::AppError};

use super::{get_inn_role, get_site_config, into_response, u32_to_ivec, Claim, PageData};

#[derive(Deserialize)]
pub(crate) struct UploadPicParams {
    page_type: String,
    iid: Option<u32>,
}

/// `POST /mod/inn_icon` && `/user/avatar`
pub(crate) async fn upload_pic_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    Query(params): Query<UploadPicParams>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let target;
    let fname = match params.page_type.as_str() {
        "inn" => {
            if let Some(iid) = params.iid {
                let inn_role = get_inn_role(&db, iid, claim.uid)?.ok_or(AppError::Unauthorized)?;
                if inn_role <= 8 {
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
        _ => unreachable!(),
    };

    if let Some(field) = multipart.next_field().await.unwrap() {
        let data = field.bytes().await.unwrap();
        let image_format_detected = image::guess_format(&data)?;
        image::load_from_memory_with_format(&data, image_format_detected)?;
        fs::write(fname, &data).await.unwrap();
    }

    Ok(Redirect::to(&target))
}

/// Page data: `upload.html`
#[derive(Template)]
#[template(path = "upload.html")]
struct PageUpload<'a> {
    page_data: PageData<'a>,
    imgs: Vec<String>,
}

/// `GET /upload`
pub(crate) async fn upload(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;
    let page_data = PageData::new("upload images", &site_config, Some(claim), false);
    let page_upload = PageUpload {
        page_data,
        imgs: vec![],
    };

    Ok(into_response(&page_upload, "html"))
}

/// `POST /upload`
pub(crate) async fn upload_post(
    State(db): State<Db>,
    cookie: Option<TypedHeader<Cookie>>,
    mut multipart: Multipart,
) -> Result<impl IntoResponse, AppError> {
    let cookie = cookie.ok_or(AppError::NonLogin)?;
    let site_config = get_site_config(&db)?;
    let claim = Claim::get(&db, &cookie, &site_config).ok_or(AppError::NonLogin)?;

    let mut imgs = Vec::with_capacity(10);
    let mut batch = Batch::default();
    while let Some(field) = multipart.next_field().await.unwrap() {
        if imgs.len() > 10 {
            break;
        }

        let data = field.bytes().await.unwrap();
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

                    comp.set_mem_dest();
                    comp.set_optimize_scans(true);
                    comp.start_compress();

                    let mut line: usize = 0;
                    let resized_img_data = resized_img.into_rgb8().into_vec();
                    loop {
                        if line > target_height - 1 {
                            break;
                        }
                        let idx = line * target_width * 3..(line + 1) * target_width * 3;
                        comp.write_scanlines(&resized_img_data[idx]);
                        line += 1;
                    }
                    comp.finish_compress();

                    if let Ok(comp) = comp.data_to_vec() {
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
        let k = [&u32_to_ivec(claim.uid), fname.as_bytes()].concat();
        batch.insert(k, &[]);

        imgs.push(fname);
    }
    db.open_tree("user_uploads")?.apply_batch(batch)?;

    let page_data = PageData::new("upload images", &site_config, Some(claim), false);
    let page_upload = PageUpload { page_data, imgs };

    Ok(into_response(&page_upload, "html"))
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
            file_size if file_size > 5000000 => Factor::new(60., 0.7),
            file_size if file_size > 1000000 => Factor::new(65., 0.75),
            file_size if file_size > 500000 => Factor::new(70., 0.8),
            file_size if file_size > 300000 => Factor::new(75., 0.85),
            file_size if file_size > 100000 => Factor::new(80., 0.9),
            _ => Factor::new(85., 1.0),
        }
    }
}
