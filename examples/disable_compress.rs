// Cargo.toml
// sled = { version = "0.34.7", features = ["compression"] }

use std::fs::rename;

fn main() {
    let old_config = sled::Config::default()
        .path("freedit.db")
        .use_compression(true);
    let old = old_config.open().unwrap();

    let new_config = sled::Config::default().path("new.db");
    let new = new_config.open().unwrap();

    new.import(old.export());

    let new_cksum = new.checksum().unwrap();
    let old_cksum = old.checksum().unwrap();

    assert_eq!(new_cksum, old_cksum);
    rename("freedit.db", "old").unwrap();
    rename("new.db", "freedit.db").unwrap();
}
