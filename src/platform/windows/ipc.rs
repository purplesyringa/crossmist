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

use crate::{handles, Deserializer, Object, Serializer};
use std::default::Default;
use std::ffi::c_void;
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Result, Write};
use std::marker::PhantomData;
use std::os::windows::io::{AsRawHandle, FromRawHandle, IntoRawHandle, RawHandle};
use windows::Win32::System::Pipes;

/// The transmitting side of a unidirectional channel.
///
/// `T` is the type of the objects this side sends via the channel and the other side receives.
#[derive(Object)]
pub struct Sender<T: Object> {
    file: File,
    marker: PhantomData<fn(T) -> T>,
}

/// The receiving side of a unidirectional channel.
///
/// `T` is the type of the objects the other side sends via the channel and this side receives.
#[derive(Object)]
pub struct Receiver<T: Object> {
    file: File,
    marker: PhantomData<fn(T) -> T>,
}

/// A side of a bidirectional channel.
///
/// `S` is the type of the objects this side sends via the channel and the other side receives, `R`
/// is the type of the objects the other side sends via the channel and this side receives.
#[derive(Object)]
pub struct Duplex<S: Object, R: Object> {
    sender_file: File,
    receiver_file: File,
    marker: PhantomData<fn(S, R) -> (S, R)>,
}

/// Create a unidirectional channel.
pub fn channel<T: Object>() -> Result<(Sender<T>, Receiver<T>)> {
    let mut tx: handles::RawHandle = Default::default();
    let mut rx: handles::RawHandle = Default::default();
    unsafe {
        Pipes::CreatePipe(
            &mut rx as *mut handles::RawHandle,
            &mut tx as *mut handles::RawHandle,
            std::ptr::null(),
            0,
        )
        .ok()?;
    }
    let tx = Sender {
        file: unsafe { File::from_raw_handle(tx.0 as *mut c_void) },
        marker: PhantomData,
    };
    let rx = Receiver {
        file: unsafe { File::from_raw_handle(rx.0 as *mut c_void) },
        marker: PhantomData,
    };
    Ok((tx, rx))
}

/// Create a bidirectional channel.
pub fn duplex<A: Object, B: Object>() -> Result<(Duplex<A, B>, Duplex<B, A>)> {
    let (tx_a, rx_a) = channel::<A>()?;
    let (tx_b, rx_b) = channel::<B>()?;
    let ours = Duplex {
        sender_file: tx_a.file,
        receiver_file: rx_b.file,
        marker: PhantomData,
    };
    let theirs = Duplex {
        sender_file: tx_b.file,
        receiver_file: rx_a.file,
        marker: PhantomData,
    };
    Ok((ours, theirs))
}

fn send_on_handle<T: Object>(file: &mut File, value: &T) -> Result<()> {
    let mut s = Serializer::new();
    s.serialize(value);

    let handles = s.drain_handles();
    if !handles.is_empty() {
        return Err(Error::new(
            ErrorKind::Other,
            "The message contains attached handles",
        ));
    }

    let serialized = s.into_vec();

    file.write_all(&serialized.len().to_ne_bytes())?;
    file.write_all(&serialized)?;
    Ok(())
}

fn recv_on_handle<T: Object>(file: &mut File) -> Result<Option<T>> {
    let mut len = [0u8; std::mem::size_of::<usize>()];
    if let Err(e) = file.read_exact(&mut len) {
        if e.kind() == ErrorKind::UnexpectedEof {
            return Ok(None);
        }
        return Err(e);
    }
    let len = usize::from_ne_bytes(len);

    let mut serialized = vec![0u8; len];
    file.read_exact(&mut serialized)?;

    let mut d = Deserializer::new(serialized, Vec::new());
    Ok(Some(d.deserialize()))
}

impl<T: Object> Sender<T> {
    fn from_file(file: File) -> Self {
        Sender {
            file,
            marker: PhantomData,
        }
    }

    /// Send a value to the other side.
    pub fn send(&mut self, value: &T) -> Result<()> {
        send_on_handle(&mut self.file, value)
    }
}

impl<T: Object> AsRawHandle for Sender<T> {
    fn as_raw_handle(&self) -> RawHandle {
        self.file.as_raw_handle()
    }
}

impl<T: Object> IntoRawHandle for Sender<T> {
    fn into_raw_handle(self) -> RawHandle {
        self.file.into_raw_handle()
    }
}

impl<T: Object> FromRawHandle for Sender<T> {
    unsafe fn from_raw_handle(handle: RawHandle) -> Self {
        Self::from_file(File::from_raw_handle(handle))
    }
}

impl<T: Object> Receiver<T> {
    fn from_file(file: File) -> Self {
        Receiver {
            file,
            marker: PhantomData,
        }
    }

    /// Receive a value from the other side.
    ///
    /// Returns `Ok(None)` if the other side has dropped the channel.
    pub fn recv(&mut self) -> Result<Option<T>> {
        recv_on_handle(&mut self.file)
    }
}

impl<T: Object> AsRawHandle for Receiver<T> {
    fn as_raw_handle(&self) -> RawHandle {
        self.file.as_raw_handle()
    }
}

impl<T: Object> IntoRawHandle for Receiver<T> {
    fn into_raw_handle(self) -> RawHandle {
        self.file.into_raw_handle()
    }
}

impl<T: Object> FromRawHandle for Receiver<T> {
    unsafe fn from_raw_handle(handle: RawHandle) -> Self {
        Self::from_file(File::from_raw_handle(handle))
    }
}

impl<S: Object, R: Object> Duplex<S, R> {
    /// Send a value to the other side.
    pub fn send(&mut self, value: &S) -> Result<()> {
        send_on_handle(&mut self.sender_file, value)
    }

    /// Receive a value from the other side.
    ///
    /// Returns `Ok(None)` if the other side has dropped the channel.
    pub fn recv(&mut self) -> Result<Option<R>> {
        recv_on_handle(&mut self.receiver_file)
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

    #[doc(hidden)]
    pub fn join(sender: Sender<S>, receiver: Receiver<R>) -> Self {
        Self {
            sender_file: sender.file,
            receiver_file: receiver.file,
            marker: PhantomData,
        }
    }

    #[doc(hidden)]
    pub fn split(self) -> (Sender<S>, Receiver<R>) {
        (
            Sender {
                file: self.sender_file,
                marker: PhantomData,
            },
            Receiver {
                file: self.receiver_file,
                marker: PhantomData,
            },
        )
    }
}
