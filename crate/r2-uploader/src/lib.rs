use std::env;

use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

/// Request payload for Cloudflare cache purge API
#[derive(Serialize, Debug)]
struct PurgeRequest {
    files: Vec<String>,
}

/// Response from Cloudflare cache purge API
#[derive(Deserialize, Debug)]
struct PurgeResponse {
    success: bool,
    errors: Vec<serde_json::Value>,
    #[allow(dead_code)]
    messages: Vec<serde_json::Value>,
    #[allow(dead_code)]
    result: Option<serde_json::Value>,
}

/// Uploads binary data to Cloudflare R2 storage
///
/// # Arguments
///
/// * `object_key` - Full object path (e.g., "erpc/log/abc123/file.bin")
/// * `binary_data` - Binary content to upload (e.g., a buffer)
///
/// # Returns
///
/// `true` on success, `false` on failure
pub async fn upload_binary_to_r2(object_key: &str, binary_data: Vec<u8>) -> bool {
    let bucket_name = match env::var("CLOUDFLARE_R2_BUCKET") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("Missing CLOUDFLARE_R2_BUCKET environment variable");
            return false;
        }
    };
    // Read required environment variables
    let account_id = match env::var("CLOUDFLARE_ACCOUNT_ID") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("Missing CLOUDFLARE_ACCOUNT_ID");
            return false;
        }
    };

    let url = format!(
        "https://api.cloudflare.com/client/v4/accounts/{}/r2/buckets/{}/objects/{}",
        account_id, bucket_name, object_key
    );

    // Prepare auth headers
    let mut headers = reqwest::header::HeaderMap::new();

    if let Ok(token) = env::var("CLOUDFLARE_API_TOKEN") {
        println!("Using API Token authentication");
        headers.insert(
            reqwest::header::AUTHORIZATION,
            format!("Bearer {}", token).parse().unwrap(),
        );
    } else {
        let email = match env::var("CLOUDFLARE_EMAIL") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("Missing CLOUDFLARE_EMAIL");
                return false;
            }
        };
        let api_key_auth = match env::var("CLOUDFLARE_API_KEY") {
            Ok(val) => val,
            Err(_) => {
                eprintln!("Missing CLOUDFLARE_API_KEY");
                return false;
            }
        };
        println!("Using API Key authentication");
        headers.insert("X-Auth-Email", email.parse().unwrap());
        headers.insert("X-Auth-Key", api_key_auth.parse().unwrap());
    }

    headers.insert("Content-Type", "application/octet-stream".parse().unwrap());

    // Send PUT request
    let client = Client::new();
    let response = match client
        .put(&url)
        .headers(headers)
        .body(binary_data)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            eprintln!("Request failed: {}", err);
            return false;
        }
    };

    if response.status() == StatusCode::OK {
        println!("✅ Successfully uploaded to R2: {}", object_key);
        true
    } else {
        let status = response.status();
        let error_text = response
            .text()
            .await
            .unwrap_or_else(|_| String::from("No error text"));
        eprintln!("❌ Upload failed: {} - {}", status, error_text);
        false
    }
}

/// Purges Cloudflare cache for specified files/URLs
///
/// # Arguments
///
/// * `files` - Vector of URLs to purge from cache
///
/// # Returns
///
/// `true` on success, `false` on failure
pub async fn purge_cloudflare_cache(files: Vec<String>) -> bool {
    // Read required environment variables
    let zone_id = match env::var("CLOUDFLARE_ZONE_ID") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("❌ Missing CLOUDFLARE_ZONE_ID environment variable");
            return false;
        }
    };

    let email = match env::var("CLOUDFLARE_PURGE_EMAIL") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("❌ Missing CLOUDFLARE_PURGE_EMAIL environment variable");
            return false;
        }
    };

    let api_token = match env::var("CLOUDFLARE_PURGE_API_TOKEN") {
        Ok(val) => val,
        Err(_) => {
            eprintln!("❌ Missing CLOUDFLARE_PURGE_API_TOKEN environment variable");
            return false;
        }
    };

    // Construct Cloudflare cache purge API URL
    let url = format!(
        "https://api.cloudflare.com/client/v4/zones/{}/purge_cache",
        zone_id
    );

    // Prepare request payload
    let purge_request = PurgeRequest { files };

    // Prepare headers
    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("X-Auth-Email", email.parse().unwrap());
    headers.insert("X-Auth-Key", api_token.parse().unwrap());
    headers.insert("Content-Type", "application/json".parse().unwrap());

    // Send POST request
    let client = Client::new();
    println!(
        "🧹 Purging cache for {} files...",
        purge_request.files.len()
    );

    let response = match client
        .post(&url)
        .headers(headers)
        .json(&purge_request)
        .send()
        .await
    {
        Ok(resp) => resp,
        Err(err) => {
            eprintln!("❌ Request failed: {}", err);
            return false;
        }
    };

    let status = response.status();
    match response.json::<PurgeResponse>().await {
        Ok(purge_response) => {
            if purge_response.success {
                println!(
                    "✅ Successfully purged cache for {} files",
                    purge_request.files.len()
                );
                for file in &purge_request.files {
                    println!("   📄 {}", file);
                }
                true
            } else {
                eprintln!("❌ Cache purge failed with errors:");
                for error in &purge_response.errors {
                    eprintln!("   • {}", error);
                }
                false
            }
        }
        Err(err) => {
            eprintln!("❌ Failed to parse response: {} (status: {})", err, status);
            false
        }
    }
}
