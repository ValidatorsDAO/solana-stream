use anyhow::Context;
use std::env;

use crate::utils::fallback::{DEFAULT_CONFIG_PATH, DEFAULT_RPC_ENDPOINT};

#[derive(Clone)]
pub struct Settings {
    pub config_path: String,
    pub grpc_endpoint: String,
    pub x_token: Option<String>,
    pub rpc_endpoint: String,
}

impl Settings {
    pub fn from_env() -> anyhow::Result<Self> {
        let config_path =
            env::var("CONFIG_PATH").unwrap_or_else(|_| DEFAULT_CONFIG_PATH.to_string());
        let grpc_endpoint = env::var("GRPC_ENDPOINT").context("GRPC_ENDPOINT is missing")?;
        let x_token = env::var("X_TOKEN").ok();
        let rpc_endpoint =
            env::var("SOLANA_RPC_ENDPOINT").unwrap_or_else(|_| DEFAULT_RPC_ENDPOINT.to_string());

        Ok(Self {
            config_path,
            grpc_endpoint,
            x_token,
            rpc_endpoint,
        })
    }

    /// A warning when `X_TOKEN` would travel unencrypted: set, with an `http://` endpoint
    /// that is not this machine.
    pub fn plaintext_token_warning(&self) -> Option<String> {
        self.x_token.as_ref()?;
        let url = url::Url::parse(&self.grpc_endpoint).ok()?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        (url.scheme() == "http" && !local).then(|| {
            format!(
                "X_TOKEN is sent unencrypted to {}: use an https:// endpoint, or leave X_TOKEN \
                 unset if the endpoint authorizes by IP",
                solana_stream_sdk::redact::redact_url(&self.grpc_endpoint)
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    fn settings(grpc_endpoint: &str, x_token: Option<&str>) -> Settings {
        Settings {
            config_path: String::new(),
            grpc_endpoint: grpc_endpoint.to_string(),
            x_token: x_token.map(str::to_string),
            rpc_endpoint: String::new(),
        }
    }

    #[test]
    fn warns_only_when_a_token_goes_over_plain_http_to_another_host() {
        let warning = settings("http://grpc-ams1.erpc.global", Some("SECRET"))
            .plaintext_token_warning()
            .expect("a token over http:// must warn");
        assert!(warning.contains("http://grpc-ams1.erpc.global") && !warning.contains("SECRET"));
        assert_eq!(
            settings("https://grpc.example.com", Some("t")).plaintext_token_warning(),
            None
        );
        assert_eq!(
            settings("http://grpc-ams1.erpc.global", None).plaintext_token_warning(),
            None
        );
        assert_eq!(
            settings("http://127.0.0.1:10000", Some("t")).plaintext_token_warning(),
            None
        );
    }
}
