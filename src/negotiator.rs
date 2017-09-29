use std::boxed::FnBox;
use std::io;
use std::str;

use bytes::Bytes;
use futures::{future, Future, Stream, Sink};
use msgio::{LengthPrefixed, Prefix, Suffix};
use slog::Logger;
use tokio_io::codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};

use accept::Acceptor;
use propose::propose_all;

const PROTOCOL_ID: &'static str = "/multistream/1.0.0";

pub struct Negotiator<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static> {
    logger: Logger,
    initiator: bool,
    transport: Framed<S, LengthPrefixed>,
    protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>,
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
                    future::Either::A(propose_all(logger, transport, protocols))
                } else {
                    debug!(logger, "Attempting to accept multistream");
                    future::Either::B(Acceptor::new(logger, transport, protocols))
                }
            })
    }
}
