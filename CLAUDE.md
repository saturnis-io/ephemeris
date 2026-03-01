# Ephemeris — Project Instructions

## What This Is

Saturnis Ephemeris: an open-core Track & Trace / serialization engine built in Rust.
See `docs/plans/2026-03-01-ephemeris-architecture-design.md` for the full architecture.

## DEALBREAKERS: License Firewall Rules

These rules protect Saturnis from viral open-source license contamination. Violating ANY of these is a blocking error. Do not proceed with a change that breaks these rules.

1. **NO EMBEDDED DATABASE ENGINES.** Never add `rusqlite`, `sled`, `rocksdb`, `redb`, `fjall`, or ANY crate that bundles/embeds a database engine. Ephemeris is ALWAYS a network client connecting to external databases over TCP/HTTP. No exceptions.

2. **NO DATABASE TYPES IN `ephemeris-core`.** The `ephemeris-core` crate must NEVER depend on `tokio-postgres`, `sqlx`, `arangodb`, `couch_rs`, `reqwest`, or any database/HTTP client crate. Domain types and trait definitions only. If you're adding a `use tokio_postgres::` to a file in `ephemeris-core/`, STOP — you're violating the abstraction.

3. **NETWORK-BOUNDARY ISOLATION.** All database communication goes over TCP or HTTP. Connector crates (`ephemeris-pg`, `ephemeris-arango`, `ephemeris-couch`) use network client libraries to talk to externally-provisioned databases. The application never directly accesses database files on disk.

4. **FEATURE-GATE ALL ENTERPRISE CODE.** `ephemeris-arango` and `ephemeris-couch` (and any future enterprise connectors) MUST be behind Cargo feature flags (`enterprise-arango`, `enterprise-couch`). Default builds MUST exclude enterprise code entirely — it should not exist in the open-core binary.

5. **NO VIRAL-LICENSED DEPENDENCIES IN DEFAULT BUILD.** Every dependency in the default (non-enterprise) feature set must be MIT, Apache-2.0, BSD, or similarly permissive. Run `cargo deny check licenses` before adding any new dependency. GPLv3, SSPL, BSL, and AGPL dependencies are forbidden in the default build path.

6. **BYOD (Bring Your Own Database).** Saturnis provides the pipeline; customers provision and license enterprise databases (ArangoDB, CouchDB). Documentation must always reflect this — never instruct users to "install ArangoDB" as part of the Ephemeris install. It's a separate, customer-managed infrastructure decision.

## Architecture Quick Reference

- **Workspace:** Cargo workspace with multiple crates in `crates/`
- **Core pattern:** Repository traits in `ephemeris-core`, implementations in connector crates
- **Ingestion:** MQTT (rumqttc) → validate → write via repository traits
- **Query:** Axum REST API + EPCIS 2.0 Query Interface → read via repository traits
- **CQRS:** Write path (event ledger) separated from Read path (aggregation hierarchy + API)
- **Tier 1 DB:** PostgreSQL (JSONB events + ltree aggregation) — open core
- **Tier 2 DB:** ArangoDB (graph) + CouchDB (document replication) — enterprise, feature-gated
- **Config:** `ephemeris.toml` → env vars → CLI flags (highest priority)

## Coding Standards

- Run `cargo fmt` before committing
- Run `cargo clippy -- -D warnings` — zero warnings policy
- Run `cargo deny check licenses` when adding dependencies
- All public trait methods must have doc comments
- Test harness boundaries (MQTT, DB) must be maintained — never remove integration test infrastructure
- Use `mockall` for trait mocking in unit tests
- Use `testcontainers-rs` for integration tests against real services

## File Structure

```
crates/
├── ephemeris-core/       # Domain types, traits, validation (ZERO DB deps)
├── ephemeris-mqtt/       # MQTT ingestion (rumqttc)
├── ephemeris-pg/         # PostgreSQL connector (Tier 1, open-core)
├── ephemeris-arango/     # ArangoDB connector (Enterprise, feature-gated)
├── ephemeris-couch/      # CouchDB connector (Enterprise, feature-gated)
├── ephemeris-api/        # REST + EPCIS 2.0 Query Interface (axum)
├── ephemeris-app/        # Binary entrypoint, config, wiring
└── ephemeris-testkit/    # Dev-only: test tools, dashboard UI
```
