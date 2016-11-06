use std::io;

use msg::{ ReadMultiStreamMessage, WriteMultiStreamMessage };

const PROTOCOL_ID: &'static [u8] = b"/multistream/1.0.0";

#[derive(Debug)]
pub enum Negotiator<S, R> where S: io::Read + io::Write {
    Builder {
        stream: S,
    },
    Finished {
        result: io::Result<R>,
    },
}

fn send_header<S>(stream: &mut S) -> io::Result<()> where S: io::Read + io::Write {
    // Expect the other end to send a multistream header
    let protocol_id = try!(stream.read_ms_msg());
    if protocol_id != PROTOCOL_ID {
        return Err(io::Error::new(io::ErrorKind::Other, format!("Server requested unknown multistream protocol {:?}", protocol_id)));
    }
    try!(stream.write_ms_msg(PROTOCOL_ID));
    Ok(())
}

fn negotiate<S>(stream: &mut S, protocol: &'static [u8]) -> io::Result<bool> where S: io::Read + io::Write {
    println!("Attempting to negotiate multistream protocol {}", String::from_utf8_lossy(&*protocol));
    try!(stream.write_ms_msg(&protocol));
    let response = try!(stream.read_ms_msg());
    if &response[..] == &protocol[..] {
        println!("Negotiated multistream protocol {}", String::from_utf8_lossy(&*protocol));
        Ok(true)
    } else if response == b"na" {
        println!("Server denied multistream protocol {}", String::from_utf8_lossy(&*protocol));
        Ok(false)
    } else {
        println!("Server returned unexpected response {}", String::from_utf8_lossy(&*response));
        Err(io::Error::new(io::ErrorKind::Other, "Unexpected response while negotiating multistream"))
    }
}

impl<S, R> Negotiator<S, R> where S: io::Read + io::Write {
    pub fn start(mut stream: S) -> Negotiator<S, R> {
        match send_header(&mut stream) {
            Ok(()) => Negotiator::Builder { stream: stream },
            Err(err) => Negotiator::Finished { result: Err(err) },
        }
    }

    pub fn negotiate<F>(self, protocol: &'static [u8], callback: F) -> Negotiator<S, R> where F: FnOnce(S) -> io::Result<R> {
        match self {
            Negotiator::Builder { mut stream } => {
                match negotiate(&mut stream, protocol) {
                    Ok(true) => Negotiator::Finished { result: callback(stream) },
                    Ok(false) => Negotiator::Builder { stream: stream },
                    Err(err) => Negotiator::Finished { result: Err(err) },
                }
            }
            Negotiator::Finished { .. } => self
        }
    }

    pub fn ls(&mut self) -> io::Result<Vec<String>> {
        match *self {
            Negotiator::Builder { ref mut stream } => {
                try!(stream.write_ms_msg(b"ls"));
                let mut response: &[u8] = &try!(stream.read_ms_msg());
                let mut protocols = Vec::new();
                while let Some(protocol) = try!(response.try_read_ms_msg()) {
                    protocols.push(String::from_utf8_lossy(&*protocol).into_owned());
                }
                Ok(protocols)
            }
            Negotiator::Finished { .. } => Err(io::Error::new(io::ErrorKind::Other, "Cannot ls on a finished multistream negotiator"))
        }
    }

    pub fn finish(self) -> io::Result<R> {
        match self {
            Negotiator::Builder { .. } => Err(io::Error::new(io::ErrorKind::Other, "Did not successfully negotiate any connection")),
            Negotiator::Finished { result } => result,
        }
    }
}
