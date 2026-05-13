check:
    cargo fmt
    cargo clippy --all-targets

full:
    cargo fmt
    cargo clippy --all-targets
    cargo test --all-targets

build:
    cargo build --release

fix:
    cargo clippy --fix --allow-dirty --lib --tests
    cargo fmt

test:
    cargo test --all-targets
