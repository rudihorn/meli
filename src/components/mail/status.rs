/*
 * meli - status tab module.
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
pub struct StatusPanel {
    cursor: (usize, usize),
    account_cursor: usize,
    status: Option<AccountStatus>,
    content: CellBuffer,
    dirty: bool,
    theme_default: ThemeAttribute,
    id: ComponentId,
}

impl core::fmt::Display for StatusPanel {
    fn fmt(&self, f: &mut core::fmt::Formatter) -> core::fmt::Result {
        write!(f, "status")
    }
}

impl Component for StatusPanel {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if let Some(ref mut status) = self.status {
            status.draw(grid, area, context);
            return;
        }
        self.draw_accounts(context);
        let (width, height) = self.content.size();
        let (cols, rows) = (width!(area), height!(area));
        self.cursor = (
            std::cmp::min(width.saturating_sub(cols), self.cursor.0),
            std::cmp::min(height.saturating_sub(rows), self.cursor.1),
        );
        clear_area(grid, area, self.theme_default);
        copy_area(
            grid,
            &self.content,
            area,
            (
                (
                    std::cmp::min((width - 1).saturating_sub(cols), self.cursor.0),
                    std::cmp::min((height - 1).saturating_sub(rows), self.cursor.1),
                ),
                (
                    std::cmp::min(self.cursor.0 + cols, width - 1),
                    std::cmp::min(self.cursor.1 + rows, height - 1),
                ),
            ),
        );
        context.dirty_areas.push_back(area);
    }
    fn process_event(&mut self, event: &mut UIEvent, context: &mut Context) -> bool {
        if let Some(ref mut status) = self.status {
            if status.process_event(event, context) {
                return true;
            }
        }

        match *event {
            UIEvent::Input(Key::Char('k')) if self.status.is_none() => {
                self.account_cursor = self.account_cursor.saturating_sub(1);
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Char('j')) if self.status.is_none() => {
                if self.account_cursor + 1 < context.accounts.len() {
                    self.account_cursor += 1;
                    self.dirty = true;
                }
                return true;
            }
            UIEvent::Input(Key::Char('\n')) if self.status.is_none() => {
                self.status = Some(AccountStatus::new(self.account_cursor, self.theme_default));
                return true;
            }
            UIEvent::Input(Key::Esc) if self.status.is_some() => {
                self.status = None;
                return true;
            }
            UIEvent::Input(Key::Left) if self.status.is_none() => {
                self.cursor.0 = self.cursor.0.saturating_sub(1);
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Right) if self.status.is_none() => {
                self.cursor.0 = self.cursor.0 + 1;
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Up) if self.status.is_none() => {
                self.cursor.1 = self.cursor.1.saturating_sub(1);
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Down) if self.status.is_none() => {
                self.cursor.1 = self.cursor.1 + 1;
                self.dirty = true;
                return true;
            }
            UIEvent::MailboxUpdate(_)
            | UIEvent::StatusEvent(StatusEvent::NewJob(_))
            | UIEvent::StatusEvent(StatusEvent::JobFinished(_))
            | UIEvent::StatusEvent(StatusEvent::JobCanceled(_)) => {
                self.set_dirty(true);
            }
            _ => {}
        }

        false
    }
    fn is_dirty(&self) -> bool {
        self.dirty || self.status.as_ref().map(|s| s.is_dirty()).unwrap_or(false)
    }
    fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
        if let Some(ref mut status) = self.status {
            status.set_dirty(value);
        }
    }

    fn id(&self) -> ComponentId {
        self.id
    }
    fn set_id(&mut self, id: ComponentId) {
        self.id = id;
    }
}

impl StatusPanel {
    pub fn new(theme_default: ThemeAttribute) -> StatusPanel {
        let default_cell = {
            let mut ret = Cell::with_char(' ');
            ret.set_fg(theme_default.fg)
                .set_bg(theme_default.bg)
                .set_attrs(theme_default.attrs);
            ret
        };
        let mut content = CellBuffer::new(120, 40, default_cell);
        content.set_growable(true);

        StatusPanel {
            cursor: (0, 0),
            account_cursor: 0,
            content,
            status: None,
            dirty: true,
            theme_default,
            id: ComponentId::new_v4(),
        }
    }
    fn draw_accounts(&mut self, context: &Context) {
        let default_cell = {
            let mut ret = Cell::with_char(' ');
            ret.set_fg(self.theme_default.fg)
                .set_bg(self.theme_default.bg)
                .set_attrs(self.theme_default.attrs);
            ret
        };
        self.content
            .resize(120, 40 + context.accounts.len() * 45, default_cell);
        write_string_to_grid(
            "Accounts",
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            self.theme_default.attrs,
            ((2, 2), (120 - 1, 2)),
            Some(2),
        );

        for (i, (_h, a)) in context.accounts.iter().enumerate() {
            for x in 2..(120 - 1) {
                set_and_join_box(&mut self.content, (x, 4 + i * 10), BoxBoundary::Horizontal);
            }
            //create_box(&mut self.content, ((2, 5 + i * 10), (120 - 1, 15 + i * 10)));
            let (x, y) = write_string_to_grid(
                a.name(),
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                Attr::BOLD,
                ((3, 4 + i * 10), (120 - 2, 4 + i * 10)),
                Some(3),
            );
            write_string_to_grid(
                " ▒██▒ ",
                &mut self.content,
                Color::Byte(32),
                self.theme_default.bg,
                self.theme_default.attrs,
                ((x, y), (120 - 2, y)),
                None,
            );
            write_string_to_grid(
                &a.settings.account().identity,
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                self.theme_default.attrs,
                ((4, y + 2), (120 - 2, y + 2)),
                None,
            );
            if i == self.account_cursor {
                for h in 1..8 {
                    self.content[(2, h + y + 1)].set_ch('*');
                }
            } else {
                for h in 1..8 {
                    self.content[(2, h + y + 1)].set_ch(' ');
                }
            }
            let count = a
                .mailbox_entries
                .values()
                .map(|entry| &entry.ref_mailbox)
                .fold((0, 0), |acc, f| {
                    let count = f.count().unwrap_or((0, 0));
                    (acc.0 + count.0, acc.1 + count.1)
                });
            let (mut column_width, _) = write_string_to_grid(
                &format!("Messages total {}, unseen {}", count.1, count.0),
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                self.theme_default.attrs,
                ((5, y + 3), (120 - 2, y + 3)),
                None,
            );
            column_width = std::cmp::max(
                column_width,
                write_string_to_grid(
                    &format!("Contacts total {}", a.address_book.len()),
                    &mut self.content,
                    self.theme_default.fg,
                    self.theme_default.bg,
                    self.theme_default.attrs,
                    ((5, y + 4), (120 - 2, y + 4)),
                    None,
                )
                .0,
            );
            column_width = std::cmp::max(
                column_width,
                write_string_to_grid(
                    &format!("Backend {}", a.settings.account().format()),
                    &mut self.content,
                    self.theme_default.fg,
                    self.theme_default.bg,
                    self.theme_default.attrs,
                    ((5, y + 5), (120 - 2, y + 5)),
                    None,
                )
                .0,
            );
            if let Err(err) = a.is_online.as_ref() {
                write_string_to_grid(
                    &err.to_string(),
                    &mut self.content,
                    self.theme_default.fg,
                    self.theme_default.bg,
                    self.theme_default.attrs,
                    ((5, y + 6), (5 + column_width, y + 6)),
                    Some(5),
                );
            }
            /* next column */
            write_string_to_grid(
                "Special Mailboxes:",
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                Attr::BOLD,
                ((5 + column_width, y + 2), (120 - 2, y + 2)),
                None,
            );
            for (i, f) in a
                .mailbox_entries
                .values()
                .map(|entry| &entry.ref_mailbox)
                .filter(|f| f.special_usage() != SpecialUsageMailbox::Normal)
                .enumerate()
            {
                write_string_to_grid(
                    &format!("{}: {}", f.special_usage(), f.path()),
                    &mut self.content,
                    self.theme_default.fg,
                    self.theme_default.bg,
                    self.theme_default.attrs,
                    ((5 + column_width, y + 3 + i), (120 - 2, y + 2)),
                    None,
                );
            }
        }
    }
}

impl Component for AccountStatus {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let (width, height) = self.content.size();
        let a = &context.accounts[self.account_pos];
        let (_x, _y) = write_string_to_grid(
            "(Press Esc to return)",
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            Attr::BOLD,
            ((1, 0), (width - 1, height - 1)),
            None,
        );
        let mut line = 2;

        let (_x, _y) = write_string_to_grid(
            "Tag support: ",
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            Attr::BOLD,
            ((1, line), (width - 1, height - 1)),
            None,
        );
        write_string_to_grid(
            if a.backend_capabilities.supports_tags {
                "yes"
            } else {
                "no"
            },
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            self.theme_default.attrs,
            ((_x, _y), (width - 1, height - 1)),
            None,
        );
        line += 1;
        let (_x, _y) = write_string_to_grid(
            "Search backend: ",
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            Attr::BOLD,
            ((1, line), (width - 1, height - 1)),
            None,
        );
        write_string_to_grid(
            &match (
                a.settings.conf.search_backend(),
                a.backend_capabilities.supports_search,
            ) {
                (SearchBackend::Auto, true) | (SearchBackend::None, true) => {
                    "backend-side search".to_string()
                }
                (SearchBackend::Auto, false) | (SearchBackend::None, false) => {
                    "none (search will be slow)".to_string()
                }
                #[cfg(feature = "sqlite3")]
                (SearchBackend::Sqlite3, _) => {
                    if let Ok(path) = crate::sqlite3::db_path() {
                        format!("sqlite3 database {}", path.display())
                    } else {
                        "sqlite3 database".to_string()
                    }
                }
            },
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            self.theme_default.attrs,
            ((_x, _y), (width - 1, height - 1)),
            None,
        );
        line += 1;

        write_string_to_grid(
            "Special Mailboxes:",
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            Attr::BOLD,
            ((1, line), (width - 1, height - 1)),
            None,
        );
        for f in a
            .mailbox_entries
            .values()
            .map(|entry| &entry.ref_mailbox)
            .filter(|f| f.special_usage() != SpecialUsageMailbox::Normal)
        {
            line += 1;
            write_string_to_grid(
                &format!("{}: {}", f.path(), f.special_usage()),
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                self.theme_default.attrs,
                ((1, line), (width - 1, height - 1)),
                None,
            );
        }
        line += 2;
        write_string_to_grid(
            "Subscribed mailboxes:",
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            Attr::BOLD,
            ((1, line), (width - 1, height - 1)),
            None,
        );
        line += 2;
        for mailbox_node in a.list_mailboxes() {
            let f: &Mailbox = &a[&mailbox_node.hash].ref_mailbox;
            if f.is_subscribed() {
                write_string_to_grid(
                    f.path(),
                    &mut self.content,
                    self.theme_default.fg,
                    self.theme_default.bg,
                    self.theme_default.attrs,
                    ((1, line), (width - 1, height - 1)),
                    None,
                );
                line += 1;
            }
        }

        line += 1;
        if let Some(ref extensions) = a.backend_capabilities.extensions {
            write_string_to_grid(
                "Server Extensions:",
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                Attr::BOLD,
                ((1, line), (width - 1, height - 1)),
                None,
            );
            let max_name_width = std::cmp::max(
                "Server Extensions:".len(),
                extensions
                    .iter()
                    .map(|(n, _)| std::cmp::min(30, n.len()))
                    .max()
                    .unwrap_or(0),
            );
            write_string_to_grid(
                "meli support:",
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                self.theme_default.attrs,
                ((max_name_width + 6, line), (width - 1, height - 1)),
                None,
            );
            line += 1;
            for (name, status) in extensions.into_iter() {
                let (width, height) = self.content.size();
                write_string_to_grid(
                    name.trim_at_boundary(30),
                    &mut self.content,
                    self.theme_default.fg,
                    self.theme_default.bg,
                    self.theme_default.attrs,
                    ((1, line), (width - 1, height - 1)),
                    None,
                );

                let (width, height) = self.content.size();
                let (x, y) = match status {
                    MailBackendExtensionStatus::Unsupported { comment: _ } => write_string_to_grid(
                        "not supported",
                        &mut self.content,
                        Color::Red,
                        self.theme_default.bg,
                        self.theme_default.attrs,
                        ((max_name_width + 6, line), (width - 1, height - 1)),
                        None,
                    ),
                    MailBackendExtensionStatus::Supported { comment: _ } => write_string_to_grid(
                        "supported",
                        &mut self.content,
                        Color::Green,
                        self.theme_default.bg,
                        self.theme_default.attrs,
                        ((max_name_width + 6, line), (width - 1, height - 1)),
                        None,
                    ),
                    MailBackendExtensionStatus::Enabled { comment: _ } => write_string_to_grid(
                        "enabled",
                        &mut self.content,
                        Color::Green,
                        self.theme_default.bg,
                        self.theme_default.attrs,
                        ((max_name_width + 6, line), (width - 1, height - 1)),
                        None,
                    ),
                };
                match status {
                    MailBackendExtensionStatus::Unsupported { comment }
                    | MailBackendExtensionStatus::Supported { comment }
                    | MailBackendExtensionStatus::Enabled { comment } => {
                        if let Some(s) = comment {
                            let (x, y) = write_string_to_grid(
                                " (",
                                &mut self.content,
                                self.theme_default.fg,
                                self.theme_default.bg,
                                self.theme_default.attrs,
                                ((x, y), (width - 1, height - 1)),
                                None,
                            );
                            let (x, y) = write_string_to_grid(
                                s,
                                &mut self.content,
                                self.theme_default.fg,
                                self.theme_default.bg,
                                self.theme_default.attrs,
                                ((x, y), (width - 1, height - 1)),
                                None,
                            );
                            write_string_to_grid(
                                ")",
                                &mut self.content,
                                self.theme_default.fg,
                                self.theme_default.bg,
                                self.theme_default.attrs,
                                ((x, y), (width - 1, height - 1)),
                                None,
                            );
                        }
                    }
                };
                line += 1;
            }
        }
        line += 2;

        write_string_to_grid(
            "In-progress jobs:",
            &mut self.content,
            self.theme_default.fg,
            self.theme_default.bg,
            Attr::BOLD,
            ((1, line), (width - 1, height - 1)),
            None,
        );

        for (job_id, req) in a.active_jobs.iter() {
            use crate::conf::accounts::JobRequest;
            let (x, y) = write_string_to_grid(
                &format!("{} {}", req, job_id),
                &mut self.content,
                self.theme_default.fg,
                self.theme_default.bg,
                self.theme_default.attrs,
                ((1, line), (width - 1, height - 1)),
                None,
            );
            if let JobRequest::DeleteMailbox { mailbox_hash, .. }
            | JobRequest::SetMailboxPermissions { mailbox_hash, .. }
            | JobRequest::SetMailboxSubscription { mailbox_hash, .. }
            | JobRequest::CopyTo {
                dest_mailbox_hash: mailbox_hash,
                ..
            }
            | JobRequest::Refresh { mailbox_hash, .. }
            | JobRequest::Fetch { mailbox_hash, .. } = req
            {
                write_string_to_grid(
                    a.mailbox_entries[mailbox_hash].name(),
                    &mut self.content,
                    self.theme_default.fg,
                    self.theme_default.bg,
                    self.theme_default.attrs,
                    ((x + 1, y), (width - 1, height - 1)),
                    None,
                );
            }

            line += 1;
        }

        /* self.content may have been resized with write_string_to_grid() calls above since it has
         * growable set */
        let (width, height) = self.content.size();
        let (cols, rows) = (width!(area), height!(area));
        self.cursor = (
            std::cmp::min(width.saturating_sub(cols), self.cursor.0),
            std::cmp::min(height.saturating_sub(rows), self.cursor.1),
        );
        clear_area(grid, area, self.theme_default);
        copy_area(
            grid,
            &self.content,
            area,
            (
                (
                    std::cmp::min((width - 1).saturating_sub(cols), self.cursor.0),
                    std::cmp::min((height - 1).saturating_sub(rows), self.cursor.1),
                ),
                (
                    std::cmp::min(self.cursor.0 + cols, width - 1),
                    std::cmp::min(self.cursor.1 + rows, height - 1),
                ),
            ),
        );
        context.dirty_areas.push_back(area);
    }
    fn process_event(&mut self, event: &mut UIEvent, _context: &mut Context) -> bool {
        match *event {
            UIEvent::Resize => {
                self.dirty = true;
            }
            UIEvent::Input(Key::Left) => {
                self.cursor.0 = self.cursor.0.saturating_sub(1);
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Right) => {
                self.cursor.0 = self.cursor.0 + 1;
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Up) => {
                self.cursor.1 = self.cursor.1.saturating_sub(1);
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Down) => {
                self.cursor.1 = self.cursor.1 + 1;
                self.dirty = true;
                return true;
            }
            _ => {}
        }
        false
    }
    fn is_dirty(&self) -> bool {
        self.dirty
    }
    fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
    }

    fn id(&self) -> ComponentId {
        self.id
    }
    fn set_id(&mut self, id: ComponentId) {
        self.id = id;
    }
}

impl AccountStatus {
    pub fn new(account_pos: usize, theme_default: ThemeAttribute) -> AccountStatus {
        let default_cell = {
            let mut ret = Cell::with_char(' ');
            ret.set_fg(theme_default.fg)
                .set_bg(theme_default.bg)
                .set_attrs(theme_default.attrs);
            ret
        };
        let mut content = CellBuffer::new(120, 5, default_cell);
        content.set_growable(true);

        AccountStatus {
            cursor: (0, 0),
            account_pos,
            content,
            dirty: true,
            theme_default,
            id: ComponentId::new_v4(),
        }
    }
}

#[derive(Debug)]
struct AccountStatus {
    cursor: (usize, usize),
    account_pos: usize,
    content: CellBuffer,
    dirty: bool,
    theme_default: ThemeAttribute,
    id: ComponentId,
}

impl fmt::Display for AccountStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "status")
    }
}
