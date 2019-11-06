/*
 * meli - imap module.
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

#[macro_use]
mod protocol_parser;
pub use protocol_parser::{UntaggedResponse::*, *};
mod folder;
pub use folder::*;
mod operations;
pub use operations::*;
mod connection;
pub use connection::*;
mod watch;
pub use watch::*;

use crate::async_workers::{Async, AsyncBuilder, AsyncStatus, WorkContext};
use crate::backends::BackendOp;
use crate::backends::FolderHash;
use crate::backends::RefreshEvent;
use crate::backends::RefreshEventKind::{self, *};
use crate::backends::{BackendFolder, Folder, FolderOperation, MailBackend, RefreshEventConsumer};
use crate::conf::AccountSettings;
use crate::email::*;
use crate::error::{MeliError, Result};
use fnv::{FnvHashMap, FnvHashSet};
use std::collections::hash_map::DefaultHasher;
use std::hash::Hasher;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
pub type UID = usize;

#[derive(Debug, Default)]
pub struct EnvelopeCache {
    bytes: Option<String>,
    headers: Option<String>,
    body: Option<String>,
    flags: Option<Flag>,
}

#[derive(Debug, Clone)]
pub struct ImapServerConf {
    pub server_hostname: String,
    pub server_username: String,
    pub server_password: String,
    pub server_port: u16,
    pub use_starttls: bool,
    pub danger_accept_invalid_certs: bool,
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
type Capabilities = FnvHashSet<Vec<u8>>;
#[derive(Debug)]
pub struct ImapType {
    account_name: String,
    online: Arc<Mutex<bool>>,
    is_subscribed: Arc<IsSubscribedFn>,
    connection: Arc<Mutex<ImapConnection>>,
    server_conf: ImapServerConf,

    folders: Arc<Mutex<FnvHashMap<FolderHash, ImapFolder>>>,
    hash_index: Arc<Mutex<FnvHashMap<EnvelopeHash, (UID, FolderHash)>>>,
    uid_index: Arc<Mutex<FnvHashMap<usize, EnvelopeHash>>>,

    byte_cache: Arc<Mutex<FnvHashMap<UID, EnvelopeCache>>>,
}

impl MailBackend for ImapType {
    fn is_online(&self) -> bool {
        *self.online.lock().unwrap()
    }
    fn get(&mut self, folder: &Folder) -> Async<Result<Vec<Envelope>>> {
        macro_rules! exit_on_error {
            ($tx:expr,$($result:expr)+) => {
                $(if let Err(e) = $result {
                    $tx.send(AsyncStatus::Payload(Err(e.into()))).unwrap();
                    std::process::exit(1);
                })+
            };
        };

        let mut w = AsyncBuilder::new();
        let handle = {
            let tx = w.tx();
            let hash_index = self.hash_index.clone();
            let uid_index = self.uid_index.clone();
            let folder_path = folder.path().to_string();
            let folder_hash = folder.hash();
            let folder_exists = self.folders.lock().unwrap()[&folder_hash].exists.clone();
            let connection = self.connection.clone();
            let closure = move |_work_context| {
                let connection = connection.clone();
                let tx = tx.clone();
                let mut response = String::with_capacity(8 * 1024);
                let conn = connection.lock();
                exit_on_error!(&tx, conn);
                let mut conn = conn.unwrap();
                debug!("locked for get {}", folder_path);

                exit_on_error!(&tx,
                               conn.send_command(format!("EXAMINE {}", folder_path).as_bytes())
                               conn.read_response(&mut response)
                );
                let examine_response = protocol_parser::select_response(&response)
                    .to_full_result()
                    .map_err(MeliError::from);
                exit_on_error!(&tx, examine_response);
                let mut exists: usize = match examine_response.unwrap() {
                    SelectResponse::Ok(ok) => ok.uidnext - 1,
                    SelectResponse::Bad(b) => b.exists,
                };
                {
                    let mut folder_exists = folder_exists.lock().unwrap();
                    *folder_exists = exists;
                }

                while exists > 1 {
                    let mut envelopes = vec![];
                    exit_on_error!(&tx,
                                   conn.send_command(format!("UID FETCH {}:{} (UID FLAGS ENVELOPE)", std::cmp::max(exists.saturating_sub(20000), 1), exists).as_bytes())
                                   conn.read_response(&mut response)
                    );
                    debug!(
                        "fetch response is {} bytes and {} lines",
                        response.len(),
                        response.lines().collect::<Vec<&str>>().len()
                    );
                    match protocol_parser::uid_fetch_envelopes_response(response.as_bytes())
                        .to_full_result()
                        .map_err(MeliError::from)
                    {
                        Ok(v) => {
                            debug!("responses len is {}", v.len());
                            for (uid, flags, mut env) in v {
                                let mut h = DefaultHasher::new();
                                h.write_usize(uid);
                                h.write(folder_path.as_bytes());
                                env.set_hash(h.finish());
                                if let Some(flags) = flags {
                                    env.set_flags(flags);
                                }
                                hash_index
                                    .lock()
                                    .unwrap()
                                    .insert(env.hash(), (uid, folder_hash));
                                uid_index.lock().unwrap().insert(uid, env.hash());
                                envelopes.push(env);
                            }
                        }
                        Err(e) => {
                            debug!(&e);
                            tx.send(AsyncStatus::Payload(Err(e))).unwrap();
                        }
                    }
                    exists = std::cmp::max(exists.saturating_sub(20000), 1);
                    debug!("sending payload");
                    tx.send(AsyncStatus::Payload(Ok(envelopes))).unwrap();
                }
                drop(conn);
                tx.send(AsyncStatus::Finished).unwrap();
            };
            Box::new(closure)
        };
        w.build(handle)
    }

    fn watch(
        &self,
        sender: RefreshEventConsumer,
        work_context: WorkContext,
    ) -> Result<std::thread::ThreadId> {
        let has_idle: bool = self
            .connection
            .lock()
            .unwrap()
            .capabilities
            .contains(&b"IDLE"[0..]);
        let folders = self.folders.clone();
        let conn = ImapConnection::new_connection(&self.server_conf);
        let main_conn = self.connection.clone();
        let is_online = self.online.clone();
        let hash_index = self.hash_index.clone();
        let uid_index = self.uid_index.clone();
        let handle = std::thread::Builder::new()
            .name(format!("{} imap connection", self.account_name.as_str(),))
            .spawn(move || {
                let thread = std::thread::current();
                work_context
                    .set_status
                    .send((thread.id(), "watching".to_string()))
                    .unwrap();
                let kit = ImapWatchKit {
                    conn,
                    is_online,
                    main_conn,
                    hash_index,
                    uid_index,
                    folders,
                    sender,
                    work_context,
                };
                if has_idle {
                    idle(kit);
                } else {
                    poll_with_examine(kit);
                }
            })?;
        Ok(handle.thread().id())
    }

    fn folders(&self) -> FnvHashMap<FolderHash, Folder> {
        let mut folders = self.folders.lock().unwrap();
        if !folders.is_empty() {
            return folders
                .iter()
                .map(|(h, f)| (*h, Box::new(Clone::clone(f)) as Folder))
                .collect();
        }
        *folders = ImapType::imap_folders(&self.connection);
        folders.retain(|_, f| (self.is_subscribed)(f.path()));
        let keys = folders.keys().cloned().collect::<FnvHashSet<FolderHash>>();
        for f in folders.values_mut() {
            f.children.retain(|c| keys.contains(c));
        }
        *self.online.lock().unwrap() = true;
        folders
            .iter()
            .map(|(h, f)| (*h, Box::new(Clone::clone(f)) as Folder))
            .collect()
    }

    fn operation(&self, hash: EnvelopeHash) -> Box<dyn BackendOp> {
        let (uid, folder_hash) = self.hash_index.lock().unwrap()[&hash];
        Box::new(ImapOp::new(
            uid,
            self.folders.lock().unwrap()[&folder_hash]
                .path()
                .to_string(),
            self.connection.clone(),
            self.byte_cache.clone(),
        ))
    }

    fn save(&self, bytes: &[u8], folder: &str, flags: Option<Flag>) -> Result<()> {
        let path = {
            let folders = self.folders.lock().unwrap();

            folders
                .values()
                .find(|v| v.name == folder)
                .map(|v| v.path().to_string())
                .ok_or(MeliError::new(""))?
        };
        let mut response = String::with_capacity(8 * 1024);
        let mut conn = self.connection.lock().unwrap();
        let flags = flags.unwrap_or(Flag::empty());
        conn.send_command(
            format!(
                "APPEND \"{}\" ({}) {{{}}}",
                &path,
                flags_to_imap_list!(flags),
                bytes.len()
            )
            .as_bytes(),
        )?;
        // wait for "+ Ready for literal data" reply
        conn.wait_for_continuation_request()?;
        conn.send_literal(bytes)?;
        conn.read_response(&mut response)?;
        Ok(())
    }

    fn folder_operation(&mut self, path: &str, op: FolderOperation) -> Result<()> {
        use FolderOperation::*;

        match (
            &op,
            self.folders
                .lock()
                .unwrap()
                .values()
                .any(|f| f.path == path),
        ) {
            (Create, true) => {
                return Err(MeliError::new(format!(
                    "Folder named `{}` in account `{}` already exists.",
                    path, self.account_name,
                )));
            }
            (op, false) if *op != Create => {
                return Err(MeliError::new(format!(
                    "No folder named `{}` in account `{}`",
                    path, self.account_name,
                )));
            }
            _ => {}
        }

        let mut response = String::with_capacity(8 * 1024);
        match op {
            Create => {
                let mut conn = self.connection.lock()?;
                conn.send_command(format!("CREATE \"{}\"", path,).as_bytes())?;
                conn.read_response(&mut response)?;
                conn.send_command(format!("SUBSCRIBE \"{}\"", path,).as_bytes())?;
                conn.read_response(&mut response)?;
            }
            Rename(dest) => {
                let mut conn = self.connection.lock()?;
                conn.send_command(format!("RENAME \"{}\" \"{}\"", path, dest).as_bytes())?;
                conn.read_response(&mut response)?;
            }
            Delete => {
                let mut conn = self.connection.lock()?;
                conn.send_command(format!("DELETE \"{}\"", path,).as_bytes())?;
                conn.read_response(&mut response)?;
            }
            Subscribe => {
                let mut conn = self.connection.lock()?;
                conn.send_command(format!("SUBSCRIBE \"{}\"", path,).as_bytes())?;
                conn.read_response(&mut response)?;
            }
            Unsubscribe => {
                let mut conn = self.connection.lock()?;
                conn.send_command(format!("UNSUBSCRIBE \"{}\"", path,).as_bytes())?;
                conn.read_response(&mut response)?;
            }
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn::std::any::Any {
        self
    }
}

macro_rules! get_conf_val {
    ($s:ident[$var:literal]) => {
        $s.extra.get($var).unwrap_or_else(|| {
            eprintln!(
                "Configuration error ({}): IMAP connection requires the field `{}` set",
                $s.name.as_str(),
                $var
            );
            std::process::exit(1);
        })
    };
    ($s:ident[$var:literal], $default:expr) => {
        $s.extra
            .get($var)
            .map(|v| {
                <_>::from_str(v).unwrap_or_else(|_| {
                    eprintln!(
                        "Configuration error ({}): Invalid value for field `{}`: {}",
                        $s.name.as_str(),
                        $var,
                        v,
                    );
                    std::process::exit(1);
                })
            })
            .unwrap_or_else(|| $default)
    };
}

impl ImapType {
    pub fn new(
        s: &AccountSettings,
        is_subscribed: Box<dyn Fn(&str) -> bool + Send + Sync>,
    ) -> Self {
        debug!(s);
        let server_hostname = get_conf_val!(s["server_hostname"]);
        let server_username = get_conf_val!(s["server_username"]);
        let server_password = get_conf_val!(s["server_password"]);
        let server_port = get_conf_val!(s["server_port"], 143);
        let use_starttls = get_conf_val!(s["use_starttls"], {
            if server_port == 993 {
                false
            } else {
                true
            }
        });
        let danger_accept_invalid_certs: bool =
            get_conf_val!(s["danger_accept_invalid_certs"], false);
        let server_conf = ImapServerConf {
            server_hostname: server_hostname.to_string(),
            server_username: server_username.to_string(),
            server_password: server_password.to_string(),
            server_port,
            use_starttls,
            danger_accept_invalid_certs,
        };
        let connection = ImapConnection::new_connection(&server_conf);

        ImapType {
            account_name: s.name().to_string(),
            online: Arc::new(Mutex::new(false)),
            server_conf,
            is_subscribed: Arc::new(IsSubscribedFn(is_subscribed)),

            folders: Arc::new(Mutex::new(Default::default())),
            connection: Arc::new(Mutex::new(connection)),
            hash_index: Default::default(),
            uid_index: Default::default(),
            byte_cache: Default::default(),
        }
    }

    pub fn shell(&mut self) {
        let mut conn = ImapConnection::new_connection(&self.server_conf);
        let mut res = String::with_capacity(8 * 1024);
        conn.read_response(&mut res).unwrap();
        debug!("out: {}", &res);

        let mut input = String::new();
        loop {
            use std::io;
            input.clear();

            match io::stdin().read_line(&mut input) {
                Ok(_) => {
                    conn.send_command(input.as_bytes()).unwrap();
                    conn.read_lines(&mut res, String::new()).unwrap();
                    if input.trim() == "IDLE" {
                        let mut iter = ImapBlockingConnection::from(conn);
                        while let Some(line) = iter.next() {
                            debug!("out: {}", unsafe { std::str::from_utf8_unchecked(&line) });
                        }
                        conn = iter.into_conn();
                    }
                    debug!("out: {}", &res);
                    if input.trim().eq_ignore_ascii_case("logout") {
                        break;
                    }
                }
                Err(error) => debug!("error: {}", error),
            }
        }
    }

    pub fn imap_folders(
        connection: &Arc<Mutex<ImapConnection>>,
    ) -> FnvHashMap<FolderHash, ImapFolder> {
        let mut folders: FnvHashMap<FolderHash, ImapFolder> = Default::default();
        let mut res = String::with_capacity(8 * 1024);
        let mut conn = connection.lock().unwrap();
        conn.send_command(b"LIST \"\" \"*\"").unwrap();
        conn.read_response(&mut res).unwrap();
        debug!("out: {}", &res);
        for l in res.lines().map(|l| l.trim()) {
            if let Ok(mut folder) =
                protocol_parser::list_folder_result(l.as_bytes()).to_full_result()
            {
                if let Some(parent) = folder.parent {
                    if folders.contains_key(&parent) {
                        folders
                            .entry(parent)
                            .and_modify(|e| e.children.push(folder.hash));
                    } else {
                        /* Insert dummy parent entry, populating only the children field. Later
                         * when we encounter the parent entry we will swap its children with
                         * dummy's */
                        folders.insert(
                            parent,
                            ImapFolder {
                                children: vec![folder.hash],
                                ..ImapFolder::default()
                            },
                        );
                    }
                }
                if folders.contains_key(&folder.hash) {
                    let entry = folders.entry(folder.hash).or_default();
                    std::mem::swap(&mut entry.children, &mut folder.children);
                    *entry = folder;
                } else {
                    folders.insert(folder.hash, folder);
                }
            } else {
                debug!("parse error for {:?}", l);
            }
        }
        debug!(folders)
    }
}
