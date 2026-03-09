# Ephemeris

**Open-core track-and-trace serialization engine for packaging lines.**

[![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-stable-orange.svg)](https://www.rust-lang.org/)

---

## Overview

Ephemeris is a serialization engine that tracks products from serial number birth through supply chain handoff. It operates as a **Site Serialization Manager (SSM)** at ISA-95 Level 3 -- receiving serial number allocations from enterprise systems, ingesting line events via MQTT, and exposing a standards-compliant query interface.

**Core capabilities:**

- **MQTT ingestion** -- receives events from packaging line controllers (PLCs, scanners, label printers) via MQTT topics
- **CQRS architecture** -- immutable event ledger (write path) separated from aggregation hierarchy (read path)
- **EPCIS 2.0 query interface** -- REST API implementing the GS1 EPCIS 2.0 standard for supply chain visibility
- **OPEN-SCS alignment** -- serial number lifecycle management aligned with the OPC Foundation's Packaging Serialization Specification
- **PostgreSQL storage** -- JSONB event store with ltree-based aggregation hierarchy (open core)

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable toolchain)
- [Docker](https://docs.docker.com/get-docker/) and Docker Compose

### Start infrastructure services

```bash
docker compose -f docker-compose.dev.yml up -d postgres mosquitto
```

### Build and run

```bash
cargo build
cargo run -- --config ephemeris.toml
```

The API server starts at `http://localhost:8080` by default. MQTT ingestion connects to the broker at `mqtt://localhost:1883`.

### Enterprise build (optional)

To include enterprise connectors (ArangoDB graph backend):

```bash
cargo build --features enterprise
```

## Architecture

Ephemeris is structured as a Cargo workspace with focused crates:

```
crates/
├── ephemeris-core       Domain types, repository traits, validation (zero DB deps)
├── ephemeris-mqtt       MQTT ingestion via rumqttc
├── ephemeris-pg         PostgreSQL connector -- JSONB events + ltree aggregation
├── ephemeris-arango     ArangoDB graph connector (enterprise, feature-gated)
├── ephemeris-api        REST API + EPCIS 2.0 Query Interface via Axum
├── ephemeris-app        Binary entrypoint, configuration, wiring
└── ephemeris-testkit    Dev-only test utilities and tools
```

### Data flow

```
Packaging Lines (LSM)         Enterprise (ESM)
       |                            |
       | MQTT                       | SN allocations
       v                            v
  [ephemeris-mqtt]            [ephemeris-api]
       |                            |
       v                            v
  [ephemeris-core]  <-- domain types + repository traits
       |
       +----------+-----------+
       |          |           |
       v          v           v
  [ephemeris-pg] [ephemeris-arango] ...
   (Tier 1)       (Tier 2)
```

## Open Core vs Enterprise

| Capability | Open Core | Enterprise |
|---|:---:|:---:|
| MQTT event ingestion | x | x |
| PostgreSQL event store (JSONB) | x | x |
| PostgreSQL aggregation (ltree) | x | x |
| REST API + EPCIS 2.0 queries | x | x |
| Serial number lifecycle management | x | x |
| Serial number pool management | x | x |
| ArangoDB graph connector | | x |
| CouchDB document replication | | planned |

Enterprise connectors are compiled behind Cargo feature flags. The default build produces a fully functional open-core binary with no enterprise code included.

## Configuration

Ephemeris uses a layered configuration system with the following priority (highest first):

1. **CLI flags** -- `--database-backend postgres`
2. **Environment variables** -- `EPHEMERIS_MQTT__BROKER_URL=mqtt://broker:1883`
3. **Config file** -- `ephemeris.toml`

See [`ephemeris.toml`](ephemeris.toml) for the full configuration reference with defaults.

## Development

### Build

```bash
cargo build                           # Open-core build (default)
cargo build --features enterprise     # Enterprise build (includes ArangoDB)
```

### Test

```bash
cargo test --workspace --lib                          # Unit tests
cargo test --workspace -- --test-threads=1            # Integration tests (sequential)
```

Integration tests use [testcontainers](https://github.com/testcontainers/testcontainers-rs) to spin up PostgreSQL and MQTT broker instances automatically -- no manual Docker setup required for testing.

### Lint and format

```bash
cargo fmt --check             # Check formatting
cargo clippy -- -D warnings   # Lint (zero warnings policy)
cargo deny check licenses     # License audit
```

## Standards

Ephemeris aligns with two complementary industry standards:

- **[OPEN-SCS PSS v1](https://opcfoundation.org/developer-tools/documents/view/165)** (OPC Foundation) -- covers the pre-commissioning lifecycle: serial number birth, pool management, label encoding, and commissioning
- **[GS1 EPCIS 2.0](https://www.gs1.org/standards/epcis)** -- covers post-commissioning: shipping, receiving, aggregation, and supply chain visibility

The handoff point between the two standards is **commissioning** -- the moment a serialized label is affixed to a physical product. See [`docs/open-scs-alignment.md`](docs/open-scs-alignment.md) for a detailed analysis.

## Contributing

We welcome contributions. Please see [CONTRIBUTING.md](CONTRIBUTING.md) for development setup, code style, testing guidelines, and the PR process.

## Security

To report a security vulnerability, please see [SECURITY.md](SECURITY.md).

## License

Ephemeris open-core is licensed under the [Apache License 2.0](LICENSE).

Enterprise connectors (`ephemeris-arango`, `ephemeris-couch`) are included in the source tree behind Cargo feature flags. The open-core default build excludes all enterprise code. Contact [sales@saturnis.io](mailto:sales@saturnis.io) for enterprise licensing.
