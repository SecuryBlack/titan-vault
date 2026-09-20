# TitanVault

Open source streaming backup, compression, and disaster recovery agent for Linux and Windows servers. Designed to run **100% autonomously with a standalone TUI** or seamlessly connected to the **SecuryBlack Cloud** ecosystem.

[![Official Site](https://img.shields.io/badge/Website-titanvault.dev-06B6D4?style=flat-square)](https://titanvault.dev)
[![Ecosystem](https://img.shields.io/badge/Ecosystem-SecuryBlack-33E1BF?style=flat-square)](https://securyblack.com)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org)

> **Part of the SecuryBlack ecosystem:**
> [OxiPulse (Metrics)](https://github.com/securyblack/oxi-pulse) · [FerroSentry (Security)](https://github.com/securyblack/ferro-sentry) · [CupraFlow (High Availability)](https://github.com/securyblack/cupra-flow) · [CromoForge (GitOps)](https://github.com/securyblack/cromo-forge) · **TitanVault (Backups)** · [SecuryBlack Cloud](https://securyblack.com)

---

## 🛡️ Philosophy & Key Features

- **Zero intermediate disk storage:** Unlike traditional scripts that write entire dumps to disk before uploading (risking VPS disk exhaustion), TitanVault streams dumps directly through memory compression and multipart cloud uploads.
- **Ultra-fast compression:** Uses Zstandard (`zstd`) for an optimal balance between compression ratio and CPU throughput.
- **Zero-Knowledge client encryption:** Client-side authenticated symmetric encryption (`ChaCha20-Poly1305`) before data leaves the server.
- **Native Multi-Cloud (Apache OpenDAL):** Unified native drivers for **Cloudflare R2**, **Hetzner Object Storage (S3)**, **AWS S3**, **MinIO**, **Google Drive**, and local storage without host dependencies (no `awscli`, no `rclone`).
- **Standalone Interactive TUI (Ratatui):** Full-featured terminal console to configure data sources, remote targets, test endpoints with `<T>`, browse snapshots, and run restores with `<R>` without a cloud dependency.
- **Automated GFS Retention:** Smart lifecycle pruning of hourly, daily, weekly, monthly, and yearly backups across remote buckets.
- **SecuryBlack Cloud Integration:** Automatic registration via `sb-agent-core`, gRPC tunnel via `nexus-agent`, and real-time streaming progress reporting (`CommandProgress`).

---

## 📦 Quickstart & Installation

### Linux — One-line Install
```bash
curl -fsSL https://install.titanvault.dev | sudo bash
```

### Windows — PowerShell (Administrator)
```powershell
irm https://install.titanvault.dev | iex
```

---

## 🚀 CLI & TUI Usage

```bash
# Launch interactive terminal UI (Standalone)
titanvault tui

# Check status and version
titanvault --version
titanvault status

# Live status monitor (sb-agent-core top)
titanvault top

# Test connectivity to configured storage targets
titanvault test

# Trigger an immediate on-demand backup
titanvault backup daily
```

---

## ⚙️ Configuration (`/etc/titanvault/config.toml`)

```toml
version = "0.1.0"
agent_name = "titanvault"
mode = "standalone"

[schedule]
enabled = true
cron = "0 2 * * *" # Daily at 02:00 UTC
hourly_cron = "0 * * * *"

[retention]
keep_hourly = 24
keep_daily = 7
keep_weekly = 4
keep_monthly = 12
keep_yearly = 3

[crypto]
enabled = true
algorithm = "chacha20-poly1305"
passphrase = "your-secure-symmetric-passphrase"

# Backup Sources
[[sources.databases]]
name = "production-postgres"
driver = "postgres"
enabled = true
container_name = "postgres"
database = "securyblack"
user = "postgres"

[[sources.filesystems]]
name = "app-configs"
enabled = true
paths = ["/opt/stack"]
excludes = ["**/data/**", "**/backups/**", "**/*.sock"]

# Remote Targets (Apache OpenDAL)
[targets.hetzner]
enabled = true
endpoint = "https://nbg1.your-objectstorage.com"
bucket = "de-nur-sb-bkp-01"
region = "nbg1"
access_key = "YOUR_ACCESS_KEY"
secret_key = "YOUR_SECRET_KEY"
prefix = "backups/securyblack"

[targets.cloudflare_r2]
enabled = true
endpoint = "https://<account_id>.r2.cloudflarestorage.com"
bucket = "prod-backups"
access_key = "YOUR_R2_ACCESS_KEY"
secret_key = "YOUR_R2_SECRET_KEY"
prefix = "backups"
```

---

## 🏗️ Architecture & Modules

```
titan-vault/
├── Cargo.toml               # Dependencies: sb-agent-core, opendal, zstd, chacha20poly1305, ratatui
├── TODO.md                  # Development checklist
├── src/
│   ├── main.rs              # Entry point, CLI (sb-agent-core) and daemon service wrapper
│   ├── config.rs            # Strongly-typed configuration (sb-agent-core::config)
│   ├── commands.rs          # Command socket handlers (backup_now, prune, test)
│   ├── storage/             # Multi-cloud drivers (Apache OpenDAL S3/Fs)
│   │   └── mod.rs           # StorageManager, upload_all, list_snapshots, test_connection
│   ├── engine/              # Backup pipeline
│   │   ├── pipeline.rs      # Streaming: Source -> zstd -> ChaCha20 -> Upload
│   │   ├── crypto.rs        # Zero-Knowledge encryption (ChaCha20-Poly1305)
│   │   ├── retention.rs     # GFS lifecycle retention engine (Grandfather-Father-Son)
│   │   ├── scheduler.rs     # Autonomous Tokio scheduler (local cron)
│   │   └── dumper/          # Streaming dumpers (PostgreSQL, Filesystem)
│   └── tui/                 # Terminal User Interface (Ratatui + Crossterm)
│       ├── mod.rs           # Event loop and raw mode handling
│       ├── app.rs           # State machine and form models
│       └── ui.rs            # Tabs, forms, tables, and progress modal
└── scripts/
    ├── install.sh           # Linux installer (sb-agent-core install-lib)
    └── install.ps1          # Windows PowerShell installer
```

---

## 🌐 SecuryBlack Open Source Ecosystem

TitanVault is a specialized storage and disaster recovery agent within the SecuryBlack modular suite:

| Agent | Core Focus | Official Website | Repository |
| :--- | :--- | :--- | :--- |
| **OxiPulse** | Telemetry, OTLP metrics, and zero-overhead vital signs | [oxipulse.dev](https://oxipulse.dev) | [securyblack/oxi-pulse](https://github.com/securyblack/oxi-pulse) |
| **FerroSentry** | Lightweight EDR, auditd, brute-force mitigation & firewall | [ferrosentry.dev](https://ferrosentry.dev) | [securyblack/ferro-sentry](https://github.com/securyblack/ferro-sentry) |
| **CupraFlow** | High availability, floating VIP failover & traffic balancing | [cupraflow.dev](https://cupraflow.dev) | [securyblack/cupra-flow](https://github.com/securyblack/cupra-flow) |
| **CromoForge** | Continuous delivery, GitOps & container management | [cromoforge.dev](https://cromoforge.dev) | [securyblack/cromo-forge](https://github.com/securyblack/cromo-forge) |
| **TitanVault** | Zero-disk streaming backups & disaster recovery | [titanvault.dev](https://titanvault.dev) | [securyblack/titan-vault](https://github.com/securyblack/titan-vault) |

All agents can be centrally managed with unified observability by connecting them to [SecuryBlack Cloud](https://securyblack.com).

---

## License

TitanVault is licensed under the [Apache License, Version 2.0](LICENSE).
