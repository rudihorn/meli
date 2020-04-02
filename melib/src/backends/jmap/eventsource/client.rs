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

// Original code by Lukas Werling (lluchs)
//! # Reqwest-based EventSource client

use super::event::{parse_event_line, Event, ParseResult};
use crate::error::*;
use reqwest;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, CONTENT_TYPE};
use std::io::{BufRead, BufReader};
use std::time::{Duration, Instant};

const DEFAULT_RETRY: u64 = 5000;

impl MeliError {
    fn http_error(status_code: reqwest::StatusCode) -> MeliError {
        MeliError {
            summary: Some("HTTP request failed".into()),
            details: format!("HTTP status code: {}", status_code).into(),
        }
    }

    fn invalid_content_type(mime_type: &str) -> MeliError {
        MeliError {
            summary: Some("unexpected Content-Type header".into()),
            details: format!("unexpected Content-Type: {}", mime_type).into(),
        }
    }
    fn no_content_type() -> MeliError {
        MeliError {
            summary: Some("no Content-Type header in response".into()),
            details: "Content-Type missing".into(),
        }
    }
}

/// A client for a Server-Sent Events endpoint.
///
/// Read events by iterating over the client.
pub struct Client {
    client: reqwest::blocking::Client,
    response: Option<BufReader<reqwest::blocking::Response>>,
    url: reqwest::Url,
    last_event_id: Option<String>,
    last_try: Option<Instant>,

    /// Reconnection time in milliseconds. Note that the reconnection time can be changed by the
    /// event stream, so changing this may not make a difference.
    pub retry: Duration,
}

impl Client {
    /// Constructs a new EventSource client for the given URL.
    ///
    /// This does not start an HTTP request.
    pub fn new(url: reqwest::Url) -> Client {
        Client {
            client: reqwest::blocking::Client::new(),
            response: None,
            url: url,
            last_event_id: None,
            last_try: None,
            retry: Duration::from_millis(DEFAULT_RETRY),
        }
    }

    fn next_request(&mut self) -> Result<()> {
        let mut headers = HeaderMap::with_capacity(2);
        headers.insert(ACCEPT, HeaderValue::from_str("text/event-stream").unwrap());
        if let Some(ref id) = self.last_event_id {
            headers.insert("Last-Event-ID", HeaderValue::from_str(id).unwrap());
        }

        let res = self.client.get(self.url.clone()).headers(headers).send()?;

        // Check status code and Content-Type.
        {
            let status = res.status();
            if !status.is_success() {
                return Err(MeliError::http_error(status));
            }

            if let Some(content_type_hv) = res.headers().get(CONTENT_TYPE) {
                let content_type = content_type_hv.to_str().unwrap().to_string();
                // Compare type and subtype only, MIME parameters are ignored.
                if content_type != "text/event-stream" {
                    return Err(MeliError::invalid_content_type(&content_type));
                }
            } else {
                return Err(MeliError::no_content_type());
            }
        }

        self.response = Some(BufReader::new(res));
        Ok(())
    }
}

// Helper macro for Option<Result<...>>
macro_rules! try_option {
    ($e:expr) => {
        match $e {
            Ok(val) => val,
            Err(err) => return Some(Err(::std::convert::From::from(err))),
        }
    };
}

/// Iterate over the client to get events.
///
/// HTTP requests are made transparently while iterating.
impl Iterator for Client {
    type Item = Result<Event>;

    fn next(&mut self) -> Option<Result<Event>> {
        if self.response.is_none() {
            // We may have to wait for the next request.
            if let Some(last_try) = self.last_try {
                let elapsed = last_try.elapsed();
                if elapsed < self.retry {
                    ::std::thread::sleep(self.retry - elapsed);
                }
            }
            // Set here in case the request fails.
            self.last_try = Some(Instant::now());

            try_option!(self.next_request());
        }

        let result = {
            let mut event = Event::new();
            let mut line = String::new();
            let reader = self.response.as_mut().unwrap();

            loop {
                match reader.read_line(&mut line) {
                    // Got new bytes from stream
                    Ok(_n) if _n > 0 => {
                        match parse_event_line(&line, &mut event) {
                            ParseResult::Next => (), // okay, just continue
                            ParseResult::Dispatch => {
                                if let Some(ref id) = event.id {
                                    self.last_event_id = Some(id.clone());
                                }
                                return Some(Ok(event));
                            }
                            ParseResult::SetRetry(ref retry) => {
                                self.retry = *retry;
                            }
                        }
                        line.clear();
                    }
                    // Nothing read from stream
                    Ok(_) => break None,
                    Err(err) => break Some(Err(::std::convert::From::from(err))),
                }
            }
        };

        match result {
            None | Some(Err(_)) => {
                // EOF or a stream error, retry after timeout
                self.last_try = Some(Instant::now());
                self.response = None;
                self.next()
            }
            _ => result,
        }
    }
}
