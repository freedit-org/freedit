use once_cell::sync::Lazy;
use rustls_pemfile::{read_one, Item};
use serde::{Deserialize, Serialize};
use std::fs::{self, read_to_string, File};
use std::io::{BufReader, Write};
use std::path::Path;
use tokio_rustls::rustls::{Certificate, PrivateKey, ServerConfig};
use tracing::{info, warn};

pub static CONFIG: Lazy<Config> = Lazy::new(Config::load_config);

#[derive(Serialize, Deserialize)]
pub struct Config {
    pub db: String,
    pub addr: String,
    pub rebuild_index: Option<bool>,
    pub(crate) avatars_path: String,
    pub(crate) inn_icons_path: String,
    pub(crate) upload_path: String,
    pub(crate) tantivy_path: String,
    pub(crate) proxy: String,
    cert: String,
    key: String,
}

impl Config {
    fn load_config() -> Config {
        let cfg_file = std::env::args()
            .nth(1)
            .unwrap_or_else(|| "config.toml".to_owned());
        let config = if let Ok(config_toml_content) = read_to_string(cfg_file) {
            let config: Config = basic_toml::from_str(&config_toml_content).unwrap();
            config
        } else {
            warn!("Config file not found, using default config.toml");
            let config = Config::default();
            let toml = basic_toml::to_string(&config).unwrap();
            let mut cfg_file = File::create("config.toml").unwrap();
            cfg_file.write_all(toml.as_bytes()).unwrap();
            config
        };

        check_path(&config.avatars_path);
        check_path(&config.inn_icons_path);
        check_path(&config.upload_path);
        check_path(&config.tantivy_path);

        config
    }

    pub async fn tls_config(&self) -> Option<ServerConfig> {
        let mut key_reader = BufReader::new(File::open(&CONFIG.key).ok()?);
        let key = match read_one(&mut key_reader).ok()?? {
            Item::Crl(key) => key.as_ref().to_vec(),
            Item::Pkcs1Key(key) => key.secret_pkcs1_der().to_vec(),
            Item::Pkcs8Key(key) => key.secret_pkcs8_der().to_vec(),
            Item::Sec1Key(key) => key.secret_sec1_der().to_vec(),
            _ => return None,
        };

        let key = PrivateKey(key);

        let mut cert_reader = BufReader::new(File::open(&CONFIG.cert).ok()?);
        let cert = match read_one(&mut cert_reader).ok()?? {
            Item::X509Certificate(cert) => cert,
            _ => return None,
        };

        let certs = vec![Certificate(cert.as_ref().to_vec())];

        ServerConfig::builder()
            .with_safe_defaults()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .ok()
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            db: "freedit.db".into(),
            addr: "127.0.0.1:3001".into(),
            rebuild_index: None,
            avatars_path: "static/imgs/avatars".into(),
            inn_icons_path: "static/imgs/inn_icons".into(),
            upload_path: "static/imgs/upload".into(),
            tantivy_path: "tantivy".into(),
            proxy: "".into(),
            cert: "".into(),
            key: "".into(),
        }
    }
}

/// Create new dir if the path doesn't exist.
fn check_path(path_str: &str) {
    let path = Path::new(path_str);
    if !path.exists() {
        fs::create_dir_all(path).unwrap();
        info!("create path: {}", path_str);
    } else {
        info!("{path_str} is ok");
    }
}
