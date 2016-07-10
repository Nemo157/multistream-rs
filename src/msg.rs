use std::io;
use msgio::ReadVpm;
use varint::WriteVarInt;

pub trait ReadMultiStreamMessage {
    fn read_ms_msg(&mut self) -> io::Result<Vec<u8>>;
    fn try_read_ms_msg(&mut self) -> io::Result<Option<Vec<u8>>>;
}

pub trait WriteMultiStreamMessage {
    fn write_ms_msg(&mut self, msg: &[u8]) -> io::Result<()>;
}

impl<R> ReadMultiStreamMessage for R where R: io::Read {
    fn read_ms_msg(&mut self) -> io::Result<Vec<u8>> {
        let mut msg = try!(self.read_vpm());
        if msg.pop() != Some(b'\n') {
            return Err(io::Error::new(io::ErrorKind::Other, "message did not end with '\\n'"));
        }
        Ok(msg)
    }

    fn try_read_ms_msg(&mut self) -> io::Result<Option<Vec<u8>>> {
        if let Some(mut msg) = try!(self.try_read_vpm()) {
            if msg.pop() != Some(b'\n') {
                return Err(io::Error::new(io::ErrorKind::Other, "message did not end with '\\n'"));
            }
            Ok(Some(msg))
        } else {
            Ok(None)
        }
    }
}

impl<W> WriteMultiStreamMessage for W where W: io::Write {
    fn write_ms_msg(&mut self, msg: &[u8]) -> io::Result<()> {
        try!(self.write_usize_varint(msg.len() + 1));
        try!(self.write_all(msg));
        try!(self.write_all(b"\n"));
        Ok(())
    }
}
