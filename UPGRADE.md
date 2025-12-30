## Upgrade Log

> [!CAUTION]
> 1. You **cannot** upgrade directly from v0.7.x to v0.9.x. You must first upgrade to v0.8.x, then to v0.9.x.
> 2. Always back up your database before performing an upgrade and stop the running instance of Freedit.
> 3. Please view the upgrade examples in the [`examples/`](examples/) directory for more details.
> 4. You need to have Rust installed to run the upgrade scripts. Install Rust from [here](https://www.rust-lang.org/tools/install).

### Upgrading from v0.8.x to v0.9.x

#### Why?

Change the underlying database from sled to fjall.

#### How?

1. `cargo run --example v0_8-v0_9 -- <path to v0.8 db>`
2. Set db path to the new db file in `config.toml` file.
3. Replace the old binary with the new binary.

### Upgrading from v0.7.x to v0.8.x

#### Why?

A lot of technical debt has been paid off in v0.8.x, including:

1. The following trees have been removed: home_pages, lang, pub_keys, inns_private.All relevant data is now stored on the users tree.
2. Support podcast feature.

#### How?

1. `cargo run --example v0_7-v0_8 -- <path to v0.7 db>`
2. Add `podcast_path = "data/podcasts"` and `snapshots_path = "data/snapshots"` in config.toml file.
3. Set db path to the new db file in `config.toml` file.
4. Replace the old binary with the new binary.

