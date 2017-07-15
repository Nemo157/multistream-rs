#![feature(conservative_impl_trait)]
#![feature(fnbox)]

#[macro_use]
extern crate futures;
extern crate msgio;
extern crate bytes;

mod negotiator;

pub use negotiator::*;
