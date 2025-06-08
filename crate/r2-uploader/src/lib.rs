use reqwest::{Client, StatusCode};
use std::env;

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
