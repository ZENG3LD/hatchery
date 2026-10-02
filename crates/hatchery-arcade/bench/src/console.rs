//! Raw Win32 console I/O for `live`'s own round-trip clock.
//!
//! `live` needs to know how long the REAL terminal (not our own process,
//! not the ConPTY pipe) takes to catch up to a frame we just wrote. The
//! technique: after every SIXEL payload, write a Device Status Report
//! query (`ESC[6n`, "report cursor position") into the SAME byte stream,
//! then time how long the terminal's own reply (`ESC[row;colR`) takes to
//! come back on stdin. Windows Terminal's VT parser consumes bytes from
//! that stream strictly in order, so a reply only arrives once the parser
//! has finished ingesting (decoding + storing) everything written before
//! it -- including the sixel image. This is the same round-trip-probe
//! technique terminal benchmarking tools (e.g. vtebench/termbench-style
//! harnesses) use; it is a proxy for "ingested", not literally "painted
//! to the screen" (compositing is a separate, GPU-vsync-gated step
//! downstream of the parser) -- `live`'s own report notes that caveat
//! rather than overclaiming.
//!
//! `std::io::Stdin` cannot do this cleanly: on Windows it line-buffers by
//! default (waits for Enter) and Rust exposes no portable way to flip
//! Win32 console modes. So this module talks to the console handles
//! directly: clears `ENABLE_LINE_INPUT`/`ENABLE_ECHO_INPUT`/
//! `ENABLE_PROCESSED_INPUT` and sets `ENABLE_VIRTUAL_TERMINAL_INPUT` on
//! stdin (raw byte-at-a-time delivery, escape sequences passed through as
//! literal bytes rather than translated into key events), and best-effort
//! sets `ENABLE_VIRTUAL_TERMINAL_PROCESSING` on stdout (already implied
//! under ConPTY, but harmless to also set explicitly).

use std::io;
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::ReadFile;
use windows_sys::Win32::System::Console::{
    GetConsoleMode, GetStdHandle, SetConsoleMode, CONSOLE_MODE, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
    ENABLE_VIRTUAL_TERMINAL_INPUT, ENABLE_VIRTUAL_TERMINAL_PROCESSING, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
};

const INVALID_HANDLE_BITS: isize = -1;

fn std_handle(which: u32) -> io::Result<HANDLE> {
    let handle = unsafe { GetStdHandle(which) };
    if handle.is_null() || handle as isize == INVALID_HANDLE_BITS {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

/// Enables raw stdin mode for the process's lifetime of use, restoring
/// the original mode on drop (so a crashed or short-lived `live` run
/// never leaves the owner's next terminal session in raw mode).
pub struct RawMode {
    stdin_handle: HANDLE,
    stdin_original: CONSOLE_MODE,
}

impl RawMode {
    pub fn enable() -> io::Result<Self> {
        let stdin_handle = std_handle(STD_INPUT_HANDLE)?;
        let mut stdin_original: CONSOLE_MODE = 0;
        if unsafe { GetConsoleMode(stdin_handle, &mut stdin_original) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let raw = (stdin_original & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT)) | ENABLE_VIRTUAL_TERMINAL_INPUT;
        if unsafe { SetConsoleMode(stdin_handle, raw) } == 0 {
            return Err(io::Error::last_os_error());
        }

        // Best-effort: under ConPTY this is normally already implied, and
        // a failure here (e.g. stdout redirected to a file) must not stop
        // raw stdin mode from taking effect.
        if let Ok(stdout_handle) = std_handle(STD_OUTPUT_HANDLE) {
            let mut stdout_mode: CONSOLE_MODE = 0;
            if unsafe { GetConsoleMode(stdout_handle, &mut stdout_mode) } != 0 {
                let _ = unsafe { SetConsoleMode(stdout_handle, stdout_mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) };
            }
        }

        Ok(Self { stdin_handle, stdin_original })
    }

    /// Spawns the background raw-byte reader this mode's own stdin now
    /// delivers into. One reader per process is the intended lifetime --
    /// callers keep the returned [`Receiver`] for the process's duration.
    pub fn spawn_reader(&self) -> Receiver<u8> {
        let handle_bits = self.stdin_handle as isize;
        let (tx, rx) = mpsc::channel::<u8>();
        thread::spawn(move || {
            let handle = handle_bits as HANDLE;
            let mut buf = [0u8; 256];
            loop {
                let mut read: u32 = 0;
                let ok = unsafe { ReadFile(handle, buf.as_mut_ptr(), buf.len() as u32, &mut read, std::ptr::null_mut()) };
                if ok == 0 || read == 0 {
                    return;
                }
                for &b in &buf[..read as usize] {
                    if tx.send(b).is_err() {
                        return;
                    }
                }
            }
        });
        rx
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = unsafe { SetConsoleMode(self.stdin_handle, self.stdin_original) };
    }
}

/// Blocks until a Device Status Report reply (`ESC[row;colR`) terminator
/// byte (`R`) arrives on `rx`, or `timeout` elapses. Returns the `Instant`
/// the terminator was seen, or `None` on timeout (the terminal never
/// answered within budget -- a genuine result, not an error, worth
/// reporting as a stall rather than discarding).
///
/// Only this crate's own `ESC[6n` queries ever produce input on this
/// stream (nothing else asks the owner to type into this window), so
/// treating any `R` byte as the terminator is safe in this controlled
/// harness -- a general-purpose VT parser would need to validate the full
/// `ESC[...R` shape instead.
pub fn wait_for_dsr_reply(rx: &Receiver<u8>, timeout: Duration) -> Option<Instant> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        match rx.recv_timeout(remaining) {
            Ok(b'R') => return Some(Instant::now()),
            Ok(_) => continue,
            Err(_) => return None,
        }
    }
}
