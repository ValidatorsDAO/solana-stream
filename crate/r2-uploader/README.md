# R2 Uploader

A CLI tool for uploading compiled binaries to Cloudflare R2 storage.

## Overview

This tool is a simple CLI tool for uploading compiled Rust binaries to Cloudflare R2 storage.
It uploads binaries to the appropriate location in the R2 bucket using the specified binary name and version.

## Installation

Install the tool using Cargo:

```bash
cargo install r2-uploader
```

This will install the `r2` command globally on your system.

## Usage

### Required Environment Variables

Before using this tool, you need to set the following environment variables:

```bash
# Cloudflare account ID (required)
export CLOUDFLARE_ACCOUNT_ID="your_account_id"

# Authentication method: API token or API key + email
# Method 1: API token (recommended)
export CLOUDFLARE_API_TOKEN="your_api_token"

# Method 2: API key + email
export CLOUDFLARE_API_KEY="your_api_key"
export CLOUDFLARE_EMAIL="your_email@example.com"
```

### Command Examples

#### Basic Usage

```bash
# Upload {app_name} binary from target/release directory as version 0.3.0
r2 --name {app_name} --binary-version 0.3.0
```

#### Auto-detect Version from Cargo.toml

```bash
# Upload binary and auto-detect version from Cargo.toml
r2 --name {app_name}
```

#### Specify Custom File Path

```bash
# Upload binary from a specific file path
r2 --name {app_name} --binary-version 0.3.0 --file-path /path/to/custom/binary
```

#### Specify Custom Target Directory

```bash
# Upload binary from a different target directory
r2 --name {app_name} --binary-version 0.3.0 --target-dir ./target/debug
```

### Options

| Option             | Description                               | Required | Default Value    |
| ------------------ | ----------------------------------------- | -------- | ---------------- |
| `--name`           | Name of the binary to upload              | Yes      | -                |
| `--binary-version` | Version of the binary                     | Yes      | -                |
| `--file-path`      | Custom file path to upload                | No       | -                |
| `--target-dir`     | Target directory to search for the binary | No       | ./target/release |

## How It Works

1. Reads the binary file from the specified path or target directory
2. Uses the Cloudflare R2 API to upload the file with the specified name and version
3. The upload destination path format is `bin/{version}/{name}`

## R2 Bucket Structure

Uploaded binaries are stored with the following structure:

```
{bucket_name}/
└── bin/
    └── {version}/
        └── {name}
```

Example: `{bucket_name}/bin/0.3.0/{app_name}`
