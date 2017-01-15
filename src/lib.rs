#![feature(conservative_impl_trait)]
#![feature(fnbox)]

extern crate varmint;
extern crate futures;
extern crate tokio_core;

mod msg;
mod negotiator;

pub use negotiator::*;
