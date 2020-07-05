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
use crate::backends::*;
use crate::email::*;
use crate::error::{MeliError, Result};
use std::sync::{Arc, Mutex};

/// `BackendOp` implementor for Imap
#[derive(Debug, Clone)]
pub struct ImapOp {
    uid: usize,
    mailbox_hash: MailboxHash,
    connection: Arc<Mutex<ImapConnection>>,
    uid_store: Arc<UIDStore>,
}

impl ImapOp {
    pub fn new(
        uid: usize,
        mailbox_hash: MailboxHash,
        connection: Arc<Mutex<ImapConnection>>,
        uid_store: Arc<UIDStore>,
    ) -> Self {
        ImapOp {
            uid,
            connection,
            mailbox_hash,
            uid_store,
        }
    }
}

impl BackendOp for ImapOp {
    fn as_bytes(&mut self) -> ResultFuture<Vec<u8>> {
        let mut bytes_cache = self.uid_store.byte_cache.lock()?;
        let cache = bytes_cache.entry(self.uid).or_default();
        if cache.bytes.is_none() {
            let mut response = String::with_capacity(8 * 1024);
            {
                let mut conn = try_lock(&self.connection, Some(std::time::Duration::new(2, 0)))?;
                conn.examine_mailbox(self.mailbox_hash, &mut response)?;
                conn.send_command(format!("UID FETCH {} (FLAGS RFC822)", self.uid).as_bytes())?;
                conn.read_response(&mut response, RequiredResponses::FETCH_REQUIRED)?;
            }
            debug!(
                "fetch response is {} bytes and {} lines",
                response.len(),
                response.lines().count()
            );
            let UidFetchResponse {
                uid, flags, body, ..
            } = protocol_parser::uid_fetch_response(&response)?.1;
            assert_eq!(uid, self.uid);
            assert!(body.is_some());
            let mut bytes_cache = self.uid_store.byte_cache.lock()?;
            let cache = bytes_cache.entry(self.uid).or_default();
            if let Some((flags, _)) = flags {
                cache.flags = Some(flags);
            }
            cache.bytes = Some(unsafe { std::str::from_utf8_unchecked(body.unwrap()).to_string() });
        }
        let ret = cache.bytes.clone().unwrap().into_bytes();
        Ok(Box::pin(async move { Ok(ret) }))
    }

    fn fetch_flags(&self) -> ResultFuture<Flag> {
        let connection = self.connection.clone();
        let mailbox_hash = self.mailbox_hash;
        let uid = self.uid;
        let uid_store = self.uid_store.clone();

        let mut response = String::with_capacity(8 * 1024);
        Ok(Box::pin(async move {
            let exists_in_cache = {
                let mut bytes_cache = uid_store.byte_cache.lock()?;
                let cache = bytes_cache.entry(uid).or_default();
                cache.flags.is_some()
            };
            if !exists_in_cache {
                let mut conn = try_lock(&connection, Some(std::time::Duration::new(2, 0)))?;
                conn.examine_mailbox(mailbox_hash, &mut response, false)?;
                conn.send_command(format!("UID FETCH {} FLAGS", uid).as_bytes())?;
                conn.read_response(&mut response, RequiredResponses::FETCH_REQUIRED)?;
                debug!(
                    "fetch response is {} bytes and {} lines",
                    response.len(),
                    response.lines().count()
                );
                let v = protocol_parser::uid_fetch_flags_response(response.as_bytes())
                    .map(|(_, v)| v)
                    .map_err(MeliError::from)?;
                if v.len() != 1 {
                    debug!("responses len is {}", v.len());
                    debug!(&response);
                    /* TODO: Trigger cache invalidation here. */
                    debug!(format!("message with UID {} was not found", uid));
                    return Err(MeliError::new(format!(
                        "Invalid/unexpected response: {:?}",
                        response
                    ))
                    .set_summary(format!("message with UID {} was not found?", uid)));
                }
                let (_uid, (_flags, _)) = v[0];
                assert_eq!(_uid, uid);
                let mut bytes_cache = uid_store.byte_cache.lock()?;
                let cache = bytes_cache.entry(uid).or_default();
                cache.flags = Some(_flags);
            }
            {
                let val = {
                    let mut bytes_cache = uid_store.byte_cache.lock()?;
                    let cache = bytes_cache.entry(uid).or_default();
                    cache.flags
                };
                Ok(val.unwrap())
            }
        }))
    }

    fn set_flag(
        &mut self,
        f: Flag,
        value: bool,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>> + Send>>> {
        let flags = self.fetch_flags()?;
        let connection = self.connection.clone();
        let mailbox_hash = self.mailbox_hash;
        let uid = self.uid;
        let uid_store = self.uid_store.clone();

        let mut response = String::with_capacity(8 * 1024);
        Ok(Box::pin(async move {
            let mut flags = flags.await?;
            flags.set(f, value);
            let mut conn = try_lock(&connection, Some(std::time::Duration::new(2, 0)))?;
            conn.select_mailbox(mailbox_hash, &mut response, false)?;
            debug!(&response);
            conn.send_command(
                format!(
                    "UID STORE {} FLAGS.SILENT ({})",
                    uid,
                    flags_to_imap_list!(flags)
                )
                .as_bytes(),
            )?;
            conn.read_response(&mut response, RequiredResponses::STORE_REQUIRED)?;
            debug!(&response);
            match protocol_parser::uid_fetch_flags_response(response.as_bytes())
                .map(|(_, v)| v)
                .map_err(MeliError::from)
            {
                Ok(v) => {
                    if v.len() == 1 {
                        debug!("responses len is {}", v.len());
                        let (uid, (_flags, _)) = v[0];
                        assert_eq!(uid, uid);
                    }
                }
                Err(e) => {
                    return Err(e);
                }
            }
            let mut bytes_cache = uid_store.byte_cache.lock()?;
            let cache = bytes_cache.entry(uid).or_default();
            cache.flags = Some(flags);
            Ok(())
        }))
    }

    fn set_tag(
        &mut self,
        tag: String,
        value: bool,
    ) -> Result<Pin<Box<dyn Future<Output = Result<()>> + Send>>> {
        let mut response = String::with_capacity(8 * 1024);
        let connection = self.connection.clone();
        let mailbox_hash = self.mailbox_hash;
        let uid = self.uid;
        let uid_store = self.uid_store.clone();

        Ok(Box::pin(async move {
            let mut conn = try_lock(&connection, Some(std::time::Duration::new(2, 0)))?;
            conn.select_mailbox(mailbox_hash, &mut response, false)?;
            conn.send_command(
                format!(
                    "UID STORE {} {}FLAGS.SILENT ({})",
                    uid,
                    if value { "+" } else { "-" },
                    &tag
                )
                .as_bytes(),
            )?;
            conn.read_response(&mut response, RequiredResponses::STORE_REQUIRED)?;
            protocol_parser::uid_fetch_flags_response(response.as_bytes())
                .map(|(_, v)| v)
                .map_err(MeliError::from)?;
            let hash = tag_hash!(tag);
            if value {
                uid_store.tag_index.write().unwrap().insert(hash, tag);
            } else {
                uid_store.tag_index.write().unwrap().remove(&hash);
            }
            Ok(())
        }))
    }
}
