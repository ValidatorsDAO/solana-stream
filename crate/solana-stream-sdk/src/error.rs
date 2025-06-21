use thiserror::Error;

#[derive(Error, Debug)]
pub enum SolanaStreamError {
    #[error("Transport error: {0}")]
    Transport(#[from] tonic::transport::Error),

    #[error("gRPC status error: {0}")]
    Status(#[from] tonic::Status),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("Connection error: {0}")]
    Connection(String),

    #[error("Configuration error: {0}")]
    Configuration(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serde JSONC error: {0}")]
    SerdeJsonc(#[from] serde_jsonc::Error),

    #[error("Invalid URI: {0}")]
    InvalidUri(#[from] http::uri::InvalidUri),

    #[error("Builder error: {0}")]
    Builder(#[from] yellowstone_grpc_client::GeyserGrpcBuilderError),

    #[error("Send error: {0}")]
    SendError(#[from] futures::channel::mpsc::SendError),

    #[error("Client error: {0}")]
    Client(#[from] yellowstone_grpc_client::GeyserGrpcClientError),

    #[error("URL Parse error: {0}")]
    UrlParse(#[from] url::ParseError),
}
