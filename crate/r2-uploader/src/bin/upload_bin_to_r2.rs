use std::{
    env,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};

use clap::{Parser, Subcommand};
use r2_uploader::purge_cloudflare_cache;
use reqwest::{Client, StatusCode};

/// CLI arguments
#[derive(Parser, Debug)]
#[command(
    author,
    about = "Cloudflare R2 and cache management tool",
    disable_version_flag(true)
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Upload compiled binary to Cloudflare R2
    Upload {
        /// Name of the binary
        #[arg(long)]
        name: String,

        /// Version of the binary (optional, reads from Cargo.toml if not provided)
        #[arg(long)]
        binary_version: Option<String>,

        /// Custom file path to upload (optional)
        #[arg(long)]
        file_path: Option<String>,

        /// Target directory (default: ./target/release)
        #[arg(long, default_value = "./target/release")]
        target_dir: String,
    },
    /// Purge Cloudflare cache for specified URLs
    Purge {
        /// URLs to purge from cache
        #[arg(short, long, value_delimiter = ',')]
        files: Vec<String>,
    },
}

/// Determines the appropriate content type based on file extension
fn get_content_type(file_path: &Path) -> &'static str {
    match file_path.extension().and_then(|ext| ext.to_str()) {
        Some("gz") => {
            // Check if it's a .tar.gz file
            if file_path
                .file_stem()
                .and_then(|stem| Path::new(stem).extension())
                .and_then(|ext| ext.to_str())
                == Some("tar")
            {
                "application/gzip"
            } else {
                "application/gzip"
            }
        }
        Some("tar") => "application/x-tar",
        Some("zip") => "application/zip",
        Some("bz2") => "application/x-bzip2",
        Some("xz") => "application/x-xz",
        _ => "application/x-elf", // Default for binary files
    }
}

/// Finds the best matching file for the given binary name and target directory
fn find_binary_file(binary_name: &str, target_dir: &str) -> Option<PathBuf> {
    let base_path = PathBuf::from(target_dir);

    // List of possible file patterns to check, in order of preference
    let patterns = vec![
        // Exact binary name (original behavior)
        format!("{}", binary_name),
        // Compressed archives with binary name
        format!("{}.tar.gz", binary_name),
        format!("{}.tar.bz2", binary_name),
        format!("{}.tar.xz", binary_name),
        format!("{}.zip", binary_name),
        format!("{}.gz", binary_name),
    ];

    for pattern in patterns {
        let candidate_path = base_path.join(&pattern);
        if candidate_path.exists() {
            println!("🔍 Found file: {}", candidate_path.display());
            return Some(candidate_path);
        }
    }

    None
}

/// Reads version from Cargo.toml
fn read_version_from_cargo_toml(binary_name: &str) -> Result<String, String> {
    // First try to read from project-specific Cargo.toml
    let project_cargo_path = format!("./{}/Cargo.toml", binary_name);
    let content = match fs::read_to_string(&project_cargo_path) {
        Ok(content) => {
            println!("📋 Reading version from {}", project_cargo_path);
            content
        }
        Err(_) => {
            println!("📋 Project-specific Cargo.toml not found, trying workspace Cargo.toml");
            // Fall back to workspace Cargo.toml
            match fs::read_to_string("Cargo.toml") {
                Ok(content) => content,
                Err(_) => return Err("Failed to read any Cargo.toml".to_string()),
            }
        }
    };

    // For project-specific Cargo.toml, look for version field
    for line in content.lines() {
        let line = line.trim();

        // Check for project-specific version
        if line.starts_with("version") {
            return match line.split_once('=') {
                Some((_, version)) => {
                    let version = version.trim().trim_matches('"').trim_matches('\'');
                    Ok(version.to_string())
                }
                None => Err("Invalid version format in Cargo.toml".to_string()),
            };
        }

        // Check for workspace package.version
        if line.starts_with("package.version") {
            return match line.split_once('=') {
                Some((_, version)) => {
                    let version = version.trim().trim_matches('"').trim_matches('\'');
                    Ok(version.to_string())
                }
                None => Err("Invalid package.version format in Cargo.toml".to_string()),
            };
        }
    }

    Err("Version not found in any Cargo.toml".to_string())
}

/// Uploads a compiled Rust binary to Cloudflare R2.
///
/// # Arguments
///
/// * `binary_name` - Name of the binary to upload to R2
/// * `version` - Version string for the binary
/// * `file_path` - Optional custom path to the binary file
/// * `target_dir` - Directory to look for the binary if file_path not provided
///
/// # Returns
/// true if upload succeeded, false otherwise
pub async fn upload_compiled_binary_to_r2(
    binary_name: &str,
    version: &str,
    file_path: Option<&str>,
    target_dir: &str,
) -> bool {
    // 1. Determine file path
    let binary_path = if let Some(path) = file_path {
        PathBuf::from(path)
    } else {
        match find_binary_file(binary_name, target_dir) {
            Some(path) => path,
            None => {
                eprintln!("❌ Could not find binary file for '{}' in directory '{}'", binary_name, target_dir);
                eprintln!("💡 Searched for patterns: {}, {}.tar.gz, {}.tar.bz2, {}.tar.xz, {}.zip, {}.gz", 
                    binary_name, binary_name, binary_name, binary_name, binary_name, binary_name);
                eprintln!("💡 Use --file-path to specify a custom file path");
                return false;
            }
        }
    };

    // 2. Read binary file
    println!("🔍 Reading file from: {}", binary_path.display());
    let mut file = match File::open(&binary_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("❌ Failed to open file {}: {}", binary_path.display(), e);
            return false;
        }
    };

    let mut buffer = Vec::new();
    if let Err(e) = file.read_to_end(&mut buffer) {
        eprintln!("❌ Failed to read binary {}: {}", binary_path.display(), e);
        return false;
    }

    // 3. Construct Cloudflare R2 upload URL
    let account_id = match env::var("CLOUDFLARE_ACCOUNT_ID") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("❌ Missing CLOUDFLARE_ACCOUNT_ID");
            return false;
        }
    };

    let bucket_name = match env::var("CLOUDFLARE_R2_BUCKET") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("❌ Missing CLOUDFLARE_R2_BUCKET");
            return false;
        }
    };
    let object_key = format!("bin/{}/{}/{}", binary_name, version, binary_name);
    let latest_object_key = format!("bin/latest/{}", binary_name);
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}/objects/{}",
        account_id, bucket_name, object_key
    );
    println!("🔗 Upload URL: {}", url);

    // 4. Setup headers
    let mut headers = reqwest::header::HeaderMap::new();

    if let Ok(token) = env::var("CLOUDFLARE_API_TOKEN") {
        println!("🔑 Using API Token authentication");
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", token).parse().unwrap(),
        );
    } else {
        let email = match env::var("CLOUDFLARE_EMAIL") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("❌ Missing CLOUDFLARE_EMAIL");
                return false;
            }
        };
        let api_key = match env::var("CLOUDFLARE_API_KEY") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("❌ Missing CLOUDFLARE_API_KEY");
                return false;
            }
        };
        println!("🔑 Using API Key authentication");
        headers.insert("X-Auth-Email", email.parse().unwrap());
        headers.insert("X-Auth-Key", api_key.parse().unwrap());
    }

    // Determine and set appropriate content type based on file extension
    let content_type = get_content_type(&binary_path);
    headers.insert("Content-Type", content_type.parse().unwrap());
    println!("📄 Content-Type: {}", content_type);

    // 5. Upload to versioned path
    println!(
        "📤 Uploading {} to R2 as {}",
        binary_path.display(),
        object_key
    );
    let client = Client::new();
    let response = match client
        .put(&url)
        .headers(headers.clone())
        .body(buffer.clone())
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("❌ HTTP request failed for versioned path: {}", e);
            return false;
        }
    };

    if response.status() != StatusCode::OK {
        let status = response.status();
        let body = response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".into());
        eprintln!("❌ Upload failed for versioned path: {} - {}", status, body);
        return false;
    }

    println!("✅ Successfully uploaded to R2: {}", object_key);

    // 6. Upload to latest path
    let latest_url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}/objects/{}",
        account_id, bucket_name, latest_object_key
    );

    println!(
        "📤 Uploading {} to R2 as {}",
        binary_path.display(),
        latest_object_key
    );

    let latest_response = match client
        .put(&latest_url)
        .headers(headers)
        .body(buffer)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(e) => {
            eprintln!("❌ HTTP request failed for latest path: {}", e);
            return false;
        }
    };

    if latest_response.status() == StatusCode::OK {
        println!("✅ Successfully uploaded to R2: {}", latest_object_key);
        true
    } else {
        let status = latest_response.status();
        let body = latest_response
            .text()
            .await
            .unwrap_or_else(|_| "Unknown error".into());
        eprintln!("❌ Upload failed for latest path: {} - {}", status, body);
        false
    }
}

#[tokio::main]
async fn main() {
    dotenv::dotenv().ok();

    // Parse command line arguments
    let cli = Cli::parse();

    match cli.command {
        Commands::Upload {
            name,
            binary_version,
            file_path,
            target_dir,
        } => {
            // Get version from CLI args or Cargo.toml
            let version = match binary_version {
                Some(v) => v,
                None => {
                    println!("🔍 No version specified, reading from Cargo.toml...");
                    match read_version_from_cargo_toml(&name) {
                        Ok(v) => {
                            println!("📋 Found version {}", v);
                            v
                        }
                        Err(e) => {
                            eprintln!("❌ Error reading version: {}", e);
                            std::process::exit(1);
                        }
                    }
                }
            };

            println!("🔄 Uploading {} (version {}) to R2...", name, version);

            let result =
                upload_compiled_binary_to_r2(&name, &version, file_path.as_deref(), &target_dir)
                    .await;

            if result {
                println!("✅ Upload succeeded!");
            } else {
                eprintln!("❌ Upload failed.");
                std::process::exit(1);
            }
        }
        Commands::Purge { files } => {
            if files.is_empty() {
                eprintln!(
                    "❌ No files specified for purging. Use --files to specify URLs to purge."
                );
                std::process::exit(1);
            }

            println!("🧹 Starting cache purge operation...");
            let result = purge_cloudflare_cache(files).await;

            if result {
                println!("✅ Cache purge succeeded!");
            } else {
                eprintln!("❌ Cache purge failed.");
                std::process::exit(1);
            }
        }
    }
}
