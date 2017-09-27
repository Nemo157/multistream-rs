use std::boxed::FnBox;
use std::io;
use std::mem;
use std::str;

use bytes::Bytes;
use futures::{ future, stream, Future, Stream, Sink, Poll, Async };
use futures::sink::Send;
use msgio::{LengthPrefixed, Prefix, Suffix};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Framed, FramedParts};
use slog::Logger;

const PROTOCOL_ID: &'static str = "/multistream/1.0.0";

pub struct Negotiator<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static> {
    logger: Logger,
    initiator: bool,
    transport: Framed<S, LengthPrefixed>,
    protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>,
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
        callback: Box<FnBox(FramedParts<S>) -> R>,
    },
}

pub struct Acceptor<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static> {
    logger: Logger,
    protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>,
    state: AcceptorState<S, R>,
}

fn send_header<S: AsyncRead + AsyncWrite + 'static>(logger: Logger, transport: Framed<S, LengthPrefixed>) -> impl Future<Item=Framed<S, LengthPrefixed>, Error=io::Error> {
    transport.send(Bytes::from(PROTOCOL_ID))
        .and_then(|transport| transport.into_future().map_err(|(error, _stream)| error))
        .and_then(move |(response, transport)| {
            let response = response.as_ref().map(|b| str::from_utf8(b));
            trace!(logger, "multistream header received: {:?}", response);
            match response {
                Some(Ok(response)) => if response == PROTOCOL_ID  {
                    Ok(transport)
                } else {
                    Err(io::Error::new(io::ErrorKind::Other, format!("Server requested unknown multistream protocol {:?}", response)))
                },
                Some(Err(e)) => Err(io::Error::new(io::ErrorKind::InvalidData, e)),
                None => Err(io::Error::new(io::ErrorKind::Other, "Server unexpectedly closed the connection")),
            }
        })
}

fn negotiate<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static>(logger: Logger, transport: Framed<S, LengthPrefixed>, protocol: P) -> impl Future<Item=(bool, Framed<S, LengthPrefixed>), Error=io::Error> {
    debug!(logger, "Attempting to negotiate multistream protocol");
    transport.send(Bytes::from(protocol.as_ref()))
        .and_then(|transport| transport.into_future().map_err(|(error, _stream)| error))
        .and_then(move |(response, transport)| {
            match response.as_ref().map(|b| str::from_utf8(b)) {
                Some(Ok(response)) => if response == protocol.as_ref() {
                    debug!(logger, "Negotiated multistream protocol");
                    Ok((true, transport))
                } else if response == "na" {
                    debug!(logger, "Server denied multistream protocol");
                    Ok((false, transport))
                } else {
                    debug!(logger, "Server returned unexpected response {}", response);
                    Err(io::Error::new(io::ErrorKind::Other, "Unexpected response while negotiating multistream"))
                },
                Some(Err(e)) => {
                    debug!(logger, "Server sent non-utf8 message");
                    Err(io::Error::new(io::ErrorKind::InvalidData, e))
                }
                None => {
                    debug!(logger, "Server unexpectedly closed the connection");
                    Err(io::Error::new(io::ErrorKind::Other, "Server unexpectedly closed the connection"))
                }
            }
        })
}

fn negotiate_all<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static>(logger: Logger, transport: Framed<S, LengthPrefixed>, protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>) -> impl Future<Item=R, Error=io::Error> {
    stream::iter(protocols.into_iter().map(Ok))
        .fold(Err(transport), move |result, (protocol, callback)| -> Box<Future<Item=_, Error=_> + 'static> {
            match result {
                Ok(result) => Box::new(future::ok(Ok(result))),
                Err(transport) => Box::new(negotiate(logger.new(o!("protocol" => protocol.as_ref().to_owned())), transport, protocol)
                    .and_then(move |(success, transport)| -> Box<Future<Item=_, Error=_> + 'static> {
                        if success {
                            Box::new(future::ok(Ok(callback(transport.into_parts()))))
                        } else {
                            Box::new(future::ok(Err(transport)))
                        }
                    })),
            }
        })
        .and_then(|result| result.map_err(|_| io::Error::new(io::ErrorKind::Other, "No protocol was negotiated")))
}

impl<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static> Negotiator<P, S, R> {
    pub fn start(logger: Logger, transport: S, initiator: bool) -> Negotiator<P, S, R> {
        let protocols = Vec::new();
        let transport = transport.framed(LengthPrefixed(Prefix::VarInt, Suffix::NewLine));
        Negotiator { logger, initiator, transport, protocols }
    }

    pub fn negotiate<F>(mut self, protocol: P, callback: F) -> Self where F: FnBox(FramedParts<S>) -> R + 'static {
        self.protocols.push((protocol, Box::new(callback)));
        self
    }

    pub fn finish(self) -> impl Future<Item=R, Error=io::Error> {
        let Negotiator { logger, initiator, transport, protocols } = self;
        send_header(logger.clone(), transport)
            .and_then(move |transport| {
                if initiator {
                    debug!(logger, "Attempting to negotiate multistream");
                    future::Either::A(negotiate_all(logger, transport, protocols))
                } else {
                    debug!(logger, "Attempting to accept multistream");
                    future::Either::B(Acceptor::new(logger, transport, protocols))
                }
            })
    }
}

impl<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static> Acceptor<P, S, R> {
    fn new(logger: Logger, transport: Framed<S, LengthPrefixed>, protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>) -> Acceptor<P, S, R> {
        let state = AcceptorState::Ready { transport };
        Acceptor { logger, state, protocols }
    }
}

impl<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static> Future for Acceptor<P, S, R> {
    type Item = R;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        loop {
            let new_state = match self.state {
                AcceptorState::Denying { ref mut sending } => {
                    let transport = try_ready!(sending.poll());
                    Some(AcceptorState::Ready { transport })
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
                            match str::from_utf8(&message) {
                                Ok(message) => if let Some(i) = self.protocols.iter().position(|&(ref p, _)| p.as_ref() == message) {
                                    let (protocol, callback) = self.protocols.swap_remove(i);
                                    debug!(self.logger, "Negotiated multistream protocol {}", protocol.as_ref());
                                    self.state = AcceptorState::Accepting {
                                        sending: transport.send(Bytes::from(protocol.as_ref())),
                                        callback,
                                    };
                                    continue;
                                } else if message == "ls" {
                                    debug!(self.logger, "TODO: Server requested ls");
                                    return Err(io::Error::new(io::ErrorKind::Other, "TODO: Server requested ls"));
                                } else {
                                    debug!(self.logger, "Server asked for unknown protocol {}", message);
                                    self.state = AcceptorState::Denying {
                                        sending: transport.send(Bytes::from(&b"na"[..])),
                                    };
                                    continue;
                                }
                                Err(e) => {
                                    debug!(self.logger, "Server requested non-utf8 protocol");
                                    return Err(io::Error::new(io::ErrorKind::InvalidData, e));
                                }
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
                            return Ok(Async::Ready(callback(transport.into_parts())));
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
