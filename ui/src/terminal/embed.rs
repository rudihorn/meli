use crate::terminal::position::Area;
use nix::fcntl::{open, OFlag};
use nix::ioctl_write_ptr_bad;
use nix::libc::{STDERR_FILENO, STDIN_FILENO, STDOUT_FILENO};
use nix::pty::{grantpt, posix_openpt, ptsname, unlockpt, Winsize};
use nix::sys::stat::Mode;
use nix::sys::{stat, wait};
use nix::unistd::{dup2, fork, setsid, ForkResult};
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};

mod grid;

use crate::terminal::cells::{Cell, CellBuffer};
pub use grid::*;

// ioctl command to set window size of pty:
use libc::TIOCSWINSZ;
use std::path::Path;
use std::process::{Command, Stdio};

use std::io::Read;
use std::io::Write;
use std::sync::{Arc, Mutex};

ioctl_write_ptr_bad!(set_window_size, TIOCSWINSZ, Winsize);

static SWITCHALTERNATIVE_1049: &'static [u8] = &[b'1', b'0', b'4', b'9'];

#[derive(Debug)]
pub struct EmbedPty {
    pub grid: Arc<Mutex<CellBuffer>>,
    pub stdin: std::fs::File,
    pub terminal_size: (usize, usize),
}

pub fn create_pty(area: Area) -> nix::Result<EmbedPty> {
    // Open a new PTY master
    let master_fd = posix_openpt(OFlag::O_RDWR)?;

    // Allow a slave to be generated for it
    grantpt(&master_fd)?;
    unlockpt(&master_fd)?;

    // Get the name of the slave
    let slave_name = unsafe { ptsname(&master_fd) }?;

    // Try to open the slave
    //let _slave_fd = open(Path::new(&slave_name), OFlag::O_RDWR, Mode::empty())?;
    {
        let winsize = Winsize {
            ws_row: 40,
            ws_col: 80,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };

        let master_fd = master_fd.clone().into_raw_fd();
        unsafe { set_window_size(master_fd, &winsize).unwrap() };
    }
    match fork() {
        Ok(ForkResult::Child) => {
            setsid().unwrap(); // create new session with child as session leader
            let slave_fd = open(Path::new(&slave_name), OFlag::O_RDWR, stat::Mode::empty())?;

            // assign stdin, stdout, stderr to the tty, just like a terminal does
            dup2(slave_fd, STDIN_FILENO).unwrap();
            dup2(slave_fd, STDOUT_FILENO).unwrap();
            dup2(slave_fd, STDERR_FILENO).unwrap();
            std::process::Command::new("vim").status().unwrap();
        }
        Ok(ForkResult::Parent { child: _ }) => {}
        Err(e) => panic!(e),
    };

    let stdin = unsafe { std::fs::File::from_raw_fd(master_fd.clone().into_raw_fd()) };
    let stdin_ = unsafe { std::fs::File::from_raw_fd(master_fd.clone().into_raw_fd()) };
    let grid = Arc::new(Mutex::new(CellBuffer::new(80, 40, Cell::default())));
    let grid_ = grid.clone();
    let terminal_size = (80, 40);

    std::thread::Builder::new()
        .spawn(move || {
            let master_fd = master_fd.into_raw_fd();
            let master_file = unsafe { std::fs::File::from_raw_fd(master_fd) };
            forward_pty_translate_escape_codes(master_file, area, grid_, stdin_);
        })
        .unwrap();
    Ok(EmbedPty {
        grid,
        stdin,
        terminal_size,
    })
}

#[derive(Debug)]
pub enum State {
    ExpectingControlChar,
    G0,            // Designate G0 Character Set
    Osc1(Vec<u8>), //ESC ] Operating System Command (OSC  is 0x9d).
    Osc2(Vec<u8>, Vec<u8>),
    Csi, // ESC [ Control Sequence Introducer (CSI  is 0x9b).
    Csi1(Vec<u8>),
    Csi2(Vec<u8>, Vec<u8>),
    Csi3(Vec<u8>, Vec<u8>, Vec<u8>),
    CsiQ(Vec<u8>),
    Normal,
}

struct EscCode<'a>(&'a State, u8);

impl<'a> From<(&'a mut State, u8)> for EscCode<'a> {
    fn from(val: (&mut State, u8)) -> EscCode {
        let (s, b) = val;
        EscCode(s, b)
    }
}

impl<'a> From<(&'a State, u8)> for EscCode<'a> {
    fn from(val: (&State, u8)) -> EscCode {
        let (s, b) = val;
        EscCode(s, b)
    }
}

impl std::fmt::Display for EscCode<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        use State::*;
        macro_rules! unsafestr {
            ($buf:ident) => {
                unsafe { std::str::from_utf8_unchecked($buf) }
            };
        }
        match self {
            EscCode(G0, c) => write!(f, "ESC({}\t\tG0 charset set", *c as char),
            EscCode(Osc1(ref buf), ref c) => {
                write!(f, "ESC]{}{}\t\tOSC", unsafestr!(buf), *c as char)
            }
            EscCode(Osc2(ref buf1, ref buf2), c) => write!(
                f,
                "ESC]{};{}{}\t\tOSC [UNKNOWN]",
                unsafestr!(buf1),
                unsafestr!(buf2),
                *c as char
            ),
            EscCode(Csi, b'm') => write!(
                f,
                "ESC[m\t\tCSI Character Attributes | Set Attr and Color to Normal (default)"
            ),
            EscCode(Csi, b'K') => write!(
                f,
                "ESC[K\t\tCSI Erase from the cursor to the end of the line [BAD]"
            ),
            EscCode(Csi, b'J') => write!(
                f,
                "ESC[J\t\tCSI Erase from the cursor to the end of the screen [BAD]"
            ),
            EscCode(Csi, b'H') => write!(f, "ESC[H\t\tCSI Move the cursor to home position. [BAD]"),
            EscCode(Csi, c) => write!(f, "ESC[{}\t\tCSI [UNKNOWN]", *c as char),
            EscCode(Csi1(ref buf), b'm') => write!(
                f,
                "ESC[{}m\t\tCSI Character Attributes | Set fg, bg color",
                unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b'n') => write!(
                f,
                "ESC[{}n\t\tCSI Device Status Report (DSR)| Report Cursor Position",
                unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b't') if buf == b"18" => write!(
                f,
                "ESC[18t\t\tReport the size of the text area in characters",
            ),
            EscCode(Csi1(ref buf), b't') => write!(
                f,
                "ESC[{buf}t\t\tWindow manipulation, skipped",
                buf = unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b'B') => write!(
                f,
                "ESC[{buf}B\t\tCSI Cursor Down {buf} Times",
                buf = unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b'C') => write!(
                f,
                "ESC[{buf}C\t\tCSI Cursor Forward {buf} Times",
                buf = unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b'D') => write!(
                f,
                "ESC[{buf}D\t\tCSI Cursor Backward {buf} Times",
                buf = unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b'E') => write!(
                f,
                "ESC[{buf}E\t\tCSI Cursor Next Line {buf} Times",
                buf = unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b'F') => write!(
                f,
                "ESC[{buf}F\t\tCSI Cursor Preceding Line {buf} Times",
                buf = unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), b'G') => write!(
                f,
                "ESC[{buf}G\t\tCursor Character Absolute  [column={buf}] (default = [row,1])",
                buf = unsafestr!(buf)
            ),
            EscCode(Csi1(ref buf), c) => {
                write!(f, "ESC[{}{}\t\tCSI [UNKNOWN]", unsafestr!(buf), *c as char)
            }
            EscCode(Csi2(ref buf1, ref buf2), c) => write!(
                f,
                "ESC[{};{}{}\t\tCSI",
                unsafestr!(buf1),
                unsafestr!(buf2),
                *c as char
            ),
            EscCode(Csi3(ref buf1, ref buf2, ref buf3), b'm') => write!(
                f,
                "ESC[{};{};{}m\t\tCSI Character Attributes | Set fg, bg color",
                unsafestr!(buf1),
                unsafestr!(buf2),
                unsafestr!(buf3),
            ),
            EscCode(Csi3(ref buf1, ref buf2, ref buf3), c) => write!(
                f,
                "ESC[{};{};{}{}\t\tCSI [UNKNOWN]",
                unsafestr!(buf1),
                unsafestr!(buf2),
                unsafestr!(buf3),
                *c as char
            ),
            EscCode(CsiQ(ref buf), b's') => write!(
                f,
                "ESC[?{}r\t\tCSI Save DEC Private Mode Values",
                unsafestr!(buf)
            ),
            EscCode(CsiQ(ref buf), b'r') => write!(
                f,
                "ESC[?{}r\t\tCSI Restore DEC Private Mode Values",
                unsafestr!(buf)
            ),
            EscCode(CsiQ(ref buf), b'h') if buf == &[b'2', b'5'] => write!(
                f,
                "ESC[?25h\t\tCSI DEC Private Mode Set (DECSET) show cursor",
            ),
            EscCode(CsiQ(ref buf), b'h') => write!(
                f,
                "ESC[?{}h\t\tCSI DEC Private Mode Set (DECSET). [UNKNOWN]",
                unsafestr!(buf)
            ),
            EscCode(CsiQ(ref buf), b'l') if buf == &[b'2', b'5'] => write!(
                f,
                "ESC[?25l\t\tCSI DEC Private Mode Set (DECSET) hide cursor",
            ),
            EscCode(CsiQ(ref buf), c) => {
                write!(f, "ESC[?{}{}\t\tCSI [UNKNOWN]", unsafestr!(buf), *c as char)
            }
            _ => unreachable!(),
        }
    }
}

fn forward_pty_translate_escape_codes(
    pty_fd: std::fs::File,
    area: Area,
    grid: Arc<Mutex<CellBuffer>>,
    stdin: std::fs::File,
) {
    let (upper_left, bottom_right) = area;
    let (upper_x, upper_y) = upper_left;
    let (bottom_x, bottom_y) = bottom_right;
    let upper_x_str = upper_x.to_string();
    let upper_y_str = upper_y.to_string();
    let bottom_x_str = bottom_x.to_string();
    let bottom_y_str = bottom_y.to_string();

    debug!(area);
    debug!(&upper_x_str);
    debug!(&upper_y_str);
    debug!(&bottom_x_str);
    debug!(&bottom_y_str);
    let mut embed_grid = EmbedGrid::new(grid, stdin);
    embed_grid.set_terminal_size((79, 39));
    let mut bytes_iter = pty_fd.bytes();
    let mut prev_char = b'\0';
    debug!("waiting for bytes");
    while let Some(Ok(byte)) = bytes_iter.next() {
        debug!("got byte {}", byte as char);
        debug!(
            "{}{} byte is {} and state is {:?}",
            prev_char as char, byte as char, byte as char, &embed_grid.state
        );
        prev_char = byte;
        embed_grid.process_byte(byte);
    }
}
