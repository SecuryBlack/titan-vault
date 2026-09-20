# TitanVault — Checklist y Roadmap de Implementación

Este documento registra todas las piezas acordadas para **TitanVault**, el agente de backups, compresión en streaming y disaster recovery de SecuryBlack. A medida que completamos cada funcionalidad, se va tachando de la lista.

---

## [x] Fase 1: Cimientos del Agente e Integración con `sb-agent-core`
- [x] Crear estructura de proyecto en `d:\titan-vault` con `Cargo.toml`.
- [x] Configurar dependencias: `sb-agent-core`, `tokio`, `opendal`, `zstd`, `chacha20poly1305`, `ratatui`, `crossterm`, `serde`, `toml`, `chrono`, `anyhow`, `tracing`.
- [x] Implementar `src/config.rs`: Estructuras de configuración fuertemente tipadas (`Config`, `SourcesConfig`, `TargetsConfig`, `RetentionConfig`, `CryptoConfig`, `ScheduleConfig`) consumiendo `sb_agent_core::config`.
- [x] Implementar `src/main.rs`: Despacho de argumentos comunes de `sb_agent_core::cli` (`--version`, `status`, `top`) y enrutado de subcomandos (`tui`, `backup`, `restore`, `service`).
- [x] Implementar arranque como servicio daemon usando `sb_agent_core::service`.
- [x] Inicialización de logging con rotación usando `sb_agent_core::logging`.

---

## [x] Fase 2: Motor de Almacenamiento Multi-Proveedor (Apache OpenDAL)
- [x] Implementar `src/storage/mod.rs`: Trait unificado `StorageBackend` y factory de operadores de almacenamiento.
- [x] Implementar conector S3-Compatible para **Cloudflare R2** y **Hetzner Object Storage** (`opendal::services::S3`).
- [x] Implementar conector para **Google Drive** (OAuth / Service Account).
- [x] Implementar conector para almacenamiento **Local / NAS / NFS** (`opendal::services::Fs`).
- [x] Implementar función interactiva de test de conexión (`test_connection`) para validar credenciales en caliente.

---

## [x] Fase 3: Pipeline de Streaming, Compresión zstd y Cifrado Zero-Knowledge
- [x] Implementar `src/engine/crypto.rs`: Cifrado y descifrado simétrico de streams mediante `ChaCha20-Poly1305` y derivación de claves seguras.
- [x] Implementar `src/engine/dumper/postgres.rs`: Extracción en streaming de PostgreSQL (`pg_dump` vía Docker o local sin escribir archivos temporales).
- [x] Implementar `src/engine/dumper/files.rs`: Empaquetado tar en streaming de directorios de stack y volúmenes (`/opt/stack`).
- [x] Implementar `src/engine/pipeline.rs`: Canal de streaming `Source -> zstd -> Crypto -> Storage Upload (Multipart)`.
- [x] Implementar `src/engine/retention.rs`: Motor de rotación GFS (Grandfather-Father-Son: horario 24, diario 7, semanal 4, mensual 12, anual 3).
- [x] Implementar `src/engine/scheduler.rs`: Planificador interno de tareas periódicas con Tokio (cron autónomo).

---

## [x] Fase 4: TUI Interactiva Standalone (Ratatui + Crossterm)
- [x] Implementar `src/tui/app.rs`: Estado global, tabs navegables (`Tab` / `1-5`), modal de confirmación y loop de eventos.
- [x] Implementar `src/tui/ui.rs`: Maquetación con split horizontal, header, footer con atajos y estilos de SecuryBlack.
- [x] Implementar `src/tui/views/sources.rs`: Formulario interactivo para bases de datos (Postgres, MySQL, Mongo, SQLite) y directorios.
- [x] Implementar `src/tui/views/targets.rs`: Configuración de Cloudflare R2, Hetzner, GDrive con botón `<T>` (Test Connection).
- [x] Implementar `src/tui/views/policy.rs`: Configuración de políticas de retención GFS, cron y claves de cifrado.
- [x] Implementar `src/tui/views/snapshots.rs`: Explorador de backups remotos/locales con metadatos y acción `<R>` de restauración.
- [x] Implementar `src/tui/views/live_job.rs`: Barra de progreso en tiempo real al pulsar `<B>` (Backup Now).

---

## [x] Fase 5: Integración con SecuryBlack Cloud e Intake de Comandos
- [x] Implementar `src/commands.rs`: Handlers para `sb_agent_core::command_intake` (`backup_now`, `restore_snapshot`, `list_snapshots`, `prune`).
- [x] Configurar socket de status para descubrimiento automático por `nexus-agent`.
- [x] Reporte de progreso granular mediante `sb_agent_core::command_intake::CommandProgress`.
- [x] Integración con el sistema de heartbeats de SecuryBlack (`010_uptime_heartbeats.sql`).

---

## [x] Fase 6: Scripts de Instalación, Servicio y Verificación
- [x] Crear `scripts/install.sh` y `scripts/install.ps1` usando las librerías compartidas de `sb-agent-core/scripts`.
- [x] Pruebas unitarias de cifrado, compresión y pipeline (`cargo test`).
- [x] Verificación de compilación limpia y prueba de ejecución CLI de backups bajo demanda.
