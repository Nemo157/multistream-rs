use std::boxed::FnBox;
use std::io;
use std::str;

use bytes::Bytes;
use futures::{future, stream, Future, Stream, Sink};
use msgio::LengthPrefixed;
use slog::Logger;
use tokio_io::codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};

fn propose<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static>(logger: Logger, transport: Framed<S, LengthPrefixed>, protocol: P) -> impl Future<Item=(bool, Framed<S, LengthPrefixed>), Error=io::Error> {
    debug!(logger, "Attempting to propose multistream protocol");
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

pub fn propose_all<P: AsRef<str> + 'static, S: AsyncRead + AsyncWrite + 'static, R: 'static>(logger: Logger, transport: Framed<S, LengthPrefixed>, protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>) -> impl Future<Item=R, Error=io::Error> {
    stream::iter(protocols.into_iter().map(Ok))
        .fold(Err(transport), move |result, (protocol, callback)| -> Box<Future<Item=_, Error=_> + 'static> {
            match result {
                Ok(result) => Box::new(future::ok(Ok(result))),
                Err(transport) => Box::new(propose(logger.new(o!("protocol" => protocol.as_ref().to_owned())), transport, protocol)
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
