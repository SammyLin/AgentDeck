# Releasing AgentDeck

AgentDeck uses the version in `Cargo.toml` as its application version. A Git tag
with the same version triggers `.github/workflows/release.yml`, which publishes
checksummed and attested binaries for supported Linux and macOS platforms.

## Release checklist

1. Choose the next semantic version, for example `0.2.0`.
2. Update `version` in `Cargo.toml`.
3. Run `cargo check --locked` after updating `Cargo.lock` with `cargo check`.
4. Add user-visible changes to the release commit or its GitHub release notes.
5. Run the local release check:

   ```bash
   ./scripts/release-check.sh 0.2.0
   ```

6. Commit and push the release preparation.
7. Create and push the matching tag:

   ```bash
   git tag v0.2.0
   git push origin v0.2.0
   ```

8. Wait for the Release workflow and test the installer on a clean shell.

Do not reuse or move a published version tag. If a release needs a correction,
publish a new patch version instead.

## What users receive

Running AgentDeck installations check GitHub Releases at most once every 24
hours. When a newer version exists, the TUI displays an update notice and lets
the user press `u`. Updates remain opt-in and are verified against the release
SHA-256 checksum before the executable is replaced.
