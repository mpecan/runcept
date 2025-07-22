use crate::error::{Result, RunceptError};
use interprocess::local_socket::{
    GenericFilePath, ListenerOptions, Name, ToFsName,
    tokio::{Listener as LocalListener, Stream as LocalStream, prelude::*},
};

#[cfg(windows)]
use interprocess::local_socket::{GenericNamespaced, ToNsName};
use std::path::{Path, PathBuf};
use tokio::io::{AsyncRead, AsyncWrite};

/// Cross-platform IPC path abstraction using interprocess library
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IpcPath {
    inner: PathBuf,
}

impl IpcPath {
    /// Create a new IPC path
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { inner: path.into() }
    }

    /// Get the platform-specific path string
    pub fn as_str(&self) -> &str {
        self.inner.to_str().unwrap_or("")
    }

    /// Get the underlying PathBuf
    pub fn as_path(&self) -> &Path {
        &self.inner
    }

    /// Check if the path exists
    pub fn exists(&self) -> bool {
        self.inner.exists()
    }

    /// Get a display string for the path
    pub fn display(&self) -> String {
        self.inner.display().to_string()
    }

    /// Convert to PathBuf
    pub fn to_path_buf(&self) -> PathBuf {
        self.inner.clone()
    }

    /// Create default IPC path for the current user
    pub fn default_path() -> Result<Self> {
        let home = std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .map_err(|_| {
                RunceptError::SystemError("Cannot determine home directory".to_string())
            })?;

        let runcept_dir = PathBuf::from(home).join(".runcept");

        #[cfg(unix)]
        let socket_path = runcept_dir.join("daemon.sock");

        #[cfg(windows)]
        let socket_path = runcept_dir.join("daemon");

        Ok(Self::new(socket_path))
    }

    /// Convert to interprocess Name for creating sockets
    pub fn to_socket_name(&self) -> Result<Name> {
        #[cfg(unix)]
        {
            // Use filesystem path on Unix
            self.inner
                .clone()
                .to_fs_name::<GenericFilePath>()
                .map_err(|e| {
                    RunceptError::PlatformError(format!("Failed to create Unix socket name: {e}"))
                })
        }
        #[cfg(windows)]
        {
            // Use namespace name on Windows
            let name = self
                .inner
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("runcept-daemon");
            name.to_ns_name::<GenericNamespaced>().map_err(|e| {
                RunceptError::PlatformError(format!("Failed to create Windows pipe name: {e}"))
            })
        }
    }
}

impl From<PathBuf> for IpcPath {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl From<&str> for IpcPath {
    fn from(path: &str) -> Self {
        Self::new(PathBuf::from(path))
    }
}

impl AsRef<Path> for IpcPath {
    fn as_ref(&self) -> &Path {
        &self.inner
    }
}

impl AsRef<std::ffi::OsStr> for IpcPath {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.inner.as_ref()
    }
}

impl PartialEq<PathBuf> for IpcPath {
    fn eq(&self, other: &PathBuf) -> bool {
        self.inner == *other
    }
}

impl PartialEq<IpcPath> for PathBuf {
    fn eq(&self, other: &IpcPath) -> bool {
        *self == other.inner
    }
}

/// Cross-platform IPC stream using interprocess library
pub struct IpcStream {
    inner: LocalStream,
}

impl IpcStream {
    /// Split the stream into read and write halves
    pub fn into_split(
        self,
    ) -> (
        Box<dyn AsyncRead + Send + Unpin>,
        Box<dyn AsyncWrite + Send + Unpin>,
    ) {
        let (read, write) = self.inner.split();
        (Box::new(read), Box::new(write))
    }
}

impl AsyncRead for IpcStream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl AsyncWrite for IpcStream {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        std::pin::Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::pin::Pin::new(&mut self.inner).poll_shutdown(cx)
    }
}

/// Cross-platform IPC listener using interprocess library
pub struct IpcListener {
    inner: LocalListener,
}

impl IpcListener {
    /// Bind to the given IPC path
    pub async fn bind(path: &IpcPath) -> Result<Self> {
        let name = path.to_socket_name()?;

        // Remove existing socket file if it exists (Unix only)
        #[cfg(unix)]
        if path.exists() {
            std::fs::remove_file(path.as_path()).map_err(RunceptError::IoError)?;
        }

        let listener = ListenerOptions::new()
            .name(name)
            .create_tokio()
            .map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to bind IPC listener: {e}"))
            })?;

        Ok(Self { inner: listener })
    }

    /// Accept a new connection
    pub async fn accept(&mut self) -> Result<IpcStream> {
        let stream = self.inner.accept().await.map_err(|e| {
            RunceptError::ConnectionError(format!("Failed to accept IPC connection: {e}"))
        })?;

        Ok(IpcStream { inner: stream })
    }

    /// Get the local address/path of the listener
    pub fn local_addr(&self) -> String {
        // interprocess doesn't provide a direct way to get the address,
        // so we'll return a generic identifier
        "local_socket".to_string()
    }
}

/// Connect to an IPC endpoint
pub async fn connect(path: &IpcPath) -> Result<IpcStream> {
    let name = path.to_socket_name()?;

    let stream = LocalStream::connect(name).await.map_err(|e| {
        RunceptError::ConnectionError(format!("Failed to connect to IPC endpoint: {e}"))
    })?;

    Ok(IpcStream { inner: stream })
}

/// Create a platform-specific IPC listener
pub async fn create_listener(path: &IpcPath) -> Result<IpcListener> {
    IpcListener::bind(path).await
}

// Type aliases for compatibility with existing code
pub type PlatformStream = IpcStream;
pub type PlatformListener = IpcListener;
pub type PlatformClient = (); // Not needed with the simplified API

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipc_path_creation() {
        let path = IpcPath::new("/tmp/test.sock");
        assert_eq!(path.as_str(), "/tmp/test.sock");
    }

    #[test]
    fn test_default_ipc_path() {
        // This test might fail in CI without HOME set, so we'll make it conditional
        if std::env::var("HOME").is_ok() || std::env::var("USERPROFILE").is_ok() {
            let path = IpcPath::default_path().unwrap();
            assert!(path.as_str().contains(".runcept"));
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_unix_socket_path() {
        let path = IpcPath::new("/tmp/test.sock");
        let name = path.to_socket_name();
        // We can't easily test the exact name format, but we can ensure it doesn't error
        assert!(name.is_ok());
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_pipe_path() {
        let path = IpcPath::new("test");
        let name = path.to_socket_name();
        // We can't easily test the exact name format, but we can ensure it doesn't error
        assert!(name.is_ok());
    }

    #[tokio::test]
    async fn test_ipc_listener_creation() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let socket_path = temp_dir.path().join("test.sock");
        let ipc_path = IpcPath::new(socket_path);

        // This should succeed in creating a listener
        let listener = IpcListener::bind(&ipc_path).await;
        assert!(listener.is_ok());
    }
}
