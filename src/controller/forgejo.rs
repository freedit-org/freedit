use crate::config::CONFIG;
use reqwest::{Client, StatusCode};
use serde::Serialize;
use tracing::error;

#[derive(Serialize)]
struct CreateUserRequest<'a> {
    email: String,
    username: &'a str,
    password: &'a str,
    must_change_password: bool,
    send_notify: bool,
    source_id: i64,
    visibility: &'a str,
}

#[derive(Serialize)]
struct UpdatePasswordRequest<'a> {
    password: &'a str,
}

pub(super) async fn create_user(username: &str, password: &str) {
    if let (Some(url), Some(token)) = (&CONFIG.forgejo_url, &CONFIG.forgejo_token) {
        let full_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_owned()
        } else {
            format!("http://{}", url)
        };

        tracing::info!(
            "Attempting to create user {} in Forgejo at {}",
            username,
            full_url
        );
        let client = Client::new();
        // User requested hardcoded email domain to avoid invalid email format with protocol in site_config.domain
        let email = format!("{}@email.com", username);
        let body = CreateUserRequest {
            email,
            username,
            password,
            must_change_password: false,
            send_notify: false,
            source_id: 0,
            visibility: "public",
        };

        let res: Result<reqwest::Response, reqwest::Error> = client
            .post(format!("{}/api/v1/admin/users", full_url))
            .header("Authorization", format!("token {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        match res {
            Ok(res) => {
                let status = res.status();
                if status != StatusCode::CREATED {
                    let text = res.text().await.unwrap_or_default();
                    error!(
                        "Failed to create forgejo user. Status: {}. Body: {}",
                        status, text
                    );
                } else {
                    tracing::info!("Successfully created user {} in Forgejo", username);
                }
            }
            Err(e) => error!("Failed to create forgejo user: {}", e),
        }
    } else {
        tracing::warn!("Forgejo URL or Token not configured. Skipping user sync.");
    }
}

pub(super) async fn update_password(username: &str, password: &str) {
    if let (Some(url), Some(token)) = (&CONFIG.forgejo_url, &CONFIG.forgejo_token) {
        let full_url = if url.starts_with("http://") || url.starts_with("https://") {
            url.to_owned()
        } else {
            format!("http://{}", url)
        };

        tracing::info!(
            "Attempting to update password for user {} in Forgejo at {}",
            username,
            full_url
        );
        let client = Client::new();
        let body = UpdatePasswordRequest { password };

        let res: Result<reqwest::Response, reqwest::Error> = client
            .patch(format!("{}/api/v1/admin/users/{}", full_url, username))
            .header("Authorization", format!("token {}", token))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await;

        match res {
            Ok(res) => {
                let status = res.status();
                if status != StatusCode::OK {
                    let text = res.text().await.unwrap_or_default();
                    error!(
                        "Failed to update forgejo password. Status: {}. Body: {}",
                        status, text
                    );
                } else {
                    tracing::info!(
                        "Successfully updated password for user {} in Forgejo",
                        username
                    );
                }
            }
            Err(e) => error!("Failed to update forgejo password: {}", e),
        }
    } else {
        tracing::warn!("Forgejo URL or Token not configured. Skipping password sync.");
    }
}
