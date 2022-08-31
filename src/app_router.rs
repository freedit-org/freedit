use crate::{
    config::CONFIG,
    controller::{
        admin::{admin, admin_post, admin_stats, admin_view},
        handler_404, health_check, home,
        inn::{
            comment_downvote, comment_post, comment_upvote, edit_post, edit_post_post, inn,
            inn_feed, inn_join, inn_list, mod_inn, mod_inn_post, post, post_downvote, post_hide,
            post_lock, post_upvote, tag,
        },
        notification, serve_dir,
        solo::{solo, solo_delete, solo_like, solo_post},
        style, upload_pic_post,
        user::{
            remove_session, role_post, signin, signin_post, signout, signup, signup_post, user,
            user_follow, user_list, user_password_post, user_setting, user_setting_post,
        },
    },
};
use axum::{error_handling::HandleErrorLayer, http::StatusCode, routing::get, BoxError, Router};
use sled::Db;
use std::time::Duration;
use tower::{timeout::TimeoutLayer, ServiceBuilder};
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::info;

pub(super) async fn router(db: Db) -> Router {
    let middleware_stack = ServiceBuilder::new()
        .layer(HandleErrorLayer::new(|_: BoxError| async {
            StatusCode::REQUEST_TIMEOUT
        }))
        .layer(TimeoutLayer::new(Duration::from_secs(10)))
        .layer(CompressionLayer::new())
        .layer(TraceLayer::new_for_http().on_request(()).on_response(()));

    let router_db = Router::with_state(db)
        .route("/", get(home))
        .route("/signup", get(signup).post(signup_post))
        .route("/signin", get(signin).post(signin_post))
        .route("/signout", get(signout))
        .route("/user/:uid", get(user))
        .route("/user/:uid/follow", get(user_follow))
        .route("/user/setting", get(user_setting).post(user_setting_post))
        .route("/user/avatar", get(user_setting).post(upload_pic_post))
        .route("/user/password", get(user_setting).post(user_password_post))
        .route("/user/list", get(user_list))
        .route("/user/remove/:session_id", get(remove_session))
        .route("/role/:id/:uid", get(user_list).post(role_post))
        .route("/notification", get(notification))
        .route("/admin", get(admin).post(admin_post))
        .route("/admin/view", get(admin_view))
        .route("/admin/stats", get(admin_stats))
        .route("/mod/:iid", get(mod_inn).post(mod_inn_post))
        .route("/mod/inn_icon", get(mod_inn).post(upload_pic_post))
        .route("/mod/:iid/:pid/lock", get(post_lock))
        .route("/mod/:iid/:pid/hide", get(post_hide))
        .route("/inn/list", get(inn_list))
        .route("/inn/tag/:tag", get(tag))
        .route("/inn/:iid", get(inn))
        .route("/inn/:iid/join", get(inn_join))
        .route("/inn/:iid/feed", get(inn_feed))
        .route("/post/:iid/:pid", get(post).post(comment_post))
        .route("/post/:iid/edit/:pid", get(edit_post).post(edit_post_post))
        .route("/post/:iid/:pid/upvote", get(post_upvote))
        .route("/post/:iid/:pid/downvote", get(post_downvote))
        .route("/post/:iid/:pid/:cid/upvote", get(comment_upvote))
        .route("/post/:iid/:pid/:cid/downvote", get(comment_downvote))
        .route("/solo/user/:uid", get(solo).post(solo_post))
        .route("/solo/:sid/like", get(solo_like))
        .route("/solo/:sid/delete", get(solo_delete));

    let mut router_static = Router::new()
        .route("/health_check", get(health_check))
        .route("/static/style.css", get(style))
        .nest("/static/avatars", serve_dir(&CONFIG.avatars_path).await)
        .nest("/static/inn_icons", serve_dir(&CONFIG.inn_icons_path).await)
        .nest(
            "/static/inn",
            serve_dir(&format!("{}/inn", &CONFIG.html_path)).await,
        )
        .nest(
            "/static/post",
            serve_dir(&format!("{}/post", &CONFIG.html_path)).await,
        );

    for (path, dir, _) in &CONFIG.serve_dir {
        let path = format!("/{}", path);
        info!("serve dir: {} -> {}", path, dir);
        router_static = router_static.nest(&path, serve_dir(dir).await);
    }

    let app = Router::new().merge(router_db).merge(router_static);
    app.layer(middleware_stack).fallback(handler_404)
}
