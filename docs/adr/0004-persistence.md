# ADR 0004 — Persistence and document identity

- Status: **accepted**
- Date: 2026-11-20

## Context

The product promise is "remembers exactly where you stopped", including after a
crash or a reboot. That needs a store that survives process death, tolerates a
concurrently running second instance, and cannot be fooled by a file being
renamed, copied or edited. It also has to work on a machine with no SQLite
headers, because the project takes no crate dependencies.

## Decisions

### 1. SQLite through `dlopen`, metadata only

`src/db/sql.rs` declares the ~20 sqlite3 entry points the app uses and resolves
them at run time (`libsqlite3.so.0`, `winsqlite3.dll`). No headers, no bindgen,
no vendored amalgamation. If the library is missing the app keeps working with an
in-memory session and says so once.

Only metadata is stored: fingerprints, session state, recent list, highlights,
window geometry. Page bitmaps never touch disk - opening another document relies
on the render cache or a fresh render.

### 2. A document is its bytes

`doc/identity.rs` computes a SHA-256 over the file contents. The fingerprint is
the identity; path, size and modification time are an index for "have I seen this
file before" but never the key. Consequences:

* Renaming or moving a document keeps its reading position.
* Editing a document in place starts a new session (the bytes changed).
* A copy of a document shares the session of the original, which is what most
  readers do and what users expect from "continue where I left off".

The hash costs ~4 ms per MB on the UI thread before the first frame; for very
large files it runs on the worker while the first page is already rendering.

### 3. WAL, `synchronous=NORMAL`, batched writes

`journal_mode=WAL` + `synchronous=NORMAL` gives crash safety without an fsync per
statement. Session updates are accumulated in memory and flushed at most every
2 s, on document close, on quit and from the panic hook. A crash can therefore
lose at most two seconds of scrolling, never the database.

### 4. Forward-only migrations

`schema_version` records the applied version; migrations are small functions
applied in order inside a transaction. Downgrades are refused rather than
silently corrupting newer rows.

### 5. Restore is silent and idempotent

On open, the stored session (page, scroll offset, zoom, rotation, view mode) is
applied as the initial state; there is no "resume?" prompt. If the stored page no
longer exists the nearest valid page is used, and the session is rewritten so the
next start is correct.

## Alternatives considered

* **Plain text/JSON state file** — no concurrent-writer safety, no partial-write
  safety. Rejected.
* **Path as the key** — breaks on rename and on symlinked copies. Rejected.
* **Store rendered pages on disk** — needs a cache-invalidation scheme and
  gigabytes of disk. Rejected; the in-memory budget is enough.
