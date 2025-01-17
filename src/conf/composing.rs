/*
 * meli - conf module
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

//! Configuration for composing email.
use super::default_vals::{false_val, none, true_val};
use std::collections::HashMap;

/// Settings for writing and sending new e-mail
#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(deny_unknown_fields)]
pub struct ComposingSettings {
    /// A command to pipe new emails to
    /// Required
    pub send_mail: SendMail,
    /// Command to launch editor. Can have arguments. Draft filename is given as the last argument. If it's missing, the environment variable $EDITOR is looked up.
    #[serde(
        default = "none",
        alias = "editor-command",
        alias = "editor-cmd",
        alias = "editor_cmd"
    )]
    pub editor_command: Option<String>,
    /// Embed editor (for terminal interfaces) instead of forking and waiting.
    #[serde(default = "false_val")]
    pub embed: bool,
    /// Set "format=flowed" in plain text attachments.
    /// Default: true
    #[serde(default = "true_val", alias = "format-flowed")]
    pub format_flowed: bool,
    ///Set User-Agent
    ///Default: empty
    #[serde(default = "true_val", alias = "insert_user_agent")]
    pub insert_user_agent: bool,
    /// Set default header values for new drafts
    /// Default: empty
    #[serde(default, alias = "default-header-values")]
    pub default_header_values: HashMap<String, String>,
}

impl Default for ComposingSettings {
    fn default() -> Self {
        ComposingSettings {
            send_mail: SendMail::ShellCommand("/bin/false".into()),
            editor_command: None,
            embed: false,
            format_flowed: true,
            insert_user_agent: true,
            default_header_values: HashMap::default(),
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum SendMail {
    #[cfg(feature = "smtp")]
    Smtp(melib::smtp::SmtpServerConf),
    ShellCommand(String),
}
