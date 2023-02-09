#![doc = include_str!("../README.md")]

pub mod app_router;
pub mod config;
pub mod controller;
pub mod error;

use std::{
    env,
    fs::File,
    io::{BufReader, Read},
};

use data_encoding::HEXLOWER;
use once_cell::sync::Lazy;
use ring::digest::{Context, Digest, SHA256};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const GIT_COMMIT: &str = env!("GIT_COMMIT");

/// Returns SHA256 of the current running executable.
/// Cookbook: [Calculate the SHA-256 digest of a file](https://rust-lang-nursery.github.io/rust-cookbook/cryptography/hashing.html)
pub static CURRENT_SHA256: Lazy<String> = Lazy::new(|| {
    fn sha256_digest<R: Read>(mut reader: R) -> Digest {
        let mut context = Context::new(&SHA256);
        let mut buffer = [0; 1024];

        loop {
            let count = reader.read(&mut buffer).unwrap();
            if count == 0 {
                break;
            }
            context.update(&buffer[..count]);
        }
        context.finish()
    }

    let file = env::current_exe().unwrap();
    let input = File::open(file).unwrap();
    let reader = BufReader::new(input);
    let digest = sha256_digest(reader);

    HEXLOWER.encode(digest.as_ref())
});
