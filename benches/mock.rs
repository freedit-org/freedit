#![feature(test)]
extern crate test;

use once_cell::sync::Lazy;
use reqwest::{blocking::Client, header};
use test::Bencher;

const COOKIE: &str = "62df8540#tc_xeKLGAjPl3AO2PSebb";
const URL: &str = "https://localhost:3001";

static CLIENT: Lazy<Client> = Lazy::new(|| {
    let mut headers = header::HeaderMap::new();
    let cookie = format!("__Host-id={}", COOKIE);
    headers.insert("Cookie", header::HeaderValue::from_str(&cookie).unwrap());

    reqwest::blocking::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
});

#[bench]
fn bench_create_inn(b: &mut Bencher) {
    b.iter(create_inn)
}

fn create_inn() {
    let url = format!("{}/mod/0", URL);
    let i = rand::random::<u16>();
    let inn_name = format!("inn_{}", i);
    let about = format!("about_{}", i);
    let description = format!("description_{}", i);
    let params = [
        ("inn_name", inn_name),
        ("about", about),
        ("description", description),
        ("topics", "bench".into()),
        ("inn_type", "Public".into()),
        ("mods", "1".into()),
    ];
    match CLIENT.post(&url).form(&params).send() {
        Ok(_) => println!("{}", i),
        Err(e) => println!("{}", e),
    };
}
