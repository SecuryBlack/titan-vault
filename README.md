# TitanVault

Agente open source de copias de seguridad, compresión en streaming y disaster recovery para servidores Linux y Windows. Diseñado para funcionar de manera **100% autónoma con TUI interactiva (Standalone)** o integrado con el ecosistema **SecuryBlack Cloud**.

[![Official Site](https://img.shields.io/badge/Website-titanvault.dev-06B6D4?style=flat-square)](https://titanvault.dev)
[![Ecosystem](https://img.shields.io/badge/Ecosystem-SecuryBlack-33E1BF?style=flat-square)](https://securyblack.com)
[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg?style=flat-square)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80+-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org)

> **Parte del ecosistema SecuryBlack:**
> [OxiPulse (Métricas)](https://github.com/securyblack/oxi-pulse) · [FerroSentry (Seguridad)](https://github.com/securyblack/ferro-sentry) · [CupraFlow (Alta Disponibilidad)](https://github.com/securyblack/cupra-flow) · [CromoForge (GitOps)](https://github.com/securyblack/cromo-forge) · **TitanVault (Backups)** · [SecuryBlack Cloud](https://securyblack.com)

---

## 🛡️ Filosofía y Características

- **Streaming directo sin tocar disco:** A diferencia de los scripts tradicionales que hacen el volcado en disco antes de subirlo (pudiendo llenar el VPS), TitanVault conecta directamente el volcado con el stream de compresión y subida multipart.
- **Compresión ultrarrápida:** Utiliza Zstandard (`zstd`) para una relación óptima entre velocidad y reducción de tamaño.
- **Cifrado Zero-Knowledge en origen:** Cifrado simétrico autenticado (`ChaCha20-Poly1305`) antes de que ningún byte salga hacia los proveedores remotos.
- **Multi-Cloud Nativo (Apache OpenDAL):** Soporte unificado para **Cloudflare R2**, **Hetzner Object Storage (S3)**, **AWS S3**, **MinIO**, **Google Drive** y almacenamiento local sin dependencias externas del host (sin `awscli`, sin `rclone`).
- **TUI Interactiva Standalone (Ratatui):** Asistente visual en consola para configurar bases de datos, destinos, retención y realizar tests de conectividad y restauraciones sin depender de la nube.
- **Política GFS Automática:** Rotación inteligente de copias horarias, diarias, semanales, mensuales y anuales.
- **Integración SecuryBlack Cloud:** Descubrimiento automático a través de `sb-agent-core`, túnel gRPC mediante `nexus-agent` y reporte de progreso en tiempo real (`CommandProgress`).

---

## 🚀 Uso Rápido (CLI & TUI)

```bash
# Lanzar la interfaz interactiva en terminal (Standalone)
titanvault tui

# Ver estado y versión
titanvault --version
titanvault status

# Monitor de estado en vivo (sb-agent-core top)
titanvault top

# Probar conectividad con los destinos configurados
titanvault test

# Ejecutar un backup puntual bajo demanda
titanvault backup daily
```

---

## ⚙️ Configuración (`/etc/titanvault/config.toml`)

```toml
version = "0.1.0"
agent_name = "titanvault"
mode = "standalone"

[schedule]
enabled = true
cron = "0 2 * * *" # Diario a las 02:00
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
passphrase = "clave-secreta-del-usuario"

# Orígenes a respaldar
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

# Destinos remotos (Apache OpenDAL)
[targets.hetzner]
enabled = true
endpoint = "https://nbg1.your-objectstorage.com"
bucket = "de-nur-sb-bkp-01"
region = "nbg1"
access_key = "TU_ACCESS_KEY"
secret_key = "TU_SECRET_KEY"
prefix = "backups/securyblack"

[targets.cloudflare_r2]
enabled = true
endpoint = "https://<account_id>.r2.cloudflarestorage.com"
bucket = "prod-backups"
access_key = "TU_R2_ACCESS_KEY"
secret_key = "TU_R2_SECRET_KEY"
prefix = "backups"
```

---

## 🏗️ Arquitectura de Componentes

```
titan-vault/
├── Cargo.toml               # Dependencias: sb-agent-core, opendal, zstd, chacha20poly1305, ratatui
├── TODO.md                  # Checklist maestro de tareas
├── src/
│   ├── main.rs              # Punto de entrada, CLI (sb-agent-core) y daemon service
│   ├── config.rs            # Configuración tipada (sb-agent-core::config)
│   ├── commands.rs          # Handlers del socket de comandos (backup_now, prune, test)
│   ├── storage/             # Conectores multi-cloud (Apache OpenDAL S3/Fs)
│   │   └── mod.rs           # StorageManager, upload_all, list_snapshots, test_connection
│   ├── engine/              # Motor de ejecución
│   │   ├── pipeline.rs      # Streaming: Source -> zstd -> ChaCha20 -> Upload
│   │   ├── crypto.rs        # Cifrado Zero-Knowledge (ChaCha20-Poly1305)
│   │   ├── retention.rs     # Motor de rotación GFS (Grandfather-Father-Son)
│   │   ├── scheduler.rs     # Scheduler autónomo Tokio (cron local)
│   │   └── dumper/          # Volcadores streaming (PostgreSQL, Filesystem)
│   └── tui/                 # Terminal User Interface (Ratatui + Crossterm)
│       ├── mod.rs           # Bucle de eventos y entrada raw mode
│       ├── app.rs           # Máquina de estados y datos del formulario
│       └── ui.rs            # Renderizado de pestañas, formularios y modal de progreso
└── scripts/
    ├── install.sh           # Instalador Linux (sb-agent-core install-lib)
    └── install.ps1          # Instalador Windows PowerShell
```

---

## 🌐 Ecosistema Open Source de SecuryBlack

TitanVault es una pieza especializada dentro del conjunto de agentes autónomos de infraestructura de SecuryBlack:

| Agente | Enfoque Principal | Web Oficial | Repositorio |
| :--- | :--- | :--- | :--- |
| **OxiPulse** | Telemetría, métricas OTLP y logs sin overhead | [oxipulse.dev](https://oxipulse.dev) | [securyblack/oxi-pulse](https://github.com/securyblack/oxi-pulse) |
| **FerroSentry** | EDR ligero, auditd, detección de fuerza bruta y firewall | [ferrosentry.dev](https://ferrosentry.dev) | [securyblack/ferro-sentry](https://github.com/securyblack/ferro-sentry) |
| **CupraFlow** | Alta disponibilidad, IP flotante VIP y balanceo de tráfico | [cupraflow.dev](https://cupraflow.dev) | [securyblack/cupra-flow](https://github.com/securyblack/cupra-flow) |
| **CromoForge** | Despliegues continuos, GitOps y gestión de contenedores | [cromoforge.dev](https://cromoforge.dev) | [securyblack/cromo-forge](https://github.com/securyblack/cromo-forge) |
| **TitanVault** | Copias de seguridad en streaming y recuperación ante desastres | [titanvault.dev](https://titanvault.dev) | [securyblack/titan-vault](https://github.com/securyblack/titan-vault) |

Todos los agentes pueden gestionarse de forma centralizada y visual conectándolos a [SecuryBlack Cloud](https://securyblack.com).

---

## Licencia

TitanVault is licensed under the [Apache License, Version 2.0](LICENSE).
