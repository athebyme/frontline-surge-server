//! Length-prefixed UTF-8 JSON shared by the native dedicated server and client.
use serde_json::Value;
use std::collections::VecDeque;
use std::io::{self, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::sync::Arc;

struct Outgoing {
    bytes: Arc<[u8]>,
    replaceable: bool,
}

pub const PROTOCOL: u64 = 5;
pub(crate) const SERVER_PACKET: usize = 4 * 1024 * 1024;
#[cfg(feature = "server")]
pub(crate) const INPUT_PACKET: usize = 4096;
pub(crate) struct Wire {
    stream: TcpStream,
    eof: bool,
    received: Vec<u8>,
    queue: VecDeque<Outgoing>,
    offset: usize,
    queued: usize,
}
impl Wire {
    pub(crate) fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            eof: false,
            received: Vec::new(),
            queue: VecDeque::new(),
            offset: 0,
            queued: 0,
        })
    }
    pub(crate) fn frames(&mut self, max: usize) -> io::Result<Vec<Value>> {
        if self.eof {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        let mut buf = [0u8; 8192];
        loop {
            match self.stream.read(&mut buf) {
                Ok(0) => {
                    self.eof = true;
                    break;
                }
                Ok(n) => {
                    self.received.extend_from_slice(&buf[..n]);
                    if self.received.len() > max * 4 + 16384 {
                        return Err(ErrorKind::InvalidData.into());
                    }
                    if n < buf.len() {
                        break;
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        let mut cursor = 0;
        let mut frames = Vec::new();
        while self.received.len() - cursor >= 4 {
            let n =
                u32::from_be_bytes(self.received[cursor..cursor + 4].try_into().unwrap()) as usize;
            if n == 0 || n > max {
                return Err(ErrorKind::InvalidData.into());
            }
            if frames.len() >= 160 {
                break;
            }
            if self.received.len() - cursor - 4 < n {
                break;
            }
            let value = serde_json::from_slice(&self.received[cursor + 4..cursor + 4 + n])
                .map_err(|_| io::Error::from(ErrorKind::InvalidData))?;
            frames.push(value);
            cursor += 4 + n;
        }
        if cursor > 0 {
            self.received.drain(..cursor);
        }
        if self.eof && frames.is_empty() {
            return Err(ErrorKind::UnexpectedEof.into());
        }
        Ok(frames)
    }
    pub(crate) fn packet(value: &Value) -> Arc<[u8]> {
        let body = serde_json::to_vec(value).expect("serializable protocol message");
        let mut frame = Vec::with_capacity(body.len() + 4);
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&body);
        Arc::from(frame)
    }
    pub(crate) fn enqueue(&mut self, bytes: Arc<[u8]>) -> io::Result<()> {
        self.queue_frame(bytes, false)
    }
    #[cfg(feature = "server")]
    pub(crate) fn snapshot(&mut self, bytes: Arc<[u8]>) -> io::Result<()> {
        self.queue_frame(bytes, true)
    }
    fn queue_frame(&mut self, bytes: Arc<[u8]>, replaceable: bool) -> io::Result<()> {
        // A TCP frame already partly written must finish. We may discard only
        // an untouched transient tail. Baselines, map changes and inputs persist.
        if self.queue.back().is_some_and(|frame| frame.replaceable)
            && (self.queue.len() > 1 || self.offset == 0)
        {
            let stale = self.queue.pop_back().unwrap();
            self.queued -= stale.bytes.len();
        }
        if bytes.len() > SERVER_PACKET + 4 || self.queued + bytes.len() > SERVER_PACKET * 2 + 8 {
            return Err(ErrorKind::TimedOut.into());
        }
        self.queued += bytes.len();
        self.queue.push_back(Outgoing { bytes, replaceable });
        self.flush()
    }
    pub(crate) fn flush(&mut self) -> io::Result<()> {
        while let Some(frame) = self.queue.front() {
            match self.stream.write(&frame.bytes[self.offset..]) {
                Ok(0) => return Err(ErrorKind::WriteZero.into()),
                Ok(n) => {
                    self.offset += n;
                    self.queued -= n;
                    if self.offset == frame.bytes.len() {
                        self.offset = 0;
                        self.queue.pop_front();
                    }
                }
                Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}
