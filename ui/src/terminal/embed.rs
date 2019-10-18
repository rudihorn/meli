use crate::terminal::position::Area;
use nix::fcntl::{open, OFlag};
use nix::ioctl_write_ptr_bad;
use nix::pty::{grantpt, posix_openpt, ptsname, unlockpt, Winsize};
use nix::sys::stat::Mode;

// ioctl command to set window size of pty:
use libc::TIOCSWINSZ;
use std::path::Path;
use std::process::{Command, Stdio};

use std::io::Read;
use std::io::Write;
use std::os::unix::io::{FromRawFd, IntoRawFd};

ioctl_write_ptr_bad!(set_window_size, TIOCSWINSZ, Winsize);

static SWITCHALTERNATIVE_1049: &'static [u8] = &[b'1', b'0', b'4', b'9'];

pub fn create_pty(area: Area) -> nix::Result<()> {
    // Open a new PTY master
    let master_fd = posix_openpt(OFlag::O_RDWR)?;

    // Allow a slave to be generated for it
    grantpt(&master_fd)?;
    unlockpt(&master_fd)?;

    // Get the name of the slave
    let slave_name = unsafe { ptsname(&master_fd) }?;

    // Try to open the slave
    let _slave_fd = open(Path::new(&slave_name), OFlag::O_RDWR, Mode::empty())?;

    Command::new("vim")
        .stdin(Stdio::inherit())
        .stdout(unsafe { Stdio::from_raw_fd(_slave_fd) })
        .stderr(unsafe { Stdio::from_raw_fd(_slave_fd) })
        .spawn();

    std::thread::Builder::new()
        .spawn(move || {
            let winsize = Winsize {
                ws_row: 20,
                ws_col: 80,
                ws_xpixel: 0,
                ws_ypixel: 0,
            };
            //lock.write(b"\x1b3g").unwrap(); //clear all
            let master_fd = master_fd.into_raw_fd();
            unsafe { set_window_size(master_fd, &winsize).unwrap() };
            let master_file = unsafe { std::fs::File::from_raw_fd(master_fd) };
            forward_pty_translate_escape_codes(master_file, area);
        })
        .unwrap();
    Ok(())
}

#[derive(Debug)]
enum State {
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

fn forward_pty_translate_escape_codes(pty_fd: std::fs::File, area: Area) {
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
    let stdout = std::io::stdout();
    let mut buf1: Vec<u8> = Vec::with_capacity(8);
    let mut buf2: Vec<u8> = Vec::with_capacity(8);
    let mut buf3: Vec<u8> = Vec::with_capacity(8);
    let mut lock = stdout.lock();

    let mut state = State::Normal;
    let mut bytes_iter = pty_fd.bytes();
    macro_rules! cleanup {
        (CSIQ) => {
            if let State::CsiQ(ref mut buf1_p) = state {
                std::mem::swap(buf1_p, &mut buf1);
            }
        };
        (CSI1) => {
            if let State::Csi1(ref mut buf1_p) = state {
                std::mem::swap(buf1_p, &mut buf1);
            }
        };
        (CSI2) => {
            if let State::Csi2(ref mut buf1_p, ref mut buf2_p) = state {
                std::mem::swap(buf1_p, &mut buf1);
                std::mem::swap(buf2_p, &mut buf2);
            }
        };
        (CSI3) => {
            if let State::Csi3(ref mut buf1_p, ref mut buf2_p, ref mut buf3_p) = state {
                std::mem::swap(buf1_p, &mut buf1);
                std::mem::swap(buf2_p, &mut buf2);
                std::mem::swap(buf3_p, &mut buf3);
            }
        };
        (OSC1) => {
            if let State::Osc1(ref mut buf1_p) = state {
                std::mem::swap(buf1_p, &mut buf1);
            }
        };
        (OSC2) => {
            if let State::Osc2(ref mut buf1_p, ref mut buf2_p) = state {
                std::mem::swap(buf1_p, &mut buf1);
                std::mem::swap(buf2_p, &mut buf2);
            }
        };
    }

    macro_rules! restore_global_buf {
        ($b:ident) => {
            let mut $b = std::mem::replace(&mut $b, Vec::new());
            $b.clear();
        };
    }

    let mut prev_char = b'\0';
    while let Some(Ok(byte)) = bytes_iter.next() {
        debug!(
            "{}{} byte is {} and state is {:?}",
            prev_char as char, byte as char, byte as char, &state
        );
        prev_char = byte;
        match (byte, &mut state) {
            (b'\x1b', State::Normal) => {
                state = State::ExpectingControlChar;
            }
            (b']', State::ExpectingControlChar) => {
                restore_global_buf!(buf1);
                state = State::Osc1(buf1);
            }
            (b'[', State::ExpectingControlChar) => {
                state = State::Csi;
            }
            (b'(', State::ExpectingControlChar) => {
                state = State::G0;
            }
            (c, State::ExpectingControlChar) => {
                debug!(
                    "unrecognised: byte is {} and state is {:?}",
                    byte as char, &state
                );
                state = State::Normal;
            }
            (b'?', State::Csi) => {
                restore_global_buf!(buf1);
                state = State::CsiQ(buf1);
            }
            /* ********** */
            /* ********** */
            /* ********** */
            /* OSC stuff */
            (c, State::Osc1(ref mut buf)) if (c >= b'0' && c <= b'9') || c == b'?' => {
                buf.push(c);
            }
            (b';', State::Osc1(ref mut buf1_p)) => {
                let buf1 = std::mem::replace(buf1_p, Vec::new());
                let mut buf2 = std::mem::replace(&mut buf2, Vec::new());
                buf2.clear();
                state = State::Osc2(buf1, buf2);
            }
            (c, State::Osc2(_, ref mut buf)) if (c >= b'0' && c <= b'9') || c == b'?' => {
                buf.push(c);
            }
            (c, State::Osc1(ref buf1)) => {
                lock.write_all(&[b'\x1b', b']']).unwrap();
                lock.write_all(buf1).unwrap();
                lock.write_all(&[c]).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                cleanup!(OSC1);
                state = State::Normal;
            }
            (c, State::Osc2(ref buf1, ref buf2)) => {
                lock.write_all(&[b'\x1b', b']']).unwrap();
                lock.write_all(buf1).unwrap();
                lock.write_all(&[b';']).unwrap();
                lock.write_all(buf2).unwrap();
                lock.write_all(&[c]).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                cleanup!(OSC2);
                state = State::Normal;
            }
            /* END OF OSC */
            /* ********** */
            /* ********** */
            /* ********** */
            /* ********** */
            (c, State::Normal) => {
                lock.write(&[byte]).unwrap();
                lock.flush().unwrap();
            }
            (b'u', State::Csi) => {
                /* restore cursor */
                lock.write_all(&[b'\x1b', b'[', b'u']).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                lock.flush().unwrap();
                state = State::Normal;
            }
            (b'm', State::Csi) => {
                /* Character Attributes (SGR).  Ps = 0  -> Normal (default), VT100 */
                lock.write_all(&[b'\x1b', b'[', b'm']).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                lock.flush().unwrap();
                state = State::Normal;
            }
            (b'H', State::Csi) => {
                /*  move cursor to (1,1) */
                lock.write_all(&[b'\x1b', b'[']).unwrap();
                lock.write_all(upper_x_str.as_bytes()).unwrap();
                lock.write_all(&[b';']).unwrap();
                lock.write_all(upper_y_str.as_bytes()).unwrap();
                lock.write_all(&[b'H']).unwrap();
                debug!(
                    "sending translating {} to ESC[{};{}H",
                    EscCode::from((&state, byte)),
                    upper_x_str,
                    upper_y_str,
                );
                lock.flush().unwrap();
                state = State::Normal;
            }
            /* CSI ? stuff */
            (c, State::CsiQ(ref mut buf)) if c >= b'0' && c <= b'9' => {
                buf.push(c);
            }
            (c, State::CsiQ(ref mut buf)) => {
                // we are already in AlternativeScreen so do not forward this
                if &buf.as_slice() != &SWITCHALTERNATIVE_1049 {
                    lock.write_all(&[b'\x1b', b'[', b'?']).unwrap();
                    lock.write_all(buf).unwrap();
                    lock.write_all(&[c]).unwrap();
                    debug!("sending {}", EscCode::from((&state, byte)));
                }
                cleanup!(CSIQ);
                state = State::Normal;
            }
            /* END OF CSI ? stuff */
            /* ******************* */
            /* ******************* */
            /* ******************* */
            (c, State::Csi) if c >= b'0' && c <= b'9' => {
                let mut buf1 = std::mem::replace(&mut buf1, Vec::new());
                buf1.clear();
                buf1.push(c);
                state = State::Csi1(buf1);
            }
            (c, State::Csi) => {
                lock.write_all(&[b'\x1b', b'[', c]).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                lock.flush().unwrap();
                state = State::Normal;
            }
            (b'K', State::Csi1(_)) => {
                /* Erase in Display (ED), VT100.*/
                cleanup!(CSI1);
                state = State::Normal;
            }
            (b'J', State::Csi1(_)) => {
                /* Erase in Display (ED), VT100.*/
                cleanup!(CSI1);
                state = State::Normal;
            }
            (b't', State::Csi1(_)) => {
                /* Window manipulation, skip it */
                cleanup!(CSI1);
                state = State::Normal;
            }
            (b';', State::Csi1(ref mut buf1_p)) => {
                let buf1 = std::mem::replace(buf1_p, Vec::new());
                let mut buf2 = std::mem::replace(&mut buf2, Vec::new());
                buf2.clear();
                state = State::Csi2(buf1, buf2);
            }
            (c, State::Csi1(ref mut buf)) if (c >= b'0' && c <= b'9') || c == b' ' => {
                buf.push(c);
            }
            (c, State::Csi1(ref buf)) => {
                lock.write_all(&[b'\x1b', b'[']).unwrap();
                lock.write_all(buf).unwrap();
                lock.write_all(&[c]).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                cleanup!(CSI1);
                state = State::Normal;
            }
            (b';', State::Csi2(ref mut buf1_p, ref mut buf2_p)) => {
                let buf1 = std::mem::replace(buf1_p, Vec::new());
                let buf2 = std::mem::replace(buf2_p, Vec::new());
                let mut buf3 = std::mem::replace(&mut buf3, Vec::new());
                buf3.clear();
                state = State::Csi3(buf1, buf2, buf3);
            }
            (b'n', State::Csi2(_, _)) => {
                // Report Cursor Position, skip it
                cleanup!(CSI2);
                state = State::Normal;
            }
            (b't', State::Csi2(_, _)) => {
                // Window manipulation, skip it
                cleanup!(CSI2);
                state = State::Normal;
            }
            (b'H', State::Csi2(ref x, ref y)) => {
                //Cursor Position [row;column] (default = [1,1]) (CUP).
                let orig_x = unsafe { std::str::from_utf8_unchecked(x) }
                    .parse::<usize>()
                    .unwrap();
                let orig_y = unsafe { std::str::from_utf8_unchecked(y) }
                    .parse::<usize>()
                    .unwrap();
                if orig_x + upper_x + 1 > bottom_x || orig_y + upper_y + 1 > bottom_y {
                    debug!(orig_x);
                    debug!(orig_y);
                    debug!(area);
                } else {
                    debug!("orig_x + upper_x = {}", orig_x + upper_x);
                    debug!("orig_y + upper_y = {}", orig_y + upper_y);
                    lock.write_all(&[b'\x1b', b'[']).unwrap();
                    lock.write_all((orig_x + upper_x).to_string().as_bytes())
                        .unwrap();
                    lock.write_all(&[b';']).unwrap();
                    lock.write_all((orig_y + upper_y).to_string().as_bytes())
                        .unwrap();
                    lock.write_all(&[b'H']).unwrap();
                    debug!(
                        "sending translating {} to ESC[{};{}H ",
                        EscCode::from((&state, byte)),
                        orig_x + upper_x,
                        orig_y + upper_y
                    );
                }
                cleanup!(CSI2);
                state = State::Normal;
            }
            (c, State::Csi2(_, ref mut buf)) if c >= b'0' && c <= b'9' => {
                buf.push(c);
            }
            (c, State::Csi2(ref buf1, ref buf2)) => {
                lock.write_all(&[b'\x1b', b'[']).unwrap();
                lock.write_all(buf1).unwrap();
                lock.write_all(&[b';']).unwrap();
                lock.write_all(buf2).unwrap();
                lock.write_all(&[c]).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                cleanup!(CSI2);
                state = State::Normal;
            }
            (b't', State::Csi3(_, _, _)) => {
                // Window manipulation, skip it
                cleanup!(CSI3);
                state = State::Normal;
            }

            (c, State::Csi3(_, _, ref mut buf)) if c >= b'0' && c <= b'9' => {
                buf.push(c);
            }
            (c, State::Csi3(ref buf1, ref buf2, ref buf3)) => {
                lock.write_all(&[b'\x1b', b'[']).unwrap();
                lock.write_all(buf1).unwrap();
                lock.write_all(&[b';']).unwrap();
                lock.write_all(buf2).unwrap();
                lock.write_all(&[b';']).unwrap();
                lock.write_all(buf3).unwrap();
                lock.write_all(&[c]).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                cleanup!(CSI3);
                state = State::Normal;
            }
            /* other stuff */
            /* ******************* */
            /* ******************* */
            /* ******************* */
            (c, State::G0) => {
                lock.write_all(&[b'\x1b', b'(']).unwrap();
                lock.write_all(&[c]).unwrap();
                debug!("sending {}", EscCode::from((&state, byte)));
                state = State::Normal;
            }
            (b, s) => {
                debug!("unrecognised: byte is {} and state is {:?}", b as char, s);
            }
        }
    }
}
