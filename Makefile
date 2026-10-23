# Needs GNU make. On Windows without make, run the commands inside each target by hand (see README).
-include .env
export

.PHONY: db-up db-down db-reset migrate run seed test lint fmt openapi

db-up: ## start Postgres and the mail catcher
	docker compose up -d --wait postgres mailpit

db-down: ## stop services (data is kept)
	docker compose down

db-reset: ## DELETE all local data and start from an empty database
	docker compose down -v
	docker compose up -d --wait postgres mailpit
	$(MAKE) migrate

migrate: ## apply migrations as the app_migrator role
	DATABASE_URL="$(MIGRATION_DATABASE_URL)" cargo loco db migrate

run: ## start the API on http://localhost:5150
	cargo loco start

seed: ## demo data (arrives in ticket P1-14)
	@echo "No seed task yet. See ticket P1-14."

test: ## run all tests against brightcone_test
	cargo test --all-features

lint: ## format check + clippy
	cargo fmt --all -- --check
	cargo clippy --all-features -- -D warnings

fmt: ## format code
	cargo fmt --all

openapi: ## regenerate openapi.json after changing the API
	UPDATE_OPENAPI=1 cargo test --test openapi
