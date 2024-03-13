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
* e2ee private message
* LaTex and Code highlighting support without JavaScript
* Markdown support
* inn: Subgroup like Subreddits
* solo: Personal space like Twitter
* https support
* Online rss reader

## Quick start

### From source code

Prerequisition: install [Rust](https://www.rust-lang.org/tools/install)

```bash
git clone https://github.com/freedit-org/freedit
cd freedit && cargo build -r
./target/release/freedit
```

### Using binary

1. Download freedit binary from [releases](https://github.com/freedit-org/freedit/releases)
2. `unzip freedit.zip`
3. `./freedit`

The default port for Freedit is 3001. 

You can access it at <http://127.0.0.1:3001/>

## Installation

As per the quickstart, you now have the latest released unziped, or the latest version compiled as a `freedit` binary in your current diretory.

### Running Freedit as a service

Start by generate the config file, database and other folder by running `./freedit` 

Here is where you could put the files on your system and how to use systemd to run freedit
```BASH
# Create a freedit user
useradd -d /home/freedit -s /bin/bash -m freedit

# Copy the binary with the right ownership
install -o freedit -g freedit freedit /usr/local/bin/freedit

# Create fodlers with right ownership and permissions, copy the config file.
install -o freedit -g freedit -d /var/lib/freedit /var/www/html/freedit
install -m 644 -D config.toml /etc/freedit/config.toml

# Edit and then verify the config is correct
cat /etc/freedit/config.toml 
db = "/var/lib/freedit/freedit.db"
addr = "127.0.0.1:3001"
avatars_path = "/var/www/html/freedit/static/imgs/avatars"
inn_icons_path = "/var/www/html/freedit/static/imgs/inn_icons"
upload_path = "/var/www/html/freedit/static/imgs/upload"
tantivy_path = "/var/lib/freedit/tantivy"
proxy = ""

# Move the default folder where they belong
mv freedit.db /var/lib/freedit/
mv snapshots /home/freedit/
mv tantivy /var/lib/freedit/
mv static/ /var/www/html/freedit/

# change the owner and group of the folders and files
chown -R freedit:freedit /var/lib/freedit/
chown -R freedit:freedit /var/www/html/freedit/
chown -R freedit:freedit /home/freedit/snapshots

# Create systemd service
install -o freedit -g freedit /dev/null /home/freedit/freedit.service
cat <<EOF > /home/freedit/freedit.service
[Unit]
Description=Freedit
After=network.target

[Service]
Type=simple
WorkingDirectory=/home/freedit
User=freedit
Group=freedit
ExecStart=/usr/local/bin/freedit /etc/freedit/config.toml

[Install]
WantedBy=multi-user.target
EOF

# link the service, load it and start it
ln -s /home/freedit/freedit.service /etc/systemd/system/freedit.service
systemctl daemon-reload
systemctl enable freedit.service
systemctl start freedit.service

# Look at the logs
journalctl -u freedit.service -f

# Configure a reverse proxy (here caddy)

cat /etc/caddy/Caddyfile 
# The Caddyfile is an easy way to configure your Caddy web server.
#
# Unless the file starts with a global options block, the first
# uncommented line is always the address of your site.
#
# To use your own domain name (with automatic HTTPS), first make
# sure your domain's A/AAAA DNS records are properly pointed to
# this machine's public IP, then replace ":80" below with your
# domain name.

freedit.example.org {
    reverse_proxy localhost:3001
}

```
## Administration

### Admin account
The first account your signup with will be your priviledge account
<https://freedit.example.org/signup>

### Admin panel
You can edit the behavior of Freedit from the admin panel
<https://freedit.colourful.name/admin>

## Code Documentation

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

## Credits

* icon: <https://iconoir.com/>
* CSS framework: <https://bulma.io/>
* Rust crates: [Cargo.toml](https://github.com/freedit-org/freedit/blob/main/Cargo.toml)
