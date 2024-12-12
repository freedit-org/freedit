export PATH := "./node_modules/.bin:" + env_var('PATH')

default:
    @just --choose

build:
    #!/usr/bin/env -S parallel --shebang --ungroup --jobs {{ num_cpus() }}
    just build-client
    just build-server

[working-directory: 'apps/client']
build-client:
    bun tsc -b && bun vite build

build-server:
    cargo build -r

dev:
    @echo "Starting freeding server and web client..."
    just dev-client &
    just dev-server

[working-directory: 'apps/client']
dev-client:
    bun vite dev

dev-server:
    cargo run

preview:
    vite preview
