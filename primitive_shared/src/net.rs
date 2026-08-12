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

pub async fn write_message<W, T>(writer: &mut W, msg: &T) -> Result<(), NetError>
where
    W: AsyncWriteExt + Unpin,
    T: Serialize,
{
    let payload = bincode::serialize(msg)?;
    let len = payload.len() as u32;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

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
