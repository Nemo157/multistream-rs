use std::boxed::FnBox;
use std::io;
use std::str;

use bytes::Bytes;
use futures::{Future, Stream, Sink};
use futures::prelude::{async, await};
use msgio::LengthPrefixed;
use slog::Logger;
use tokio_io::codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};

#[async]
fn propose<P, S>(logger: Logger, transport: Framed<S, LengthPrefixed>, protocol: P)
    -> impl Future<Item=(bool, Framed<S, LengthPrefixed>), Error=io::Error> + 'static
where
    P: AsRef<str> + 'static,
    S: AsyncRead + AsyncWrite + 'static
{
    debug!(logger, "Attempting to propose multistream protocol");
    let sending = transport.send(Bytes::from(protocol.as_ref()));
    let transport = await!(sending)?;
    let receiving = transport.into_future().map_err(|(error, _stream)| error);
    let (response, transport) = await!(receiving)?;
    match response.map(|b| String::from_utf8(b.to_vec())) {
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
}

#[async]
pub fn propose_all<P, S, R>(logger: Logger, mut transport: Framed<S, LengthPrefixed>, protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R + 'static>)>)
    -> impl Future<Item=R, Error=io::Error> + 'static
where
    P: AsRef<str> + 'static,
    S: AsyncRead + AsyncWrite + 'static,
    R: 'static
{
    for (protocol, callback) in protocols {
        let proposal = propose(logger.new(o!("protocol" => protocol.as_ref().to_owned())), transport, protocol);
        let (success, t) = await!(proposal)?;
        if success {
            return Ok(callback(t.into_parts()));
        } else {
            transport = t;
        }
    }
    Err(io::Error::new(io::ErrorKind::Other, "No protocol was negotiated"))
}
