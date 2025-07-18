use crate::cli::commands::{DaemonRequest, DaemonResponse};
use crate::error::{Result, RunceptError};
use crate::logging::{log_debug, log_error, log_info};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// Handles Unix socket connection processing
pub struct ConnectionHandler;

impl ConnectionHandler {
    /// Handle a single client connection
    pub async fn handle_connection<F, Fut>(stream: UnixStream, request_processor: F) -> Result<()>
    where
        F: FnOnce(DaemonRequest) -> Fut,
        Fut: std::future::Future<Output = Result<DaemonResponse>>,
    {
        log_debug("daemon", "New client connection received", None);

        let mut reader = BufReader::new(stream);
        let mut request_line = String::new();

        // Read request line
        let bytes_read = reader.read_line(&mut request_line).await.map_err(|e| {
            let err = RunceptError::ConnectionError(format!("Failed to read request: {e}"));
            log_error("daemon", &format!("Connection error: {err}"), None);
            err
        })?;

        if bytes_read == 0 {
            log_debug("daemon", "Client disconnected (0 bytes read)", None);
            return Ok(()); // Client disconnected
        }

        log_debug(
            "daemon",
            &format!(
                "Received request ({} bytes): {}",
                bytes_read,
                request_line.trim()
            ),
            None,
        );

        // Parse request
        let request: DaemonRequest = serde_json::from_str(request_line.trim()).map_err(|e| {
            let err = RunceptError::SerializationError(format!("Invalid request: {e}"));
            log_error(
                "daemon",
                &format!("Failed to parse request '{}': {err}", request_line.trim()),
                None,
            );
            err
        })?;

        log_info("daemon", &format!("Processing request: {request:?}"), None);

        // Process request
        let response = match request_processor(request).await {
            Ok(resp) => {
                log_debug(
                    "daemon",
                    &format!("Request processed successfully: {resp:?}"),
                    None,
                );
                resp
            }
            Err(e) => {
                log_error("daemon", &format!("Request processing failed: {e}"), None);
                DaemonResponse::Error {
                    error: format!("Internal server error: {e}"),
                }
            }
        };

        // Send response
        let response_json = serde_json::to_string(&response).map_err(|e| {
            let err =
                RunceptError::SerializationError(format!("Failed to serialize response: {e}"));
            log_error(
                "daemon",
                &format!("Serialization error for response {response:?}: {err}"),
                None,
            );
            err
        })?;

        log_debug(
            "daemon",
            &format!("Sending response: {response_json}"),
            None,
        );

        let mut stream = reader.into_inner();
        stream
            .write_all(format!("{response_json}\n").as_bytes())
            .await
            .map_err(|e| {
                let err = RunceptError::ConnectionError(format!("Failed to send response: {e}"));
                log_error("daemon", &format!("Failed to send response: {err}"), None);
                err
            })?;

        log_debug("daemon", "Response sent successfully", None);
        Ok(())
    }
}
