use std::boxed::FnBox;
use std::io;
use std::str;

use bytes::Bytes;
use futures::{Stream, Sink};
use futures::prelude::{async, await};
use msgio::LengthPrefixed;
use slog::Logger;
use tokio_io::codec::{Framed, FramedParts};
use tokio_io::{AsyncRead, AsyncWrite};

#[async]
pub fn accept<P, S, R>(
    logger: Logger,
    mut transport: Framed<S, LengthPrefixed>,
    mut protocols: Vec<(P, Box<FnBox(FramedParts<S>) -> R>)>)
    -> Result<R, io::Error>
where
    P: AsRef<str> + 'static,
    S: AsyncRead + AsyncWrite + 'static,
    R: 'static
{
    while let (Some(message), t) = await!(transport.into_future()).map_err(|(e, _)| e)? {
        transport = t;
        let message = {let logger = logger.clone(); String::from_utf8(message.to_vec()).map_err(move |e| {
            debug!(logger, "Server requested non-utf8 protocol");
            io::Error::new(io::ErrorKind::InvalidData, e)
        })?};
        if message == "ls" {
            debug!(logger, "TODO: Server requested ls");
            return Err(io::Error::new(io::ErrorKind::Other, "TODO: Server requested ls"));
        }

        let index = protocols.iter().position(|&(ref p, _)| p.as_ref() == message);
        if let Some(i) = index {
            let (protocol, callback) = protocols.swap_remove(i);
            debug!(logger, "Negotiated multistream protocol {}", protocol.as_ref());
            transport = await!(transport.send(Bytes::from(protocol.as_ref())))?;
            return Ok(callback(transport.into_parts()));
        } else {
            debug!(logger, "Server asked for unknown protocol {}", message);
            transport = await!(transport.send(Bytes::from(&b"na"[..])))?;
        }
    }
    return Err(io::Error::new(io::ErrorKind::Other, "Peer gave up on negotiation"));
}
