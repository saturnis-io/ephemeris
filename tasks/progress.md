# Build Progress

## Kanban Status

### Done
- US1: Scaffold workspace
- US2: Core domain types (EPCIS 2.0)
- US3: Core error types
- US4: Core repository traits
- US5: PG crate scaffold + schema
- US6: PgEventRepository (4 integration tests)
- US7: PgAggregationRepository with ltree (4 integration tests)
- US8: ArangoDB connector (3 integration tests)
- US9: MQTT ingestion (3 unit tests)
- US10: REST API (1 unit test)
- US12: Test kit + dashboard UI (5 tests)

### In Progress
- US11: App wiring + config (ephemeris-app)

### Blocked (waiting on dependencies)
- US13: E2E pipeline test (blocked by US11)
- US14: CI workflow (blocked by US11)
- US15: Final validation (blocked by US13, US14)

## Wave Plan

- **Wave 1**: US1 (foundation) — DONE
- **Wave 2**: US2 + US3 (parallel — domain types + errors) — DONE
- **Wave 3**: US4 (traits — needs US2+US3) — DONE
- **Wave 4**: US5 + US8 + US9 + US10 (parallel — PG scaffold, ArangoDB, MQTT, API) — DONE
- **Wave 5**: US6 + US7 (PG implementations) — DONE
- **Wave 6**: US11 + US12 (app wiring + testkit) — US12 DONE, US11 IN PROGRESS
- **Wave 7**: US13 + US14 (E2E + CI)
- **Wave 8**: US15 (final validation)

## Summary
11/15 user stories complete. Critical path: US11 → US13+US14 → US15
