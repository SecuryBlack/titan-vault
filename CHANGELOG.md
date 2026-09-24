# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.2] - 2026-09-22

### Added
- **Commands**: Implement `update_now` command for on-demand manual updates via command intake.
- **Updater**: Add background auto-update worker checking GitHub releases.
- **Commands**: Support querying snapshots across all configured storage targets when target is set to `"all"`.

## [0.1.1] - 2026-09-22

### Added
- **Config**: Add dynamic configuration management, backup source definitions, and target reloading without restart.
- **Commands**: Add intake handler for backup commands dispatch and status reporting.

## [0.1.0] - 2026-09-20

### Added
- **Initial Release**: High-performance streaming backup agent in Rust.
- **Streaming**: Direct memory streaming of databases and files with zstd compression and ChaCha20-Poly1305 encryption.
- **Targets**: Multi-cloud storage targets support (Cloudflare R2, Hetzner Storage Box, Google Drive, Local).
- **Core**: Built on top of `sb-agent-core` with Unix/Windows service lifecycle and CLI.

[Unreleased]: https://github.com/SecuryBlack/titan-vault/compare/v0.1.2...HEAD
[0.1.2]: https://github.com/SecuryBlack/titan-vault/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/SecuryBlack/titan-vault/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/SecuryBlack/titan-vault/releases/tag/v0.1.0
