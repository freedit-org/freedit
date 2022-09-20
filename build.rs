fn main() {
    if std::env::var("PROFILE").unwrap().as_str() == "debug" {
        println!("cargo:rustc-env=PROFILE=debug")
    }
}
