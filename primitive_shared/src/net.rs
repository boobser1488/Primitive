use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// FIX: bincode is not self-delimiting over a stream socket — two messages
/// written back to back will arrive as one contiguous byte stream with no
/// boundary. Without explicit framing, the reader can desync (read a
/// truncated or merged message) the moment two writes land in the same
/// TCP segment. We prefix every message with its length.
const MAX_MESSAGE_BYTES: u32 = 16 * 1024 * 1024; // sanity cap, avoid OOM from a bad length prefix

#[derive(Debug)]
pub enum NetError {
    Io(std::io::Error),
    Codec(bincode::Error),
    MessageTooLarge(u32),
}

impl From<std::io::Error> for NetError {
    fn from(e: std::io::Error) -> Self {
        NetError::Io(e)
    }
}

impl From<bincode::Error> for NetError {
    fn from(e: bincode::Error) -> Self {
        NetError::Codec(e)
    }
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetError::Io(e) => write!(f, "io error: {e}"),
            NetError::Codec(e) => write!(f, "codec error: {e}"),
            NetError::MessageTooLarge(n) => write!(f, "message too large: {n} bytes"),
        }
    }
}

impl std::error::Error for NetError {}

/// Serialises a message into its full on-the-wire form: the four-byte
/// length prefix followed by the bincode payload.
///
/// This is *the* definition of the framing. `write_message` goes through
/// it, and so does the server's broadcast path, which serialises a
/// message once and hands the same bytes to every recipient -- the two
/// paths cannot drift, because there is only one.
pub fn frame_message<T>(msg: &T) -> Result<Vec<u8>, NetError>
where
    T: Serialize,
{
    let payload = bincode::serialize(msg)?;
    let len = payload.len() as u32;
    let mut frame = Vec::with_capacity(4 + payload.len());
    frame.extend_from_slice(&len.to_be_bytes());
    frame.extend_from_slice(&payload);
    Ok(frame)
}

/// Writes bytes that `frame_message` already shaped. The prefix is part
/// of the frame, so this is a plain write -- nothing here to disagree
/// with what `write_message` would have produced.
pub async fn write_frame<W>(writer: &mut W, frame: &[u8]) -> Result<(), NetError>
where
    W: AsyncWriteExt + Unpin,
{
    writer.write_all(frame).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn write_message<W, T>(writer: &mut W, msg: &T) -> Result<(), NetError>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let frame = frame_message(msg)?;
    write_frame(writer, &frame).await
}

/// Reads one framed message.
pub async fn read_message<R, T>(reader: &mut R) -> Result<T, NetError>
where
    R: AsyncReadExt + Unpin,
    T: DeserializeOwned,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    if len > MAX_MESSAGE_BYTES {
        return Err(NetError::MessageTooLarge(len));
    }
    let mut payload = vec![0u8; len as usize];
    reader.read_exact(&mut payload).await?;
    let msg = bincode::deserialize(&payload)?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::ServerMessage;

    /// The pre-serialised broadcast path and the per-message path must
    /// put identical bytes on the wire. This is the property the whole
    /// arrangement rests on: a client cannot tell which path a message
    /// took, so the two producing different frames would be a silent
    /// protocol fork.
    #[tokio::test]
    async fn a_prebuilt_frame_matches_what_write_message_sends() {
        let msg = ServerMessage::TimeSync {
            tick: 12345,
            time_of_day: 0.25,
        };
        let frame = frame_message(&msg).expect("framing failed");

        let mut written: Vec<u8> = Vec::new();
        write_message(&mut written, &msg).await.expect("write failed");
        assert_eq!(frame, written, "the two send paths framed differently");

        let mut raw: Vec<u8> = Vec::new();
        write_frame(&mut raw, &frame).await.expect("write failed");
        assert_eq!(raw, written, "writing a prebuilt frame altered it");

        // ...and the frame is a real message, not merely self-consistent.
        let mut cursor = std::io::Cursor::new(written);
        let back: ServerMessage = read_message(&mut cursor).await.expect("read failed");
        assert!(matches!(back, ServerMessage::TimeSync { tick: 12345, .. }));
    }
}
