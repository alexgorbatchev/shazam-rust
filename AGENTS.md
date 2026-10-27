---
created_on: 2026-10-14 06:11
last_modified: 2026-10-14 06:11
status: current
---

# shazam-client

This is a standalone Rust library. Apply the parent Rust project instructions.

## Commands

- `just build`: build all targets.
- `just test`: run behavioral and documentation tests.
- `just fmt`: format only this package.
- `just lint`: check package formatting and Clippy with warnings denied.
- `just coverage`: require at least 90% line coverage with cargo-llvm-cov and llvm-tools.
- `just check`: run lint, tests, and coverage.

## Contracts

- Preserve native signature magic, length fields, little-endian data, sample-rate encoding, padded band records, CRC32, and base64 data URIs. Validate supplied bounds before reading or allocating.
- Keep DSP output grounded in the original algorithm and source-generated golden fixtures. Use RustFFT's planned transform and scratch buffers directly.
- Validate PCM at construction and preserve wide downmix sums before narrowing. Reset must discard all pending input.
- Preserve native reqwest proxy, decompression, timeout, header, and JSON behavior. Retry only the documented transport failures and transient status classes, retaining request bodies and headers.
- Cancellation must interrupt HTTP attempts, backoff, reader operations, and subprocesses. Keep asynchronous work independent of process-global environment changes.
- Resolve charts case-insensitively and share cached location fetches. Return owned or immutable cached metadata.
- Preserve unknown catalog and recognition fields. Only classify music service URLs using parsed host boundaries; only actual ringtone actions may populate ringtone fields.
- Verify transport through real local HTTP servers and decoding through real subprocesses. Keep live service verification separate from local results.
- Use meaningful red/green tests, temporarily disable implementation to verify failure, restore it, and run all checks. Runtime code changes require test-file changes and at least 90% line coverage.
- Keep temporary files and evidence in `.tmp/`. Keep standing policy here; keep progress and assignments elsewhere.
- Preserve copyright and MIT notices. Never publish packages, releases, or tags without explicit authorization.
