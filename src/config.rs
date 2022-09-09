use axum_server::tls_rustls::RustlsConfig;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::fs::{read_to_string, File};
use std::io::Write;
use tracing::{error, warn};

pub(crate) static CONFIG: Lazy<Config> = Lazy::new(Config::load_config);

#[derive(Serialize, Deserialize)]
pub(crate) struct Config {
    pub(crate) db: String,
    pub(crate) addr: String,
    pub(crate) avatars_path: String,
    pub(crate) inn_icons_path: String,
    pub(crate) upload_path: String,
    pub(crate) html_path: String,
    pub(crate) serve_dir: Vec<(String, String, String)>,
    cert: String,
    key: String,
}

impl Config {
    fn load_config() -> Config {
        let cfg_file = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_owned());
        if let Ok(config_toml_content) = read_to_string(cfg_file) {
            let config: Config = toml::from_str(&config_toml_content).unwrap();
            config
        } else {
            warn!("Config file not found, using default config.toml");
            let config = Config::default();
            let toml = toml::to_string_pretty(&config).unwrap();
            let mut cfg_file = File::create("config.toml").unwrap();
            cfg_file.write_all(toml.as_bytes()).unwrap();
            config
        }
    }

    pub(crate) async fn tls_config(&self) -> Option<RustlsConfig> {
        if let Ok(rustls_config) = RustlsConfig::from_pem_file(&CONFIG.cert, &CONFIG.key).await {
            Some(rustls_config)
        } else {
            error!("enable https failed, please check config toml cert and key");
            None
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            db: "freedit.db".into(),
            addr: "127.0.0.1:3001".into(),
            avatars_path: "./static/imgs/avatars".into(),
            inn_icons_path: "./static/imgs/inn_icons".into(),
            upload_path: "./static/imgs/upload".into(),
            html_path: "./static/html".into(),
            serve_dir: vec![],
            cert: "".into(),
            key: "".into(),
        }
    }
}
