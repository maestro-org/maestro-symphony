# Maestro Symphony Makefile

APP_NAME = maestro-symphony
CONFIG ?= examples/testnet.toml
RUST_LOG ?= info
COMPOSE_FILE ?= docker-compose.yml

.PHONY: all build run sync serve compose-up compose-down clean fmt fmt-check lint install-hooks help

all: build

build:
	cargo build --release

run:
	RUST_LOG=$(RUST_LOG) cargo run -- $(CONFIG) run

sync:
	RUST_LOG=$(RUST_LOG) cargo run -- $(CONFIG) sync

serve:
	RUST_LOG=$(RUST_LOG) cargo run -- $(CONFIG) serve

openapi:
	RUST_LOG=$(RUST_LOG) cargo run -- $(CONFIG) docs

compose-up:
	docker compose -f $(COMPOSE_FILE) up -d

compose-down:
	docker compose -f $(COMPOSE_FILE) down

docker-ps:
	docker compose -f $(COMPOSE_FILE) ps

fmt:
	cargo fmt --all

fmt-check:
	cargo fmt --all -- --check

lint:
	cargo clippy --all-targets --all-features -- -D warnings

install-hooks:
	git config core.hooksPath .githooks

clean:
	cargo clean

publish:
	cargo publish --locked --no-verify

help:
	@echo "Available targets:"
	@echo "  build        Build the project (release mode)"
	@echo "  run          Sync and serve using $(CONFIG) (default: examples/testnet.toml)"
	@echo "  sync         Sync only using $(CONFIG)"
	@echo "  serve        Serve only using $(CONFIG)"
	@echo "  compose-up   Start stack with Docker Compose (default: docker-compose.yml, override with COMPOSE_FILE=...)"
	@echo "  compose-down Stop stack with Docker Compose (default: docker-compose.yml, override with COMPOSE_FILE=...)"
	@echo "  docker-ps    Show running containers for the selected Compose file (override with COMPOSE_FILE=...)"
	@echo "  fmt          Format code with rustfmt"
	@echo "  fmt-check    Check formatting with rustfmt"
	@echo "  lint         Lint code with clippy"
	@echo "  install-hooks Configure Git to use the repository hooks in .githooks"
	@echo "  clean        Clean build artifacts"
	@echo "  help         Show this help message"
