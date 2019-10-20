use super::*;
use crate::terminal::cells::{Cell, CellBuffer};
use std::sync::{Arc, Mutex};

pub struct EmbedGrid {
    cursor: (usize, usize),
    terminal_size: (usize, usize),
    grid: Arc<Mutex<CellBuffer>>,
    pub state: State,
    stdin: std::fs::File,
}

impl EmbedGrid {
    pub fn new(grid: Arc<Mutex<CellBuffer>>, stdin: std::fs::File) -> Self {
        EmbedGrid {
            cursor: (1, 1),
            terminal_size: (0, 0),
            grid,
            state: State::Normal,
            stdin,
        }
    }

    pub fn set_terminal_size(&mut self, new_val: (usize, usize)) {
        self.terminal_size = new_val;
    }

    pub fn process_byte(&mut self, byte: u8) {
        let EmbedGrid {
            ref mut cursor,
            ref terminal_size,
            ref mut grid,
            ref mut state,
            ref mut stdin,
        } = self;

        macro_rules! increase_cursor_x {
            () => {
                if *cursor == *terminal_size {
                    /* do nothing */
                } else if cursor.0 == terminal_size.0 {
                    cursor.0 = 0;
                    cursor.1 += 1;
                } else {
                    cursor.0 += 1;
                }
            };
        }

        let mut state = state;
        match (byte, &mut state) {
            (b'\x1b', State::Normal) => {
                *state = State::ExpectingControlChar;
            }
            (b']', State::ExpectingControlChar) => {
                let buf1 = Vec::new();
                *state = State::Osc1(buf1);
            }
            (b'[', State::ExpectingControlChar) => {
                *state = State::Csi;
            }
            (b'(', State::ExpectingControlChar) => {
                *state = State::G0;
            }
            (c, State::ExpectingControlChar) => {
                debug!(
                    "unrecognised: byte is {} and state is {:?}",
                    byte as char, state
                );
                *state = State::Normal;
            }
            (b'?', State::Csi) => {
                let buf1 = Vec::new();
                *state = State::CsiQ(buf1);
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
                let buf2 = Vec::new();
                *state = State::Osc2(buf1, buf2);
            }
            (c, State::Osc2(_, ref mut buf)) if (c >= b'0' && c <= b'9') || c == b'?' => {
                buf.push(c);
            }
            (c, State::Osc1(_)) => {
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (c, State::Osc2(_, _)) => {
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            /* END OF OSC */
            /* ********** */
            /* ********** */
            /* ********** */
            /* ********** */
            (c, State::Normal) => {
                grid.lock().unwrap()[*cursor].set_ch(c as char);
                debug!("setting cell {:?} char '{}'", cursor, c as char);
                increase_cursor_x!();
            }
            (b'u', State::Csi) => {
                /* restore cursor */
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b'm', State::Csi) => {
                /* Character Attributes (SGR).  Ps = 0  -> Normal (default), VT100 */
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b'H', State::Csi) => {
                /*  move cursor to (1,1) */

                debug!("sending {}", EscCode::from((&(*state), byte)),);
                debug!("move cursor to (1,1) cursor before: {:?}", *cursor);
                *cursor = (0, 0);
                debug!("cursor after: {:?}", *cursor);
                *state = State::Normal;
            }
            /* CSI ? stuff */
            (c, State::CsiQ(ref mut buf)) if c >= b'0' && c <= b'9' => {
                buf.push(c);
            }
            (c, State::CsiQ(ref mut buf)) => {
                // we are already in AlternativeScreen so do not forward this
                if &buf.as_slice() != &SWITCHALTERNATIVE_1049 {
                    debug!("sending {}", EscCode::from((&(*state), byte)));
                }
                *state = State::Normal;
            }
            /* END OF CSI ? stuff */
            /* ******************* */
            /* ******************* */
            /* ******************* */
            (c, State::Csi) if c >= b'0' && c <= b'9' => {
                let mut buf1 = Vec::new();
                buf1.push(c);
                *state = State::Csi1(buf1);
            }
            (b'J', State::Csi) => {
                // "ESC[J\t\tCSI Erase from the cursor to the end of the screen [BAD]"
                debug!("sending {}", EscCode::from((&(*state), byte)));
                let mut grid = grid.lock().unwrap();
                debug!("erasing from {:?} to {:?}", cursor, terminal_size);
                for y in cursor.1..terminal_size.1 {
                    for x in cursor.0..terminal_size.0 {
                        cursor.0 = x;
                        grid[(x, y)] = Cell::default();
                    }
                    cursor.1 = y;
                }
                *state = State::Normal;
            }
            (b'K', State::Csi) => {
                // "ESC[K\t\tCSI Erase from the cursor to the end of the line [BAD]"
                debug!("sending {}", EscCode::from((&(*state), byte)));
                let mut grid = grid.lock().unwrap();
                for x in cursor.0..terminal_size.0 {
                    grid[(x, terminal_size.1)] = Cell::default();
                }
                *state = State::Normal;
            }
            (c, State::Csi) => {
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b'K', State::Csi1(_)) => {
                /* Erase in Display (ED), VT100.*/
                debug!("not sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b'J', State::Csi1(_)) => {
                /* Erase in Display (ED), VT100.*/
                debug!("not sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b't', State::Csi1(buf)) => {
                /* Window manipulation, skip it */
                if buf == b"18" {
                    // P s = 1 8 → Report the size of the text area in characters as CSI 8 ; height ; width t
                    stdin.write_all(&[b'\x1b', b'[', b'8', b';']).unwrap();
                    stdin
                        .write_all((terminal_size.0 + 1).to_string().as_bytes())
                        .unwrap();
                    stdin.write_all(&[b';']).unwrap();
                    stdin
                        .write_all((terminal_size.1 + 1).to_string().as_bytes())
                        .unwrap();
                    stdin.write_all(&[b't']).unwrap();
                }
                debug!("not sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b'n', State::Csi1(_)) => {
                /* report cursor position */
                debug!("got {}", EscCode::from((&(*state), byte)));
                stdin.write_all(&[b'\x1b', b'[']).unwrap();
                //    Ps = 6  ⇒  Report Cursor Position (CPR) [row;column].
                //Result is CSI r ; c R
                stdin
                    .write_all((cursor.0 + 1).to_string().as_bytes())
                    .unwrap();
                stdin.write_all(&[b';']).unwrap();
                stdin
                    .write_all((cursor.1 + 1).to_string().as_bytes())
                    .unwrap();
                stdin.write_all(&[b'R']).unwrap();
                *state = State::Normal;
            }
            (b'B', State::Csi1(buf)) => {
                //"ESC[{buf}B\t\tCSI Cursor Down {buf} Times",
                let offset = unsafe { std::str::from_utf8_unchecked(buf) }
                    .parse::<usize>()
                    .unwrap();
                debug!("cursor down {} times, cursor was: {:?}", offset, cursor);
                if offset + cursor.1 < terminal_size.1 {
                    cursor.1 += offset;
                }
                debug!("cursor became: {:?}", cursor);
                *state = State::Normal;
            }
            (b'C', State::Csi1(buf)) => {
                // "ESC[{buf}C\t\tCSI Cursor Forward {buf} Times",
                let offset = unsafe { std::str::from_utf8_unchecked(buf) }
                    .parse::<usize>()
                    .unwrap();
                debug!("cursor forward {} times, cursor was: {:?}", offset, cursor);
                if offset + cursor.0 < terminal_size.0 {
                    cursor.0 += offset;
                }
                debug!("cursor became: {:?}", cursor);
                *state = State::Normal;
            }
            (b'D', State::Csi1(buf)) => {
                // "ESC[{buf}D\t\tCSI Cursor Backward {buf} Times",
                let offset = unsafe { std::str::from_utf8_unchecked(buf) }
                    .parse::<usize>()
                    .unwrap();
                debug!("cursor backward {} times, cursor was: {:?}", offset, cursor);
                if offset + cursor.0 < terminal_size.0 {
                    cursor.0 += offset;
                }
                debug!("cursor became: {:?}", cursor);
                *state = State::Normal;
            }
            (b'E', State::Csi1(buf)) => {
                //"ESC[{buf}E\t\tCSI Cursor Next Line {buf} Times",
                let offset = unsafe { std::str::from_utf8_unchecked(buf) }
                    .parse::<usize>()
                    .unwrap();
                debug!(
                    "cursor next line {} times, cursor was: {:?}",
                    offset, cursor
                );
                if offset + cursor.1 < terminal_size.1 {
                    cursor.1 += offset;
                    cursor.0 = 0;
                }
                debug!("cursor became: {:?}", cursor);
                *state = State::Normal;
            }
            (b'G', State::Csi1(buf)) => {
                // "ESC[{buf}G\t\tCursor Character Absolute  [column={buf}] (default = [row,1])",
                let new_col = unsafe { std::str::from_utf8_unchecked(buf) }
                    .parse::<usize>()
                    .unwrap();
                debug!("cursor absolute {}, cursor was: {:?}", new_col, cursor);
                if new_col < terminal_size.0 {
                    cursor.0 = new_col;
                }
                debug!("cursor became: {:?}", cursor);
                *state = State::Normal;
            }
            (b'C', State::Csi1(buf)) => {
                // "ESC[{buf}F\t\tCSI Cursor Preceding Line {buf} Times",
                let offset = unsafe { std::str::from_utf8_unchecked(buf) }
                    .parse::<usize>()
                    .unwrap();
                debug!(
                    "cursor preceding {} times, cursor was: {:?}",
                    offset, cursor
                );
                if cursor.1 < offset + terminal_size.1 {
                    cursor.1 -= offset;
                    cursor.0 = 0;
                }
                debug!("cursor became: {:?}", cursor);
                *state = State::Normal;
            }
            (b';', State::Csi1(ref mut buf1_p)) => {
                let buf1 = std::mem::replace(buf1_p, Vec::new());
                let buf2 = Vec::new();
                *state = State::Csi2(buf1, buf2);
            }
            (c, State::Csi1(ref mut buf)) if (c >= b'0' && c <= b'9') || c == b' ' => {
                buf.push(c);
            }
            (c, State::Csi1(ref buf)) => {
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b';', State::Csi2(ref mut buf1_p, ref mut buf2_p)) => {
                let buf1 = std::mem::replace(buf1_p, Vec::new());
                let buf2 = std::mem::replace(buf2_p, Vec::new());
                let buf3 = Vec::new();
                *state = State::Csi3(buf1, buf2, buf3);
            }
            (b'n', State::Csi2(_, _)) => {
                // Report Cursor Position, skip it
                *state = State::Normal;
            }
            (b't', State::Csi2(_, _)) => {
                // Window manipulation, skip it
                *state = State::Normal;
            }
            (b'H', State::Csi2(ref x, ref y)) => {
                //Cursor Position [row;column] (default = [1,1]) (CUP).
                let orig_x = unsafe { std::str::from_utf8_unchecked(x) }
                    .parse::<usize>()
                    .unwrap();
                let orig_y = unsafe { std::str::from_utf8_unchecked(y) }
                    .parse::<usize>()
                    .unwrap();
                debug!("sending {}", EscCode::from((&(*state), byte)),);
                debug!(
                    "cursor set to ({},{}), cursor was: {:?}",
                    orig_x, orig_y, cursor
                );
                if orig_x - 1 <= terminal_size.0 && orig_y - 1 <= terminal_size.1 {
                    cursor.0 = orig_x - 1;
                    cursor.1 = orig_y - 1;
                }
                debug!("cursor became: {:?}", cursor);
                *state = State::Normal;
            }
            (c, State::Csi2(_, ref mut buf)) if c >= b'0' && c <= b'9' => {
                buf.push(c);
            }
            (c, State::Csi2(ref buf1, ref buf2)) => {
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b't', State::Csi3(_, _, _)) => {
                // Window manipulation, skip it
                *state = State::Normal;
            }

            (c, State::Csi3(_, _, ref mut buf)) if c >= b'0' && c <= b'9' => {
                buf.push(c);
            }
            (c, State::Csi3(ref buf1, ref buf2, ref buf3)) => {
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            /* other stuff */
            /* ******************* */
            /* ******************* */
            /* ******************* */
            (c, State::G0) => {
                debug!("sending {}", EscCode::from((&(*state), byte)));
                *state = State::Normal;
            }
            (b, s) => {
                debug!("unrecognised: byte is {} and state is {:?}", b as char, s);
            }
        }
    }
}
