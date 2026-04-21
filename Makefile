# Exchange workspace — build, lint, test, infra, local runs
# Usage: `make` or `make help`
#
# Prerequisites chain (automatic):
#   env / env-example  → ensure repo .env from .env.example
#   docker-env           → copy .env → docker/.env (compose env_file)
#   prepare              → env + docker-env (use before run-* / docker-up-*)
#   docker-up-infra      → prepare, then Postgres + Redis
#   test-integration     → docker-up-infra, then cargo test

SHELL := /bin/bash
ROOT := $(dir $(abspath $(lastword $(MAKEFILE_LIST))))
COMPOSE_DIR := $(ROOT)docker
COMPOSE_INFRA := $(COMPOSE_DIR)/docker-compose.yml
COMPOSE_CORE := $(COMPOSE_DIR)/docker-compose-core.yml

# Load .env into Make when it exists at parse time (optional).
# Recipes that run cargo also `source` .env in the shell — otherwise a .env created in the
# same invocation by `prepare` is never visible to subprocesses (Make does not re-parse).
-include $(ROOT).env
export

# Shell prefix: export vars from repo .env for one recipe (bash).
ENV_SH = cd "$(ROOT)" && set -a && . ./.env && set +a

.PHONY: help \
	env env-example prepare \
	fmt fmt-check clippy lint lint-fix \
	check build build-release clean \
	test test-lib test-integration test-all \
	docker-env docker-up-infra docker-down-infra docker-logs-infra docker-ps-infra \
	docker-up-core docker-down-core \
	run-db-processor run-engine run-router run-ws-stream run-all-hint \
	ci

help:
	@echo "Exchange — common targets"
	@echo ""
	@echo "  Setup:"
	@echo "    make env             Ensure .env exists (copy from .env.example if missing)"
	@echo "    make env-example     Same as make env"
	@echo "    make docker-env      Copy repo .env → docker/.env (for compose env_file)"
	@echo "    make prepare         env + docker-env (runs before run-* and docker-up-*)"
	@echo ""
	@echo "  Lint / format:"
	@echo "    make fmt             cargo fmt (all workspace crates)"
	@echo "    make fmt-check       cargo fmt --check"
	@echo "    make clippy          cargo clippy --workspace --all-targets"
	@echo "    make lint            fmt-check + clippy (same as CI-style gate)"
	@echo "    make lint-fix        fmt + clippy (writes formatting)"
	@echo ""
	@echo "  Build:"
	@echo "    make check           cargo check --workspace"
	@echo "    make build           cargo build --workspace"
	@echo "    make build-release   cargo build --workspace --release"
	@echo "    make clean           cargo clean"
	@echo ""
	@echo "  Test:"
	@echo "    make test-lib        Unit tests (workspace, --lib)"
	@echo "    make test-integration  prepare + docker-up-infra + router integration tests"
	@echo "    make test-all        test-lib + test-integration"
	@echo ""
	@echo "  Infra (Docker — Postgres + Redis from docker-compose.yml):"
	@echo "    make docker-up-infra   prepare + docker compose up -d"
	@echo "    make docker-down-infra"
	@echo "    make docker-logs-infra"
	@echo "    make docker-ps-infra"
	@echo ""
	@echo "  Full stack in Docker (docker-compose-core.yml):"
	@echo "    make docker-up-core    prepare + compose up -d"
	@echo "    make docker-down-core"
	@echo ""
	@echo "  Run binaries locally (prepare runs first — .env + docker/.env):"
	@echo "    make run-db-processor"
	@echo "    make run-engine"
	@echo "    make run-router"
	@echo "    make run-ws-stream"
	@echo "    make run-all-hint    Print order to start services in separate terminals"
	@echo ""
	@echo "  CI bundle:"
	@echo "    make ci              fmt-check + clippy + test-lib"
	@echo ""
	@echo "Integration tests use .env (see .env.example):"
	@echo "  DATABASE_URL=postgres://root:root@localhost:5000/exchange-db"
	@echo "  REDIS_URL=redis://localhost:6380"
	@echo "  API_KEY=…   (match across services)"

# --- Setup (ordered prerequisites) ---

env: env-example

env-example:
	@if [ -f "$(ROOT).env" ]; then \
		echo ".env already present — not overwriting"; \
	else \
		cp "$(ROOT).env.example" "$(ROOT).env" && echo "Created .env from .env.example"; \
	fi

docker-env: env
	@cp "$(ROOT).env" "$(COMPOSE_DIR)/.env"
	@echo "Synced .env → docker/.env"

prepare: env docker-env

# --- Lint / build ---

fmt:
	cd "$(ROOT)" && cargo fmt --all

fmt-check:
	cd "$(ROOT)" && cargo fmt --all -- --check

clippy:
	cd "$(ROOT)" && cargo clippy --workspace --all-targets

lint: fmt-check clippy

lint-fix: fmt
	cd "$(ROOT)" && cargo clippy --workspace --all-targets --fix --allow-dirty --allow-staged

check:
	cd "$(ROOT)" && cargo check --workspace

build:
	cd "$(ROOT)" && cargo build --workspace

build-release:
	cd "$(ROOT)" && cargo build --workspace --release

clean:
	cd "$(ROOT)" && cargo clean

# --- Test (integration pulls infra via prerequisites) ---

test-lib:
	cd "$(ROOT)" && cargo test --workspace --lib

test-integration: docker-up-infra
	$(ENV_SH) && cargo test -p router --test integration -- --test-threads=1

test-all: test-lib test-integration

# --- Infra ---

docker-up-infra: prepare
	cd "$(COMPOSE_DIR)" && docker compose -f docker-compose.yml up -d

docker-down-infra:
	cd "$(COMPOSE_DIR)" && docker compose -f docker-compose.yml down

docker-logs-infra:
	cd "$(COMPOSE_DIR)" && docker compose -f docker-compose.yml logs -f

docker-ps-infra:
	cd "$(COMPOSE_DIR)" && docker compose -f docker-compose.yml ps

docker-up-core: prepare
	cd "$(COMPOSE_DIR)" && docker compose -f docker-compose-core.yml up -d

docker-down-core:
	cd "$(COMPOSE_DIR)" && docker compose -f docker-compose-core.yml down

# --- Run (prepare ensures .env + docker/.env for compose-style workflows) ---

run-db-processor: prepare
	$(ENV_SH) && cargo run -p db-processor

run-engine: prepare
	$(ENV_SH) && cargo run -p engine

run-router: prepare
	$(ENV_SH) && cargo run -p router

run-ws-stream: prepare
	$(ENV_SH) && cargo run -p ws-stream

run-all-hint:
	@echo "Typical local stack (separate terminals):"
	@echo "  1) make docker-up-infra    # Postgres + Redis (prepare runs first)"
	@echo "  2) make run-db-processor"
	@echo "  3) make run-engine"
	@echo "  4) make run-router"
	@echo "  5) make run-ws-stream      # optional"

ci: fmt-check clippy test-lib

.DEFAULT_GOAL := help
