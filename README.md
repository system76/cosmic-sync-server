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
TEST_MODE=false
DEBUG_MODE=false
METRICS_ENABLED=true
STORAGE_ENCRYPTION=true
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
# Run in development mode
cargo run

# Build and run in release mode
cargo build --release
./target/release/cosmic-sync-server
```

### Advanced Execution Options

To run the server with specific environment variables and debug options:

```bash
# Run with development mode, debug mode, and debug logging
RUST_LOG=debug cargo run

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

## Database Structure

The COSMIC Sync Server uses a relational database (MySQL/MariaDB) to store user data, device information, files, and synchronization settings. Below is an overview of the main tables:

### accounts
- `id`: VARCHAR(36) - Unique identifier
- `email`: VARCHAR(255) - User email
- `account_hash`: VARCHAR(255) - Account hash (Primary Key)
- `name`: VARCHAR(255) - User name
- `password_hash`: VARCHAR(255) - Hashed password (for non-OAuth)
- `salt`: VARCHAR(255) - Password salt (for non-OAuth)
- `created_at`: BIGINT - Account creation timestamp
- `updated_at`: BIGINT - Last update timestamp
- `last_login`: BIGINT - Last login timestamp
- `is_active`: BOOLEAN - Account status

### devices
- `id`: VARCHAR(36) - Unique identifier
- `account_hash`: VARCHAR(255) - Reference to accounts table
- `device_hash`: VARCHAR(255) - Device hash (Primary Key)
- `device_name`: VARCHAR(255) - Device name
- `device_type`: VARCHAR(50) - Device type
- `os_type`: VARCHAR(50) - OS type
- `os_version`: VARCHAR(50) - OS version
- `app_version`: VARCHAR(50) - App version
- `last_sync`: BIGINT - Last sync timestamp
- `created_at`: BIGINT - Registration timestamp
- `updated_at`: BIGINT - Last update timestamp
- `is_active`: BOOLEAN - Device status

### files
- `id`: BIGINT UNSIGNED AUTO_INCREMENT - Internal ID (Primary Key)
- `file_id`: BIGINT UNSIGNED - Unique file identifier
- `account_hash`: VARCHAR(255) - Reference to accounts table
- `device_hash`: VARCHAR(255) - Reference to devices table
- `file_path`: VARCHAR(1024) - File path
- `filename`: VARCHAR(255) - File name
- `file_hash`: VARCHAR(255) - File hash
- `ctime`: BIGINT - Creation timestamp
- `mtime`: BIGINT - Modification timestamp
- `size`: BIGINT - File size
- `is_deleted`: BOOLEAN - Deletion status
- `is_encrypted`: BOOLEAN - Encryption status
- `revision`: BIGINT - File revision
- `created_time`: BIGINT - Upload timestamp
- `updated_time`: BIGINT - Last update timestamp
- `group_id`: INT - Watcher group ID
- `watcher_id`: INT - Watcher ID

### file_data
- `file_id`: BIGINT UNSIGNED - Reference to files table (Primary Key)
- `data`: LONGBLOB - Actual file data
- `created_at`: BIGINT - Creation timestamp
- `updated_at`: BIGINT - Last update timestamp

### auth_tokens
- `id`: VARCHAR(36) - Token ID (Primary Key)
- `account_hash`: VARCHAR(255) - Reference to accounts table
- `access_token`: VARCHAR(1024) - Access token
- `refresh_token`: VARCHAR(1024) - Refresh token
- `token_type`: VARCHAR(20) - Token type
- `expires_at`: BIGINT - Expiration timestamp
- `created_at`: BIGINT - Creation timestamp

### watcher_groups and related tables
Tables for managing file watching and synchronization:
- `watcher_groups`: Groups of watchers
- `watchers`: Individual file watchers
- `group_watchers`: Relationship between groups and watchers
- `watcher_presets`: Preset configurations

## API Documentation

The server provides a gRPC API. The API definitions can be found in the `proto/sync.proto` file.

Main API endpoints:

1. Authentication API
   - `Login` - User login
   - `VerifyLogin` - Token verification
   - `ExchangeOauthCode` - OAuth code exchange
   - `ValidateToken` - Validate authentication token
   - `CheckAuthStatus` - Check authentication status

2. Device Management API
   - `RegisterDevice` - Register a device
   - `ListDevices` - List registered devices
   - `DeleteDevice` - Delete a device
   - `UpdateDeviceInfo` - Update device information

3. File Synchronization API
   - `UploadFile` - Upload a file
   - `DownloadFile` - Download a file
   - `ListFiles` - List available files
   - `DeleteFile` - Delete a file
   - `RequestEncryptionKey` - Request encryption key

4. Watcher Management API
   - `RegisterWatcherGroup` - Register a watcher group
   - `UpdateWatcherGroup` - Update a watcher group
   - `DeleteWatcherGroup` - Delete a watcher group
   - `GetWatcherGroup` - Get a specific watcher group
   - `GetWatcherGroups` - Get all watcher groups
   - `RegisterWatcherPreset` - Register a watcher preset
   - `UpdateWatcherPreset` - Update a watcher preset
   - `GetWatcherPreset` - Get watcher presets

5. Subscription API
   - `SubscribeToFileChanges` - Subscribe to file changes
   - `SubscribeToDeviceChanges` - Subscribe to device changes
   - `SubscribeToWatcherChanges` - Subscribe to watcher changes
   - `SubscribeToGroupChanges` - Subscribe to group changes
   - `SubscribeToAuthChanges` - Subscribe to auth changes

6. System API
   - `HealthCheck` - Check server health

## Development

### Protocol Buffer Generation

After modifying the Protocol Buffer definitions, run the following command to generate Rust code:

```bash
cargo build
```

The build.rs script will automatically compile the protobuf files.

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
3. Server stores files in database with metadata
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