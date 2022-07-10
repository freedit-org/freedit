# freedit

The safest and lightest forum, powered by rust.

GitHub: <https://github.com/freedit-org/freedit>

## Features

* No javascript at all, for safety maximization. ([Why javascript is evil](https://thehackernews.com/2022/05/tails-os-users-advised-not-to-use-tor.html))
* Use embedded database [sled](https://github.com/spacejam/sled), easy to deploy, high performance.
* Powered by Rust, with code highlighting, markdown and latex support.
* Sub communities like reddit, multicentered; Personal space like twitter.

## Warnings
Project is in very early stage, please do not use for production。

## Usage

### From binary

1. Download freedit binary from [releases](https://github.com/freedit-org/freedit/releases)
2. unzip freedit.zip
3. create a config file named `config.toml`, [example](https://github.com/freedit-org/freedit/blob/main/config.toml)
4. run `./freedit`, open browser to `addr`, <http://127.0.0.1:3001/>

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