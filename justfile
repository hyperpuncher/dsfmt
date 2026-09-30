check:
    cargo fmt --check
    cargo clippy --locked --all-targets -- -D warnings

full: check
    cargo test --locked --all-targets

build:
    cargo build --locked --release

fix:
    cargo clippy --fix --allow-dirty --all-targets
    cargo fmt

test:
    cargo test --locked --all-targets

bench:
    cargo run --locked --release --example benchmark

update:
    cargo update
