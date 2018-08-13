/*
 * meli - backends module
 *
 * Copyright 2017 Manos Pitsidianakis
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
pub mod imap;
pub mod maildir;
pub mod mbox;

use async::*;
use conf::AccountSettings;
use error::Result;
//use mailbox::backends::imap::ImapType;
//use mailbox::backends::mbox::MboxType;
use mailbox::backends::maildir::MaildirType;
use mailbox::email::{Envelope, Flag};
use std::fmt;
use std::fmt::Debug;

extern crate fnv;
use self::fnv::FnvHashMap;
use std;

pub type BackendCreator = Box<Fn(&AccountSettings) -> Box<MailBackend>>;

/// A hashmap containing all available mail backends.
/// An abstraction over any available backends.
pub struct Backends {
    map: FnvHashMap<std::string::String, Box<Fn() -> BackendCreator>>,
}

impl Backends {
    pub fn new() -> Self {
        let mut b = Backends {
            map: FnvHashMap::with_capacity_and_hasher(1, Default::default()),
        };
        b.register(
            "maildir".to_string(),
            Box::new(|| Box::new(|f| Box::new(MaildirType::new(f)))),
        );
        //b.register("mbox".to_string(), Box::new(|| Box::new(MboxType::new(""))));
        //b.register("imap".to_string(), Box::new(|| Box::new(ImapType::new(""))));
        b
    }

    pub fn get(&self, key: &str) -> BackendCreator {
        if !self.map.contains_key(key) {
            panic!("{} is not a valid mail backend", key);
        }
        self.map[key]()
    }

    pub fn register(&mut self, key: String, backend: Box<Fn() -> BackendCreator>) -> () {
        if self.map.contains_key(&key) {
            panic!("{} is an already registered backend", key);
        }
        self.map.insert(key, backend);
    }
}

pub struct RefreshEvent {
    pub hash: u64,
    pub folder: String,
}

/// A `RefreshEventConsumer` is a boxed closure that must be used to consume a `RefreshEvent` and
/// send it to a UI provided channel. We need this level of abstraction to provide an interface for
/// all users of mailbox refresh events.
pub struct RefreshEventConsumer(Box<Fn(RefreshEvent) -> ()>);
unsafe impl Send for RefreshEventConsumer {}
unsafe impl Sync for RefreshEventConsumer {}
impl RefreshEventConsumer {
    pub fn new(b: Box<Fn(RefreshEvent) -> ()>) -> Self {
        RefreshEventConsumer(b)
    }
    pub fn send(&self, r: RefreshEvent) -> () {
        self.0(r);
    }
}
pub trait MailBackend: ::std::fmt::Debug {
    fn get(&mut self, folder: &Folder) -> Async<Result<Vec<Envelope>>>;
    fn watch(&self, sender: RefreshEventConsumer) -> Result<()>;
    fn folders(&self) -> Vec<Folder>;
    fn operation(&self, hash: u64) -> Box<BackendOp>;
    //login function
}

/// A `BackendOp` manages common operations for the various mail backends. They only live for the
/// duration of the operation. They are generated by `BackendOpGenerator` on demand.
///
/// # Motivation
///
/// We need a way to do various operations on individual mails regardless of what backend they come
/// from (eg local or imap).
///
/// # Example
/// ```
/// use melib::mailbox::backends::{BackendOp, BackendOpGenerator};
/// use melib::Result;
/// use melib::{Envelope, Flag};
///
/// #[derive(Debug)]
/// struct FooOp {}
///
/// impl BackendOp for FooOp {
///     fn description(&self) -> String {
///         "Foobar".to_string()
///     }
///     fn as_bytes(&mut self) -> Result<&[u8]> {
///         unimplemented!()
///     }
///     fn fetch_headers(&mut self) -> Result<&[u8]> {
///         unimplemented!()
///     }
///     fn fetch_body(&mut self) -> Result<&[u8]> {
///         unimplemented!()
///     }
///     fn fetch_flags(&self) -> Flag {
///         unimplemented!()
///     }
/// }
///
/// let foogen = BackendOpGenerator::new(Box::new(|| Box::new(FooOp {})));
/// let operation = foogen.generate();
/// assert_eq!("Foobar", &operation.description());
///
/// ```
pub trait BackendOp: ::std::fmt::Debug + ::std::marker::Send {
    fn description(&self) -> String;
    fn as_bytes(&mut self) -> Result<&[u8]>;
    //fn delete(&self) -> ();
    //fn copy(&self
    fn fetch_headers(&mut self) -> Result<&[u8]>;
    fn fetch_body(&mut self) -> Result<&[u8]>;
    fn fetch_flags(&self) -> Flag;
    fn set_flag(&mut self, &mut Envelope, &Flag) -> Result<()>;
}

/// `BackendOpGenerator` is a wrapper for a closure that returns a `BackendOp` object
/// See `BackendOp` for details.
/*
 * I know this sucks, but that's the best way I found that rustc deems safe.
 * */
pub struct BackendOpGenerator(Box<Fn() -> Box<BackendOp>>);
impl BackendOpGenerator {
    pub fn new(b: Box<Fn() -> Box<BackendOp>>) -> Self {
        BackendOpGenerator(b)
    }
    pub fn generate(&self) -> Box<BackendOp> {
        self.0()
    }
}
unsafe impl Send for BackendOpGenerator {}
unsafe impl Sync for BackendOpGenerator {}
impl fmt::Debug for BackendOpGenerator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let op = self.generate();
        write!(f, "BackendOpGenerator: {}", op.description())
    }
}

pub trait BackendFolder: Debug {
    fn hash(&self) -> u64;
    fn name(&self) -> &str;
    fn clone(&self) -> Folder;
    fn children(&self) -> &Vec<usize>;
}

#[derive(Debug)]
struct DummyFolder {
    v: Vec<usize>,
}

impl BackendFolder for DummyFolder {
    fn hash(&self) -> u64 {
        0
    }
    fn name(&self) -> &str {
        ""
    }
    fn clone(&self) -> Folder {
        folder_default()
    }
    fn children(&self) -> &Vec<usize> {
        &self.v
    }
}
pub fn folder_default() -> Folder {
    Box::new(DummyFolder {
        v: Vec::with_capacity(0),
    })
}

pub type Folder = Box<BackendFolder>;
