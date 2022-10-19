# freedit

[![CI](https://github.com/freedit-org/freedit/actions/workflows/ci.yml/badge.svg)](https://github.com/freedit-org/freedit/actions/workflows/ci.yml)
[![release](https://github.com/freedit-org/freedit/actions/workflows/release.yml/badge.svg)](https://github.com/freedit-org/freedit/releases)
[![Doc](https://img.shields.io/github/deployments/freedit-org/freedit/github-pages?label=doc)](https://freedit-org.github.io/freedit/freedit/index.html)

The safest and lightest forum, powered by rust.

Demo: <https://freedit.eu/>

GitHub: <https://github.com/freedit-org/freedit>

## Features

* Easy to deploy: one binary to run, using embedded database [sled](https://github.com/spacejam/sled) 
* No javascript at all, for safety maximization. ([Why javascript is evil](https://thehackernews.com/2022/05/tails-os-users-advised-not-to-use-tor.html))
* LaTex and Code highlighting support without JavaScript
* Markdown support
* inn: Subgroup like Subreddits
* solo: Personal space like Twitter
* https support

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

## Documentation

* online doc: <https://freedit-org.github.io/freedit/freedit/index.html>

* generate local documentation:
```bash
cargo doc --no-deps --open
```

## Development

```bash
git clone https://github.com/freedit-org/freedit
cd freedit && cargo run
```

integration testing
```bash
COOKIE=<cookie> cargo run --example mock
```

## Credits

* icon: <https://iconoir.com/>
* CSS framework: <https://bulma.io/>
* Rust crates: [Cargo.toml](./Cargo.toml)