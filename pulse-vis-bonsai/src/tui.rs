//! TUI wrapper: raw mode on stdin, alternate screen buffer, key polling, frame rendering.
use std::io::{self, Stdout, Write};

#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use termios::{ICANON, ECHO, ECHOE, ECHOK, ECHONL, ISIG, IEXTEN, VMIN, VTIME, TCSANOW, Termios};
#[cfg(unix)]
use std::os::raw::c_void;
/// Termios state (unix only) stored to restore the terminal on exit.
#[cfg(unix)]
type TermiosState = Option<Termios>;
#[cfg(not(unix))]
type TermiosState = ();

pub struct Tui {
    width: u16,
    height: u16,
    stdout: Stdout,
    original_termios: TermiosState,
}

impl Tui {
    /// Initialize raw mode on stdin and enter the alternate screen buffer.
    pub fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let (width, height) = crossterm::terminal::size().unwrap_or_default();
        let width = width.max(1);
        let height = height.max(1);

        let original_termios: TermiosState;
        #[cfg(unix)]
        {
            let original = termios::Termios::from_fd(io::stdin().as_raw_fd())?;
            let mut raw = original.clone();
            raw.c_lflag &= !(ICANON | ECHO | ECHOE | ECHOK | ECHONL | ISIG | IEXTEN);
            raw.c_cc[VMIN] = 0;
            raw.c_cc[VTIME] = 0;
            termios::tcsetattr(io::stdin().as_raw_fd(), TCSANOW, &raw)?;
            original_termios = Some(original);
        }
        #[cfg(not(unix))]
        {
            original_termios = ();
        }

        let stdout = io::stdout();
        let mut out = stdout.lock();
        // Enter the alternate screen buffer (exit with \x1b[?1049l on drop).
        write!(out, "\x1b[?1049h")?;
        // Clear and home the cursor.
        write!(out, "\x1b[2J\x1b[H")?;
        out.flush()?;

        Ok(Self {
            width,
            height,
            stdout,
            original_termios,
        })
    }

    pub fn width(&self) -> u16 { self.width }
    pub fn height(&self) -> u16 { self.height }

    /// Re-measure the terminal.
    #[allow(dead_code)]
    pub fn resize(&mut self) {
        let (w, h) = crossterm::terminal::size().unwrap_or_default();
        self.width = w.max(1);
        self.height = h.max(1);
    }

    /// Poll for a keypress from stdin (non-blocking).
    /// Returns Some(key) if a byte is available, None otherwise.
    pub fn poll_key(&self) -> Option<u8> {
        #[cfg(unix)]
        {
            let fd = io::stdin().as_raw_fd();
            let mut nbytes: i64 = 0;
            unsafe {
                if ioctl_fionread(fd, &mut nbytes) < 0 || nbytes <= 0 {
                    return None;
                }
                let mut buf = [0u8; 1];
                if read_fd(fd, &mut buf, 1) <= 0 {
                    return None;
                }
                Some(buf[0])
            }
        }
        #[cfg(not(unix))]
        {
            None
        }
    }

    /// Draw a frame of rows, each up to `width` chars.
    pub fn write_frame(&self, rows: &[String]) -> io::Result<()> {
        let mut buf = String::new();
        for (i, row) in rows.iter().enumerate() {
            buf.push('\n');
            buf.push_str(&format!("\x1b[{};1H{}", i + 1, row));
        }
        let mut out = self.stdout.lock();
        write!(out, "{}", buf)?;
        out.flush()
    }

    /// Restore raw mode and exit the alternate screen buffer.
    fn exit(&self) {
        #[cfg(unix)]
        {
            if let Some(original) = &self.original_termios {
                let _ = termios::tcsetattr(io::stdin().as_raw_fd(), TCSANOW, original);
            }
        }
        let mut out = self.stdout.lock();
        let _ = write!(out, "\x1b[?1049l");
        let _ = out.flush();
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        self.exit();
    }
}

/// FFI helpers for FIONREAD and read.
#[cfg(unix)]
use libc::{ioctl, read};

#[cfg(unix)]
unsafe fn ioctl_fionread(fd: i32, arg: *mut i64) -> i64 {
    std::mem::transmute(ioctl(fd, 0x4c00, arg as *mut c_void) as i64)
}

#[cfg(unix)]
unsafe fn read_fd(fd: i32, buf: &[u8], n: usize) -> i64 {
    std::mem::transmute(read(fd, buf.as_ptr() as *mut c_void, n) as i64)
}
