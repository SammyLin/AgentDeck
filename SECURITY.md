# Security Policy

## Supported versions

Security fixes are provided for the latest published AgentDeck release.

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting for this repository instead
of opening a public issue. Include the affected version, reproduction steps,
impact, and any suggested mitigation.

You can expect an acknowledgement within 72 hours. Please allow time for a fix
and coordinated release before disclosing the issue publicly.

## Release integrity

The installer downloads binaries from this repository's GitHub Releases and
verifies each archive against its published SHA-256 checksum before installing.
GitHub Actions also generates build provenance attestations for release
archives, which can be verified with the GitHub CLI.
CI tests every change on Linux and macOS, while the scheduled RustSec audit
checks dependencies for known vulnerabilities.
