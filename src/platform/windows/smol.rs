//! Asynchronous implementation using smol runtime.
//!
//! Check out the docs at [`asynchronous`] for more information.

use crate::{asynchronous, handles::RawHandle, FnOnceObject, Object};
use async_fs::File;
use futures_lite::io::{AsyncReadExt, AsyncWriteExt};
use std::io::Result;

/// `smol` marker struct.
pub struct Smol;

unsafe impl asynchronous::AsyncRuntime for Smol {
    type Stream = File;

    async fn write(stream: &mut Self::Stream, buf: &[u8]) -> Result<()> {
        stream.write_all(buf).await?;
        stream.flush().await?;
        Ok(())
    }

    async fn read(stream: &mut Self::Stream, buf: &mut [u8]) -> Result<()> {
        stream.read_exact(buf).await?;
        Ok(())
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
    entry: Box<dyn FnOnceObject<(RawHandle,), Output = i32>>,
) -> Result<Child<T>> {
    asynchronous::spawn::<Smol, T>(entry).await
}
