use std::io;
use std::boxed::FnBox;

use futures::{ future, stream, Future, Sink, Stream };
use tokio_core::io::{ Io, Framed };

use msg::MsgCodec;

const PROTOCOL_ID: &'static [u8] = b"/multistream/1.0.0\n";

pub struct Negotiator<S, R> where S: Io + 'static {
    transport: Framed<S, MsgCodec>,
    protocols: Vec<(&'static [u8], Box<FnBox(S) -> Box<Future<Item=R, Error=io::Error>> + 'static>)>,
}

fn send_header<S>(transport: Framed<S, MsgCodec>) -> impl Future<Item=Framed<S, MsgCodec>, Error=io::Error> where S: Io {
    transport.send(PROTOCOL_ID.to_vec())
        .and_then(|transport| transport.into_future().map_err(|(error, _stream)| error))
        .and_then(|(response, transport)| {
            if let Some(response) = response {
                if response.as_slice() == PROTOCOL_ID {
                    Ok(transport)
                } else {
                    Err(io::Error::new(io::ErrorKind::Other, format!("Server requested unknown multistream protocol {:?}", String::from_utf8_lossy(response.as_slice()))))
                }
            } else {
                Err(io::Error::new(io::ErrorKind::Other, "Server unexpectedly closed the connection"))
            }
        })
}

fn negotiate<S>(transport: Framed<S, MsgCodec>, protocol: &'static [u8]) -> impl Future<Item=(bool, Framed<S, MsgCodec>), Error=io::Error> where S: Io {
    println!("Attempting to negotiate multistream protocol {}", String::from_utf8_lossy(&*protocol));
    let mut bytes = protocol.to_vec();
    bytes.push(b'\n');
    transport.send(bytes)
        .and_then(|transport| transport.into_future().map_err(|(error, _stream)| error))
        .and_then(move |(response, transport)| {
            if let Some(response) = response {
                if response.as_slice() == protocol {
                    println!("Negotiated multistream protocol {}", String::from_utf8_lossy(protocol));
                    Ok((true, transport))
                } else if response.as_slice() == b"na" {
                    println!("Server denied multistream protocol {}", String::from_utf8_lossy(protocol));
                    Ok((false, transport))
                } else {
                    println!("Server returned unexpected response {}", String::from_utf8_lossy(response.as_slice()));
                    Err(io::Error::new(io::ErrorKind::Other, "Unexpected response while negotiating multistream"))
                }
            } else {
                println!("Server unexpectedly closed the connection");
                Err(io::Error::new(io::ErrorKind::Other, "Server unexpectedly closed the connection"))
            }
        })
}

impl<S, R> Negotiator<S, R> where S: Io, R: 'static {
    pub fn start(transport: S) -> Negotiator<S, R> {
        Negotiator { transport: transport.framed(MsgCodec), protocols: Vec::new() }
    }

    pub fn negotiate<F>(mut self, protocol: &'static [u8], callback: F) -> Self where F: FnBox(S) -> Box<Future<Item=R, Error=io::Error>> + 'static {
        self.protocols.push((protocol, Box::new(callback)));
        self
    }

    pub fn finish(self) -> impl Future<Item=R, Error=io::Error> {
        let Negotiator { transport, protocols } = self;
        send_header(transport)
            .and_then(move |transport| stream::iter(protocols.into_iter().map(Ok))
                .fold(Err(transport), move |result, (protocol, callback)| -> Box<Future<Item=Result<R, Framed<S, MsgCodec>>, Error=io::Error>> {
                    match result {
                        Ok(result) => Box::new(future::ok(Ok(result))),
                        Err(transport) => Box::new(negotiate(transport, protocol).and_then(move |(success, transport)| -> Box<Future<Item=Result<R, Framed<S, MsgCodec>>, Error=io::Error>> {
                            if success {
                                Box::new(callback(transport.into_inner()).map(Ok))
                            } else {
                                Box::new(future::ok(Err(transport)))
                            }
                        })),
                    }
                })
                .and_then(|result| result.map_err(|_| io::Error::new(io::ErrorKind::Other, "No protocol was negotiated"))))
    }
}
