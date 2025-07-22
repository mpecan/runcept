use crate::cli::commands::{DaemonRequest, DaemonResponse};
use crate::error::{Result, RunceptError};
use crate::ipc::PlatformStream;
use crate::logging::{log_debug, log_error, log_info};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Handles Unix socket connection processing
pub struct ConnectionHandler;

impl ConnectionHandler {
    /// Handle a single client connection
    pub async fn handle_connection<F, Fut>(
        stream: PlatformStream,
        request_processor: F,
    ) -> Result<()>
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

    /// Helper method to parse a request from JSON string
    #[cfg(test)]
    pub fn parse_request(json: &str) -> Result<DaemonRequest> {
        serde_json::from_str(json.trim())
            .map_err(|e| RunceptError::SerializationError(format!("Invalid request: {e}")))
    }

    /// Helper method to serialize a response to JSON string
    #[cfg(test)]
    pub fn serialize_response(response: &DaemonResponse) -> Result<String> {
        let json = serde_json::to_string(response).map_err(|e| {
            RunceptError::SerializationError(format!("Failed to serialize response: {e}"))
        })?;
        Ok(format!("{json}\n"))
    }

    /// Helper method to write response to stream
    #[cfg(test)]
    pub async fn write_response_to_stream(
        mut stream: PlatformStream,
        response: &DaemonResponse,
    ) -> Result<()> {
        let response_json = Self::serialize_response(response)?;
        stream
            .write_all(response_json.as_bytes())
            .await
            .map_err(|e| RunceptError::ConnectionError(format!("Failed to send response: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::commands::DaemonResponse;

    #[test]
    fn test_parse_request_invalid_json() {
        let invalid_json = "{ invalid json }";
        let result = ConnectionHandler::parse_request(invalid_json);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            RunceptError::SerializationError(_)
        ));
    }

    #[test]
    fn test_serialize_response_success() {
        let response = DaemonResponse::ProcessList(vec![]);
        let result = ConnectionHandler::serialize_response(&response);
        assert!(result.is_ok());

        let json = result.unwrap();
        assert!(json.contains("ProcessList"));
        assert!(json.ends_with('\n'));
    }

    #[test]
    fn test_serialize_response_error() {
        let response = DaemonResponse::Error {
            error: "Test error".to_string(),
        };
        let result = ConnectionHandler::serialize_response(&response);
        assert!(result.is_ok());

        let json = result.unwrap();
        assert!(json.contains("Error"));
        assert!(json.contains("Test error"));
        assert!(json.ends_with('\n'));
    }
}
