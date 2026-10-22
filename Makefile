.PHONY: help build test fmt lint

help:
	@echo "Objetivos disponibles:"
	@echo "  build  - Compilar en modo release"
	@echo "  test   - Ejecutar pruebas"
	@echo "  fmt    - Formatear código Rust"
	@echo "  lint   - Analizar código Rust"

build:
	cargo build --workspace --release

test:
	cargo test --workspace

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

