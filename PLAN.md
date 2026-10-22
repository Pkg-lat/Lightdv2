# PLAN — Security hardening (Lightd v2)

**Goal:** Close every confirmed finding from the security review, including a real
per-tenant ownership model (the review's root cause: authorization was flat, so
"who owns this container" had no answer anywhere in the code).

**Approach:** sequential vertical slices, one commit each, on `fix/security-hardening`.
Each slice keeps the tree coherent. Ordering front-loads the host-root chain so the
tree is meaningfully safer even if later slices need rework.

## Slices

| # | Slice | Files |
|---|-------|-------|
| S1 | **Secrets out of git** — strip live token, add `config.example.json`, gitignore, `git rm --cached` | `config.json`, `.gitignore`, `config.example.json`, `tests/*.js`, `docs/*.md` |
| S2 | **Route + auth hardening** — protect `/remote/config`, gate `/auth/tokens`, redact secrets from `Config` serialization, fix `middleware.rs:11` syntax error | `router/remote.rs`, `router/auth.rs`, `config/config.rs`, `auth/middleware.rs`, `main.rs` |
| S3 | **Input validation** — `internal_id`/`volume_id` format, mount **source** allowlist to storage root | `router/container.rs`, `container/update.rs`, `container/lifecycle.rs`, `container/network.rs` |
| S4 | **Ownership model** — `ContainerState.owner`, owner-carrying tokens, `Principal` extractor, owner-scoped manager lookups, REST + WS authz | `container/state.rs`, `container/manager.rs`, `auth/*`, `router/container.rs`, `router/sftp.rs`, `websocket/handler.rs`, `main.rs` |
| S5 | **Runtime** — actually run containers as the non-root `lightd` user (the step that never happens) | `container/lifecycle.rs`, `container/user.rs` |
| S6 | **Network** — atomic port reserve, port ownership, firewall restart reapply, drop hardcoded bridge name | `network/pool.rs`, `network/firewall.rs`, `router/network.rs`, `router/container.rs` |
| S7 | **Robustness / DoS** — auth rate limit, WS connection cap + cleanup wiring, billing prune, SFTP handle + connection caps, `servers/list.rs` char-boundary panic, auth error-body leak | `auth/middleware.rs`, `websocket/*`, `billing/tracker.rs`, `sftp/*`, `servers/list.rs` |
| S8 | **Hygiene** — remove dead `container/user.rs` spawn surface, misc | `container/user.rs` |

## Verification steps

1. `cargo check` — must pass (currently **fails** at HEAD: `auth/middleware.rs:11` missing `;`)
2. `cargo test` — unit tests, including new security tests
3. `cargo clippy -- -D warnings`
4. Negative tests added for: path traversal, mount-source escape, cross-tenant access

> **⚠ NOT RUN IN THIS SESSION.** No Rust toolchain is installed and the user
> declined installation. Nothing here is compiler-verified. `cargo check` /
> `cargo test` MUST pass before merge — expect type errors from the
> cross-cutting signature changes in S4.

## Status

- S1 pending
- S2 pending
- S3 pending
- S4 pending
- S5 pending
- S6 pending
- S7 pending
- S8 pending