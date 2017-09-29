use std::boxed::FnBox;
use std::io;
use std::mem;
use std::str;

use bytes::Bytes;
use futures::sink::Send;
use futures::{Future, Stream, Sink, Poll, Async};
use msgio::LengthPrefixed;
use slog::Logger;
use tokio_io::codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};

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

impl<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static> Acceptor<P, S, R> {
    pub(crate) fn new(logger: Logger, transport: Framed<S, LengthPrefixed>, protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>) -> Acceptor<P, S, R> {
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
