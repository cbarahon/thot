# ADR-0009: Read-only deliverable intake agents

- Status: Accepted
- Date: 2026-11-23

## Context

The sandbox runner previously addressed submissions as paths inside the project directory. This ties assignments to a local folder and prevents using instructor-owned Google Drive submissions. A sandbox should receive a staged copy of a named submission, not credentials or direct access to the source service.

## Decision

Introduce a `thot-delivery` crate with one read-only interface: `list` returns named metadata and `get(name, destination)` retrieves exactly one submission. Implement a local filesystem agent and a Google Drive agent. Keep selection in the v2 project YAML as an optional tagged `delivery_agent` section. With no section, use local intake, preferring `delivery_folder`, then the legacy student input path-template root, then `delivery`.

Assignments may identify a deliverable by its exact name or, when the source exposes one, its stable provider ID. Exact-name duplicates fail safely. The runner applies project `delivery_limits` (defaults 128 MiB/10,000 files; supported ceilings 64 GiB/100,000 files), rejects symlinks in local sources, computes SHA-256, and stages the acquired bytes before sandbox transfer. Google Drive listings retain ID, revision/version, MIME type and provider checksum when available; downloads stream to disk while SHA-256 is computed. Results retain source selector and metadata plus the acquired digest.

Google Drive uses an installed-app OAuth flow with a loopback callback and a one-use state value. The agent requests read-only `drive.readonly`, caches credentials outside project data with mode 0600, and supports refresh tokens. Client credentials are provided initially by environment variables and retained in that private user token cache. No secrets enter project YAML or run artifacts.

## Consequences

- The same assignment format works with local folders and Google Drive.
- Professors can inspect an inventory before matching students and retrieve an individual item through CLI commands.
- Google Drive API calls require network access and a user-authorized OAuth client. The requested read-only scope is broad across the professor's Drive; Google Drive native documents, nested folders, and shortcuts are currently excluded.
- The CLI's sandbox workflow uses the abstraction; the separate legacy daemon `thot run create` intake still uses local `delivery_folder` and needs a later migration.
- Assignments with duplicate remote names cannot be resolved by name; the professor must select a provider ID.
- Large-file download is streaming but not resumable; there is not yet a reserved project-wide disk quota or end-to-end large OVA qualification.
