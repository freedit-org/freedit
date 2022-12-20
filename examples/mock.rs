/// ## README
///
/// You must set `Inn mod max` `Solo interval` `Post interval` and `Comment interval` to 0 to use this example.
/// <http://localhost:3001/admin>
use once_cell::sync::Lazy;
use reqwest::{header, Client, StatusCode};

const URL: &str = "https://localhost:3001";

static COOKIE: Lazy<String> = Lazy::new(|| {
    let cookie = std::env::var("COOKIE").expect("env var COOKIE not set");
    format!("__Host-id={cookie}")
});

static CLIENT: Lazy<Client> = Lazy::new(|| {
    let mut headers = header::HeaderMap::new();
    headers.insert("Cookie", header::HeaderValue::from_str(&COOKIE).unwrap());

    reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .default_headers(headers)
        .build()
        .unwrap()
});

#[tokio::main]
async fn main() {
    let mut handlers = Vec::with_capacity(100);
    for i in 1..=10 {
        let h = tokio::spawn(async move {
            let inn_name = format!("inn_{}_{}", i, rand::random::<u16>());
            match create_inn(&inn_name).await {
                Ok(StatusCode::OK) => {}
                Ok(s) => println!("{s}"),
                Err(e) => println!("error creating {inn_name}: {e}"),
            };
            for _ in 0..10 {
                match create_post(i).await {
                    Err(e) => println!("{e}"),
                    Ok(StatusCode::UNAUTHORIZED) => join_inn(i).await,
                    Ok(StatusCode::OK) => (),
                    Ok(s) => println!("{s}"),
                };
            }
            for j in 0..1000 {
                match create_comment(i, j).await {
                    Err(e) => println!("{e}"),
                    Ok(StatusCode::UNAUTHORIZED) => join_inn(i).await,
                    Ok(StatusCode::OK) => (),
                    Ok(s) => println!("{s}"),
                };
            }
        });
        handlers.push(h);
    }

    for h in handlers {
        h.await.unwrap();
    }
}

async fn create_inn(inn_name: &str) -> Result<StatusCode, reqwest::Error> {
    let url = format!("{URL}/mod/0");
    let about = format!("about_{inn_name}");
    let description = format!("description_{inn_name}");
    let params = [
        ("inn_name", inn_name.to_owned()),
        ("about", about),
        ("description", description),
        ("topics", "bench".to_owned()),
        ("inn_type", "Public".to_owned()),
        ("early_birds", "5".to_owned()),
    ];
    send_post(&url, &params).await
}

async fn join_inn(iid: u32) {
    let url = format!("{URL}/inn/{iid}/join");
    match CLIENT.get(&url).send().await {
        Ok(_) => {}
        Err(e) => eprintln!("{e}"),
    };
}

async fn create_post(iid: u32) -> Result<StatusCode, reqwest::Error> {
    let url = format!("{URL}/post/{iid}/edit/0");
    let title = format!("inn_{iid}, auto generate post");
    let description = format!("description_{title}");
    let params = [
        ("title", title),
        ("tags", "auto".to_owned()),
        ("content", description),
    ];
    send_post(&url, &params).await
}

async fn create_comment(iid: u32, pid: u32) -> Result<StatusCode, reqwest::Error> {
    let url = format!("{URL}/post/{iid}/{pid}");
    let comment = format!("pid_{pid}, auto generate post");
    let params = [("content", comment)];
    send_post(&url, &params).await
}

async fn send_post(url: &str, params: &[(&str, String)]) -> Result<StatusCode, reqwest::Error> {
    CLIENT
        .post(url)
        .form(&params)
        .send()
        .await
        .map(|r| r.status())
}
