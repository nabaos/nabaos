.PHONY: build build-arm build-all test clippy fmt fmt-check docker run clean docs docs-serve install

build:
	PATH="$$HOME/.cargo/bin:/usr/bin:/usr/local/bin:$$PATH" cargo build --release

build-arm:
	PATH="$$HOME/.cargo/bin:/usr/bin:/usr/local/bin:$$PATH" cross build --release --target aarch64-unknown-linux-musl

build-all: build build-arm

test:
	cargo test

clippy:
	cargo clippy -- -D warnings

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

docker:
	docker build -t nabaos .

run:
	cargo run --release -- daemon

clean:
	cargo clean

docs:
	mdbook build docs/book

docs-serve:
	mdbook serve docs/book --open

install:
	cargo install --path .
