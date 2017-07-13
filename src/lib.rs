#![feature(conservative_impl_trait)]
#![feature(fnbox)]

extern crate futures;
extern crate msgio;
extern crate bytes;

mod negotiator;

pub use negotiator::*;
