.PHONY: help build test fmt lint deploy

DEPLOY_BRANCH ?= main
RELEASE_VERSION := $(shell awk '/^\[workspace\.package\]$$/ { in_workspace=1; next } /^\[/ { in_workspace=0 } in_workspace && /^version = / { gsub(/[" ]/, "", $$3); print $$3; exit }' Cargo.toml)
RELEASE_TAG := v$(RELEASE_VERSION)

help:
	@echo "Objetivos disponibles:"
	@echo "  build  - Compilar en modo release"
	@echo "  test   - Ejecutar pruebas"
	@echo "  fmt    - Formatear código Rust"
	@echo "  lint   - Analizar código Rust"
	@echo "  deploy - Compilar y publicar la etiqueta $(RELEASE_TAG) para crear un release GitHub"

build:
	cargo build --workspace --release

test:
	cargo test --workspace

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

# Publish a clean commit already present on the remote release branch. GitHub
# Actions builds and publishes the downloadable release; this target never
# connects to a deployment server.
deploy:
	@test -n "$(RELEASE_VERSION)" || { echo "Could not read workspace version from Cargo.toml" >&2; exit 1; }
	@printf '%s\n' "$(RELEASE_VERSION)" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$$' || { echo "Workspace version must be stable MAJOR.MINOR.PATCH for a release" >&2; exit 1; }
	@test "$$(git branch --show-current)" = "$(DEPLOY_BRANCH)" || { echo "Run make deploy from branch $(DEPLOY_BRANCH)" >&2; exit 1; }
	@test -z "$$(git status --porcelain)" || { echo "Commit or discard all changes before deploying" >&2; exit 1; }
	@git fetch origin --tags
	@test "$$(git rev-parse HEAD)" = "$$(git rev-parse origin/$(DEPLOY_BRANCH))" || { echo "Push or merge this commit to origin/$(DEPLOY_BRANCH) before deploying" >&2; exit 1; }
	@if git show-ref --verify --quiet "refs/tags/$(RELEASE_TAG)" || git ls-remote --exit-code --tags --refs origin "refs/tags/$(RELEASE_TAG)" >/dev/null 2>&1; then echo "Release tag $(RELEASE_TAG) already exists; bump workspace version first" >&2; exit 1; fi
	@cargo build --release --locked -p thot-cli -p thotd || { \
		echo "If the workspace version changed, run cargo check --offline --workspace and commit/push the updated Cargo.lock before retrying." >&2; \
		exit 1; \
	}
	@git tag -a "$(RELEASE_TAG)" -m "Thot $(RELEASE_TAG)"
	@if git push origin "refs/tags/$(RELEASE_TAG)"; then \
		echo "Pushed $(RELEASE_TAG). GitHub Actions will publish the release."; \
		echo "Installer: https://github.com/cbarahon/thot/releases/latest/download/thot-install.sh"; \
	else \
		git tag -d "$(RELEASE_TAG)" >/dev/null; \
		exit 1; \
	fi

