# freedit

[![CI](https://github.com/freedit-org/freedit/actions/workflows/ci.yml/badge.svg)](https://github.com/freedit-org/freedit/actions/workflows/ci.yml)
[![Github Release](https://github.com/freedit-org/freedit/actions/workflows/release.yml/badge.svg)](https://github.com/freedit-org/freedit/actions/workflows/release.yml)
[![Release](https://img.shields.io/github/v/release/freedit-org/freedit.svg?sort=semver)](https://github.com/freedit-org/freedit/releases)
[![Doc](https://img.shields.io/github/deployments/freedit-org/freedit/github-pages?label=doc)](https://freedit-org.github.io/freedit/freedit/index.html)
[![License](https://img.shields.io/github/license/freedit-org/freedit)](https://github.com/freedit-org/freedit/blob/main/LICENSE)

The safest and lightest forum, powered by rust.

Demo: <https://freedit.eu/>

GitHub: <https://github.com/freedit-org/freedit>

## Features

- **Single-binary deployment** – easy to run with an embedded [sled](https://github.com/spacejam/sled) database.  
- **Works without JavaScript** – fully functional and secure even with JavaScript disabled. ([Why avoiding JavaScript matters](https://thehackernews.com/2022/05/tails-os-users-advised-not-to-use-tor.html))  
- **End-to-end encrypted private messages** – only sender and recipient can read.  
- **Math & code highlighting** – rich formatting without JavaScript.  
- **Markdown support** – write in clean, readable syntax.  
- **Inn** – group spaces, like Subreddits.  
- **Solo** – personal microblogging, like Twitter.  
- **Online RSS reader** – follow and read feeds in one place.  
- **Podcast support** – subscribe and listen directly through the platform.  

## Usage

### From binary

1. Download freedit binary from [releases](https://github.com/freedit-org/freedit/releases)
2. unzip freedit.zip
3. run `./freedit`, open browser to `addr`, <http://127.0.0.1:3001/>

### From source code

Prerequisition: install [Rust](https://www.rust-lang.org/tools/install)

```bash
git clone https://github.com/freedit-org/freedit
cd freedit && cargo build -r
./target/release/freedit
```

## FAQ

### How to generate podcast srt file?

[whisper](https://github.com/openai/whisper) is used.

* Usage: `whisper ~/podcasts/1.mp3 --word_timestamps True  --highlight_words True -f srt`

## Documentation

* online doc: <https://freedit-org.github.io/freedit/freedit/index.html>

* generate local documentation:
```bash
cargo doc --no-deps --document-private-items --open
```

## Development

```bash
git clone https://github.com/freedit-org/freedit
cd freedit && cargo run
```

## Credits

* icon: <https://iconoir.com/>
* CSS framework: <https://bulma.io/>
* Rust crates: [Cargo.toml](https://github.com/freedit-org/freedit/blob/main/Cargo.toml)
