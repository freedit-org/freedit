# freedit

The safest and lightest forum, powered by rust.

## Features

* No javascript at all, for safety maximization. ([Why javascript is evil](https://thehackernews.com/2022/05/tails-os-users-advised-not-to-use-tor.html))
* Use embedded database [sled](https://github.com/spacejam/sled), easy to deploy, high performance.
* Powered by Rust, with code highlighting, markdown and latex support.
* Sub communities like reddit, multicentered; Personal space like twitter.

## Warnings
Project is in very early stage, please do not use for production。

## Usage

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
git clone https://github.com/freedit-org/freedit.git
git clone https://github.com/freedit-org/doc.git
cd freedit && cargo doc --target-dir ../doc --no-deps
```