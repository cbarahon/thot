# Sandbox contract fixtures

`module-manifest.json` is a metadata-only fixture for the shared P1 module
identity contract. Its `package_sha256` matches
`module-package.fixture`; it is not an executable module and does not define
the portable module ABI. The host engine and Alpine guest-agent crates parse
the same manifest through `thot-core::sandbox_contracts`.

Module references pin `(module_id, version, package_sha256, manifest_version)`.
Capabilities are versioned IDs, not arbitrary command strings. OVA operation
types are defined centrally in `TestOperation`. The eventual module runtime
will negotiate these typed capabilities with the provider; P1 does not ship a
runtime or claim that OVA operations can already execute.
