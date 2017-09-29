#![feature(conservative_impl_trait)]
#![feature(fnbox)]
#![feature(generators)]
#![feature(proc_macro)]

extern crate bytes;
extern crate futures_await as futures;
extern crate msgio;
#[macro_use]
extern crate slog;
extern crate tokio_io;

mod negotiator;
mod propose;
mod accept;

pub use negotiator::Negotiator;
