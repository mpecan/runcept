use crate::error::{Result, RunceptError};
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};

/// Cross-platform IPC stream trait
pub trait IpcStream: AsyncRead + AsyncWrite + Send + Unpin {
    /// Split the stream into read and write halves
    fn into_split(
        self,
    ) -> (
        Box<dyn AsyncRead + Send + Unpin>,
        Box<dyn AsyncWrite + Send + Unpin>,
    );
}

/// Cross-platform IPC listener trait
pub trait IpcListener: Send {
    type Stream: IpcStream;
    
    /// Accept a new connection
    #[allow(async_fn_in_trait)]
    async fn accept(&mut self) -> Result<Self::Stream>;
    /// Get the local address/path of the listener
    fn local_addr(&self) -> String;
}

/// Cross-platform IPC client trait
pub trait IpcClient: Send {
    type Stream: IpcStream;
    
    /// Connect to the IPC endpoint
    #[allow(async_fn_in_trait)]
    async fn connect(path: &IpcPath) -> Result<Self::Stream>;}

/// Cross-platform IPC path abstraction
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
    pub fn as_path(&self) -> &std::path::Path {
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

    /// Convert to platform-specific socket path
    pub fn to_socket_path(&self) -> String {
        #[cfg(unix)]
        {
            self.inner.to_string_lossy().to_string()
        }
        #[cfg(windows)]
        {
            // Convert to Windows named pipe format
            format!(
                r"\\.\pipe\{}",
                self.inner
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("runcept-daemon")
            )
        }
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

impl AsRef<std::path::Path> for IpcPath {
    fn as_ref(&self) -> &std::path::Path {
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

// Platform-specific implementations

#[cfg(unix)]
mod unix_impl {
    use super::*;

    use tokio::net::{UnixListener, UnixStream};

    /// Unix domain socket stream wrapper
    pub struct UnixIpcStream(UnixStream);

    impl IpcStream for UnixIpcStream {
        fn into_split(
            self,
        ) -> (
            Box<dyn AsyncRead + Send + Unpin>,
            Box<dyn AsyncWrite + Send + Unpin>,
        ) {
            let (read, write) = self.0.into_split();
            (Box::new(read), Box::new(write))
        }
    }

    impl AsyncRead for UnixIpcStream {
        fn poll_read(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.0).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for UnixIpcStream {
        fn poll_write(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &[u8],
        ) -> std::task::Poll<std::io::Result<usize>> {
            std::pin::Pin::new(&mut self.0).poll_write(cx, buf)
        }

        fn poll_flush(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.0).poll_flush(cx)
        }

        fn poll_shutdown(
            mut self: std::pin::Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<std::io::Result<()>> {
            std::pin::Pin::new(&mut self.0).poll_shutdown(cx)
        }
    }

    /// Unix domain socket listener wrapper
    pub struct UnixIpcListener(UnixListener);

    impl IpcListener for UnixIpcListener {
        type Stream = UnixIpcStream;

        async fn accept(&mut self) -> Result<Self::Stream> {
            let (stream, _) = self.0.accept().await.map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to accept connection: {e}"))
            })?;
            Ok(UnixIpcStream(stream))
        }

        fn local_addr(&self) -> String {
            self.0
                .local_addr()
                .map(|addr| {
                    addr.as_pathname()
                        .unwrap_or_else(|| std::path::Path::new("unknown"))
                        .to_string_lossy()
                        .to_string()
                })
                .unwrap_or_else(|_| "unknown".to_string())
        }
    }

    impl UnixIpcListener {
        pub async fn bind(path: &IpcPath) -> Result<Self> {
            // Remove existing socket file if it exists
            let socket_path = path.as_path();
            if socket_path.exists() {
                std::fs::remove_file(socket_path).map_err(RunceptError::IoError)?;
            }

            let listener = UnixListener::bind(socket_path).map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to bind Unix socket: {e}"))
            })?;

            Ok(Self(listener))
        }
    }

    /// Unix domain socket client wrapper
    pub struct UnixIpcClient;

    impl IpcClient for UnixIpcClient {
        type Stream = UnixIpcStream;

        async fn connect(path: &IpcPath) -> Result<Self::Stream> {
            let stream = UnixStream::connect(path.as_path()).await.map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to connect to Unix socket: {e}"))
            })?;
            Ok(UnixIpcStream(stream))
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, AsyncWrite, ReadHalf, WriteHalf};
    use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeServer};

    /// Windows named pipe stream wrapper that can be either server or client
    pub enum WindowsIpcStream {
        Server(NamedPipeServer),
        Client(tokio::net::windows::named_pipe::NamedPipeClient),
    }

    impl IpcStream for WindowsIpcStream {
        fn into_split(
            self,
        ) -> (
            Box<dyn AsyncRead + Send + Unpin>,
            Box<dyn AsyncWrite + Send + Unpin>,
        ) {
            match self {
                WindowsIpcStream::Server(pipe) => {
                    let (read, write) = tokio::io::split(pipe);
                    (Box::new(read), Box::new(write))
                }
                WindowsIpcStream::Client(pipe) => {
                    let (read, write) = tokio::io::split(pipe);
                    (Box::new(read), Box::new(write))
                }
            }
        }
    }

    impl AsyncRead for WindowsIpcStream {
        fn poll_read(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &mut tokio::io::ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            match &mut *self {
                WindowsIpcStream::Server(pipe) => Pin::new(pipe).poll_read(cx, buf),
                WindowsIpcStream::Client(pipe) => Pin::new(pipe).poll_read(cx, buf),
            }
        }
    }

    impl AsyncWrite for WindowsIpcStream {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<std::io::Result<usize>> {
            match &mut *self {
                WindowsIpcStream::Server(pipe) => Pin::new(pipe).poll_write(cx, buf),
                WindowsIpcStream::Client(pipe) => Pin::new(pipe).poll_write(cx, buf),
            }
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
            match &mut *self {
                WindowsIpcStream::Server(pipe) => Pin::new(pipe).poll_flush(cx),
                WindowsIpcStream::Client(pipe) => Pin::new(pipe).poll_flush(cx),
            }
        }

        fn poll_shutdown(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
        ) -> Poll<std::io::Result<()>> {
            match &mut *self {
                WindowsIpcStream::Server(pipe) => Pin::new(pipe).poll_shutdown(cx),
                WindowsIpcStream::Client(pipe) => Pin::new(pipe).poll_shutdown(cx),
            }
        }
    }

    /// Windows named pipe listener wrapper
    pub struct WindowsIpcListener {
        pipe_name: String,
    }

    impl IpcListener for WindowsIpcListener {
        type Stream = WindowsIpcStream;

        async fn accept(&mut self) -> Result<Self::Stream> {
            let server = NamedPipeServer::create(&self.pipe_name).map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to create named pipe: {e}"))
            })?;

            server.connect().await.map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to accept connection: {e}"))
            })?;

            Ok(WindowsIpcStream::Server(server))
        }

        fn local_addr(&self) -> String {
            self.pipe_name.clone()
        }
    }

    impl WindowsIpcListener {
        pub async fn bind(path: &IpcPath) -> Result<Self> {
            let pipe_name = path.to_socket_path();

            // Test that we can create a pipe with this name
            let _test_server = NamedPipeServer::create(&pipe_name).map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to create named pipe: {e}"))
            })?;

            Ok(Self { pipe_name })
        }
    }

    /// Windows named pipe client wrapper
    pub struct WindowsIpcClient;

    impl IpcClient for WindowsIpcClient {
        type Stream = WindowsIpcStream;

        async fn connect(path: &IpcPath) -> Result<Self::Stream> {
            let pipe_name = path.to_socket_path();

            let client = ClientOptions::new().open(&pipe_name).map_err(|e| {
                RunceptError::ConnectionError(format!("Failed to connect to named pipe: {e}"))
            })?;

            Ok(WindowsIpcStream::Client(client))
        }
    }
}

// Re-export platform-specific types
#[cfg(unix)]
pub use unix_impl::{
    UnixIpcClient as PlatformClient, UnixIpcListener as PlatformListener,
    UnixIpcStream as PlatformStream,
};

#[cfg(windows)]
pub use windows_impl::{
    WindowsIpcClient as PlatformClient, WindowsIpcListener as PlatformListener,
    WindowsIpcStream as PlatformStream,
};

/// Create a platform-specific IPC listener
pub async fn create_listener(path: &IpcPath) -> Result<PlatformListener> {
    PlatformListener::bind(path).await
}

/// Connect to a platform-specific IPC endpoint
pub async fn connect(path: &IpcPath) -> Result<PlatformStream> {
    PlatformClient::connect(path).await
}

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
        assert_eq!(path.to_socket_path(), "/tmp/test.sock");
    }

    #[cfg(windows)]
    #[test]
    fn test_windows_pipe_path() {
        let path = IpcPath::new("test");
        assert_eq!(path.to_socket_path(), r"\\.\pipe\test");
    }
}
