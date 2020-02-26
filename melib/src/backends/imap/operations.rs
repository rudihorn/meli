/*
 * meli - imap module.
 *
 * Copyright 2017 - 2019 Manos Pitsidianakis
 *
 * This file is part of meli.
 *
 * meli is free software: you can redistribute it and/or modify
 * it under the terms of the GNU General Public License as published by
 * the Free Software Foundation, either version 3 of the License, or
 * (at your option) any later version.
 *
 * meli is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 * GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License
 * along with meli. If not, see <http://www.gnu.org/licenses/>.
 */

use super::*;

use crate::backends::BackendOp;
use crate::email::*;
use crate::error::{MeliError, Result};
use std::cell::Cell;
use std::sync::{Arc, Mutex};

/// `BackendOp` implementor for Imap
#[derive(Debug, Clone)]
pub struct ImapOp {
    uid: usize,
    bytes: Option<String>,
    headers: Option<String>,
    body: Option<String>,
    mailbox_path: String,
    flags: Cell<Option<Flag>>,
    connection: Arc<Mutex<ImapConnection>>,
    uid_store: Arc<UIDStore>,
    tag_index: Arc<RwLock<BTreeMap<u64, String>>>,
}

impl ImapOp {
    pub fn new(
        uid: usize,
        mailbox_path: String,
        connection: Arc<Mutex<ImapConnection>>,
        uid_store: Arc<UIDStore>,
        tag_index: Arc<RwLock<BTreeMap<u64, String>>>,
    ) -> Self {
        ImapOp {
            uid,
            connection,
            bytes: None,
            headers: None,
            body: None,
            mailbox_path,
            flags: Cell::new(None),
            uid_store,
            tag_index,
        }
    }
}

impl BackendOp for ImapOp {
    fn description(&self) -> String {
        unimplemented!();
    }

    fn as_bytes(&mut self) -> Result<&[u8]> {
        if self.bytes.is_none() {
            let mut bytes_cache = self.uid_store.byte_cache.lock()?;
            let cache = bytes_cache.entry(self.uid).or_default();
            if cache.bytes.is_some() {
                self.bytes = cache.bytes.clone();
            } else {
                let mut response = String::with_capacity(8 * 1024);
                {
                    let mut conn = self.connection.lock().unwrap();
                    conn.send_command(format!("SELECT \"{}\"", &self.mailbox_path,).as_bytes())?;
                    conn.read_response(&mut response)?;
                    conn.send_command(format!("UID FETCH {} (FLAGS RFC822)", self.uid).as_bytes())?;
                    conn.read_response(&mut response)?;
                }
                debug!(
                    "fetch response is {} bytes and {} lines",
                    response.len(),
                    response.lines().collect::<Vec<&str>>().len()
                );
                let UidFetchResponse {
                    uid, flags, body, ..
                } = protocol_parser::uid_fetch_response(&response)?.1;
                assert_eq!(uid, self.uid);
                assert!(body.is_some());
                if let Some((flags, _)) = flags {
                    self.flags.set(Some(flags));
                    cache.flags = Some(flags);
                }
                cache.bytes =
                    Some(unsafe { std::str::from_utf8_unchecked(body.unwrap()).to_string() });
                self.bytes = cache.bytes.clone();
            }
        }
        Ok(self.bytes.as_ref().unwrap().as_bytes())
    }

    fn fetch_flags(&self) -> Flag {
        if self.flags.get().is_some() {
            return self.flags.get().unwrap();
        }
        let mut bytes_cache = self.uid_store.byte_cache.lock().unwrap();
        let cache = bytes_cache.entry(self.uid).or_default();
        if cache.flags.is_some() {
            self.flags.set(cache.flags);
        } else {
            let mut response = String::with_capacity(8 * 1024);
            let mut conn = self.connection.lock().unwrap();
            conn.send_command(format!("EXAMINE \"{}\"", &self.mailbox_path,).as_bytes())
                .unwrap();
            conn.read_response(&mut response).unwrap();
            conn.send_command(format!("UID FETCH {} FLAGS", self.uid).as_bytes())
                .unwrap();
            conn.read_response(&mut response).unwrap();
            debug!(
                "fetch response is {} bytes and {} lines",
                response.len(),
                response.lines().collect::<Vec<&str>>().len()
            );
            match protocol_parser::uid_fetch_flags_response(response.as_bytes())
                .to_full_result()
                .map_err(MeliError::from)
            {
                Ok(v) => {
                    if v.len() != 1 {
                        debug!("responses len is {}", v.len());
                        /* TODO: Trigger cache invalidation here. */
                        panic!(format!("message with UID {} was not found", self.uid));
                    }
                    let (uid, (flags, _)) = v[0];
                    assert_eq!(uid, self.uid);
                    cache.flags = Some(flags);
                    self.flags.set(Some(flags));
                }
                Err(e) => Err(e).unwrap(),
            }
        }
        self.flags.get().unwrap()
    }

    fn set_flag(&mut self, _envelope: &mut Envelope, f: Flag, value: bool) -> Result<()> {
        let mut flags = self.fetch_flags();
        flags.set(f, value);

        let mut response = String::with_capacity(8 * 1024);
        let mut conn = self.connection.lock().unwrap();
        conn.send_command(format!("SELECT \"{}\"", &self.mailbox_path,).as_bytes())?;
        conn.read_response(&mut response)?;
        debug!(&response);
        conn.send_command(
            format!(
                "UID STORE {} FLAGS.SILENT ({})",
                self.uid,
                flags_to_imap_list!(flags)
            )
            .as_bytes(),
        )?;
        conn.read_response(&mut response)?;
        debug!(&response);
        match protocol_parser::uid_fetch_flags_response(response.as_bytes())
            .to_full_result()
            .map_err(MeliError::from)
        {
            Ok(v) => {
                if v.len() == 1 {
                    debug!("responses len is {}", v.len());
                    let (uid, (flags, _)) = v[0];
                    assert_eq!(uid, self.uid);
                    self.flags.set(Some(flags));
                }
            }
            Err(e) => Err(e).unwrap(),
        }
        let mut bytes_cache = self.uid_store.byte_cache.lock()?;
        let cache = bytes_cache.entry(self.uid).or_default();
        cache.flags = Some(flags);
        Ok(())
    }

    fn set_tag(&mut self, envelope: &mut Envelope, tag: String, value: bool) -> Result<()> {
        let mut response = String::with_capacity(8 * 1024);
        let mut conn = self.connection.lock().unwrap();
        conn.send_command(format!("SELECT \"{}\"", &self.mailbox_path,).as_bytes())?;
        conn.read_response(&mut response)?;
        conn.send_command(
            format!(
                "UID STORE {} {}FLAGS.SILENT ({})",
                self.uid,
                if value { "+" } else { "-" },
                &tag
            )
            .as_bytes(),
        )?;
        conn.read_response(&mut response)?;
        protocol_parser::uid_fetch_flags_response(response.as_bytes())
            .to_full_result()
            .map_err(MeliError::from)?;
        let hash = tag_hash!(tag);
        if value {
            self.tag_index.write().unwrap().insert(hash, tag);
        } else {
            self.tag_index.write().unwrap().remove(&hash);
        }
        if !envelope.labels().iter().any(|&h_| h_ == hash) {
            if value {
                envelope.labels_mut().push(hash);
            }
        }
        if !value {
            if let Some(pos) = envelope.labels().iter().position(|&h_| h_ == hash) {
                envelope.labels_mut().remove(pos);
            }
        }
        Ok(())
    }
}
