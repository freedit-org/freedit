use crate::{
    config::CONFIG,
    controller::{
        admin::{admin, admin_post, admin_stats, admin_view},
        feed::{feed, feed_add, feed_add_post, feed_read, feed_star, feed_subscribe, feed_update},
        handler_404, health_check, home,
        inn::{
            comment_delete, comment_downvote, comment_hide, comment_post, comment_upvote,
            edit_post, edit_post_post, inn, inn_feed, inn_join, inn_list, mod_inn, mod_inn_post,
            post, post_downvote, post_hide, post_lock, post_upvote, preview, tag,
        },
        notification, serve_dir,
        solo::{solo, solo_delete, solo_like, solo_list, solo_post},
        style, upload, upload_pic_post, upload_post,
        user::{
            remove_session, reset, reset_post, role_post, signin, signin_post, signout, signup,
            signup_post, user, user_follow, user_list, user_password_post, user_recovery_code,
            user_setting, user_setting_post,
        },
    },
};
use axum::{
    error_handling::HandleErrorLayer, extract::DefaultBodyLimit, handler::Handler,
    http::StatusCode, routing::get, BoxError, Router,
};
use sled::Db;
use std::time::Duration;
use tower::{timeout::TimeoutLayer, ServiceBuilder};
use tower_http::{
    compression::CompressionLayer,
    trace::{DefaultMakeSpan, TraceLayer},
};
use tracing::{info, Level};

const UPLOAD_LIMIT: usize = 20 * 1024 * 1024;

pub(super) async fn router(db: Db) -> Router {
    let middleware_stack = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|_: BoxError| async {
            StatusCode::REQUEST_TIMEOUT
        }))
        .layer(TimeoutLayer::new(Duration::from_secs(10)))
        .layer(CompressionLayer::new())
        .layer(
            TraceLayer::new_for_http().make_span_with(DefaultMakeSpan::new().level(Level::INFO)),
        );

    let router_db = Router::new()
        .route("/", get(home))
        .route("/signup", get(signup).post(signup_post))
        .route("/signin", get(signin).post(signin_post))
        .route("/signout", get(signout))
        .route("/user/:uid", get(user))
        .route("/user/:uid/follow", get(user_follow))
        .route("/user/setting", get(user_setting).post(user_setting_post))
        .route("/user/avatar", get(user_setting).post(upload_pic_post))
        .route("/user/password", get(user_setting).post(user_password_post))
        .route("/user/recovery", get(user_setting).post(user_recovery_code))
        .route("/user/reset", get(reset).post(reset_post))
        .route("/user/list", get(user_list))
        .route("/user/remove/:session_id", get(remove_session))
        .route("/role/:id/:uid", get(user_list).post(role_post))
        .route("/notification", get(notification))
        .route("/admin", get(admin).post(admin_post))
        .route("/admin/view", get(admin_view))
        .route("/admin/stats", get(admin_stats))
        .route("/mod/:iid", get(mod_inn).post(mod_inn_post))
        .route(
            "/mod/inn_icon",
            get(mod_inn).post(upload_pic_post.layer(DefaultBodyLimit::max(UPLOAD_LIMIT))),
        )
        .route("/mod/:iid/:pid/lock", get(post_lock))
        .route("/mod/:iid/:pid/hide", get(post_hide))
        .route("/inn/list", get(inn_list))
        .route("/inn/tag/:tag", get(tag))
        .route("/inn/:iid", get(inn))
        .route("/inn/:iid/join", get(inn_join))
        .route("/inn/:iid/feed", get(inn_feed))
        .route("/post/:iid/:pid", get(post).post(comment_post))
        .route("/post/:iid/:pid/:cid/delete", get(comment_delete))
        .route("/post/:iid/:pid/:cid/hide", get(comment_hide))
        .route("/post/edit/:pid", get(edit_post).post(edit_post_post))
        .route("/post/:iid/:pid/upvote", get(post_upvote))
        .route("/post/:iid/:pid/downvote", get(post_downvote))
        .route("/post/:iid/:pid/:cid/upvote", get(comment_upvote))
        .route("/post/:iid/:pid/:cid/downvote", get(comment_downvote))
        .route("/preview", get(post).post(preview))
        .route("/solo/user/:uid", get(solo_list).post(solo_post))
        .route("/solo/:sid/like", get(solo_like))
        .route("/solo/:sid/delete", get(solo_delete))
        .route("/solo/:sid", get(solo))
        .route(
            "/upload",
            get(upload).post(upload_post.layer(DefaultBodyLimit::max(UPLOAD_LIMIT))),
        )
        .route("/feed/:uid", get(feed))
        .route("/feed/add", get(feed_add).post(feed_add_post))
        .route("/feed/update", get(feed_update))
        .route("/feed/star/:item_id", get(feed_star))
        .route("/feed/subscribe/:uid/:item_id", get(feed_subscribe))
        .route("/feed/read/:item_id", get(feed_read))
        .with_state(db);

    let mut router_static = Router::new()
        .route("/health_check", get(health_check))
        .route("/static/style.css", get(style))
        .nest_service("/static/avatars", serve_dir(&CONFIG.avatars_path).await)
        .nest_service("/static/inn_icons", serve_dir(&CONFIG.inn_icons_path).await)
        .nest_service("/static/upload", serve_dir(&CONFIG.upload_path).await);

    for (path, dir, _) in &CONFIG.serve_dir {
        let path = format!("/{path}");
        info!("serve dir: {} -> {}", path, dir);
        router_static = router_static.nest_service(&path, serve_dir(dir).await);
    }

    let app = router_static.merge(router_db);
    app.layer(middleware_stack).fallback(handler_404)
}
