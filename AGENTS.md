# Repository Guidelines

## Project Structure

Thot is a Rust Cargo workspace. Reusable crates live in `crates/`: `thot-core` holds domain types, `thot-engine` implements execution and assessment, `thot-api` exposes HTTP routes, and `thot-runner` handles execution backends. Applications are in `apps/thotd` (service) and `apps/thot-cli` (CLI). Engine tests are mainly colocated in source modules and `crates/thot-engine/src/tests/`; API integration tests are in `crates/thot-api/tests/`. Documentation, examples, deployment assets, and development tooling live in `docs/`, `examples/`, `deploy/`, and `dev/` respectively.

## Build, Test, and Development

- `make build` builds all workspace crates in release mode.
- `make test` runs workspace tests with Cargo.
- `make fmt` formats all Rust code using `cargo fmt`.
- `make lint` runs Clippy across workspace targets and treats warnings as errors.
- For a quicker local build, use `cargo check --workspace`.

VirtualBox-related integration behavior requires a suitable Linux host and VirtualBox installation; ordinary unit tests do not establish that VM integration works on a host.

## Coding Style

Use Rust 2024 conventions and let `rustfmt` set layout (four-space indentation). Keep modules and functions focused, use `snake_case` for modules/functions/variables and `UpperCamelCase` for types, and add tests alongside the code they cover. Keep product crates independent of tooling in `dev/`. Run `make fmt` and `make lint` before submitting Rust changes.

## Testing Guidelines

Add unit tests for behavior changes and integration tests for public API or cross-crate behavior. Use descriptive test names that state the expected behavior; tests use Rust's built-in `#[test]` and `#[tokio::test]` attributes. Run `make test`; when changing VM execution, also validate on a configured VirtualBox host and describe that environment in the change summary. No coverage threshold is configured in the repository.

## Commits and Pull Requests

Recent commits use short, direct subject lines, usually phrased as an action (for example, “Improve grade report readability”). Keep each commit focused. A pull request should explain the behavior change and motivation, list relevant validation commands and results, link related issues when available, and call out configuration, API, or deployment changes. Include screenshots for user-visible web or CLI presentation changes where useful.

## Configuration and Sensitive Data

Do not commit credentials, real student data, or machine-specific VM paths. Treat project test programs as trusted code: they run with the daemon account's permissions. Keep runtime and host setup changes documented in `deploy/` or the relevant application README.
