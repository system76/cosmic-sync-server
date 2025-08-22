# COSMIC Sync Server

COSMIC Sync Server is a server for synchronizing user settings and files in System76's COSMIC desktop environment.

## Features

- OAuth-based authentication
- Device registration and management
- File synchronization
- Encryption key management
- Watcher group management

## Installation and Running

### Requirements

- Rust 1.70.0 or higher
- MySQL or MariaDB
- Protobuf compiler (protoc)
- (Optional) RabbitMQ broker if enabling message bus

### Environment Configuration

Create a `.env` file in the project root or copy the provided `.env.sample`:

```bash
cp .env.sample .env
```

Then edit the `.env` file to configure the following settings:

```
# Server configuration
SERVER_HOST=0.0.0.0
SERVER_PORT=50051
WORKER_THREADS=4

# Authentication
AUTH_TOKEN_EXPIRY_HOURS=24

# Request limits
MAX_CONCURRENT_REQUESTS=100
MAX_FILE_SIZE=52428800  # 50MB in bytes

# Database configuration
DATABASE_URL=mysql://username:password@localhost:3306/cosmic_sync

# Logging configuration
LOG_LEVEL=info
LOG_TO_FILE=true
LOG_FILE=logs/cosmic-sync-server.log
LOG_MAX_FILE_SIZE=10485760  # 10MB in bytes
LOG_MAX_BACKUPS=5

# OAuth configuration
OAUTH_CLIENT_ID=your_client_id
OAUTH_CLIENT_SECRET=your_client_secret
OAUTH_REDIRECT_URI=http://localhost:50051/oauth/callback
OAUTH_AUTH_URL=https://oauth-provider.com/auth
OAUTH_TOKEN_URL=https://oauth-provider.com/token
OAUTH_USER_INFO_URL=https://oauth-provider.com/userinfo

# Feature flags
test_MODE=false
DEBUG_MODE=false
METRICS_ENABLED=true
STORAGE_ENCRYPTION=true

# Message broker (RabbitMQ)
MESSAGE_BROKER_ENABLED=false
MESSAGE_BROKER_URL=amqps://user:pass@host:5671/vhost
MESSAGE_BROKER_EXCHANGE=cosmic.sync
MESSAGE_BROKER_QUEUE_PREFIX=cosmic
MESSAGE_BROKER_PREFETCH=64
MESSAGE_BROKER_DURABLE=true
# Consumer tuning (optional)
RETRY_TTL_MS=5000
MAX_RETRIES=3
```

### Database Preparation

Create the MySQL database:

```bash
# Create the database
mysql -u root -p -e "CREATE DATABASE cosmic_sync;"
```

The server will initialize the necessary tables when first started.

### Running the Server

```bash
# Build (prefer filtered lbuild)
sudo -E /home/yongjinchong/.cargo/bin/cargo lbuild

# Run in development mode
./target/debug/cosmic-sync-server

# Build and run in release mode
sudo -E /home/yongjinchong/.cargo/bin/cargo build --release
./target/release/cosmic-sync-server
```

### Advanced Execution Options

To run the server with specific environment variables and debug options:

```bash
# Run with development mode, debug mode, and debug logging
RUST_LOG=debug sudo -E /home/yongjinchong/.cargo/bin/cargo run

# Run the compiled binary directly with root privileges
RUST_LOG=debug sudo -E ./target/debug/cosmic-sync-server
```

## Project Structure

The COSMIC Sync Server is organized into several modules:

- **Server**: Contains HTTP and gRPC server implementations 
- **Handlers**: Request handlers for different API endpoints
- **Services**: Business logic for handling authentication, devices, files, etc.
- **Storage**: Data storage implementations (MySQL, in-memory)
- **Models**: Data models for accounts, devices, files, etc.
- **Auth**: Authentication and OAuth implementations
- **Utils**: Utility functions for crypto, time, etc.
- **Config**: Centralized constants and settings (including message broker)
- **Event Bus**: RabbitMQ integration behind trait abstraction
- **Tests**: Integration tests under top-level `tests/`

### Storage Module Architecture

The storage system is designed with a modular approach using traits:

- `Storage`: Main trait defining all storage operations
- Implementation options:
  - `MySqlStorage`: Production storage using MySQL
  - `MemoryStorage`: In-memory storage for testing

The MySQL implementation is split into multiple files for better maintainability:
- `mysql.rs`: Core MySQL implementation and connection management
- `mysql_account.rs`: Account-related operations
- `mysql_auth.rs`: Authentication token operations
- `mysql_device.rs`: Device management operations
- `mysql_file.rs`: File storage and retrieval operations
- `mysql_watcher.rs`: Watcher and synchronization operations

## Event Bus (RabbitMQ)

- Abstraction: `src/server/event_bus.rs`
  - `EventBus` trait with `publish`/`subscribe`
  - `RabbitMqEventBus` (lapin) and `NoopEventBus` (disabled)
- Routing keys (topic):
  - Files: `file.uploaded.{account_hash}.{group_id}.{watcher_id}`, `file.deleted.{account_hash}`
  - Versions: `version.created.{account_hash}.{file_id}`, `version.deleted.{account_hash}.{file_id}`, `version.restored.{account_hash}.{file_id}`
  - Devices/Watchers as applicable
- Emission sites: handlers layer (`src/handlers/...`), not services
- Consumer example: `src/bin/rabbit_consumer.rs`
  - Declares DLX (`<exchange>.dlx`), main/retry/DLQ queues, basic idempotency
  - Run: `sudo -E /home/yongjinchong/.cargo/bin/cargo run --bin rabbit_consumer`

## Tests

- Integration tests live in `tests/` with shared utilities in `tests/common/mod.rs`
- Quick compile: `sudo -E /home/yongjinchong/.cargo/bin/cargo test --no-run`
- Run: `sudo -E /home/yongjinchong/.cargo/bin/cargo test`

## Development

### Protocol Buffer Generation

After modifying the Protocol Buffer definitions, run the following command to generate Rust code:

```bash
sudo -E /home/yongjinchong/.cargo/bin/cargo build
```

The `build.rs` script will automatically compile the protobuf files.

### Authentication Flow

The server uses OAuth for authentication:

1. Client requests auth URL from server
2. User completes OAuth flow in browser
3. OAuth provider redirects to callback URL with auth code
4. Server exchanges code for tokens
5. Server creates an account for the user if needed
6. Client registers the device
7. Authentication complete

### Device Registration

Device registration is a separate process from authentication:

1. Client completes authentication flow and receives `auth.json`
2. Client sends device information to register
3. Server creates or updates device record
4. Server returns success/failure status

### File Synchronization 

Files are synchronized based on watcher groups:

1. Client registers watcher groups for specific directories
2. When files change, client uploads to server
3. Server stores files in database with metadata (or S3, depending on config)
4. Other devices can download changed files
5. Encryption is supported for secure file storage

## Troubleshooting

### Common Issues

#### Database Connection

If you encounter database connection issues:
- Verify MySQL/MariaDB is running
- Check credentials in .env file
- Ensure database exists and is accessible

#### Authentication Failures

OAuth authentication issues:
- Verify OAuth provider settings
- Check client ID and secret
- Ensure redirect URI is properly configured
- Check logs for detailed error messages

#### File Synchronization Problems

If file synchronization isn't working:
- Check device registration status
- Verify watcher groups are properly configured
- Check file permissions on client
- Increase log level for detailed diagnostics

## License

This project is licensed under the terms of the GNU General Public License v3.0.