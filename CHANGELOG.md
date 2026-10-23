# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Fixed
- **Logs**: Old log files are deleted again; only the last 7 days are kept (sb-agent-core 0.4.1). Rotated files (`titanvault.log.YYYY-MM-DD`) were never removed.

## [0.3.0] - 2026-10-23

### Added
- **Commands**: `restore_snapshot` (`target`, `path`) recovers a snapshot from the panel, the same way as `titanvault restore`: into a new path under `/var/lib/titanvault/restores` (`%ProgramData%\SecuryBlack\titanvault\restores` on Windows), named after the snapshot with the time in front. A SQL dump becomes a `.sql` file that is never run; a files snapshot becomes a new folder. Nothing existing is overwritten. Returns the same report as the CLI.

## [0.2.3] - 2026-10-16

### Security
- **Commands**: Verify S4 signed commands (ed25519 signature, expiry, replay protection) via `sb-agent-core` `command_intake`, bound to the server token: `cloud.token`, or Nexus's `agent.toml` token when there is none. During the transition any verification failure (unsigned, invalid or expired) is accepted with a warning.

## [0.2.2] - 2026-10-14

### Security
- Auto-update only installs ed25519-signed releases (`sb-agent-core` 0.3).

## [0.2.1] - 2026-10-13

### Added
- `update_config` accepts `cloud.heartbeat_urls`, so SecuryBlack can wire its heartbeats to each backup level.
- Config responses include `host_timezone` (the zone the schedule crons run in).

### Fixed
- `update_config` merges partial payloads by section. A payload with only retention, crypto or cloud no longer resets sources and targets to defaults.

## [0.2.0] - 2026-10-06

### Added
- Configurable five-level schedules and per-level heartbeat URLs.
- Recursive snapshot catalog, retention per source/level, and CLI `snapshots`/`prune --level`.
- Safe CLI recovery to a new destination (`restore`): filesystem extraction or SQL export, with encrypted archive support. SQL import is an isolated verification step, not an automatic restore to the source database.
- Parallel infra migration tooling and acceptance procedure in the [workspace wiki](../sb-wiki/Tecnica/TitanVault%20-%20Migracion%20paralela%20infra.md).

### Fixed
- Backup failures remain in reports and suppress success heartbeats; missing targets/configurations return errors.
- Enabled encryption requires a valid key; invalid config updates are validated before persistence.
- Missing source paths and archive write failures are no longer silently skipped.
- CLI failures propagate nonzero exit status to cron/adapters. Native Google Drive settings fail explicitly; parallel Drive replication uses rclone.

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

[Unreleased]: https://github.com/SecuryBlack/titan-vault/compare/v0.2.2...HEAD
[0.2.2]: https://github.com/SecuryBlack/titan-vault/compare/v0.2.1...v0.2.2
[0.2.1]: https://github.com/SecuryBlack/titan-vault/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/SecuryBlack/titan-vault/compare/v0.1.2...v0.2.0
[0.1.2]: https://github.com/SecuryBlack/titan-vault/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/SecuryBlack/titan-vault/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/SecuryBlack/titan-vault/releases/tag/v0.1.0
