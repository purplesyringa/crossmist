//! Asynchronous implementation using tokio runtime.
//!
//! Check out the docs at [`asynchronous`] for more information.

use crate::{
    asynchronous,
    handles::{AsHandle, AsRawHandle, BorrowedHandle, RawHandle},
    FnOnceObject, Object,
};
use std::io::Result;

/// `tokio` marker struct.
#[derive(Debug, Object)]
pub struct Tokio(
    #[cfg(unix)] tokio::net::UnixStream,
    #[cfg(windows)] tokio::fs::File,
);

unsafe impl asynchronous::AsyncStream for Tokio {
    fn try_new(stream: asynchronous::SyncStream) -> Result<Self> {
        #[cfg(unix)]
        {
            stream.set_nonblocking(true)?;
            stream.try_into().map(Self)
        }
        #[cfg(windows)]
        return Ok(Self(stream.into()));
    }

    fn as_handle(&self) -> BorrowedHandle<'_> {
        self.0.as_handle()
    }

    fn as_raw_handle(&self) -> RawHandle {
        self.0.as_raw_handle()
    }

    #[cfg(unix)]
    const IS_BLOCKING: bool = false;

    #[cfg(unix)]
    async fn blocking_write<T>(&self, f: impl FnMut() -> Result<T> + Send) -> Result<T> {
        self.0.async_io(tokio::io::Interest::WRITABLE, f).await
    }
    #[cfg(windows)]
    async fn write(&mut self, buf: &[u8]) -> Result<()> {
        use tokio::io::AsyncWriteExt;
        self.0.write_all(buf).await
    }

    #[cfg(unix)]
    async fn blocking_read<T>(&self, f: impl FnMut() -> Result<T> + Send) -> Result<T> {
        self.0.async_io(tokio::io::Interest::READABLE, f).await
    }
    #[cfg(windows)]
    async fn read(&mut self, buf: &mut [u8]) -> Result<()> {
        use tokio::io::AsyncReadExt;
        self.0.read_exact(buf).await?;
        Ok(())
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
    entry: Box<dyn FnOnceObject<(RawHandle,), Output = i32>>,
) -> Result<Child<T>> {
    asynchronous::spawn::<Tokio, T>(entry).await
}
