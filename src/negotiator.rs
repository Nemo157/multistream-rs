use std::io;
use std::boxed::FnBox;

use bytes::BytesMut;
use futures::{ future, stream, Future, Stream };
use msgio::MsgIo;

const PROTOCOL_ID: &'static [u8] = b"/multistream/1.0.0";

pub struct Negotiator<S: MsgIo + 'static, R: 'static> {
    transport: S,
    protocols: Vec<(&'static [u8], Box<FnBox(S) -> Box<Future<Item=R, Error=io::Error>> + 'static>)>,
}

fn send_header<S: MsgIo + 'static>(transport: S) -> impl Future<Item=S, Error=io::Error> {
    transport.send(BytesMut::from(PROTOCOL_ID))
        .and_then(|transport| transport.into_future().map_err(|(error, _stream)| error))
        .and_then(|(response, transport)| {
            if let Some(response) = response {
                if response == PROTOCOL_ID {
                    Ok(transport)
                } else {
                    Err(io::Error::new(io::ErrorKind::Other, format!("Server requested unknown multistream protocol {:?}", String::from_utf8_lossy(&response))))
                }
            } else {
                Err(io::Error::new(io::ErrorKind::Other, "Server unexpectedly closed the connection"))
            }
        })
}

fn negotiate<S: MsgIo + 'static>(transport: S, protocol: &'static [u8]) -> impl Future<Item=(bool, S), Error=io::Error> {
    println!("Attempting to negotiate multistream protocol {}", String::from_utf8_lossy(&*protocol));
    transport.send(BytesMut::from(protocol))
        .and_then(|transport| transport.into_future().map_err(|(error, _stream)| error))
        .and_then(move |(response, transport)| {
            if let Some(response) = response {
                if response == protocol {
                    println!("Negotiated multistream protocol {}", String::from_utf8_lossy(protocol));
                    Ok((true, transport))
                } else if response == &b"na"[..] {
                    println!("Server denied multistream protocol {}", String::from_utf8_lossy(protocol));
                    Ok((false, transport))
                } else {
                    println!("Server returned unexpected response {}", String::from_utf8_lossy(&response));
                    Err(io::Error::new(io::ErrorKind::Other, "Unexpected response while negotiating multistream"))
                }
            } else {
                println!("Server unexpectedly closed the connection");
                Err(io::Error::new(io::ErrorKind::Other, "Server unexpectedly closed the connection"))
            }
        })
}

impl<S: MsgIo + 'static, R: 'static> Negotiator<S, R> {
    pub fn start(transport: S) -> Negotiator<S, R> {
        Negotiator {
            transport: transport,
            protocols: Vec::new(),
        }
    }

    pub fn negotiate<F>(mut self, protocol: &'static [u8], callback: F) -> Self where F: FnBox(S) -> Box<Future<Item=R, Error=io::Error>> + 'static {
        self.protocols.push((protocol, Box::new(callback)));
        self
    }

    pub fn finish(self) -> impl Future<Item=R, Error=io::Error> {
        let Negotiator { transport, protocols } = self;
        send_header(transport)
            .and_then(move |transport| stream::iter(protocols.into_iter().map(Ok))
                .fold(Err(transport), move |result, (protocol, callback)| -> Box<Future<Item=_, Error=_> + 'static> {
                    match result {
                        Ok(result) => Box::new(future::ok(Ok(result))),
                        Err(transport) => Box::new(negotiate(transport, protocol)
                            .and_then(move |(success, transport)| -> Box<Future<Item=_, Error=_> + 'static> {
                                if success {
                                    Box::new(callback(transport).map(Ok))
                                } else {
                                    Box::new(future::ok(Err(transport)))
                                }
                            })),
                    }
                })
                .and_then(|result| result.map_err(|_| io::Error::new(io::ErrorKind::Other, "No protocol was negotiated"))))
    }
}
