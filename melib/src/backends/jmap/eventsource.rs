/*
 * meli - jmap module.
 *
 * Copyright 2019 Lukas Werling (lluchs)
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

//! # EventSource
//!
//! EventSource is a Rust library for reading from Server-Sent Events endpoints. It transparently
//! sends HTTP requests and only exposes a stream of events to the user. It handles automatic
//! reconnection and parsing of the `text/event-stream` data format.
//!
//! # Examples
//!
//! ```no_run
//! extern crate eventsource;
//! extern crate reqwest;
//! use eventsource::reqwest::Client;
//! use reqwest::Url;
//!
//! fn main() {
//!     let client = Client::new(Url::parse("http://example.com").unwrap());
//!     for event in client {
//!         println!("{}", event.unwrap());
//!     }
//! }
//! ```
//!

// Generic text/event-stream parsing and serialization.
pub mod event;
// HTTP interface
pub mod client;
