# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

Please report security vulnerabilities through
[GitHub Security Advisories](https://github.com/nicatdcw/fast-html-parser/security/advisories).

**Do not** open a public issue for security vulnerabilities.

## Scope

The following areas are of particular interest:

- **SIMD unsafe code** — memory safety issues in NEON/SSE/AVX intrinsics
- **Denial of service** — inputs that cause excessive memory or CPU usage
- **Encoding attacks** — malformed byte sequences that bypass sanitization

## Response Timeline

- **48 hours** — initial acknowledgment of your report
- **90 days** — coordinated disclosure deadline

We will work with you to understand the issue and coordinate a fix before
any public disclosure. Credit will be given to reporters unless anonymity
is requested.
