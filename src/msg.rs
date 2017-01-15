use std::io::{ self, Cursor };

use varmint::{ ReadVarInt, WriteVarInt };
use tokio_core::io::{ Codec, EasyBuf };

pub struct MsgCodec;

impl Codec for MsgCodec {
    type In = EasyBuf;
    type Out = Vec<u8>;

    fn decode(&mut self, buf: &mut EasyBuf) -> io::Result<Option<Self::In>> {
        let (len, len_len) = {
            let mut cursor = Cursor::new(&buf);
            (cursor.try_read_usize_varint()?, cursor.position() as usize)
        };
        if let Some(len) = len {
            if len + len_len < buf.len() {
                buf.drain_to(len_len); // discard the length
                let mut msg = buf.drain_to(len);
                if msg.as_slice()[len - 1] == b'\n' {
                    Ok(Some(msg.drain_to(len - 1)))
                } else {
                    Err(io::Error::new(io::ErrorKind::Other, "message did not end with '\\n'"))
                }
            } else {
                Ok(None)
            }
        } else {
            Ok(None)
        }
    }

    fn encode(&mut self, msg: Self::Out, buf: &mut Vec<u8>) -> io::Result<()> {
        buf.write_usize_varint(msg.len() + 1)?;
        buf.extend_from_slice(&msg);
        buf.push(b'\n');
        Ok(())
    }
}
