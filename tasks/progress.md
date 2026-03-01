# Build Progress

## Kanban Status

### Backlog
- US8: ArangoDB connector
- US9: MQTT ingestion
- US10: REST API
- US12: Test kit
- US13: E2E pipeline test
- US14: CI workflow
- US15: Final validation

### Blocked (waiting on dependencies)
- US2: Core domain types (blocked by US1)
- US3: Core error types (blocked by US1)
- US4: Repository traits (blocked by US2, US3)
- US5: PG crate scaffold (blocked by US4)
- US6: PgEventRepository (blocked by US5)
- US7: PgAggregationRepository (blocked by US5)
- US11: App wiring (blocked by US6, US9, US10)

### In Progress
- US1: Scaffold workspace (foundation-agent)

### Done
_(none yet)_

## Wave Plan

- **Wave 1**: US1 (foundation)
- **Wave 2**: US2 + US3 (parallel — domain types + errors)
- **Wave 3**: US4 (traits — needs US2+US3)
- **Wave 4**: US5 + US8 + US9 + US10 (parallel — PG scaffold, ArangoDB, MQTT, API)
- **Wave 5**: US6 + US7 (PG implementations)
- **Wave 6**: US11 + US12 (app wiring + testkit)
- **Wave 7**: US13 + US14 (E2E + CI)
- **Wave 8**: US15 (final validation)
