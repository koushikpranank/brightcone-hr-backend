# Needs GNU make. On Windows without make, run the commands inside each target by hand (see README).
-include .env
export

.PHONY: run seed test lint fmt openapi

run: ## start the API on http://localhost:5150
	cargo run --bin brightcone_hr_backend-cli -- start

seed: ## demo data (arrives in ticket P1-14)
	@echo "No seed task yet. See ticket P1-14."

test: ## run all tests against the isolated brightcone_test database
	cargo test --all-features

lint: ## format check + clippy
	cargo fmt --all -- --check
	cargo clippy --all-features -- -D warnings

fmt: ## format code
	cargo fmt --all

openapi: ## regenerate openapi.json after changing the API
	UPDATE_OPENAPI=1 cargo test --test openapi
