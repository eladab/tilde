//! How requests reach the engine.

use std::io::Write;
use std::os::unix::net::UnixStream;

pub trait Transport {
    /// Sends one request line. Returns any lines the engine produced
    /// synchronously; the socket transport delivers its lines through the
    /// event loop instead and returns nothing.
    fn send(&mut self, line: &str) -> Vec<String>;
}

pub struct SocketTransport {
    stream: UnixStream,
}

impl SocketTransport {
    pub fn new(stream: UnixStream) -> Self {
        Self { stream }
    }
}

impl Transport for SocketTransport {
    fn send(&mut self, line: &str) -> Vec<String> {
        // A failed write means the app is gone; the reader thread reports that.
        let _ = self.stream.write_all(line.as_bytes());
        let _ = self.stream.write_all(b"\n");
        Vec::new()
    }
}
