use crate::{Deserializer, Object, Serializer, subprocess::HANDLE_BROKER};
use std::io::{Error, ErrorKind, Result};
use std::net::TcpStream;
use std::os::windows::io::{
    AsRawHandle, AsRawSocket, FromRawHandle, FromRawSocket, IntoRawHandle, OwnedHandle,
    OwnedSocket, RawHandle, RawSocket,
};
use std::path::PathBuf;
use std::sync::LazyLock;
use windows::Win32::{
    Foundation::{self, HANDLE},
    Networking::WinSock,
    Security::Cryptography,
    System::{IO, Threading},
};

pub(crate) fn socketpair() -> Result<(TcpStream, TcpStream)> {
    loop {
        if let Some(out) = try_socketpair()? {
            return Ok(out);
        }
    }
}

fn new_socket() -> Result<(OwnedSocket, WinSock::SOCKET)> {
    let socket = unsafe {
        WinSock::WSASocketW(
            WinSock::AF_UNIX as i32,
            WinSock::SOCK_STREAM.0,
            0,
            None,
            0,
            WinSock::WSA_FLAG_OVERLAPPED | WinSock::WSA_FLAG_NO_HANDLE_INHERIT,
        )
    }?;
    Ok((
        unsafe { OwnedSocket::from_raw_socket(socket.0 as RawSocket) },
        socket,
    ))
}

pub(crate) fn try_socketpair() -> Result<Option<(TcpStream, TcpStream)>> {
    // Achieves two purposes: initializes WSA and generates a temporary directory path once, since
    // Wine spams FIXMEs on `GetTempPath2W` and I don't want to read them.
    static INIT: LazyLock<core::result::Result<PathBuf, WinSock::WSA_ERROR>> =
        LazyLock::new(|| {
            let mut data = WinSock::WSADATA::default();
            if unsafe { WinSock::WSAStartup(0x0202, &raw mut data) } != 0 {
                return Err(unsafe { WinSock::WSAGetLastError() });
            }
            Ok(std::env::temp_dir())
        });
    let temp_dir = match INIT.as_deref() {
        Ok(temp_dir) => temp_dir,
        Err(err) => return Err(Error::from_raw_os_error(err.0)),
    };

    let mut rng = [0u8; 32];
    let _ = unsafe { Cryptography::ProcessPrng(&mut rng) }; // documented to return TRUE

    let mut name = "crossmist-".to_string();
    for c in rng {
        name.push((b'a' + (c & 15)) as char);
    }
    let path = temp_dir.join(name);
    let path_bytes = path.as_os_str().as_encoded_bytes();

    let mut addr = WinSock::SOCKADDR_UN {
        sun_family: WinSock::ADDRESS_FAMILY(WinSock::AF_UNIX),
        ..Default::default()
    };
    if path_bytes.len() > addr.sun_path.len() {
        return Err(Error::from(ErrorKind::InvalidFilename));
    }
    for (dst, src) in addr.sun_path.iter_mut().zip(path_bytes) {
        *dst = *src as i8;
    }

    unsafe {
        let (_server, raw_server) = new_socket()?;
        if WinSock::bind(
            raw_server,
            (&raw const addr).cast(),
            size_of_val(&addr) as i32,
        ) != 0
        {
            return Err(Error::from_raw_os_error(WinSock::WSAGetLastError().0));
        }
        struct SockGuard(PathBuf);
        impl Drop for SockGuard {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let _guard = SockGuard(path);
        // Backlog 0 is load-bearing for Wine -- see below
        if WinSock::listen(raw_server, 0) != 0 {
            return Err(Error::from_raw_os_error(WinSock::WSAGetLastError().0));
        }

        let (client, raw_client) = new_socket()?;
        let connect_result = WinSock::connect(
            raw_client,
            (&raw const addr).cast(),
            size_of_val(&addr) as i32,
        );
        if connect_result != 0 {
            let err = WinSock::WSAGetLastError();
            if err == WinSock::WSAENOBUFS
                || err == WinSock::WSAECONNREFUSED
                || err == WinSock::WSAEWOULDBLOCK
            {
                // Someone raced us to connection and filled the backlog, retry. Real Windows
                // returns WSAENOBUFS, but Wine forwards native errors, which are WSAECONNREFUSED on
                // POSIX (including FreeBSD) and WSAEWOULDBLOCK on Linux.
                return Ok(None);
            }
            return Err(Error::from_raw_os_error(err.0));
        }

        // Use `AcceptEx` rather than `accept`/`WSAAccept` to atomically make the socket
        // non-inheritable (`AcceptEx` allow accepting into an existing socket).
        let (connected, raw_connected) = new_socket()?;
        let mut overlapped = IO::OVERLAPPED::default();
        // `AcceptEx` docs say we need to reserve 16 more bytes for the internal format.
        const ADDR_SIZE: usize = size_of::<WinSock::SOCKADDR_UN>() + 16;
        let mut addresses = [0; ADDR_SIZE * 2];
        let mut tmp = 0;
        if WinSock::AcceptEx(
            raw_server,
            raw_connected,
            (&raw mut addresses).cast(),
            0,
            ADDR_SIZE as u32,
            ADDR_SIZE as u32,
            &raw mut tmp,
            &raw mut overlapped,
        ) == false
        {
            let err = WinSock::WSAGetLastError();
            if err.0 != Foundation::ERROR_IO_PENDING.0 as i32 {
                return Err(Error::from_raw_os_error(err.0));
            }
            let mut flags = 0;
            WinSock::WSAGetOverlappedResult(
                raw_server,
                &raw const overlapped,
                &raw mut tmp,
                true,
                &raw mut flags,
            )?;
        }
        // Make the socket usable for all operations: https://stackoverflow.com/a/9174331/5417677
        if WinSock::setsockopt(
            raw_connected,
            WinSock::SOL_SOCKET,
            WinSock::SO_UPDATE_ACCEPT_CONTEXT,
            Some(core::slice::from_raw_parts(
                (&raw const raw_server).cast(),
                size_of_val(&raw_server),
            )),
        ) != 0
        {
            return Err(Error::from_raw_os_error(WinSock::WSAGetLastError().0));
        }

        // POSIX says [1] the backlog argument to `listen` is merely a hint, so even if we set the
        // smallest backlog value possible and our connection succeeds, we can't be sure that no one
        // else has raced us. Therefore we need to validate the PID.
        // [1]: https://pubs.opengroup.org/onlinepubs/009695099/functions/listen.html
        let mut pid = 0u32;
        if WinSock::ioctlsocket(
            raw_connected,
            WinSock::SIO_AF_UNIX_GETPEERPID as i32,
            (&raw mut pid).cast(),
        ) == 0
        {
            if pid != Threading::GetCurrentProcessId() {
                return Ok(None);
            }
        } else {
            let err = WinSock::WSAGetLastError();
            // Wine doesn't implement SIO_AF_UNIX_GETPEERPID [1], so this gets messy.
            //
            // Linux interprets `backlog = 0` as exactly one connection allowed [2], which produces
            // the right semantics: since our `connect` has completed before we `accept`ed any
            // connection, we know the client in the queue must be us. FreeBSD multiplies `backlog`
            // by 1.5, but Sonya checked it behaves the same way for `backlog = 0`.
            //
            // [1]: https://bugs.winehq.org/show_bug.cgi?id=60201
            // [2]: https://github.com/torvalds/linux/commit/64a146513f8f
            if err != WinSock::WSAEOPNOTSUPP {
                return Err(Error::from_raw_os_error(err.0));
            }
        }

        Ok(Some((TcpStream::from(client), TcpStream::from(connected))))
    }
}

pub(crate) fn serialize_with_handles<T: Object>(value: T) -> Result<Vec<u8>> {
    let broker = HANDLE_BROKER
        .get()
        .expect("broker has not been initialized");

    let mut s = Serializer::new();
    s.serialize(value);

    let copy_handle = |handle: RawHandle| -> Result<usize> {
        let mut remote_handle: HANDLE = Default::default();
        unsafe {
            Foundation::DuplicateHandle(
                Threading::GetCurrentProcess(),
                HANDLE(handle),
                HANDLE(broker.process.as_raw_handle()),
                &mut remote_handle,
                0,
                false,
                Foundation::DUPLICATE_SAME_ACCESS,
            )?;
        }
        Ok(remote_handle.0.addr())
    };

    let remote_handles = s
        .handles
        .into_iter()
        .map(|handle| copy_handle(handle.as_raw_handle()))
        .collect::<Result<Vec<usize>>>()?;

    let remote_sockets = s
        .sockets
        .into_iter()
        .map(|socket| {
            // On modern Windows, sockets are always valid handles. Just make sure to let the normal
            // `OwnedSocket` destructor to be invoked, so that it's closed by `closesocket` rather
            // than `CloseHandle` to correctly free userland resources.
            copy_handle(socket.as_raw_socket() as RawHandle)
        })
        .collect::<Result<Vec<usize>>>()?;

    let mut s1 = Serializer::new();
    s1.serialize(remote_handles);
    s1.serialize(remote_sockets);
    s1.write(&s.data);
    Ok(s1.data)
}

pub(crate) unsafe fn deserialize_with_handles<T: Object>(serialized: Vec<u8>) -> Result<T> {
    let broker = HANDLE_BROKER
        .get()
        .expect("broker has not been initialized");

    let mut d = Deserializer::from(Serializer {
        data: serialized,
        handles: Vec::new(),
        sockets: Vec::new(),
    });

    let steal_handle = |remote_handle: usize| -> Result<OwnedHandle> {
        let mut handle = Default::default();
        unsafe {
            Foundation::DuplicateHandle(
                HANDLE(broker.process.as_raw_handle()),
                HANDLE(core::ptr::without_provenance_mut(remote_handle)),
                Threading::GetCurrentProcess(),
                &mut handle,
                0,
                false,
                Foundation::DUPLICATE_CLOSE_SOURCE | Foundation::DUPLICATE_SAME_ACCESS,
            )
        }?;
        Ok(unsafe { OwnedHandle::from_raw_handle(handle.0) })
    };

    let handles = unsafe { d.deserialize::<Vec<usize>>() }
        .into_iter()
        .map(steal_handle)
        .collect::<Result<Vec<_>>>()?;

    let sockets = unsafe { d.deserialize::<Vec<usize>>() }
        .into_iter()
        .map(|remote_socket| {
            // Modern Winsock2 lazily fetches socket information from the native handle even without
            // going through `WSADuplicateSocketW`/`WSASocket`, see
            // https://purplesyringa.moe/blog/duplicatehandle-works-on-sockets-mostly.
            let socket = steal_handle(remote_socket)?.into_raw_handle() as RawSocket;
            Ok(unsafe { OwnedSocket::from_raw_socket(socket) })
        })
        .collect::<Result<Vec<_>>>()?;

    let data = d.get_rest().to_vec();
    Ok(unsafe {
        Deserializer::from(Serializer {
            data,
            handles,
            sockets,
        })
        .deserialize()
    })
}
