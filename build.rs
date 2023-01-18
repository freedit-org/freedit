use std::process::Command;

fn main() {
    match std::env::var("PROFILE") {
        Ok(key) if key.as_str() == "debug" => println!("cargo:rustc-env=PROFILE=debug"),
        _ => {}
    }

    let mut git_hash = match Command::new("git").args(["rev-parse", "HEAD"]).output() {
        Ok(output) if !output.stdout.is_empty() => {
            String::from_utf8_lossy(&output.stdout).to_string()
        }
        _ => "not found".into(),
    };

    git_hash.truncate(7);
    println!("cargo:rustc-env=GIT_HASH={git_hash}");
}
