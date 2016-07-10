use std::borrow::Cow;
use std::io;

use msg::{ ReadMultiStreamMessage, WriteMultiStreamMessage };

const PROTOCOL_ID: &'static [u8] = b"/multistream/1.0.0";

#[derive(Debug)]
pub struct MultiStream<S> where S: io::Read + io::Write {
    protocol: Cow<'static, [u8]>,
    stream: S,
}

impl<S> MultiStream<S> where S: io::Read + io::Write {
    pub fn negotiate(mut stream: S, protocol: Cow<'static, [u8]>) -> io::Result<MultiStream<S>> {
        // Expect the other end to send a multistream header
        let protocol_id = try!(stream.read_ms_msg());
        if protocol_id != PROTOCOL_ID {
            return Err(io::Error::new(io::ErrorKind::Other, format!("Server requested unknown multistream protocol {:?}", protocol_id)));
        }
        try!(stream.write_ms_msg(PROTOCOL_ID));
        println!("Attempting to negotiate multistream sub-protocol {:?}", protocol);
        try!(stream.write_ms_msg(&protocol));
        let response = try!(stream.read_ms_msg());
        if &response[..] == &protocol[..] {
            println!("Negotiated multistream sub-protocol {:?}", protocol);
            Ok(MultiStream {
                protocol: protocol,
                stream: stream,
            })
        } else if response == b"na" {
            println!("Server denied multistream sub-protocol {:?}", protocol);
            Err(io::Error::new(io::ErrorKind::Other, "Failed to negotiate a multistream sub-protocol"))
        } else {
            println!("Server returned unexpected response {:?}", response);
            Err(io::Error::new(io::ErrorKind::Other, "Failed to negotiate a multistream sub-protocol"))
        }
    }
}

impl<S> io::Read for MultiStream<S> where S: io::Read + io::Write {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }

    fn read_to_end(&mut self, buf: &mut Vec<u8>) -> io::Result<usize> {
        self.stream.read_to_end(buf)
    }

    fn read_to_string(&mut self, buf: &mut String) -> io::Result<usize> {
        self.stream.read_to_string(buf)
    }

    fn read_exact(&mut self, buf: &mut [u8]) -> io::Result<()> {
        self.stream.read_exact(buf)
    }
}

impl<S> io::Write for MultiStream<S> where S: io::Read + io::Write {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }

    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.stream.write_all(buf)
    }
}
