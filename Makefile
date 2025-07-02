# Maestro Symphony Makefile

APP_NAME = maestro-symphony
CONFIG ?= examples/testnet.toml
RUST_LOG ?= info
COMPOSE_FILE ?= docker-compose.yml

.PHONY: all build run sync serve docker-up docker-down clean fmt lint help

all: build

build:
	cargo build --release

run:
	RUST_LOG=$(RUST_LOG) cargo run -- $(CONFIG) run

sync:
	RUST_LOG=$(RUST_LOG) cargo run -- $(CONFIG) sync

serve:
	RUST_LOG=$(RUST_LOG) cargo run -- $(CONFIG) serve

docker-up:
	docker compose -f $(COMPOSE_FILE) up -d

docker-down:
	docker compose -f $(COMPOSE_FILE) down

docker-ps:
	docker compose -f $(COMPOSE_FILE) ps

fmt:
	cargo fmt --all

lint:
	cargo clippy --all-targets --all-features -- -D warnings

clean:
	cargo clean

help:
	@echo "Available targets:"
	@echo "  build        Build the project (release mode)"
	@echo "  run          Sync and serve using $(CONFIG) (default: examples/testnet.toml)"
	@echo "  sync         Sync only using $(CONFIG)"
	@echo "  serve        Serve only using $(CONFIG)"
	@echo "  docker-up    Start stack with Docker Compose (default: docker-compose.yml, override with COMPOSE_FILE=...)"
	@echo "  docker-down  Stop stack with Docker Compose (default: docker-compose.yml, override with COMPOSE_FILE=...)"
	@echo "  docker-ps    Show running containers for the selected Compose file (override with COMPOSE_FILE=...)"
	@echo "  fmt          Format code with rustfmt"
	@echo "  lint         Lint code with clippy"
	@echo "  clean        Clean build artifacts"
	@echo "  help         Show this help message"
