use std::{
    env,
    fs::{self, File},
    io::Read,
    path::PathBuf,
};

use clap::Parser;
use reqwest::{Client, StatusCode};

/// CLI arguments
#[derive(Parser, Debug)]
#[command(
    author,
    about = "Upload compiled binary to Cloudflare R2",
    disable_version_flag(true)
)]
struct Cli {
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
        PathBuf::from(format!("{}/{}", target_dir, binary_name))
    };

    // 2. Read binary file
    println!("🔍 Reading binary from: {}", binary_path.display());
    let mut file = match File::open(&binary_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("❌ Failed to open binary {}: {}", binary_path.display(), e);
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

    let bucket_name = "elsoul";
    let object_key = format!("bin/{}/{}/{}", binary_name, version, binary_name);
    let latest_object_key = format!("bin/latest/{}", binary_name);
    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}/objects/{}",
        account_id, bucket_name, object_key
    );

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

    headers.insert("Content-Type", "application/x-elf".parse().unwrap());

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

    // Get version from CLI args or Cargo.toml
    let version = match &cli.binary_version {
        Some(v) => v.clone(),
        None => {
            println!("🔍 No version specified, reading from Cargo.toml...");
            match read_version_from_cargo_toml(&cli.name) {
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

    println!("🔄 Uploading {} (version {}) to R2...", cli.name, version);

    let result = upload_compiled_binary_to_r2(
        &cli.name,
        &version,
        cli.file_path.as_deref(),
        &cli.target_dir,
    )
    .await;

    if result {
        println!("✅ Upload succeeded!");
    } else {
        eprintln!("❌ Upload failed.");
        std::process::exit(1);
    }
}