//! Asynchronous implementation using smol runtime.
//!
//! Check out the docs at [`asynchronous`] for more information.

use crate::{asynchronous, FnOnceObject, Object};
use async_io::Async;
use std::io::Result;
use std::os::unix::{io::RawFd, net::UnixStream};

/// `smol` marker struct.
pub struct Smol;

unsafe impl asynchronous::AsyncRuntime for Smol {
    type Stream = Async<UnixStream>;

    async fn blocking_write<T>(
        stream: &Self::Stream,
        mut f: impl FnMut() -> Result<T> + Send,
    ) -> Result<T> {
        stream.write_with(|_| f()).await
    }

    async fn blocking_read<T>(
        stream: &Self::Stream,
        mut f: impl FnMut() -> Result<T> + Send,
    ) -> Result<T> {
        stream.read_with(|_| f()).await
    }
}

/// The transmitting side of a unidirectional channel.
///
/// `T` is the type of the objects this side sends via the channel and the other side receives.
pub type Sender<T> = asynchronous::Sender<Smol, T>;

/// The receiving side of a unidirectional channel.
///
/// `T` is the type of the objects the other side sends via the channel and this side receives.
pub type Receiver<T> = asynchronous::Receiver<Smol, T>;

/// A side of a bidirectional channel.
///
/// `S` is the type of the objects this side sends via the channel and the other side receives, `R`
/// is the type of the objects the other side sends via the channel and this side receives.
pub type Duplex<S, R> = asynchronous::Duplex<Smol, S, R>;

/// The subprocess object created by calling `spawn_smol` on a function annotated with `#[func]`.
pub type Child<T> = asynchronous::Child<Smol, T>;

/// Create a unidirectional channel.
pub fn channel<T: Object>() -> Result<(Sender<T>, Receiver<T>)> {
    asynchronous::channel::<Smol, T>()
}

/// Create a bidirectional channel.
pub fn duplex<A: Object, B: Object>() -> Result<(Duplex<A, B>, Duplex<B, A>)> {
    asynchronous::duplex::<Smol, A, B>()
}

#[doc(hidden)]
pub async unsafe fn spawn<T: Object>(
    entry: Box<dyn FnOnceObject<(RawFd,), Output = i32>>,
) -> Result<Child<T>> {
    asynchronous::spawn::<Smol, T>(entry).await
}
