//! Asynchronous implementation using smol runtime.
//!
//! Check out the docs at [`asynchronous`] for more information.

use crate::{Object, asynchronous};
use std::io::Result;
#[cfg(unix)]
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};
#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, AsSocket, BorrowedSocket, RawSocket};

/// `smol` marker type.
#[derive(Debug, Object)]
pub struct Smol(async_io::Async<asynchronous::SyncStream>);

unsafe impl asynchronous::AsyncStream for Smol {
    fn try_new(stream: asynchronous::SyncStream) -> Result<Self> {
        stream.set_nonblocking(true)?;
        stream.try_into().map(Self)
    }

    #[cfg(unix)]
    const IS_BLOCKING: bool = false;

    #[cfg(unix)]
    async fn blocking_write<T>(&self, mut f: impl FnMut() -> Result<T> + Send) -> Result<T> {
        self.0.write_with(|_| f()).await
    }
    #[cfg(windows)]
    async fn write(&mut self, buf: &[u8]) -> Result<()> {
        use futures_lite::io::AsyncWriteExt;
        self.0.write_all(buf).await?;
        self.0.flush().await
    }

    #[cfg(unix)]
    async fn blocking_read<T>(&self, mut f: impl FnMut() -> Result<T> + Send) -> Result<T> {
        self.0.read_with(|_| f()).await
    }
    #[cfg(windows)]
    async fn read(&mut self, buf: &mut [u8]) -> Result<()> {
        use futures_lite::io::AsyncReadExt;
        self.0.read_exact(buf).await?;
        Ok(())
    }
}

#[cfg(unix)]
impl AsRawFd for Smol {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_raw_fd()
    }
}
#[cfg(windows)]
impl AsRawSocket for Smol {
    fn as_raw_socket(&self) -> RawSocket {
        self.0.as_raw_socket()
    }
}

#[cfg(unix)]
impl AsFd for Smol {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}
#[cfg(windows)]
impl AsSocket for Smol {
    fn as_socket(&self) -> BorrowedSocket<'_> {
        self.0.as_socket()
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
pub async unsafe fn spawn<
    Func: FnOnce(Box<dyn FnOnce() -> Args>) -> Ret,
    Args: Object,
    Ret: Object,
>(
    func: Func,
    args: Args,
) -> Result<Child<Ret>> {
    unsafe { asynchronous::spawn::<Smol, _, _, _>(func, args).await }
}
