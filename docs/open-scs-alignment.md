# OPEN-SCS Alignment Analysis

**Standard:** OPEN-SCS Packaging Serialization Specification (PSS) Version 1 (OPC Foundation, 2019-03-18)

**Source documents:** `.internal/OPEN-SCS-Specification-FINAL.pdf` (54pp), `.internal/OPEN-SCS-Use-Cases-FINAL.pdf` (26pp)

---

## 1. Relationship Between OPEN-SCS and EPCIS 2.0

OPEN-SCS and EPCIS 2.0 are complementary standards covering different halves of the serial number lifecycle:

| Phase | Standard | Coverage |
|-------|----------|----------|
| Pre-commissioning | OPEN-SCS PSS | SN birth → pool management → allocation → label printing → commissioning |
| Post-commissioning | GS1 EPCIS 2.0 | Commissioning → shipping → receiving → aggregation → supply chain visibility |

The handoff point is **commissioning** — the moment a label is affixed to a physical product. OPEN-SCS explicitly adopts GS1 CBV terminology for post-commissioning events (shipping, destroying, decommissioning, packing, unpacking all use `urn:epcglobal:cbv:bizstep:*` URIs).

## 2. ISA-95 Tier Mapping

OPEN-SCS defines three system roles:

| OPEN-SCS Role | ISA-95 Level | Ephemeris Mapping |
|---|---|---|
| **ESM** (Enterprise Serialization Manager) | Level 4 | Corporate cloud / ERP — upstream of Ephemeris |
| **SSM** (Site Serialization Manager) | Level 3 | **Ephemeris's deployment target** |
| **LSM** (Line Serialization Manager) | Level 2 | Packaging line controllers publishing to MQTT |

Ephemeris is the SSM that:
1. Receives SN allocations from the ESM (enterprise)
2. Manages local pools for production runs
3. Receives line events from LSMs via MQTT
4. Tracks the full lifecycle from allocation through shipping
5. Reports upstream via EPCIS events

## 3. What Ephemeris Already Implements

| Ephemeris Feature | OPEN-SCS Equivalent | PSS Reference |
|---|---|---|
| `EpcisEvent::AggregationEvent` with `bizStep: "packing"` | Aggregation Packing Event | §7.20 |
| `AggregationRepository::remove_child` | Aggregation Unpacking Event | §7.21 |
| `CommonEventFields::biz_step` / `disposition` | URI-based state identifiers | §4.4 |
| `EventRepository::store_event` | EPCIS event capture (Product Delivery activity) | §4.2.6 |
| `EpcisEvent::ObjectEvent` with shipping/destroying | SID Shipping/Destroying Events | §7.17, §7.16 |
| MQTT ingestion from scanners/PLCs | Edge pattern at Level 2/3 | §4.3 |
| PostgreSQL JSONB event store | Immutable event ledger | — |
| ArangoDB graph for hierarchy | Graph traversal for aggregation trees | — |

## 4. OPEN-SCS Concepts Not Yet in Ephemeris

### 4.1 Serial Number State Machine (PSS §5)

12 states with 17 transitions:

```
Unassigned → Unallocated → Allocated → Encoded → Commissioned → Released
                                          ↘ Label Scrapped     ↘ Destroyed
                                          ↘ Label Sampled      ↘ Inactive
                                          ↘ SN Invalid         ↘ Sampled
```

States with their disposition URIs:

| State | URI | Description |
|---|---|---|
| Unassigned | `http://open-scs.org/disp/unassigned` | Not yet assigned to production |
| Unallocated | `http://open-scs.org/disp/unallocated` | Assigned to production, not yet to a specific run |
| Allocated | `http://open-scs.org/disp/allocated` | Assigned to a specific product/packaging run |
| SN Invalid | `http://open-scs.org/disp/sn_invalid` | No longer viable, will not be subject to further events |
| Encoded | `http://open-scs.org/disp/encoded` | Written to barcode/RFID, not yet commissioned |
| Label Sampled | `http://open-scs.org/disp/label_sampled` | Printed label retained for batch records |
| Label Scrapped | `http://open-scs.org/disp/label_scrapped` | Encoded label made unusable before commissioning |
| Commissioned | `http://open-scs.org/disp/commissioned` | Associated with physical product, still in production |
| Sampled | `http://open-scs.org/disp/sampled` | Product pulled for testing |
| Inactive | `urn:epcglobal:cbv:disp:inactive` | Decommissioned, may be reintroduced |
| Destroyed | `urn:epcglobal:cbv:disp:destroyed` | Physically terminated |
| Released | `http://open-scs.org/disp/released` | Left production responsibility |

Transitions (business steps):

| Transition | URI | From → To |
|---|---|---|
| provisioning | `http://open-scs.org/bizstep/provisioning` | Unassigned → Unallocated |
| sn_returning | `http://open-scs.org/bizstep/sn_returning` | Unallocated → Unassigned |
| sn_allocating | `http://open-scs.org/bizstep/sn_allocating` | Unallocated → Allocated |
| sn_deallocating | `http://open-scs.org/bizstep/sn_deallocating` | Allocated → Unallocated |
| sn_invalidating | `http://open-scs.org/bizstep/sn_invalidating` | Provisioned → SN Invalid |
| sn_encoding | `http://open-scs.org/bizstep/sn_encoding` | Allocated → Encoded |
| label_inspecting | `http://open-scs.org/bizstep/label_inspecting` | Encoded → Encoded (no state change) |
| label_sampling | `http://open-scs.org/bizstep/label_sampling` | Encoded → Label Sampled |
| label_scrapping | `http://open-scs.org/bizstep/label_scrapping` | Encoded → Label Scrapped |
| commissioning | `urn:epcglobal:cbv:bizstep:commissioning` | Encoded → Commissioned |
| inspecting | `urn:epcglobal:cbv:bizstep:inspecting` | Commissioned → Sampled |
| shipping | `urn:epcglobal:cbv:bizstep:shipping` | Commissioned → Released |
| decommissioning | `urn:epcglobal:cbv:bizstep:decommissioning` | Commissioned → Inactive |
| destroying | `urn:epcglobal:cbv:bizstep:destroying` | Commissioned → Destroyed |
| packing | `urn:epcglobal:cbv:bizstep:packing` | Aggregation event (no SN state change) |
| unpacking | `urn:epcglobal:cbv:bizstep:unpacking` | Aggregation event (no SN state change) |

### 4.2 Information Model (PSS §6)

**SID Class** (§6.1) — Defines the format/standard for identifiers:
- SID Class ID, Owner, Description, Syntax Specification, Allowed Character Set, Intended Use
- SID Class Properties (extensible key-value pairs)
- Common classes: GS1 SGTIN, GS1 SSCC, IPI (Italy), CIP-13 (France)

**Collection** (§6.2-6.3) — Batch containers for serial numbers:
- Serial Number Collection: just the numbers (pre-print phase)
- Label Collection: numbers + label properties (lot, expiry, manufactured date)
- All SNs in a collection share the same state

**Serial Number Pool** (§6.4) — Managed set of SNs with selection criteria:
- Pool ID, associated SID Class
- Pool Selection Criteria: product code, SID format, packaging level, etc.

### 4.3 Transaction Functions (PSS §7)

20 transaction types organized by pattern:

**Pull transactions (request/response):**
- Serial Number Request Unassigned (§7.2) — request new SNs from ESM
- Serial Number Request Unallocated (§7.3) — request SNs from site pool
- Serial Number Request Allocated (§7.4) — request SNs for a specific run

**Push transactions (SN state changes):**
- Serial Number Return Unallocated (§7.5) — return unused unallocated SNs
- Serial Number Return Allocated (§7.6) — return unused allocated SNs
- Serial Number to Unallocated (§7.7) — transition SNs to unallocated
- Serial Number to Allocated (§7.8) — transition SNs to allocated
- Serial Number to Encoded (§7.9) — transition SNs to encoded

**Push transactions (event notifications):**
- SN Invalidating Event (§7.10)
- Label Encoding Event (§7.11)
- Label Scrapping Event (§7.12)
- Label Inspecting Event (§7.13)
- Label Sampling Event (§7.14)
- SID Commissioning Event (§7.15)
- SID Destroying Event (§7.16)
- SID Shipping Event (§7.17)
- SID Inspecting Event (§7.18)
- SID Decommissioning Event (§7.19)
- Aggregation Packing Event (§7.20)
- Aggregation Unpacking Event (§7.21)

### 4.4 Use Cases (from Use Cases document)

5 use cases with 14 sub-scenarios:

- **UC1:** Obtaining serial numbers (sync single-request, sync multi-request, async with tokens)
- **UC2:** Return unused serial numbers
- **UC3:** Master data exchange (SID class and pool configuration)
- **UC4:** Serialization events reporting (label encoding, commissioning, aggregation, shipping)
- **UC5:** Packaging orders (enterprise pushes production orders to site)

6 deployment architectures covering various ESM/SSM/LSM distributions.

## 5. Recommended Feature Roadmap

### Phase 1 — Serial Number Lifecycle (highest value)
- `SerialNumberState` enum (12 states from PSS §5)
- `SerialNumberRepository` trait with state transition methods
- PG: `serial_numbers` table with state, pool_id, timestamps
- MQTT topics: `ephemeris/sn/+/state` for state change events
- REST: `GET /serial-numbers/{epc}/state`, `GET /serial-numbers?state=commissioned`

### Phase 2 — Pool Management
- `SerialNumberPool` and `PoolSelectionCriteria` domain types
- `PoolRepository` trait: create_pool, request_numbers, return_numbers
- PG: `sn_pools` + `pool_criteria` tables
- REST: `POST /pools`, `POST /pools/{id}/request`, `POST /pools/{id}/return`

### Phase 3 — Label Events
- Extend MQTT handler to recognize OPEN-SCS URI prefixes
- Route label events through state machine
- Stored as events in `EventRepository` + state changes in `SerialNumberRepository`

### Phase 4 — SID Class Validation (enterprise feature candidate)
- `SidClass` domain type with format validation rules
- Validate incoming EPCs against expected SID class
- Enterprise tier: open core stores anything, enterprise validates

### Phase 5 — Collection/Batch Operations
- Batch endpoints on REST API
- `POST /serial-numbers/allocate` with count and pool selection criteria
- Efficient bulk state transitions
