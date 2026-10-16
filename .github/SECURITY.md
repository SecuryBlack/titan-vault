# Security Policy

## Supported Versions

SecuryBlack actively supports and maintains the latest minor release line of TitanVault with security updates and bug fixes.

| Version | Supported          | Status |
| ------- | ------------------ | ------ |
| 0.2.x   | :white_check_mark: | Active support |
| < 0.2.0 | :x:                | End of life |

---

## Privileges and network access

You should know exactly what an agent does before running its installer with `sudo`. This section describes the current behaviour of TitanVault, including its limitations.

### Installation (Linux)

The install script needs root to:

- place the binary at `/usr/local/bin/titanvault`;
- write `/etc/titanvault/config.toml` (file mode `0600`, because it may contain your encryption passphrase, storage keys and SecuryBlack token);
- register and start the systemd unit `titanvault.service`.

It downloads the release archive from GitHub Releases and verifies it against the published SHA-256 checksum before installing it. The installer does not ask for a token; the generated config runs in `standalone` mode with no sources or targets.

### Runtime

- **The service currently runs as root.** The generated unit has no `User=` directive: the agent needs to read the paths you back up and the self-updater replaces the binary in `/usr/local/bin`. Running as a dedicated unprivileged user is on the roadmap.
- **What it reads.** Only the filesystem paths listed in `[[sources.filesystems]]` and the PostgreSQL databases listed in `[[sources.databases]]`. Databases are dumped by running `pg_dump`, either inside the configured container (`docker exec`) or on the host.
- **Encryption.** When `[crypto] enabled = true`, every archive is compressed with zstd and encrypted on the host with ChaCha20-Poly1305 before upload. The key is the SHA-256 of your `passphrase` (or of the contents of `key_file`), with no salt or slow KDF, so use a long random passphrase. Encryption is **off** in the default config written by the installer. If you lose the passphrase, the encrypted archives cannot be decrypted.
- **No shell or remote code execution.** Apart from `pg_dump` (and `docker exec` for containerised databases) there is no shell spawning or dynamic evaluation. Commands (`get_config`, `update_config`, `add_source`, `delete_source`, `backup_now`, `list_snapshots`, `prune`, `test_target`, `update_now`) are only accepted through a local Unix socket / named pipe used by the SecuryBlack companion agent (`nexus-agent`).
- **Signed commands.** Commands are checked with `sb-agent-core` (ed25519 signature, expiry, replay protection), bound to your server token (`cloud.token`, or the token in Nexus's `agent.toml` when there is none). During the current transition a command that fails verification (unsigned, invalid or expired) is still accepted and a warning is logged.
- **Remote config changes are limited.** `update_config` can change sources, targets, schedule, retention and crypto, but in the `cloud` section it only accepts `heartbeat_urls`; the SecuryBlack token and endpoint cannot be changed remotely.

### Outbound connections

| Destination | When | What is sent |
|---|---|---|
| Your storage targets (Hetzner Object Storage, Cloudflare R2, any S3-compatible endpoint) | On every scheduled or on-demand backup, `test`, snapshot listing and retention pruning | The backup archives (encrypted if `[crypto]` is enabled) and S3 list/delete requests. Your `access_key`/`secret_key` are only used to sign requests to that endpoint |
| Local target | Same as above | Nothing leaves the host; archives are written to the configured `path` |
| Heartbeat URLs (`cloud.heartbeat_url` / `cloud.heartbeat_urls`) | After a successful backup of that level, only if set | An empty HTTP `POST`, redirects disabled, 10 s timeout. Failed backups do not ping. None by default |
| `api.github.com` / GitHub Releases | 60 s after start, then once a day, and on `update_now` | A request for the latest release; if there is a newer one, downloads it over HTTPS, checks its ed25519 signature against the SecuryBlack keys built into the binary (`sb-agent-core`) and only then replaces it. There is currently no setting to disable the self-updater |

TitanVault does not talk to the SecuryBlack panel directly: the `cloud.endpoint` setting is not used for any connection. In `securyblack` mode the panel reaches the agent through `nexus-agent` and the local command socket. Native Google Drive targets are not implemented and are rejected at startup.

---

## Reporting a Vulnerability

We take the security of TitanVault and our users' infrastructure seriously. If you discover a security vulnerability in TitanVault, please report it through private channels.

**Please DO NOT open public GitHub issues, discussions, or pull requests for security vulnerabilities.**

Instead, please send a private email to:
**security@securyblack.com**

Please include in your report:
- A clear description of the vulnerability and potential security impact.
- Step-by-step reproduction instructions or a minimal proof-of-concept (PoC).
- Affected environment, architecture (`x86_64` or `aarch64`), and TitanVault version.
- Any suggested mitigations or patches, if available.

### What to Expect

- **Acknowledgment**: We will acknowledge receipt of your vulnerability report within **48 hours**.
- **Assessment**: Our core security team will investigate, reproduce, and confirm the report within **5 business days**.
- **Remediation & Advisory**: Once confirmed, we will develop a patch, release an updated version, and coordinate responsible public disclosure, crediting your contribution unless you prefer to remain anonymous.
