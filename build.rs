fn main() {
    match std::env::var("PROFILE") {
        Ok(key) if key.as_str() == "debug" => println!("cargo:rustc-env=PROFILE=debug"),
        _ => {}
    }
}
