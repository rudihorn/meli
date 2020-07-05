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

use super::*;

#[derive(Debug)]
pub struct JmapConnection {
    pub session: JmapSession,
    pub request_no: Arc<Mutex<usize>>,
    pub client: Arc<Mutex<Client>>,
    pub online_status: Arc<Mutex<(Instant, Result<()>)>>,
    pub server_conf: JmapServerConf,
    pub account_id: Arc<Mutex<String>>,
    pub method_call_states: Arc<Mutex<HashMap<&'static str, String>>>,
}

impl JmapConnection {
    pub fn new(
        server_conf: &JmapServerConf,
        online_status: Arc<Mutex<(Instant, Result<()>)>>,
    ) -> Result<Self> {
        use reqwest::header;
        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );
        headers.insert(
            header::CONTENT_TYPE,
            header::HeaderValue::from_static("application/json"),
        );
        let client = reqwest::blocking::ClientBuilder::new()
            .danger_accept_invalid_certs(server_conf.danger_accept_invalid_certs)
            .default_headers(headers)
            .build()?;
        let mut jmap_session_resource_url = if server_conf.server_hostname.starts_with("https://") {
            server_conf.server_hostname.to_string()
        } else {
            format!("https://{}", &server_conf.server_hostname)
        };
        if server_conf.server_port != 443 {
            jmap_session_resource_url.push(':');
            jmap_session_resource_url.push_str(&server_conf.server_port.to_string());
        }
        jmap_session_resource_url.push_str("/.well-known/jmap");

        let req = client
            .get(&jmap_session_resource_url)
            .basic_auth(
                &server_conf.server_username,
                Some(&server_conf.server_password),
            )
            .send()?;
        let res_text = req.text()?;

        let session: JmapSession = serde_json::from_str(&res_text).map_err(|_| {
            let err = MeliError::new(format!("Could not connect to JMAP server endpoint for {}. Is your server hostname setting correct? (i.e. \"jmap.mailserver.org\") (Note: only session resource discovery via /.well-known/jmap is supported. DNS SRV records are not suppported.)\nReply from server: {}", &server_conf.server_hostname, &res_text));
                *online_status.lock().unwrap() = (Instant::now(), Err(err.clone()));
                err
        })?;
        if !session
            .capabilities
            .contains_key("urn:ietf:params:jmap:core")
        {
            let err = MeliError::new(format!("Server {} did not return JMAP Core capability (urn:ietf:params:jmap:core). Returned capabilities were: {}", &server_conf.server_hostname, session.capabilities.keys().map(String::as_str).collect::<Vec<&str>>().join(", ")));
            *online_status.lock().unwrap() = (Instant::now(), Err(err.clone()));
            return Err(err);
        }
        if !session
            .capabilities
            .contains_key("urn:ietf:params:jmap:mail")
        {
            let err = MeliError::new(format!("Server {} does not support JMAP Mail capability (urn:ietf:params:jmap:mail). Returned capabilities were: {}", &server_conf.server_hostname, session.capabilities.keys().map(String::as_str).collect::<Vec<&str>>().join(", ")));
            *online_status.lock().unwrap() = (Instant::now(), Err(err.clone()));
            return Err(err);
        }

        *online_status.lock().unwrap() = (Instant::now(), Ok(()));
        let server_conf = server_conf.clone();
        Ok(JmapConnection {
            session,
            request_no: Arc::new(Mutex::new(0)),
            client: Arc::new(Mutex::new(client)),
            online_status,
            server_conf,
            account_id: Arc::new(Mutex::new(String::new())),
            method_call_states: Arc::new(Mutex::new(Default::default())),
        })
    }

    pub fn mail_account_id(&self) -> &Id {
        &self.session.primary_accounts["urn:ietf:params:jmap:mail"]
    }
}
