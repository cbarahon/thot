# ADR-0014: Portable verified sandbox run archives

- Status: Accepted for initial local workflow
- Date: 2026-11-23
- Scope: export/import of completed sandbox run evidence

## Context

Professors need to move or archive reports without depending on a live Thot
daemon or carrying reusable VM disks. A loose folder is portable, but copying
it incorrectly or using an unbounded archive importer can lose evidence or
create path traversal and archive expansion risks.

## Decision

Treat the committed run folder as the source of truth. Export only folders
whose run status is complete and whose configuration snapshot, revision,
journal chain, results and evidence verify. Store ZIP entries without
recompression. Exclude no run-folder files; include only regular files and
reject symlinks and special files. The destination must be outside the run
folder and must not already exist.

Import only into a new directory. Reject absolute/traversal paths,
platform-specific separators, duplicate members, symlinks, special files,
compressed members, more than 100,000 entries, and archives or expanded
contents exceeding 128 GiB. Ignore archived permissions; imported directories are private (`0700`)
and files are private (`0600`) on Unix. Verify the extracted run before
accepting it and remove the partial extraction on failure.

An imported archive restores evidence and reportability only. It does not
restore VM disks, secrets, provider leases, delivery-agent credentials,
replay capability, or a guarantee of reproducible execution. Hash chains
detect accidental modification; they are not signatures or authenticity
proofs.

## Consequences

- `thot sandbox export RUN_DIR FILE.zip` and `thot sandbox import FILE.zip
  DEST_DIR` work without `thotd`.
- `thot sandbox report` verifies the imported folder before displaying or
  exporting reports.
- Future schema migrations must be explicit and preserve the original archive.
- Long-term archive signing and data-retention policy remain open.

## Validation

Engine integration tests create, commit, export, import and verify runs, and
reject path traversal and compressed members. A real three-student Alpine
smoke run was also exported and re-imported; the imported CSV retained the
10/2/0 marks. Symlink, duplicate, and decompression-limit cases still need
dedicated negative fixtures.
