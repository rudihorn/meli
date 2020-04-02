/*
 * meli - jmap module.
 *
 * Copyright 2019 Manos Pitsidianakis
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

use crate::async_workers::{Async, AsyncBuilder, AsyncStatus, WorkContext};
use crate::backends::BackendOp;
use crate::backends::MailboxHash;
use crate::backends::{BackendMailbox, MailBackend, Mailbox, RefreshEventConsumer};
use crate::conf::AccountSettings;
use crate::email::*;
use crate::error::{MeliError, Result};
use fnv::FnvHashMap;
use reqwest::blocking::Client;
use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

#[macro_export]
macro_rules! _impl {
        ($(#[$outer:meta])*$field:ident : $t:ty) => {
            $(#[$outer])*
            pub fn $field(mut self, new_val: $t) -> Self {
                self.$field = new_val;
                self
            }
        };
        (get_mut $(#[$outer:meta])*$method:ident, $field:ident : $t:ty) => {
            $(#[$outer])*
            pub fn $method(&mut self) -> &mut $t {
                &mut self.$field
            }
        };
        (get $(#[$outer:meta])*$method:ident, $field:ident : $t:ty) => {
            $(#[$outer])*
            pub fn $method(&self) -> &$t {
                &self.$field
            }
        }
    }

pub mod operations;
use operations::*;

pub mod connection;
use connection::*;

pub mod protocol;
use protocol::*;

pub mod rfc8620;
use rfc8620::*;

pub mod objects;
use objects::*;

pub mod mailbox;
use mailbox::*;

pub mod eventsource;

#[derive(Debug, Default)]
pub struct EnvelopeCache {
    bytes: Option<String>,
    headers: Option<String>,
    body: Option<String>,
    flags: Option<Flag>,
}

#[derive(Debug, Clone)]
pub struct JmapServerConf {
    pub server_hostname: String,
    pub server_username: String,
    pub server_password: String,
    pub server_port: u16,
    pub danger_accept_invalid_certs: bool,
}

macro_rules! get_conf_val {
    ($s:ident[$var:literal]) => {
        $s.extra.get($var).ok_or_else(|| {
            MeliError::new(format!(
                "Configuration error ({}): JMAP connection requires the field `{}` set",
                $s.name.as_str(),
                $var
            ))
        })
    };
    ($s:ident[$var:literal], $default:expr) => {
        $s.extra
            .get($var)
            .map(|v| {
                <_>::from_str(v).map_err(|e| {
                    MeliError::new(format!(
                        "Configuration error ({}): Invalid value for field `{}`: {}\n{}",
                        $s.name.as_str(),
                        $var,
                        v,
                        e
                    ))
                })
            })
            .unwrap_or_else(|| Ok($default))
    };
}

impl JmapServerConf {
    pub fn new(s: &AccountSettings) -> Result<Self> {
        Ok(JmapServerConf {
            server_hostname: get_conf_val!(s["server_hostname"])?.to_string(),
            server_username: get_conf_val!(s["server_username"])?.to_string(),
            server_password: get_conf_val!(s["server_password"])?.to_string(),
            server_port: get_conf_val!(s["server_port"], 443)?,
            danger_accept_invalid_certs: get_conf_val!(s["danger_accept_invalid_certs"], false)?,
        })
    }
}

struct IsSubscribedFn(Box<dyn Fn(&str) -> bool + Send + Sync>);

impl std::fmt::Debug for IsSubscribedFn {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "IsSubscribedFn Box")
    }
}

impl std::ops::Deref for IsSubscribedFn {
    type Target = Box<dyn Fn(&str) -> bool + Send + Sync>;
    fn deref(&self) -> &Box<dyn Fn(&str) -> bool + Send + Sync> {
        &self.0
    }
}
macro_rules! get_conf_val {
    ($s:ident[$var:literal]) => {
        $s.extra.get($var).ok_or_else(|| {
            MeliError::new(format!(
                "Configuration error ({}): JMAP connection requires the field `{}` set",
                $s.name.as_str(),
                $var
            ))
        })
    };
    ($s:ident[$var:literal], $default:expr) => {
        $s.extra
            .get($var)
            .map(|v| {
                <_>::from_str(v).map_err(|e| {
                    MeliError::new(format!(
                        "Configuration error ({}): Invalid value for field `{}`: {}\n{}",
                        $s.name.as_str(),
                        $var,
                        v,
                        e
                    ))
                })
            })
            .unwrap_or_else(|| Ok($default))
    };
}

#[derive(Debug, Default)]
pub struct Store {
    byte_cache: FnvHashMap<EnvelopeHash, EnvelopeCache>,
    id_store: FnvHashMap<EnvelopeHash, Id>,
    blob_id_store: FnvHashMap<EnvelopeHash, Id>,
}

#[derive(Debug)]
pub struct JmapType {
    account_name: String,
    online: Arc<Mutex<(Instant, Result<()>)>>,
    is_subscribed: Arc<IsSubscribedFn>,
    server_conf: JmapServerConf,
    connection: Arc<JmapConnection>,
    store: Arc<RwLock<Store>>,
    tag_index: Arc<RwLock<BTreeMap<u64, String>>>,
    mailboxes: Arc<RwLock<FnvHashMap<MailboxHash, JmapMailbox>>>,
}

impl MailBackend for JmapType {
    fn is_online(&self) -> Result<()> {
        self.online.lock().unwrap().1.clone()
    }

    fn connect(&mut self) {
        if self.is_online().is_err() {
            if Instant::now().duration_since(self.online.lock().unwrap().0)
                >= std::time::Duration::new(2, 0)
            {
                let _ = self.mailboxes();
            }
        }
    }

    fn get(&mut self, mailbox: &Mailbox) -> Async<Result<Vec<Envelope>>> {
        let mut w = AsyncBuilder::new();
        let mailboxes = self.mailboxes.clone();
        let store = self.store.clone();
        let tag_index = self.tag_index.clone();
        let connection = self.connection.clone();
        let mailbox_hash = mailbox.hash();
        let handle = {
            let tx = w.tx();
            let closure = move |_work_context| {
                tx.send(AsyncStatus::Payload(protocol::get(
                    &connection,
                    &store,
                    &tag_index,
                    &mailboxes.read().unwrap()[&mailbox_hash],
                )))
                .unwrap();
                tx.send(AsyncStatus::Finished).unwrap();
            };
            Box::new(closure)
        };
        w.build(handle)
    }

    fn watch(
        &self,
        _sender: RefreshEventConsumer,
        _work_context: WorkContext,
    ) -> Result<std::thread::ThreadId> {
        Err(MeliError::from("JMAP watch for updates is unimplemented"))
    }

    fn mailboxes(&self) -> Result<FnvHashMap<MailboxHash, Mailbox>> {
        if self.mailboxes.read().unwrap().is_empty() {
            let mailboxes = std::dbg!(protocol::get_mailboxes(&self.connection))?;
            *self.mailboxes.write().unwrap() = mailboxes;
        }

        Ok(self
            .mailboxes
            .read()
            .unwrap()
            .iter()
            .filter(|(_, f)| f.is_subscribed)
            .map(|(&h, f)| (h, BackendMailbox::clone(f) as Mailbox))
            .collect())
    }

    fn operation(&self, hash: EnvelopeHash) -> Box<dyn BackendOp> {
        Box::new(JmapOp::new(
            hash,
            self.connection.clone(),
            self.store.clone(),
        ))
    }

    fn save(&self, _bytes: &[u8], _mailbox: &str, _flags: Option<Flag>) -> Result<()> {
        Ok(())
    }

    fn as_any(&self) -> &dyn::std::any::Any {
        self
    }

    fn tags(&self) -> Option<Arc<RwLock<BTreeMap<u64, String>>>> {
        Some(self.tag_index.clone())
    }
}

impl JmapType {
    pub fn new(
        s: &AccountSettings,
        is_subscribed: Box<dyn Fn(&str) -> bool + Send + Sync>,
    ) -> Result<Box<dyn MailBackend>> {
        let online = Arc::new(Mutex::new((
            std::time::Instant::now(),
            Err(MeliError::new("Account is uninitialised.")),
        )));
        let server_conf = JmapServerConf::new(s)?;

        Ok(Box::new(JmapType {
            connection: Arc::new(JmapConnection::new(&server_conf, online.clone())?),
            store: Arc::new(RwLock::new(Store::default())),
            tag_index: Arc::new(RwLock::new(Default::default())),
            mailboxes: Arc::new(RwLock::new(FnvHashMap::default())),
            account_name: s.name.clone(),
            online,
            is_subscribed: Arc::new(IsSubscribedFn(is_subscribed)),
            server_conf,
        }))
    }

    pub fn validate_config(s: &AccountSettings) -> Result<()> {
        get_conf_val!(s["server_hostname"])?;
        get_conf_val!(s["server_username"])?;
        get_conf_val!(s["server_password"])?;
        get_conf_val!(s["server_port"], 443)?;
        get_conf_val!(s["danger_accept_invalid_certs"], false)?;
        Ok(())
    }
}
