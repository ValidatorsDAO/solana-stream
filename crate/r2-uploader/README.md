# R2 Uploader

A CLI tool for uploading compiled binaries to Cloudflare R2 storage and managing Cloudflare cache.

## Overview

This tool provides two main functionalities:

1. **Upload**: Upload compiled Rust binaries to Cloudflare R2 storage
2. **Purge**: Purge Cloudflare cache for specified URLs

The upload feature stores binaries in both versioned and latest paths for easy access.

## Installation

Install the tool using Cargo:

```bash
cargo install r2-uploader
```

This will install the `r2` command globally on your system.

## Usage

### Required Environment Variables

#### For Binary Upload

```bash
# Cloudflare R2 bucket name (required)
export CLOUDFLARE_R2_BUCKET="your_bucket_name"

# Cloudflare account ID (required)
export CLOUDFLARE_ACCOUNT_ID="your_account_id"

# Authentication method: API token or API key + email
# Method 1: API token (recommended)
export CLOUDFLARE_API_TOKEN="your_api_token"

# Method 2: API key + email
export CLOUDFLARE_API_KEY="your_api_key"
export CLOUDFLARE_EMAIL="your_email@example.com"
```

#### For Cache Purging

```bash
# Cloudflare zone ID (required for cache purging)
export CLOUDFLARE_ZONE_ID="your_zone_id"

# Cache purge authentication
export CLOUDFLARE_PURGE_EMAIL="your_email@example.com"
export CLOUDFLARE_PURGE_API_TOKEN="your_purge_api_token"
```

### Commands

#### Upload Binary

Upload compiled binaries to Cloudflare R2 storage.

**Basic Usage:**

```bash
# Upload binary with auto-detected version from Cargo.toml
r2 upload --name {app_name}

# Upload binary with specific version
r2 upload --name {app_name} --binary-version 0.3.0
```

**Advanced Options:**

```bash
# Upload binary from a specific file path
r2 upload --name {app_name} --binary-version 0.3.0 --file-path /path/to/custom/binary

# Upload binary from a different target directory
r2 upload --name {app_name} --binary-version 0.3.0 --target-dir ./target/debug
```

**Upload Options:**

| Option             | Description                               | Required | Default Value    |
| ------------------ | ----------------------------------------- | -------- | ---------------- |
| `--name`           | Name of the binary to upload              | Yes      | -                |
| `--binary-version` | Version of the binary                     | No       | Auto-detected    |
| `--file-path`      | Custom file path to upload                | No       | -                |
| `--target-dir`     | Target directory to search for the binary | No       | ./target/release |

#### Purge Cache

Purge Cloudflare cache for specified URLs.

```bash
# Purge cache for multiple URLs
r2 purge --files https://example.com/file1.bin,https://example.com/file2.bin

# Purge cache for a single URL
r2 purge -f https://example.com/app.bin
```

**Purge Options:**

| Option    | Description                  | Required |
| --------- | ---------------------------- | -------- |
| `--files` | Comma-separated list of URLs | Yes      |
| `-f`      | Short form of `--files`      | Yes      |

### Help

Get help for any command:

```bash
# General help
r2 --help

# Upload command help
r2 upload --help

# Purge command help
r2 purge --help
```

## How It Works

### Binary Upload

1. Reads the binary file from the specified path or target directory
2. Automatically detects version from Cargo.toml if not specified
3. Uses the Cloudflare R2 API to upload the file to two locations:
   - Versioned path: `bin/{name}/{version}/{name}`
   - Latest path: `bin/latest/{name}`

### Cache Purging

1. Takes a list of URLs to purge from Cloudflare cache
2. Uses the Cloudflare cache purge API to invalidate the specified URLs
3. Provides detailed feedback on the purge operation

## R2 Bucket Structure

Uploaded binaries are stored with the following structure:

```
{bucket_name}/
└── bin/
    ├── {binary_name}/
    │   └── {version}/
    │       └── {binary_name}
    └── latest/
        └── {binary_name}
```

Example:

- Versioned: `{bucket_name}/bin/my-app/0.3.0/my-app`
- Latest: `{bucket_name}/bin/latest/my-app`

## Examples

### Complete Upload Workflow

```bash
# Set environment variables
export CLOUDFLARE_ACCOUNT_ID="your_account_id"
export CLOUDFLARE_API_TOKEN="your_api_token"

# Build your binary
cargo build --release

# Upload to R2 (version auto-detected from Cargo.toml)
r2 upload --name my-awesome-app

# Upload with specific version
r2 upload --name my-awesome-app --binary-version 1.0.0
```

### Cache Management

```bash
# Set cache purge environment variables
export CLOUDFLARE_ZONE_ID="your_zone_id"
export CLOUDFLARE_PURGE_EMAIL="your_email@example.com"
export CLOUDFLARE_PURGE_API_TOKEN="your_purge_token"

# Purge cache for updated binaries
r2 purge --files https://cdn.example.com/bin/latest/my-app,https://cdn.example.com/bin/my-app/1.0.0/my-app
```
