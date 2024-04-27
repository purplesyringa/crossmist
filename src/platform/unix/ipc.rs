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
    internals::{SingleObjectReceiver, SingleObjectSender},
    Object,
};
use nix::libc::{AF_UNIX, SOCK_CLOEXEC, SOCK_SEQPACKET};
use std::io::Result;
use std::marker::PhantomData;
use std::os::unix::{
    io::{AsRawFd, FromRawFd, IntoRawFd, RawFd},
    net::UnixStream,
};

/// The transmitting side of a unidirectional channel.
///
/// `T` is the type of the objects this side sends via the channel and the other side receives.
#[derive(Object)]
pub struct Sender<T: Object> {
    fd: UnixStream,
    marker: PhantomData<fn(T)>,
}

/// The receiving side of a unidirectional channel.
///
/// `T` is the type of the objects the other side sends via the channel and this side receives.
#[derive(Object)]
pub struct Receiver<T: Object> {
    fd: UnixStream,
    marker: PhantomData<fn() -> T>,
}

/// A side of a bidirectional channel.
///
/// `S` is the type of the objects this side sends via the channel and the other side receives, `R`
/// is the type of the objects the other side sends via the channel and this side receives.
#[derive(Object)]
pub struct Duplex<S: Object, R: Object> {
    fd: UnixStream,
    marker: PhantomData<fn(S) -> R>,
}

/// Create a unidirectional channel.
pub fn channel<T: Object>() -> Result<(Sender<T>, Receiver<T>)> {
    let (tx, rx) = duplex::<T, T>()?;
    Ok((tx.into_sender(), rx.into_receiver()))
}

/// Create a bidirectional channel.
pub fn duplex<A: Object, B: Object>() -> Result<(Duplex<A, B>, Duplex<B, A>)> {
    // UnixStream creates a SOCK_STREAM by default, while we need SOCK_SEQPACKET
    unsafe {
        let mut fds = [0, 0];
        if nix::libc::socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0, fds.as_mut_ptr()) == -1
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok((Duplex::from_raw_fd(fds[0]), Duplex::from_raw_fd(fds[1])))
    }
}

fn send_on_fd<T: Object>(fd: &UnixStream, value: &T) -> Result<()> {
    SingleObjectSender::new(fd.as_raw_fd(), value).send_next()
}

unsafe fn recv_on_fd<T: Object>(fd: &UnixStream) -> Result<Option<T>> {
    SingleObjectReceiver::new(fd.as_raw_fd()).recv_next()
}

impl<T: Object> Sender<T> {
    unsafe fn from_unix_stream(fd: UnixStream) -> Self {
        Sender {
            fd,
            marker: PhantomData,
        }
    }

    /// Send a value to the other side.
    pub fn send(&mut self, value: &T) -> Result<()> {
        send_on_fd(&self.fd, value)
    }
}

impl<T: Object> AsRawFd for Sender<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<T: Object> IntoRawFd for Sender<T> {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

impl<T: Object> FromRawFd for Sender<T> {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::from_unix_stream(UnixStream::from_raw_fd(fd))
    }
}

impl<T: Object> Receiver<T> {
    unsafe fn from_unix_stream(fd: UnixStream) -> Self {
        Receiver {
            fd,
            marker: PhantomData,
        }
    }

    /// Receive a value from the other side.
    ///
    /// Returns `Ok(None)` if the other side has dropped the channel.
    pub fn recv(&mut self) -> Result<Option<T>> {
        unsafe { recv_on_fd(&self.fd) }
    }
}

impl<T: Object> AsRawFd for Receiver<T> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<T: Object> IntoRawFd for Receiver<T> {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

impl<T: Object> FromRawFd for Receiver<T> {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::from_unix_stream(UnixStream::from_raw_fd(fd))
    }
}

impl<S: Object, R: Object> Duplex<S, R> {
    unsafe fn from_unix_stream(fd: UnixStream) -> Self {
        Duplex {
            fd,
            marker: PhantomData,
        }
    }

    /// Send a value to the other side.
    pub fn send(&mut self, value: &S) -> Result<()> {
        send_on_fd(&self.fd, value)
    }

    /// Receive a value from the other side.
    ///
    /// Returns `Ok(None)` if the other side has dropped the channel.
    pub fn recv(&mut self) -> Result<Option<R>> {
        unsafe { recv_on_fd(&self.fd) }
    }

    /// Send a value from the other side and wait for a response immediately.
    ///
    /// If the other side closes the channel before responding, an error is returned.
    pub fn request(&mut self, value: &S) -> Result<R> {
        self.send(value)?;
        self.recv()?.ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "The subprocess exitted before responding to the request",
            )
        })
    }

    pub(crate) fn into_sender(self) -> Sender<S> {
        unsafe { Sender::from_unix_stream(self.fd) }
    }

    pub(crate) fn into_receiver(self) -> Receiver<R> {
        unsafe { Receiver::from_unix_stream(self.fd) }
    }
}

impl<S: Object, R: Object> AsRawFd for Duplex<S, R> {
    fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }
}

impl<S: Object, R: Object> IntoRawFd for Duplex<S, R> {
    fn into_raw_fd(self) -> RawFd {
        self.fd.into_raw_fd()
    }
}

impl<S: Object, R: Object> FromRawFd for Duplex<S, R> {
    unsafe fn from_raw_fd(fd: RawFd) -> Self {
        Self::from_unix_stream(UnixStream::from_raw_fd(fd))
    }
}
