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
