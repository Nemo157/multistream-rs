#![feature(conservative_impl_trait)]
#![feature(fnbox)]

extern crate futures;
extern crate msgio;

mod negotiator;

pub use negotiator::*;
