# Additional dependency fixes 2026-11-06

Updated Zod to4.6.7 and Vitest to4.1.12 where used, within their existing major versions. Addresses constrained-collection memory exhaustion and manual mock registration/traversal issues.

Validation on the working tree: Mens42 tests and typecheck; Compass50 tests and build; Report build and19 browser tests all pass. Existing script edits, unreleased version changes and other work were excluded. No production deployment or visibility change.

Official advisories: https://github.com/advisories/GHSA-mrhx-c7xc-m3w5 and https://github.com/advisories/GHSA-qxg6-89mg-rqv8.
