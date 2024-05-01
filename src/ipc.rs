//! Uni- and bidirectional channels between processes.
//!
//! Create and use a unidirectional channel:
//!
//! ```rust
//! # use crossmist::{channel, Receiver, Sender};
//! let (mut sender, mut receiver): (Sender<i32>, Receiver<i32>) = channel::<i32>()?;
//! sender.send(&57)?;
//! drop(sender);
//! assert_eq!(receiver.recv()?, Some(57));
//! assert_eq!(receiver.recv()?, None);
//! # std::io::Result::Ok(())
//! ```
//!
//! Create and use a bidirectional channel:
//!
//! ```rust
//! # use crossmist::{duplex, Duplex};
//! let (mut side1, mut side2) = duplex::<i32, (i32, i32)>()?;
//! side1.send(&57)?;
//! assert_eq!(side2.recv()?, Some(57));
//! side2.send(&(1, 2))?;
//! assert_eq!(side1.recv()?, Some((1, 2)));
//! drop(side1);
//! assert_eq!(side2.recv()?, None);
//! # std::io::Result::Ok(())
//! ```

use crate::{
    asynchronous,
    handles::{AsRawHandle, RawHandle},
    Object,
};
use std::future::Future;
use std::io::Result;
use std::pin::pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

fn block_on<F: Future>(f: F) -> F::Output {
    // https://github.com/rust-lang/rust/issues/98286
    const VTABLE: RawWakerVTable = RawWakerVTable::new(|_| RAW, |_| {}, |_| {}, |_| {});
    const RAW: RawWaker = RawWaker::new(std::ptr::null(), &VTABLE);
    let waker = unsafe { Waker::from_raw(RAW) };
    let mut cx = Context::from_waker(&waker);
    match pin!(f).poll(&mut cx) {
        Poll::Ready(value) => value,
        Poll::Pending => unreachable!(),
    }
}

#[cfg(unix)]
pub(crate) type SyncStream = std::os::unix::net::UnixStream;
#[cfg(windows)]
pub(crate) type SyncStream = std::fs::File;

unsafe impl asynchronous::AsyncStream for SyncStream {
    fn try_new(stream: SyncStream) -> Result<Self> {
        Ok(stream)
    }

    fn as_raw_handle(&self) -> RawHandle {
        AsRawHandle::as_raw_handle(self)
    }

    #[cfg(unix)]
    const IS_BLOCKING: bool = true;

    #[cfg(unix)]
    async fn blocking_write<T>(&self, mut f: impl FnMut() -> Result<T> + Send) -> Result<T> {
        f()
    }
    #[cfg(windows)]
    async fn write(&mut self, buf: &[u8]) -> Result<()> {
        use std::io::Write;
        self.write_all(buf)
    }

    #[cfg(unix)]
    async fn blocking_read<T>(&self, mut f: impl FnMut() -> Result<T> + Send) -> Result<T> {
        f()
    }
    #[cfg(windows)]
    async fn read(&mut self, buf: &mut [u8]) -> Result<()> {
        use std::io::Read;
        self.read_exact(buf)
    }
}

/// The transmitting side of a unidirectional channel.
///
/// `T` is the type of the objects this side sends via the channel and the other side receives.
#[derive(Object)]
pub struct Sender<T: Object>(pub(crate) asynchronous::Sender<SyncStream, T>);

/// The receiving side of a unidirectional channel.
///
/// `T` is the type of the objects the other side sends via the channel and this side receives.
#[derive(Object)]
pub struct Receiver<T: Object>(pub(crate) asynchronous::Receiver<SyncStream, T>);

/// A side of a bidirectional channel.
///
/// `S` is the type of the objects this side sends via the channel and the other side receives, `R`
/// is the type of the objects the other side sends via the channel and this side receives.
#[derive(Object)]
pub struct Duplex<S: Object, R: Object>(pub(crate) asynchronous::Duplex<SyncStream, S, R>);

/// Create a unidirectional channel.
pub fn channel<T: Object>() -> Result<(Sender<T>, Receiver<T>)> {
    let (tx, rx) = asynchronous::channel::<SyncStream, T>()?;
    Ok((Sender(tx), Receiver(rx)))
}

/// Create a bidirectional channel.
pub fn duplex<A: Object, B: Object>() -> Result<(Duplex<A, B>, Duplex<B, A>)> {
    let (tx, rx) = asynchronous::duplex::<SyncStream, A, B>()?;
    Ok((Duplex(tx), Duplex(rx)))
}

impl<T: Object> Sender<T> {
    /// Send a value to the other side.
    pub fn send(&mut self, value: &T) -> Result<()> {
        block_on(self.0.send(value))
    }
}

#[cfg(unix)]
impl<T: Object> std::os::unix::io::AsRawFd for Sender<T> {
    fn as_raw_fd(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}
#[cfg(windows)]
impl<T: Object> std::os::windows::io::AsRawHandle for Sender<T> {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        std::os::windows::io::AsRawHandle::as_raw_handle(&self.0)
    }
}

#[cfg(unix)]
impl<T: Object> std::os::unix::io::IntoRawFd for Sender<T> {
    fn into_raw_fd(self) -> RawHandle {
        self.0.into_raw_fd()
    }
}
#[cfg(windows)]
impl<T: Object> std::os::windows::io::IntoRawHandle for Sender<T> {
    fn into_raw_handle(self) -> std::os::windows::io::RawHandle {
        self.0.into_raw_handle()
    }
}

#[cfg(unix)]
impl<T: Object> std::os::unix::io::FromRawFd for Sender<T> {
    unsafe fn from_raw_fd(fd: RawHandle) -> Self {
        Self(asynchronous::Sender::from_stream(SyncStream::from_raw_fd(
            fd,
        )))
    }
}
#[cfg(windows)]
impl<T: Object> std::os::windows::io::FromRawHandle for Sender<T> {
    unsafe fn from_raw_handle(fd: std::os::windows::io::RawHandle) -> Self {
        Self(asynchronous::Sender::from_stream(
            SyncStream::from_raw_handle(fd),
        ))
    }
}

impl<T: Object> Receiver<T> {
    /// Receive a value from the other side.
    ///
    /// Returns `Ok(None)` if the other side has dropped the channel.
    pub fn recv(&mut self) -> Result<Option<T>> {
        block_on(self.0.recv())
    }
}

#[cfg(unix)]
impl<T: Object> std::os::unix::io::AsRawFd for Receiver<T> {
    fn as_raw_fd(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}
#[cfg(windows)]
impl<T: Object> std::os::windows::io::AsRawHandle for Receiver<T> {
    fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
        std::os::windows::io::AsRawHandle::as_raw_handle(&self.0)
    }
}

#[cfg(unix)]
impl<T: Object> std::os::unix::io::IntoRawFd for Receiver<T> {
    fn into_raw_fd(self) -> RawHandle {
        self.0.into_raw_fd()
    }
}
#[cfg(windows)]
impl<T: Object> std::os::windows::io::IntoRawHandle for Receiver<T> {
    fn into_raw_handle(self) -> std::os::windows::io::RawHandle {
        self.0.into_raw_handle()
    }
}

#[cfg(unix)]
impl<T: Object> std::os::unix::io::FromRawFd for Receiver<T> {
    unsafe fn from_raw_fd(fd: RawHandle) -> Self {
        Self(asynchronous::Receiver::from_stream(
            SyncStream::from_raw_fd(fd),
        ))
    }
}
#[cfg(windows)]
impl<T: Object> std::os::windows::io::FromRawHandle for Receiver<T> {
    unsafe fn from_raw_handle(fd: std::os::windows::io::RawHandle) -> Self {
        Self(asynchronous::Receiver::from_stream(
            SyncStream::from_raw_handle(fd),
        ))
    }
}

impl<S: Object, R: Object> Duplex<S, R> {
    /// Send a value to the other side.
    pub fn send(&mut self, value: &S) -> Result<()> {
        block_on(self.0.send(value))
    }

    /// Receive a value from the other side.
    ///
    /// Returns `Ok(None)` if the other side has dropped the channel.
    pub fn recv(&mut self) -> Result<Option<R>> {
        block_on(self.0.recv())
    }

    /// Send a value from the other side and wait for a response immediately.
    ///
    /// If the other side closes the channel before responding, an error is returned.
    pub fn request(&mut self, value: &S) -> Result<R> {
        block_on(self.0.request(value))
    }

    pub fn into_sender(self) -> Sender<S> {
        Sender(self.0.into_sender())
    }

    pub fn into_receiver(self) -> Receiver<R> {
        Receiver(self.0.into_receiver())
    }
}

#[cfg(unix)]
impl<S: Object, R: Object> std::os::unix::io::AsRawFd for Duplex<S, R> {
    fn as_raw_fd(&self) -> RawHandle {
        self.0.as_raw_handle()
    }
}

#[cfg(unix)]
impl<S: Object, R: Object> std::os::unix::io::IntoRawFd for Duplex<S, R> {
    fn into_raw_fd(self) -> RawHandle {
        self.0.into_raw_fd()
    }
}

#[cfg(unix)]
impl<S: Object, R: Object> std::os::unix::io::FromRawFd for Duplex<S, R> {
    unsafe fn from_raw_fd(fd: RawHandle) -> Self {
        Self(asynchronous::Duplex::from_stream(SyncStream::from_raw_fd(
            fd,
        )))
    }
}
