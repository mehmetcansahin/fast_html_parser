# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.2.x   | :white_check_mark: |
| 0.1.x   | :x:                |

## Reporting a Vulnerability

Please report security vulnerabilities through
[GitHub Security Advisories](https://github.com/mehmetcansahin/fast_html_parser/security/advisories).

**Do not** open a public issue for security vulnerabilities.

## Scope

`fast-html-parser` is a pragmatic web-scraping parser, not a browser engine or
an HTML sanitizer. Reports should evaluate behavior against the documented
[compatibility contract](COMPATIBILITY.md). Missing browser wrapper synthesis,
foreign-content integration, template/scripting algorithms, and context-aware
fragment parsing are documented non-goals rather than security defects.

The following areas are of particular interest:

- **SIMD unsafe code** — memory safety issues in NEON/SSE/AVX intrinsics
- **Denial of service** — inputs that cause excessive memory or CPU usage
- **Encoding attacks** — malformed byte sequences that bypass the documented
  BOM/meta/UTF-8 decoding policy
- **Limit bypasses** — raw or decoded input, 512-element nesting, selector
  complexity, or terminal streaming errors that fail open
- **Source draining** — iterator or async adapters continuing to read after the
  first parse failure or an early-stop result

The parser does not make untrusted markup safe for insertion into HTML, CSS,
JavaScript, URLs, or other output contexts. Use a dedicated sanitizer and
context-appropriate escaping.

## Response Timeline

- **48 hours** — initial acknowledgment of your report
- **90 days** — coordinated disclosure deadline

We will work with you to understand the issue and coordinate a fix before
any public disclosure. Credit will be given to reporters unless anonymity
is requested.
