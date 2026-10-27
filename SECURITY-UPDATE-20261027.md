# Security update verification: 2026-10-27

Scope: Brotli CVE-2026-111497, eMudhra/emSign trust incident and PHP2Shell CVE-2026-109618 / CVE-2026-109619.

## Changes

- `apps/desktop/src-tauri/Cargo.lock`

TLS lock updates: rustls-webpki 0.103.16 and webpki-root-certs 1.0.11; World Compass also uses rustls 0.23.46. Native binaries were rebuilt locally.

Shared development PC: Node.js 24.22.1, Python.org 3.14.10, uv 0.13.6, pip 26.2.2, certifi 2026.10.23, Git for Windows 2.56.0.windows.4 (native Brotli 1.2.1), .NET 10.0.14 / 8.0.33 and WebView2 156.0.4314.50. These installations are machine changes, not files included in this commit.

## Validation and limits

Frontend 50 tests and Rust 33 tests passed (one large model download test ignored). Updated native app passed isolated WebView2, search and SQLite chat persistence checks. All 7 browser acceptance tests pass using the production frontend build on dedicated preview port 1437.

Checks were run against the existing local working tree, which includes separately maintained changes. This commit intentionally contains only the security update and this record, preserving that other work. No PHP runtime or PHP application was found in the checked project and inspected PC/VPS environment. No exploit code was executed, TLS verification was not disabled, and repository visibility was not changed. Already installed copies on other PCs and provider-internal Cloudflare runtimes remain unverified. A normal HTTPS success alone is not evidence of rejecting the compromised key.

## Official references

- [Node.js security releases](https://nodejs.org/en/blog/vulnerability/october-2026-security-releases)
- [Python security releases](https://blog.python.org/2026/10/python-3152-31410-31318-31216-31118/)
- [Ubuntu Brotli backport](https://ubuntu.com/security/CVE-2026-111497)
- [Git for Windows security release](https://github.com/git-for-windows/git/releases/tag/v2.56.0.windows.4)
- [PHP official changelog](https://www.php.net/ChangeLog-8.php)

## Browser verification follow-up: 2026-10-28

The navigation timeout came from delayed Vite development-module responses before UI interaction. Standard Playwright now builds the production frontend and serves it on an exclusive preview port, without reusing an unrelated development server. Existing test assertions and workflows are unchanged. Native application code and user data were not changed.
