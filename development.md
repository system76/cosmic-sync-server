## COSMIC Sync Server — Development Guide for LLMs

This document distills project-wide development knowledge that helps LLMs implement changes safely, consistently, and with high signal. It focuses on: what exists, where to change, how to reason about side effects, and conventions you must respect.

### Build, Run, Tooling
- Build (preferred):
  - `cargo lbuild` (reduced error output; shows errors first)
  - `cargo build`
- Run server:
  - `./target/debug/cosmic-sync-server` (gRPC + HTTP)
- Default servers:
  - gRPC: `config.server.host:config.server.port` (e.g., 0.0.0.0:50051)
  - HTTP: `config.server.host:8080` (health, OAuth)
- Background processes: prefer running server in background and tailing `server_log.txt` for analysis.

### Configuration and Environments
- Primary config loader: `src/config/settings.rs` and env variables (e.g., `SERVER_HOST`, `GRPC_PORT`).
- Storage selection:
  - If `ServerConfig.storage_path` starts with `mysql://`, server uses MySQL.
  - Otherwise falls back to in‑memory storage.
- OAuth service defaults via environment variables:
  - `OAUTH_CLIENT_ID`, `OAUTH_CLIENT_SECRET`, `OAUTH_REDIRECT_URI`, `OAUTH_AUTH_URL`, `OAUTH_TOKEN_URL`, `OAUTH_USER_INFO_URL`, `OAUTH_SCOPE`.
- Dev/test relaxations:
  - `COSMIC_SYNC_DEV_MODE=1` or `COSMIC_SYNC_TEST_MODE=1` may relax certain validations (e.g., device validation in some subscriptions).

### High‑level Architecture
- gRPC services (entrypoint): `src/server/service.rs` wires handlers to proto methods.
- HTTP endpoints: `src/server/startup.rs` mounts handlers for OAuth and health/metrics.
- Handlers (request logic): `src/handlers/*` (auth/device/file/watcher/sync, etc.).
- Storage abstraction (trait): `src/storage/mod.rs` with concrete impls:
  - MySQL: `src/storage/mysql.rs` (+ submodules: account/auth/device/file/watcher)
  - Memory (for testing): `src/storage/memory.rs`
- Auth/OAuth service: `src/auth/oauth.rs`, token helpers in `src/auth/token.rs`.
- Models (DB/domain): `src/models/*`.
- Utilities: `src/utils/*` (crypto/time/helpers, etc.).

### Server Startup and State
- Startup: `src/server/startup.rs` creates storage via `init_storage_from_config(...)` and injects it into `AppState::new_with_storage_and_server_config(...)`.
- Important: Use the single storage instance created at startup. Do not re‑create memory storage elsewhere; that leads to writes not reaching MySQL.

### Authentication and Account Hash Canonicalization
- OAuth code exchange: `handlers/auth_handler.rs::handle_oauth_exchange` → `auth::oauth::process_oauth_code`.
- Token verification: `OAuthService::verify_token` returns `{ valid, account_hash }`.
- Canonical account hash: server always trusts the hash from the verified token.
  - If request.account_hash differs, server normalizes to token’s hash and logs at debug.
  - Several handlers already implement this rule (e.g., device/register, file handlers, watcher handlers).

### Watcher Model and Mapping (critical semantics)
- Semantics: user‑level logical grouping is always by `watcher_group`. A `watcher` is a local monitor tied to a folder/conditions.
- Mapping (client → server IDs):
  - Use `MySqlStorage::ensure_server_ids_for(account_hash, device_hash, client_group_id, client_watcher_id, folder_hint)`.
  - This function auto‑creates missing `watcher_groups`/`watchers` and returns actual server IDs, eliminating FK failures.
- Path normalization: `utils::helpers::normalize_path_preserve_tilde` keeps `~` for home paths. Handlers and storage consistently preserve this.

### File Lifecycle (Upload/Find/List/Download/Delete)
- Upload: `handlers/file_handler.rs`.
  - Verifies token; normalizes account_hash; normalizes path; performs watcher validation (soft‑fail → debug only); ensures server IDs with `ensure_server_ids_for` if MySQL.
  - Generates deterministic `file_id`; persists metadata; stores data via configured file storage (DB or S3), depending on setup.
- Find/List: converts client IDs to server IDs as needed and returns client IDs in responses when appropriate.
  - Recovery sync supports optional time filtering; see `list_files` in `FileHandler`.
- Download/Delete: standard token check, fetch by id or (path,name), and stream response.

### Database (MySQL) Overview
- Storage core: `src/storage/mysql.rs` (plus per‑domain ext modules under `src/storage/`).
- Schema initializer/migrator lives in MySQL storage layers; tables include:
  - `accounts(account_hash PK, id UUID, email, name, timing cols, is_active)`
  - `auth_tokens(token PK, account_hash FK→accounts, created_at, expires_at, is_active)`
  - `devices(device_hash PK, account_hash FK, os_version, app_version, last_sync, created_at, updated_at, is_active)`
  - `files(id AUTO, file_id, account_hash FK, device_hash, file_path, filename, file_hash, size, flags, revision, times, group_id, watcher_id)`
  - `encryption_keys(id AUTO, account_hash UNIQUE FK, encryption_key, created_at, updated_at)`
  - `watcher_groups(id AUTO PK, account_hash, group_id (client), title, created_at, updated_at, is_active, [indexes])`
  - `watchers(id AUTO PK, account_hash, watcher_id (client), group_id (server), local_group_id (client), folder, title, flags, times, extra_json)`
  - `watcher_conditions(id AUTO PK, account_hash, watcher_id FK, type, key, value(JSON), operator, times)`
- Canonical identifiers:
  - `account_hash`: always trust token.
  - `(group_id, watcher_id)`: client‑side IDs; map to server IDs via mapping logic.
  - `file_id`: client’s deterministic id; server’s `files.id` is an auto key for DB metadata; do not confuse them.

### HTTP Endpoints
- Health: `/health`, `/health/ready`, `/health/live`.
- Metrics: `/metrics`, `/metrics/detailed`.
- OAuth: `/oauth/login`, `/oauth/callback` (used during desktop OAuth flows).
- Session helpers: `/auth/session`, `/auth/status` (assist OAuth login UX on desktop).

### gRPC Surfaces (selected)
- OAuth: `exchange_oauth_code`.
- Device: `register_device`, `list_devices`, `delete_device`, `update_device_info`.
- Files: `upload_file`, `download_file`, `list_files`, `delete_file`, `find_file_by_criteria`, `check_file_exists`.
- Watchers: `register_watcher_group`, `update_watcher_group`, `delete_watcher_group`, `get_watcher_group`, `get_watcher_groups`, presets.
- Sync: `request_encryption_key`, version history and restore APIs, streaming subscriptions.

### Logging and Error‑handling Conventions
- Use `tracing` macros. Keep noisy runtime details at `debug!`. Reserve `info!` for milestones; `warn!` for degraded behavior; `error!` for failures.
- Handlers return well‑formed response messages instead of surfacing raw internal errors where feasible.
- Build logs: prefer `lbuild` when many warnings obscure errors.

### Development Rules and Pitfalls (must follow)
- Do not re‑introduce multiple storage initializations. Always route through `start_server(...)` or `start_server_with_storage(...)` and pass the same storage to `AppState::new_with_storage_and_server_config(...)`.
- Do not fail device registration or uploads due to client‑provided `account_hash` mismatch. Normalize to token’s `account_hash` and continue (log at debug).
- Watcher semantics: always reason and sync by `watcher_group` as the user’s primary logical unit. `watchers` are local physical monitors.
- MySQL SSL options: when using CLI, `--ssl-mode=DISABLED` may be required; in code use plain MySQL URLs and let the driver handle options. Do not depend on CLI‑only flags in driver URLs.
- Be careful with identifier domains:
  - Server DB `files.id` is auto; `file_id` in protocol/clients is deterministic and not the same. Align mapping accordingly.

### Where to Change What (common tasks)
- Add/modify an API method: update `src/server/service.rs` to wire to the corresponding handler in `src/handlers/...`.
- Add storage behavior:
  - Define trait functions in `src/storage/mod.rs`.
  - Implement in both `src/storage/mysql.rs` (and its ext module) and `src/storage/memory.rs` when feasible.
- Change OAuth: adjust `src/auth/oauth.rs` and ensure handlers (`handlers/auth_handler.rs`, HTTP routes) are consistent.
- Watcher/group operations: extend `src/storage/mysql_watcher.rs` and corresponding handler logic.
- File operations: see `handlers/file_handler.rs` and `storage/mysql_file.rs`.

### Performance Notes
- Use a single MySQL connection pool; avoid blocking operations in handlers.
- Batch notifications in subscriptions to avoid flooding clients.
- Prefer server‑side mapping creation (`ensure_server_ids_for`) to prevent FK failures and retries.

### Coding Style and Safety
- Prefer clear, verbose code with explicit naming.
- Handle error cases first (guard clauses), early returns.
- Avoid deep nesting and avoid creating silent fallthroughs.
- For new behavior, update both storage backends (MySQL/Memory) or explicitly document the limitation.

### Quick File Map (frequently referenced)
- Startup/state: `src/server/startup.rs`, `src/server/app_state.rs`
- gRPC wiring: `src/server/service.rs`
- HTTP routes: `src/server/startup.rs`, `src/handlers/*`
- Auth: `src/auth/oauth.rs`, `src/handlers/auth_handler.rs`
- Files: `src/handlers/file_handler.rs`, `src/storage/mysql_file.rs`
- Watchers: `src/handlers/watcher_handler.rs`, `src/services/watcher_service.rs`, `src/storage/mysql_watcher.rs`
- Storage core: `src/storage/mysql.rs`, `src/storage/memory.rs`, `src/storage/mod.rs`
- Utils: `src/utils/*` (tilde path normalization, crypto, time)

### Example Dev Checks
- MySQL quick check (CLI):
  - `mysql -h 127.0.0.1 -P 3306 -u root -precognizer --ssl-mode=DISABLED -e "USE cosmic_sync; SHOW TABLES;"`
- Verify account/token:
  - After OAuth, check `accounts` and `auth_tokens` for entries matching the token’s `account_hash`.
- Watcher mapping sanity:
  - Upload with a new `(group_id, watcher_id)` and verify `watcher_groups`/`watchers` created via `ensure_server_ids_for`.

---

This guide is designed for LLM agents to make safe, incremental edits without regressing core flows (OAuth, file sync, watcher mapping). Always respect canonical `account_hash` (token), `watcher_group` as the user’s unit, and avoid duplicating storage initialization.

### Message Broker and Event Bus (RabbitMQ)
- Event bus abstraction: `src/server/event_bus.rs`
  - `EventBus` trait with `publish` and `subscribe`
  - `NoopEventBus` (fallback when disabled)
  - `RabbitMqEventBus` (lapin-based, topic exchange, publisher confirms)
- Configuration: `MessageBrokerConfig` in `src/config/settings.rs`
  - Environment variables (examples):
    - `MESSAGE_BROKER_ENABLED=1|0`
    - `MESSAGE_BROKER_URL=amqps://user:pass@host:5671/vhost`
    - `MESSAGE_BROKER_EXCHANGE=cosmic.sync`
    - `MESSAGE_BROKER_QUEUE_PREFIX=cosmic` (consumer queues derive from this)
    - `MESSAGE_BROKER_PREFETCH=64` (per-consumer QoS)
    - `MESSAGE_BROKER_DURABLE=true`
    - Optional consumer tuning:
      - `RETRY_TTL_MS=5000` (retry delay for invalid payloads)
      - `MAX_RETRIES=3` (after this, message goes to DLQ)
- Publishing patterns (topic routing keys):
  - Files: `file.uploaded.{account_hash}.{group_id}.{watcher_id}`, `file.deleted.{account_hash}`
  - Versions: `version.created.{account_hash}.{file_id}`, `version.deleted.{account_hash}.{file_id}`, `version.restored.{account_hash}.{file_id}`
  - Devices: `device.registered.{account_hash}`, `device.updated.{account_hash}`
  - Watchers: `watcher.group.update.{account_hash}`, `watcher.preset.update.{account_hash}`
- Emission sites (handlers layer):
  - Upload: `src/handlers/file/upload.rs` → publishes `file.uploaded`, `version.created`
  - Delete: `src/handlers/file/delete.rs` → publishes `file.deleted`, `version.deleted`
  - Restore: `src/handlers/sync_handler.rs` (broadcast) → publishes `version.restored`
  - Note: Service layer does not publish directly; handlers are the single point for side effects
- Consumer sample: `src/bin/rabbit_consumer.rs`
  - Subscribes to `file.*.#`, `version.*.#`, `device.*.#`, `watcher.*.update.#`
  - Declares DLX (`<exchange>.dlx`), durable queues, retry queues (TTL), and DLQ
  - Simple idempotency via in-process cache (consider Redis/MySQL for production)
  - Run: `cargo run --bin rabbit_consumer`

### Centralized Constants and Settings
- Central constants: `src/config/constants.rs` (ports, HTTP keepalive, DB/S3 defaults, etc.)
- Settings model: `src/config/settings.rs`
  - Uses constants for defaults
  - Includes `MessageBrokerConfig`
- Secrets/config loader: `src/config/secrets.rs`
  - Ensures `message_broker` is populated in the composite `Config`

### Tests Layout (Unified)
- Integration/scenario tests live under top-level `tests/`
  - Shared helpers: `tests/common/mod.rs`
  - Examples: `tests/device_handler_integration.rs`, `tests/grpc_auth_status.rs`
- Unit tests may remain inline with `#[cfg(test)]` in modules
- Running:
  - `cargo test --no-run` (compile)
  - `cargo test` (execute)

### Performance Optimizations
- S3 client lazy init: `tokio::sync::OnceCell` in `src/storage/file_storage.rs`
  - One-time bucket existence check; internal `get_client().await?` APIs
- Path normalization memoization: `once_cell::sync::Lazy + Mutex<HashMap>` in `src/utils/helpers.rs`
  - Short inputs cached to reduce repeated work
- Error handling hardening: replaced `unwrap/expect` in critical paths
- Event emission at handler layer to avoid redundant work or circular deps

### Quick Runbook (updated)
- Build (prefer lbuild):
  - `cargo lbuild`
- Run server (debug):
  - `./target/debug/cosmic-sync-server`
- Run consumer (debug):
  - Set `MESSAGE_BROKER_ENABLED=1` and broker env vars
  - `cargo run --bin rabbit_consumer`

### Pitfalls and Conventions (event-driven)
- Do not publish from service layer; consolidate side effects in handlers
- Standardize event payloads to JSON; include `account_hash`, `device_hash`, identifiers, and `timestamp`
- Prefer topic routing keys that reflect domain: `entity.action.dimensions...`
- For idempotency, add an `id` to payloads (e.g., `nanoid`) and enforce at consumers


