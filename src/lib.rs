#![feature(conservative_impl_trait)]
#![feature(fnbox)]

#[macro_use]
extern crate futures;
extern crate msgio;
extern crate bytes;
extern crate tokio_io;
#[macro_use]
extern crate slog;

mod negotiator;

pub use negotiator::*;
