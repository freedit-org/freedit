use axum_server::tls_rustls::RustlsConfig;
use lazy_static::lazy_static;
use serde::Deserialize;
use std::fs::read_to_string;
use tracing::error;

lazy_static! {
    pub(crate) static ref CONFIG: Config = Config::load_config();
}

#[derive(Deserialize)]
pub(crate) struct Config {
    pub(crate) db: String,
    pub(crate) addr: String,
    pub(crate) avatars_path: String,
    pub(crate) inn_icons_path: String,
    pub(crate) serve_dir: Vec<(String, String, String)>,
    https: bool,
    tls_cert: Option<String>,
    tls_key: Option<String>,
}

impl Config {
    fn load_config() -> Config {
        let cfg_file = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_owned());
        let config_toml_content = read_to_string(cfg_file).expect("Failed to read config.toml");
        let config: Config = toml::from_str(&config_toml_content).unwrap();
        config
    }

    pub(crate) async fn tls_config(&self) -> Option<RustlsConfig> {
        let mut res = None;
        if self.https {
            if let (Some(cert), Some(key)) = (&self.tls_cert, &self.tls_key) {
                match RustlsConfig::from_pem_file(&cert, &key).await {
                    Ok(rustls_config) => res = Some(rustls_config),
                    Err(e) => error!("enable https failed: {}", e),
                }
            } else {
                error!("enable https failed: no tls cert or key is provided");
            }
        }
        res
    }
}
