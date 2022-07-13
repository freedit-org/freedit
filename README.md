# freedit

[![clippy](https://github.com/freedit-org/freedit/actions/workflows/rust-clippy.yml/badge.svg)](https://github.com/freedit-org/freedit/actions/workflows/rust-clippy.yml)
[![release](https://github.com/freedit-org/freedit/actions/workflows/release.yml/badge.svg)](https://github.com/freedit-org/freedit/releases)
[![MIT](https://img.shields.io/badge/license-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Doc](https://img.shields.io/badge/Doc-Latest-success)](https://freedit-org.github.io/doc/doc/freedit/index.html)

The safest and lightest forum, powered by rust.

GitHub: <https://github.com/freedit-org/freedit>

## Features

* No javascript at all, for safety maximization. ([Why javascript is evil](https://thehackernews.com/2022/05/tails-os-users-advised-not-to-use-tor.html))
* Use embedded database [sled](https://github.com/spacejam/sled), easy to deploy, high performance.
* Powered by Rust, with code highlighting, markdown and latex support.
* Sub communities like reddit, multicentered; Personal space like twitter.

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

* online doc: <https://freedit-org.github.io/doc/doc/freedit/index.html>

* generate local documentation:
```bash
cargo doc --target-dir ../doc --no-deps --open
```