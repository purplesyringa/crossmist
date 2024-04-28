//! Asynchronous implementation using tokio runtime.
//!
//! Check out the docs at [`asynchronous`] for more information.

use crate::{asynchronous, FnOnceObject, Object};
use std::io::Result;
use std::os::unix::io::RawFd;
use tokio::{io::Interest, net::UnixStream};

/// `tokio` marker struct.
pub struct Tokio;

unsafe impl asynchronous::AsyncRuntime for Tokio {
    type Stream = UnixStream;

    async fn blocking_write<T>(
        stream: &Self::Stream,
        f: impl FnMut() -> Result<T> + Send,
    ) -> Result<T> {
        stream.async_io(Interest::WRITABLE, f).await
    }

    async fn blocking_read<T>(
        stream: &Self::Stream,
        f: impl FnMut() -> Result<T> + Send,
    ) -> Result<T> {
        stream.async_io(Interest::READABLE, f).await
    }
}

/// The transmitting side of a unidirectional channel.
///
/// `T` is the type of the objects this side sends via the channel and the other side receives.
pub type Sender<T> = asynchronous::Sender<Tokio, T>;

/// The receiving side of a unidirectional channel.
///
/// `T` is the type of the objects the other side sends via the channel and this side receives.
pub type Receiver<T> = asynchronous::Receiver<Tokio, T>;

/// A side of a bidirectional channel.
///
/// `S` is the type of the objects this side sends via the channel and the other side receives, `R`
/// is the type of the objects the other side sends via the channel and this side receives.
pub type Duplex<S, R> = asynchronous::Duplex<Tokio, S, R>;

/// The subprocess object created by calling `spawn_tokio` on a function annotated with `#[func]`.
pub type Child<T> = asynchronous::Child<Tokio, T>;

/// Create a unidirectional channel.
pub fn channel<T: Object>() -> Result<(Sender<T>, Receiver<T>)> {
    asynchronous::channel::<Tokio, T>()
}

/// Create a bidirectional channel.
pub fn duplex<A: Object, B: Object>() -> Result<(Duplex<A, B>, Duplex<B, A>)> {
    asynchronous::duplex::<Tokio, A, B>()
}

#[doc(hidden)]
pub async unsafe fn spawn<T: Object>(
    entry: Box<dyn FnOnceObject<(RawFd,), Output = i32>>,
) -> Result<Child<T>> {
    asynchronous::spawn::<Tokio, T>(entry).await
}
