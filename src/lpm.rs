use std::io;
use varint::{ ReadVarInt, WriteVarInt };

trait ReadHelper {
    fn read_lpm_data(&mut self, len: usize) -> io::Result<Vec<u8>>;
}

pub trait ReadLpm {
    fn read_lpm(&mut self) -> io::Result<Vec<u8>>;
    fn try_read_lpm(&mut self) -> io::Result<Option<Vec<u8>>>;
}

pub trait WriteLpm {
    fn write_lpm(&mut self, msg: &[u8]) -> io::Result<()>;
}

impl<R> ReadHelper for R where R: io::Read {
    fn read_lpm_data(&mut self, len: usize) -> io::Result<Vec<u8>> {
        println!("reading {} byte lpm", len);
        let mut msg = vec![0; len];
        try!(self.read_exact(&mut msg));
        println!("read lpm {:?}", msg);
        if msg.pop() != Some(b'\n') {
            return Err(io::Error::new(io::ErrorKind::Other, "message did not end with '\\n'"));
        }
        Ok(msg)
    }
}

impl<R> ReadLpm for R where R: io::Read {
    fn read_lpm(&mut self) -> io::Result<Vec<u8>> {
        let len = try!(self.read_usize_varint());
        self.read_lpm_data(len)
    }

    fn try_read_lpm(&mut self) -> io::Result<Option<Vec<u8>>> {
        if let Some(len) = try!(self.try_read_usize_varint()) {
            Ok(Some(try!(self.read_lpm_data(len))))
        } else {
            println!("no lpm");
            Ok(None)
        }
    }
}

impl<W> WriteLpm for W where W: io::Write {
    fn write_lpm(&mut self, msg: &[u8]) -> io::Result<()> {
        try!(self.write_usize_varint(msg.len() + 1));
        try!(self.write_all(msg));
        try!(self.write_all(b"\n"));
        Ok(())
    }
}
