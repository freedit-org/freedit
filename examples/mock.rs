use once_cell::sync::Lazy;
use reqwest::{header, Client, Response};

const URL: &str = "http://localhost:3001";

static COOKIE: Lazy<String> = Lazy::new(|| {
    let cookie = std::env::var("cookie").expect("env var cookie not set");
    format!("__Host-id={}", cookie)
});

static CLIENT: Lazy<Client> = Lazy::new(|| {
    let mut headers = header::HeaderMap::new();
    headers.insert("Cookie", header::HeaderValue::from_str(&COOKIE).unwrap());

    reqwest::Client::builder()
        .default_headers(headers)
        .build()
        .unwrap()
});

#[tokio::main]
async fn main() {
    let mut handlers = Vec::with_capacity(100);
    for i in 1..=100 {
        let h = tokio::spawn(async move {
            let inn_name = format!("inn{}", i);
            match create_inn(&inn_name).await {
                Ok(_) => {
                    join_inn(i).await;
                    for _ in 0..100 {
                        create_post(i).await;
                    }
                }
                Err(e) => println!("error creating {}: {}", inn_name, e),
            };
        });
        handlers.push(h);
    }

    for h in handlers {
        h.await.unwrap();
    }
}

async fn create_inn(inn_name: &str) -> Result<Response, reqwest::Error> {
    let url = format!("{}/mod/0", URL);
    let about = format!("about_{}", inn_name);
    let description = format!("description_{}", inn_name);
    let params = [
        ("inn_name", inn_name),
        ("about", &about),
        ("description", &description),
        ("topics", "bench"),
        ("inn_type", "Public"),
        ("mods", "1"),
    ];
    CLIENT.post(&url).form(&params).send().await
}

async fn join_inn(iid: u64) {
    let url = format!("{}/inn/{}/join", URL, iid);
    match CLIENT.get(&url).send().await {
        Ok(_) => {}
        Err(e) => eprintln!("{}", e),
    };
}

async fn create_post(iid: u64) {
    let url = format!("{}/post/{}/edit/0", URL, iid);
    let title = format!("inn_{}, auto generate post", iid);
    let description = format!("description_{}", title);
    let params = [
        ("title", title),
        ("tags", "auto".to_owned()),
        ("content", description),
    ];
    match CLIENT.post(&url).form(&params).send().await {
        Ok(_) => {}
        Err(e) => eprintln!("{}", e),
    };
}
