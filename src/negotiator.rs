use std::boxed::FnBox;
use std::io;
use std::mem;

use bytes::Bytes;
use futures::{ future, stream, Future, Stream, Sink, Poll, Async };
use futures::sink::Send;
use msgio::{LengthPrefixed, Prefix, Suffix};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Framed, FramedParts};

const PROTOCOL_ID: &'static [u8] = b"/multistream/1.0.0";

pub struct Negotiator<S: AsyncRead + AsyncWrite + 'static, R: 'static> {
    initiator: bool,
    transport: Framed<S, LengthPrefixed>,
    protocols: Vec<(&'static [u8], Box<FnBox(FramedParts<S>) -> Box<Future<Item=R, Error=io::Error>> + 'static>)>,
}

enum AcceptorState<S: AsyncRead + AsyncWrite + 'static, R: 'static> {
    Invalid,
    Ready {
        transport: Framed<S, LengthPrefixed>,
    },
    Denying {
        sending: Send<Framed<S, LengthPrefixed>>,
    },
    Accepting {
        sending: Send<Framed<S, LengthPrefixed>>,
        callback: Box<FnBox(FramedParts<S>) -> Box<Future<Item=R, Error=io::Error>> + 'static>,
    },
    Wrapping {
        wrapping: Box<Future<Item=R, Error=io::Error>>,
    },
}

pub struct Acceptor<S: AsyncRead + AsyncWrite + 'static, R: 'static> {
    protocols: Vec<(&'static [u8], Box<FnBox(FramedParts<S>) -> Box<Future<Item=R, Error=io::Error>> + 'static>)>,
    state: AcceptorState<S, R>,
}

fn send_header<S: AsyncRead + AsyncWrite + 'static>(transport: Framed<S, LengthPrefixed>) -> impl Future<Item=Framed<S, LengthPrefixed>, Error=io::Error> {
    transport.send(Bytes::from(PROTOCOL_ID))
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

fn negotiate<S: AsyncRead + AsyncWrite + 'static>(transport: Framed<S, LengthPrefixed>, protocol: &'static [u8]) -> impl Future<Item=(bool, Framed<S, LengthPrefixed>), Error=io::Error> {
    println!("Attempting to negotiate multistream protocol {}", String::from_utf8_lossy(&*protocol));
    transport.send(Bytes::from(protocol))
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

fn negotiate_all<S: AsyncRead + AsyncWrite + 'static, R: 'static>(transport: Framed<S, LengthPrefixed>, protocols: Vec<(&'static [u8], Box<FnBox(FramedParts<S>) -> Box<Future<Item=R, Error=io::Error>> + 'static>)>) -> impl Future<Item=R, Error=io::Error> {
    stream::iter(protocols.into_iter().map(Ok))
        .fold(Err(transport), move |result, (protocol, callback)| -> Box<Future<Item=_, Error=_> + 'static> {
            match result {
                Ok(result) => Box::new(future::ok(Ok(result))),
                Err(transport) => Box::new(negotiate(transport, protocol)
                    .and_then(move |(success, transport)| -> Box<Future<Item=_, Error=_> + 'static> {
                        if success {
                            Box::new(callback(transport.into_parts()).map(Ok))
                        } else {
                            Box::new(future::ok(Err(transport)))
                        }
                    })),
            }
        })
        .and_then(|result| result.map_err(|_| io::Error::new(io::ErrorKind::Other, "No protocol was negotiated")))
}

impl<S: AsyncRead + AsyncWrite + 'static, R: 'static> Negotiator<S, R> {
    pub fn start(transport: S, initiator: bool) -> Negotiator<S, R> {
        let protocols = Vec::new();
        let transport = transport.framed(LengthPrefixed(Prefix::VarInt, Suffix::NewLine));
        Negotiator { initiator, transport, protocols }
    }

    pub fn negotiate<F>(mut self, protocol: &'static [u8], callback: F) -> Self where F: FnBox(FramedParts<S>) -> Box<Future<Item=R, Error=io::Error>> + 'static {
        self.protocols.push((protocol, Box::new(callback)));
        self
    }

    pub fn finish(self) -> impl Future<Item=R, Error=io::Error> {
        let Negotiator { initiator, transport, protocols } = self;
        send_header(transport)
            .and_then(move |transport| {
                if initiator {
                    future::Either::A(negotiate_all(transport, protocols))
                } else {
                    println!("Attempting to accept multistream protocol");
                    future::Either::B(Acceptor::new(transport, protocols))
                }
            })
    }
}

impl<S: AsyncRead + AsyncWrite + 'static, R: 'static> Acceptor<S, R> {
    fn new(transport: Framed<S, LengthPrefixed>, protocols: Vec<(&'static [u8], Box<FnBox(FramedParts<S>) -> Box<Future<Item=R, Error=io::Error>> + 'static>)>) -> Acceptor<S, R> {
        Acceptor {
            state: AcceptorState::Ready { transport },
            protocols: protocols,
        }
    }
}

impl<S: AsyncRead + AsyncWrite + 'static, R: 'static> Future for Acceptor<S, R> {
    type Item = R;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let new_state = match self.state {
                AcceptorState::Denying { ref mut sending } => {
                    let transport = try_ready!(sending.poll());
                    Some(AcceptorState::Ready { transport })
                }
                AcceptorState::Wrapping { ref mut wrapping } => {
                    return wrapping.poll();
                }
                AcceptorState::Invalid => {
                    panic!("Acceptor future invalid")
                }
                AcceptorState::Accepting { .. } | AcceptorState::Ready { .. } => {
                    // Handled below since they needs to take arguments by value
                    None
                }
            };

            if let Some(state) = new_state {
                self.state = state;
                continue;
            }

            match mem::replace(&mut self.state, AcceptorState::Invalid) {
                AcceptorState::Ready { mut transport } => {
                    match transport.poll()? {
                        Async::Ready(Some(message)) => {
                            if let Some(i) = self.protocols.iter().position(|&(p, _)| p == message) {
                                let (protocol, callback) = self.protocols.swap_remove(i);
                                println!("Negotiated multistream protocol {}", String::from_utf8_lossy(protocol));
                                self.state = AcceptorState::Accepting {
                                    sending: transport.send(Bytes::from(protocol)),
                                    callback,
                                };
                                continue;
                            } else if message == &b"ls"[..] {
                                println!("TODO: Server requested ls");
                                return Err(io::Error::new(io::ErrorKind::Other, "TODO: Server requested ls"));
                            } else {
                                println!("Server asked for unknown protocol {}", String::from_utf8_lossy(&message));
                                self.state = AcceptorState::Denying {
                                    sending: transport.send(Bytes::from(&b"na"[..])),
                                };
                                continue;
                            }
                        }
                        Async::Ready(None) => {
                            return Err(io::Error::new(io::ErrorKind::Other, "Peer gave up on negotiation"));
                        }
                        Async::NotReady => {
                            self.state = AcceptorState::Ready { transport };
                            return Ok(Async::NotReady);
                        }
                    }
                }

                AcceptorState::Accepting { mut sending, callback } => {
                    match sending.poll()? {
                        Async::Ready(transport) => {
                            self.state = AcceptorState::Wrapping {
                                wrapping: callback(transport.into_parts()),
                            };
                        }
                        Async::NotReady => {
                            self.state = AcceptorState::Accepting { sending, callback };
                        }
                    }
                    continue;
                }

                _ => panic!("Acceptor future unreachable reached")
            }
        }
    }
}
