# Security Policy

## Reporting a Vulnerability

If you discover a security vulnerability in Ephemeris, please report it responsibly. **Do not open a public GitHub issue for security vulnerabilities.**

Send an email to **[security@saturnis.io](mailto:security@saturnis.io)** with:

- A description of the vulnerability
- Steps to reproduce
- The potential impact
- Any suggested fixes (optional)

## Scope

The following are in scope for security reports:

- The Ephemeris codebase (all crates in this repository)
- Authentication and authorization flaws in the REST API
- Data integrity issues in the event store or aggregation hierarchy
- MQTT message handling vulnerabilities (injection, parsing, denial of service)
- Dependency vulnerabilities in the default or enterprise build

The following are out of scope:

- Vulnerabilities in third-party infrastructure (PostgreSQL, ArangoDB, MQTT brokers) unless caused by Ephemeris misconfiguration
- Denial of service through expected resource exhaustion (e.g., sending large volumes of valid MQTT messages)

## Response Timeline

- **Acknowledgment** -- within 3 business days of receiving the report
- **Initial assessment** -- within 7 business days
- **Fix or mitigation** -- target within 30 days for confirmed vulnerabilities, depending on severity and complexity

We will coordinate with you on disclosure timing. We ask that you allow us reasonable time to address the issue before any public disclosure.

## Supported Versions

Security fixes are applied to the latest release. We do not backport fixes to older versions at this time.
