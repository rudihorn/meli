/*
 * meli
 *
 * Copyright 2017-2018 Manos Pitsidianakis
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

/*! Various useful components that can be used in a generic fashion.
 */
use super::*;
use text_processing::Reflow;

mod pager;
pub use self::pager::*;

mod widgets;
pub use self::widgets::*;

mod layouts;
pub use self::layouts::*;

use crate::jobs::JobId;
use std::collections::HashSet;

#[derive(Default, Debug, Clone)]
pub struct SearchPattern {
    pattern: String,
    positions: Vec<(usize, usize)>,
    cursor: usize,
    movement: Option<PageMovement>,
}

/// Status bar.
#[derive(Debug)]
pub struct StatusBar {
    container: Box<dyn Component>,
    status: String,
    status_message: String,
    ex_buffer: Field,
    ex_buffer_cmd_history_pos: Option<usize>,
    display_buffer: String,
    mode: UIMode,
    mouse: bool,
    height: usize,
    dirty: bool,
    id: ComponentId,
    progress_spinner: ProgressSpinner,
    in_progress_jobs: HashSet<JobId>,
    done_jobs: HashSet<JobId>,

    auto_complete: AutoComplete,
    cmd_history: Vec<String>,
}

impl fmt::Display for StatusBar {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "status bar")
    }
}

impl StatusBar {
    pub fn new(context: &Context, container: Box<dyn Component>) -> Self {
        let mut progress_spinner = ProgressSpinner::new(19, context);
        match context.settings.terminal.progress_spinner_sequence.as_ref() {
            Some(conf::terminal::ProgressSpinnerSequence::Integer(k)) => {
                progress_spinner.set_kind(*k);
            }
            Some(conf::terminal::ProgressSpinnerSequence::Custom(ref s)) => {
                progress_spinner.set_custom_kind(s.clone());
            }
            None => {}
        }

        StatusBar {
            container,
            status: String::with_capacity(256),
            status_message: String::with_capacity(256),
            ex_buffer: Field::Text(UText::new(String::with_capacity(256)), None),
            ex_buffer_cmd_history_pos: None,
            display_buffer: String::with_capacity(8),
            dirty: true,
            mode: UIMode::Normal,
            mouse: context.settings.terminal.use_mouse.is_true(),
            height: 1,
            id: ComponentId::new_v4(),
            auto_complete: AutoComplete::new(Vec::new()),
            progress_spinner,
            in_progress_jobs: HashSet::default(),
            done_jobs: HashSet::default(),
            cmd_history: crate::command::history::old_cmd_history(),
        }
    }

    fn draw_status_bar(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        let mut attribute = crate::conf::value(context, "status.bar");
        if !context.settings.terminal.use_color() {
            attribute.attrs |= Attr::REVERSE;
        }
        let (x, y) = write_string_to_grid(
            &self.status,
            grid,
            attribute.fg,
            attribute.bg,
            attribute.attrs,
            area,
            None,
        );
        for c in grid.row_iter(x..(get_x(bottom_right!(area)) + 1), y) {
            grid[c]
                .set_ch(' ')
                .set_fg(attribute.fg)
                .set_bg(attribute.bg)
                .set_attrs(attribute.attrs);
        }
        let offset = self.status.find('|').unwrap_or_else(|| self.status.len());
        if y < get_y(bottom_right!(area)) + 1 {
            for x in get_x(upper_left!(area))
                ..std::cmp::min(
                    get_x(upper_left!(area)) + offset,
                    get_x(bottom_right!(area)),
                )
            {
                grid[(x, y)].set_attrs(attribute.attrs | Attr::BOLD);
            }
        }

        let (mut x, y) = bottom_right!(area);
        if self.progress_spinner.is_active() {
            x = x.saturating_sub(1 + self.progress_spinner.width);
        }
        if self.progress_spinner.is_dirty() {
            self.progress_spinner.draw(
                grid,
                (
                    pos_dec(bottom_right!(area), (self.progress_spinner.width, 0)),
                    bottom_right!(area),
                ),
                context,
            );
        }
        for (idx, c) in self.display_buffer.chars().rev().enumerate() {
            if let Some(cell) = grid.get_mut(x.saturating_sub(idx).saturating_sub(1), y) {
                cell.set_ch(c);
            } else {
                break;
            }
        }

        context.dirty_areas.push_back(area);
    }

    fn draw_command_bar(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        clear_area(grid, area, crate::conf::value(context, "theme_default"));
        let (_, y) = write_string_to_grid(
            self.ex_buffer.as_str(),
            grid,
            Color::Byte(219),
            Color::Byte(88),
            Attr::DEFAULT,
            area,
            None,
        );
        if let Some(ref mut cell) = grid.get_mut(
            pos_inc(upper_left!(area), (self.ex_buffer.cursor(), 0)).0,
            y,
        ) {
            cell.set_attrs(Attr::UNDERLINE);
        }
        change_colors(grid, area, Color::Byte(219), Color::Byte(88));
        context.dirty_areas.push_back(area);
    }
}

impl Component for StatusBar {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if !is_valid_area!(area) {
            return;
        }
        let upper_left = upper_left!(area);
        let bottom_right = bottom_right!(area);

        let total_rows = get_y(bottom_right) - get_y(upper_left);
        if total_rows <= self.height {
            return;
        }
        let height = self.height;

        self.container.draw(
            grid,
            (
                upper_left,
                (get_x(bottom_right), get_y(bottom_right) - height),
            ),
            context,
        );

        self.dirty = false;
        self.draw_status_bar(
            grid,
            (set_y(upper_left, get_y(bottom_right)), bottom_right),
            context,
        );

        if self.mode != UIMode::Command && !self.is_dirty() {
            return;
        }
        match self.mode {
            UIMode::Normal => {}
            UIMode::Command => {
                self.draw_command_bar(
                    grid,
                    (
                        set_y(upper_left, get_y(bottom_right) - height + 1),
                        set_y(bottom_right, get_y(bottom_right) - height + 1),
                    ),
                    context,
                );
                /* don't autocomplete for less than 3 characters */
                if self.ex_buffer.as_str().split_graphemes().len() <= 2 {
                    return;
                }

                let mut unique_suggestions: HashSet<&str> = HashSet::default();
                let mut suggestions: Vec<AutoCompleteEntry> = self
                    .cmd_history
                    .iter()
                    .rev()
                    .filter_map(|h| {
                        let sug = self.ex_buffer.as_str();
                        if h.starts_with(sug) && !unique_suggestions.contains(sug) {
                            unique_suggestions.insert(sug);
                            Some(h.clone().into())
                        } else {
                            None
                        }
                    })
                    .collect();
                let command_completion_suggestions =
                    crate::command::command_completion_suggestions(self.ex_buffer.as_str());

                suggestions.extend(command_completion_suggestions.iter().filter_map(|e| {
                    if !unique_suggestions.contains(e.as_str()) {
                        unique_suggestions.insert(e.as_str());
                        Some(e.clone().into())
                    } else {
                        None
                    }
                }));
                /*
                suggestions.extend(crate::command::COMMAND_COMPLETION.iter().filter_map(|e| {
                    if e.0.starts_with(self.ex_buffer.as_str()) {
                        Some(e.into())
                    } else {
                        None
                    }
                }));
                */
                if let Some(p) = self
                    .ex_buffer
                    .as_str()
                    .split_whitespace()
                    .last()
                    .map(std::path::Path::new)
                {
                    suggestions.extend(
                        debug!(debug!(p).complete(true))
                            .into_iter()
                            .map(|m| format!("{}{}", self.ex_buffer.as_str(), m).into()),
                    );
                }
                if suggestions.is_empty() && !self.auto_complete.suggestions().is_empty() {
                    self.auto_complete.set_suggestions(suggestions);
                    /* redraw self.container because we have got ridden of an autocomplete
                     * box, and it must be drawn over */
                    self.container.set_dirty(true);
                    return;
                }
                /* redraw self.container because we have less suggestions than before */
                if suggestions.len() < self.auto_complete.suggestions().len() {
                    self.container.set_dirty(true);
                }

                suggestions.sort_by(|a, b| a.entry.cmp(&b.entry));
                suggestions.dedup_by(|a, b| &a.entry == &b.entry);
                if self.auto_complete.set_suggestions(suggestions) {
                    let len = self.auto_complete.suggestions().len() - 1;
                    self.auto_complete.set_cursor(len);

                    self.container.set_dirty(true);
                }
                let hist_height = std::cmp::min(15, self.auto_complete.suggestions().len());
                let hist_area = if height < self.auto_complete.suggestions().len() {
                    ScrollBar::default().set_show_arrows(false).draw(
                        grid,
                        (
                            (
                                get_x(upper_left),
                                get_y(bottom_right) - height - hist_height + 1,
                            ),
                            set_y(bottom_right, get_y(bottom_right) - height),
                        ),
                        context,
                        self.auto_complete.cursor(),
                        hist_height,
                        self.auto_complete.suggestions().len(),
                    );
                    change_colors(
                        grid,
                        (
                            (
                                get_x(upper_left),
                                get_y(bottom_right) - height - hist_height + 1,
                            ),
                            set_y(bottom_right, get_y(bottom_right) - height),
                        ),
                        Color::Byte(197), // DeepPink2,
                        Color::Byte(174), //LightPink3
                    );
                    context.dirty_areas.push_back((
                        (
                            get_x(upper_left),
                            get_y(bottom_right) - height - hist_height + 1,
                        ),
                        set_y(bottom_right, get_y(bottom_right) - height),
                    ));
                    (
                        (
                            get_x(upper_left) + 1,
                            get_y(bottom_right) - height - hist_height + 1,
                        ),
                        set_y(bottom_right, get_y(bottom_right) - height),
                    )
                } else {
                    (
                        (
                            get_x(upper_left),
                            get_y(bottom_right) - height - hist_height + 1,
                        ),
                        set_y(bottom_right, get_y(bottom_right) - height),
                    )
                };
                let offset = if hist_height
                    > (self.auto_complete.suggestions().len() - self.auto_complete.cursor())
                {
                    self.auto_complete.suggestions().len() - hist_height
                } else {
                    self.auto_complete.cursor()
                };
                clear_area(
                    grid,
                    hist_area,
                    crate::conf::value(context, "theme_default"),
                );
                if hist_height > 0 {
                    change_colors(
                        grid,
                        hist_area,
                        Color::Byte(88),  // DarkRed,
                        Color::Byte(174), //LightPink3
                    );
                }
                for (y_offset, s) in self
                    .auto_complete
                    .suggestions()
                    .iter()
                    .skip(offset)
                    .take(hist_height)
                    .enumerate()
                {
                    let (x, y) = write_string_to_grid(
                        s.as_str(),
                        grid,
                        Color::Byte(88),  // DarkRed,
                        Color::Byte(174), //LightPink3
                        Attr::DEFAULT,
                        (
                            set_y(
                                upper_left!(hist_area),
                                get_y(bottom_right!(hist_area)) - hist_height + y_offset + 1,
                            ),
                            bottom_right!(hist_area),
                        ),
                        Some(get_x(upper_left!(hist_area))),
                    );
                    write_string_to_grid(
                        &s.description,
                        grid,
                        Color::White,
                        Color::Byte(174),
                        Attr::DEFAULT,
                        ((x + 2, y), bottom_right!(hist_area)),
                        None,
                    );
                    if y_offset + offset == self.auto_complete.cursor() {
                        change_colors(
                            grid,
                            (
                                set_y(
                                    upper_left!(hist_area),
                                    get_y(bottom_right!(hist_area)) - hist_height + y_offset + 1,
                                ),
                                set_y(
                                    bottom_right!(hist_area),
                                    get_y(bottom_right!(hist_area)) - hist_height + y_offset + 1,
                                ),
                            ),
                            Color::Byte(88),  // DarkRed,
                            Color::Byte(173), //LightSalmon3
                        );
                        write_string_to_grid(
                            &s.as_str()[self.ex_buffer.as_str().len()..],
                            grid,
                            Color::Byte(97), // MediumPurple3,
                            Color::Byte(88), //LightPink3
                            Attr::DEFAULT,
                            (
                                (
                                    get_x(upper_left)
                                        + self.ex_buffer.as_str().split_graphemes().len(),
                                    get_y(bottom_right) - height + 1,
                                ),
                                set_y(bottom_right, get_y(bottom_right) - height + 1),
                            ),
                            None,
                        );
                    }
                }
                context.dirty_areas.push_back(hist_area);
            }
            _ => {}
        }
    }
    fn process_event(&mut self, event: &mut UIEvent, context: &mut Context) -> bool {
        if self.container.process_event(event, context) {
            return true;
        }

        match event {
            UIEvent::ChangeMode(m) => {
                let offset = self.status.find('|').unwrap_or_else(|| self.status.len());
                self.status.replace_range(
                    ..offset,
                    &format!(
                        "{} {}",
                        m,
                        if self.mouse {
                            context
                                .settings
                                .terminal
                                .mouse_flag
                                .as_ref()
                                .map(|s| s.as_str())
                                .unwrap_or("🖱️ ")
                        } else {
                            ""
                        },
                    ),
                );
                self.set_dirty(true);
                self.container.set_dirty(true);
                self.mode = *m;
                match m {
                    UIMode::Normal => {
                        self.height = 1;
                        if !self.ex_buffer.is_empty() {
                            context
                                .replies
                                .push_back(UIEvent::Command(self.ex_buffer.as_str().to_string()));
                        }
                        if parse_command(&self.ex_buffer.as_str().as_bytes()).is_ok()
                            && self.cmd_history.last().map(String::as_str)
                                != Some(self.ex_buffer.as_str())
                        {
                            crate::command::history::log_cmd(self.ex_buffer.as_str().to_string());
                            self.cmd_history.push(self.ex_buffer.as_str().to_string());
                        }
                        self.ex_buffer.clear();
                        self.ex_buffer_cmd_history_pos.take();
                    }
                    UIMode::Command => {
                        self.height = 2;
                    }
                    _ => {
                        self.height = 1;
                    }
                };
            }
            UIEvent::CmdInput(Key::Char('\t')) => {
                if let Some(suggestion) = self.auto_complete.get_suggestion().or_else(|| {
                    if self.auto_complete.cursor() == 0 {
                        self.auto_complete
                            .suggestions()
                            .last()
                            .map(|e| e.entry.clone())
                    } else {
                        None
                    }
                }) {
                    let mut utext = UText::new(suggestion);
                    let len = utext.as_str().len();
                    utext.set_cursor(len);
                    self.container.set_dirty(true);
                    self.set_dirty(true);
                    self.ex_buffer = Field::Text(utext, None);
                }
            }
            UIEvent::CmdInput(Key::Char(c)) => {
                self.dirty = true;
                self.ex_buffer
                    .process_event(&mut UIEvent::InsertInput(Key::Char(*c)), context);
                return true;
            }
            UIEvent::CmdInput(Key::Paste(s)) => {
                self.dirty = true;
                self.ex_buffer
                    .process_event(&mut UIEvent::InsertInput(Key::Paste(s.clone())), context);
                return true;
            }
            UIEvent::CmdInput(Key::Ctrl('u')) => {
                self.dirty = true;
                self.ex_buffer.clear();
                self.ex_buffer_cmd_history_pos.take();
                return true;
            }
            UIEvent::CmdInput(Key::Up) => {
                self.auto_complete.dec_cursor();
                self.dirty = true;
            }
            UIEvent::CmdInput(Key::Down) => {
                self.auto_complete.inc_cursor();
                self.dirty = true;
            }
            UIEvent::CmdInput(Key::Left) => {
                if let Field::Text(ref mut utext, _) = self.ex_buffer {
                    utext.cursor_dec();
                } else {
                    unsafe {
                        std::hint::unreachable_unchecked();
                    }
                }
                self.dirty = true;
            }
            UIEvent::CmdInput(Key::Right) => {
                if let Field::Text(ref mut utext, _) = self.ex_buffer {
                    utext.cursor_inc();
                } else {
                    unsafe {
                        std::hint::unreachable_unchecked();
                    }
                }
                self.dirty = true;
            }
            UIEvent::CmdInput(Key::Ctrl('p')) => {
                if self.cmd_history.is_empty() {
                    return true;
                }
                let pos = self.ex_buffer_cmd_history_pos.map(|p| p + 1).unwrap_or(0);
                let pos = Some(std::cmp::min(pos, self.cmd_history.len().saturating_sub(1)));
                if pos != self.ex_buffer_cmd_history_pos {
                    let mut utext = UText::new(
                        self.cmd_history[self.cmd_history.len().saturating_sub(1) - pos.unwrap()]
                            .clone(),
                    );
                    let len = utext.as_str().len();
                    utext.set_cursor(len);
                    self.container.set_dirty(true);
                    self.set_dirty(true);
                    self.ex_buffer = Field::Text(utext, None);
                    self.ex_buffer_cmd_history_pos = pos;
                    self.dirty = true;
                }

                return true;
            }
            UIEvent::CmdInput(Key::Ctrl('n')) => {
                if self.cmd_history.is_empty() {
                    return true;
                }
                if Some(0) == self.ex_buffer_cmd_history_pos {
                    self.ex_buffer_cmd_history_pos = None;
                    self.ex_buffer.clear();
                    self.dirty = true;
                } else if self.ex_buffer_cmd_history_pos.is_some() {
                    let pos = self.ex_buffer_cmd_history_pos.map(|p| p - 1);
                    let mut utext = UText::new(
                        self.cmd_history[self.cmd_history.len().saturating_sub(1) - pos.unwrap()]
                            .clone(),
                    );
                    let len = utext.as_str().len();
                    utext.set_cursor(len);
                    self.container.set_dirty(true);
                    self.set_dirty(true);
                    self.ex_buffer = Field::Text(utext, None);
                    self.ex_buffer_cmd_history_pos = pos;
                    self.dirty = true;
                }

                return true;
            }
            UIEvent::CmdInput(k @ Key::Backspace) | UIEvent::CmdInput(k @ Key::Ctrl(_)) => {
                self.dirty = true;
                self.ex_buffer
                    .process_event(&mut UIEvent::InsertInput(k.clone()), context);
                return true;
            }
            UIEvent::CmdInput(Key::Esc) => {
                self.ex_buffer.clear();
                context
                    .replies
                    .push_back(UIEvent::ChangeMode(UIMode::Normal));
                self.dirty = true;
                return true;
            }
            UIEvent::Resize => {
                self.dirty = true;
            }
            UIEvent::StatusEvent(StatusEvent::BufClear) => {
                self.display_buffer.clear();
                self.dirty = true;
            }
            UIEvent::StatusEvent(StatusEvent::BufSet(s)) => {
                self.display_buffer = s.clone();
                self.dirty = true;
            }
            UIEvent::StatusEvent(StatusEvent::UpdateStatus(ref mut s)) => {
                self.status_message.clear();
                self.status_message.push_str(s.as_str());
                self.status = format!(
                    "{} {}| {}",
                    self.mode,
                    if self.mouse {
                        context
                            .settings
                            .terminal
                            .mouse_flag
                            .as_ref()
                            .map(|s| s.as_str())
                            .unwrap_or("🖱️ ")
                    } else {
                        ""
                    },
                    &self.status_message,
                );
                self.dirty = true;
            }
            UIEvent::StatusEvent(StatusEvent::SetMouse(val)) => {
                self.mouse = *val;
                self.status = format!(
                    "{} {}| {}",
                    self.mode,
                    if self.mouse {
                        context
                            .settings
                            .terminal
                            .mouse_flag
                            .as_ref()
                            .map(|s| s.as_str())
                            .unwrap_or("🖱️ ")
                    } else {
                        ""
                    },
                    &self.status_message,
                );
                self.dirty = true;
            }
            UIEvent::StatusEvent(StatusEvent::JobCanceled(ref job_id))
            | UIEvent::StatusEvent(StatusEvent::JobFinished(ref job_id)) => {
                self.done_jobs.insert(*job_id);
                self.in_progress_jobs.remove(job_id);
                if self.in_progress_jobs.is_empty() {
                    self.progress_spinner.stop();
                }
                self.progress_spinner.set_dirty(true);
            }
            UIEvent::StatusEvent(StatusEvent::NewJob(ref job_id))
                if !self.done_jobs.contains(job_id) =>
            {
                if self.in_progress_jobs.is_empty() {
                    self.progress_spinner.start();
                }
                self.progress_spinner.set_dirty(true);
                self.in_progress_jobs.insert(*job_id);
            }
            UIEvent::Timer(_) => {
                if self.progress_spinner.process_event(event, context) {
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn is_dirty(&self) -> bool {
        self.dirty || self.container.is_dirty() || self.progress_spinner.is_dirty()
    }

    fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
        self.progress_spinner.set_dirty(value);
    }

    fn get_shortcuts(&self, context: &Context) -> ShortcutMaps {
        self.container.get_shortcuts(context)
    }

    fn id(&self) -> ComponentId {
        self.id
    }
    fn set_id(&mut self, id: ComponentId) {
        self.id = id;
    }

    fn can_quit_cleanly(&mut self, context: &Context) -> bool {
        self.container.can_quit_cleanly(context)
    }
}

#[derive(Debug)]
pub struct Tabbed {
    pinned: usize,
    children: Vec<Box<dyn Component>>,
    cursor_pos: usize,

    show_shortcuts: bool,
    help_screen_cursor: (usize, usize),
    help_content: CellBuffer,
    help_curr_views: ShortcutMaps,
    help_search: Option<SearchPattern>,

    dirty: bool,
    id: ComponentId,
}

impl Tabbed {
    pub fn new(children: Vec<Box<dyn Component>>, context: &Context) -> Self {
        let pinned = children.len();
        let mut ret = Tabbed {
            help_curr_views: children
                .get(0)
                .map(|c| c.get_shortcuts(context))
                .unwrap_or_default(),
            help_content: CellBuffer::default(),
            help_screen_cursor: (0, 0),
            help_search: None,
            pinned,
            children,
            cursor_pos: 0,
            show_shortcuts: false,
            dirty: true,
            id: ComponentId::new_v4(),
        };
        ret.help_curr_views
            .extend(ret.get_shortcuts(context).into_iter());
        ret
    }
    fn draw_tabs(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        let upper_left = upper_left!(area);
        let bottom_right = bottom_right!(area);

        let tab_bar_attribute = crate::conf::value(context, "tab.bar");
        if self.children.is_empty() {
            clear_area(grid, area, tab_bar_attribute);
            return;
        }
        let tab_unfocused_attribute = crate::conf::value(context, "tab.unfocused");
        let mut tab_focused_attribute = crate::conf::value(context, "tab.focused");
        if !context.settings.terminal.use_color() {
            tab_focused_attribute.attrs |= Attr::REVERSE;
        }

        let mut x = get_x(upper_left);
        let y: usize = get_y(upper_left);
        for (idx, c) in self.children.iter().enumerate() {
            let ThemeAttribute { fg, bg, attrs } = if idx == self.cursor_pos {
                tab_focused_attribute
            } else {
                tab_unfocused_attribute
            };
            let (x_, _y_) = write_string_to_grid(
                &format!(" {} ", c),
                grid,
                fg,
                bg,
                attrs,
                (set_x(upper_left, x), bottom_right!(area)),
                None,
            );
            x = x_ + 1;
            if idx == self.pinned.saturating_sub(1) {
                x += 2;
            }
            if y != _y_ {
                break;
            }
            if x > get_x(bottom_right) {
                x = get_x(bottom_right);
                break;
            }
            grid[(x_, _y_)]
                .set_fg(tab_bar_attribute.fg)
                .set_bg(tab_bar_attribute.bg)
                .set_attrs(tab_bar_attribute.attrs);
        }
        let (cols, _) = grid.size();
        let cslice: &mut [Cell] = grid;
        let cslice_len = cslice.len();
        for c in cslice[(y * cols) + x.saturating_sub(1)
            ..std::cmp::min((y * cols) + x.saturating_sub(1), cslice_len)]
            .iter_mut()
        {
            c.set_ch(' ').set_bg(tab_unfocused_attribute.bg);
            //.set_fg(tab_unfocused_attribute.bg)
            //.set_bg(Color::Byte(7));
        }

        if self.cursor_pos == self.children.len() - 1 {
            cslice[(y * cols) + x]
                .set_ch('▍')
                .set_fg(tab_unfocused_attribute.bg)
                .set_bg(tab_unfocused_attribute.fg)
                .set_attrs(tab_unfocused_attribute.attrs);
        }
        for c in grid.row_iter(x..cols, get_y(upper_left)) {
            grid[c]
                .set_fg(tab_bar_attribute.fg)
                .set_bg(tab_bar_attribute.bg)
                .set_attrs(tab_bar_attribute.attrs);
        }

        context.dirty_areas.push_back(area);
    }
    pub fn add_component(&mut self, new: Box<dyn Component>) {
        self.children.push(new);
    }
}

impl fmt::Display for Tabbed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "tabs")
    }
}

impl Component for Tabbed {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if self.dirty {
            clear_area(
                grid,
                (
                    upper_left!(area),
                    set_x(upper_left!(area), get_x(bottom_right!(area))),
                ),
                crate::conf::value(context, "tab.bar"),
            );
            context.dirty_areas.push_back((
                upper_left!(area),
                set_x(upper_left!(area), get_x(bottom_right!(area))),
            ));
        }

        /* If children are dirty but self isn't and the shortcuts panel is visible, it will get
         * overwritten. */
        let must_redraw_shortcuts: bool = self.show_shortcuts && !self.dirty && self.is_dirty();

        /* children should be drawn after the shortcuts/help panel lest they overwrite the panel on
         * the grid. the drawing order is determined by the dirty_areas queue which is LIFO */
        if self.children.len() > 1 {
            self.draw_tabs(
                grid,
                (
                    upper_left!(area),
                    set_x(upper_left!(area), get_x(bottom_right!(area))),
                ),
                context,
            );
            let y = get_y(upper_left!(area));
            self.children[self.cursor_pos].draw(
                grid,
                (set_y(upper_left!(area), y + 1), bottom_right!(area)),
                context,
            );
        } else {
            self.children[self.cursor_pos].draw(grid, area, context);
        }

        if (self.show_shortcuts && self.dirty) || must_redraw_shortcuts {
            let area = (
                pos_inc(upper_left!(area), (2, 1)),
                set_x(
                    bottom_right!(area),
                    get_x(bottom_right!(area)).saturating_sub(2),
                ),
            );
            context.dirty_areas.push_back(area);
            clear_area(grid, area, crate::conf::value(context, "theme_default"));
            let area = create_box(grid, area);
            let mut children_maps = self.children[self.cursor_pos].get_shortcuts(context);
            let our_map = self.get_shortcuts(context);
            children_maps.extend(our_map.into_iter());
            if children_maps.is_empty() {
                return;
            }
            if (children_maps == self.help_curr_views) && must_redraw_shortcuts {
                let (width, height) = self.help_content.size();
                let (cols, rows) = (width!(area), height!(area));
                copy_area(
                    grid,
                    &self.help_content,
                    area,
                    (
                        (
                            std::cmp::min(
                                (width - 1).saturating_sub(cols),
                                self.help_screen_cursor.0,
                            ),
                            std::cmp::min(
                                (height - 1).saturating_sub(rows),
                                self.help_screen_cursor.1,
                            ),
                        ),
                        (
                            std::cmp::min(self.help_screen_cursor.0 + cols, width - 1),
                            std::cmp::min(self.help_screen_cursor.1 + rows, height - 1),
                        ),
                    ),
                );
                self.dirty = false;
                return;
            }
            let mut max_length = 6;
            let mut max_width =
                "Press ? to close, use COMMAND \"search\" to find shortcuts".len() + 3;

            for (desc, shortcuts) in children_maps.iter() {
                max_length += shortcuts.len() + 3;
                max_width = std::cmp::max(
                    max_width,
                    std::cmp::max(
                        desc.len(),
                        shortcuts
                            .values()
                            .map(|v| v.to_string().len() + 5)
                            .max()
                            .unwrap_or(0),
                    ),
                );
            }
            self.help_content =
                CellBuffer::new_with_context(max_width, max_length + 2, Cell::default(), context);
            self.help_content.set_growable(true);
            let (width, height) = self.help_content.size();
            let (cols, rows) = (width!(area), height!(area));
            if cols == 0 || rows == 0 {
                return;
            }
            /* trim cursor if it's bigger than the help screen */
            self.help_screen_cursor = (
                std::cmp::min((width - 1).saturating_sub(cols), self.help_screen_cursor.0),
                std::cmp::min((height - 1).saturating_sub(rows), self.help_screen_cursor.1),
            );

            let (x, y) = write_string_to_grid(
                "shortcut maps",
                &mut self.help_content,
                Color::Default,
                Color::Default,
                Attr::BOLD,
                ((2, 0), (max_width.saturating_sub(2), max_length - 1)),
                None,
            );
            write_string_to_grid(
                "Press ? to close",
                &mut self.help_content,
                Color::Default,
                Color::Default,
                Attr::DEFAULT,
                ((x + 1, y), (max_width.saturating_sub(2), max_length - 1)),
                None,
            );
            write_string_to_grid(
                "use COMMAND \"search\" to find shortcuts",
                &mut self.help_content,
                Color::Default,
                Color::Default,
                Attr::DEFAULT,
                ((2, 1), (max_width.saturating_sub(2), max_length - 1)),
                None,
            );
            /* In this case we will be scrolling, so show the user how to do it */
            if height.wrapping_div(rows) > 0 || width.wrapping_div(cols) > 0 {
                write_string_to_grid(
                    "Use Up, Down, Left, Right to scroll.",
                    &mut self.help_content,
                    Color::Default,
                    Color::Default,
                    Attr::DEFAULT,
                    ((2, 2), (max_width.saturating_sub(2), max_length - 1)),
                    None,
                );
            }
            let mut idx = 2;
            for (desc, shortcuts) in children_maps.iter() {
                write_string_to_grid(
                    desc,
                    &mut self.help_content,
                    Color::Default,
                    Color::Default,
                    Attr::DEFAULT,
                    ((2, 2 + idx), (max_width.saturating_sub(2), max_length - 1)),
                    None,
                );
                idx += 2;
                for (k, v) in shortcuts {
                    debug!(&(k, v));
                    let (x, y) = write_string_to_grid(
                        &format!("{:1$}", v, max_width),
                        &mut self.help_content,
                        Color::Default,
                        Color::Default,
                        Attr::BOLD,
                        ((2, 2 + idx), (max_width.saturating_sub(2), max_length - 1)),
                        None,
                    );
                    write_string_to_grid(
                        &k,
                        &mut self.help_content,
                        Color::Default,
                        Color::Default,
                        Attr::DEFAULT,
                        ((x + 2, y), (max_width.saturating_sub(2), max_length - 1)),
                        None,
                    );
                    idx += 1;
                }
                idx += 1;
            }
            self.help_curr_views = children_maps;
            if let Some(ref mut search) = self.help_search {
                use crate::melib::text_processing::search::KMP;
                search.positions = self
                    .help_content
                    .kmp_search(&search.pattern)
                    .into_iter()
                    .map(|offset| (offset / width, offset % width))
                    .collect::<Vec<(usize, usize)>>();
                let results_attr = crate::conf::value(context, "pager.highlight_search");
                let results_current_attr =
                    crate::conf::value(context, "pager.highlight_search_current");
                search.cursor =
                    std::cmp::min(search.positions.len().saturating_sub(1), search.cursor);
                for (i, (y, x)) in search.positions.iter().enumerate() {
                    for c in self
                        .help_content
                        .row_iter(*x..*x + search.pattern.grapheme_len(), *y)
                    {
                        if i == search.cursor {
                            self.help_content[c]
                                .set_fg(results_current_attr.fg)
                                .set_bg(results_current_attr.bg)
                                .set_attrs(results_current_attr.attrs);
                        } else {
                            self.help_content[c]
                                .set_fg(results_attr.fg)
                                .set_bg(results_attr.bg)
                                .set_attrs(results_attr.attrs);
                        }
                    }
                }
                if !search.positions.is_empty() {
                    if let Some(mvm) = search.movement.take() {
                        match mvm {
                            PageMovement::Home => {
                                if self.help_screen_cursor.1 > search.positions[search.cursor].0 {
                                    self.help_screen_cursor.1 = search.positions[search.cursor].0;
                                }
                                if self.help_screen_cursor.1 + rows
                                    < search.positions[search.cursor].0
                                {
                                    self.help_screen_cursor.1 = search.positions[search.cursor].0;
                                }
                            }
                            PageMovement::Up(_) => {
                                if self.help_screen_cursor.1 > search.positions[search.cursor].0 {
                                    self.help_screen_cursor.1 = search.positions[search.cursor].0;
                                }
                            }
                            PageMovement::Down(_) => {
                                if self.help_screen_cursor.1 + rows
                                    < search.positions[search.cursor].0
                                {
                                    self.help_screen_cursor.1 = search.positions[search.cursor].0;
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            copy_area(
                grid,
                &self.help_content,
                area,
                (
                    (
                        std::cmp::min((width - 1).saturating_sub(cols), self.help_screen_cursor.0),
                        std::cmp::min((height - 1).saturating_sub(rows), self.help_screen_cursor.1),
                    ),
                    (
                        std::cmp::min(self.help_screen_cursor.0 + cols, width - 1),
                        std::cmp::min(self.help_screen_cursor.1 + rows, height - 1),
                    ),
                ),
            );
        }
        self.dirty = false;
    }
    fn process_event(&mut self, mut event: &mut UIEvent, context: &mut Context) -> bool {
        let shortcuts = &self.help_curr_views;
        match &mut event {
            UIEvent::Input(Key::Alt(no)) if *no >= '1' && *no <= '9' => {
                let no = *no as usize - '1' as usize;
                if no < self.children.len() {
                    self.cursor_pos = no % self.children.len();
                    let mut children_maps = self.children[self.cursor_pos].get_shortcuts(context);
                    children_maps.extend(self.get_shortcuts(context));
                    self.help_curr_views = children_maps;
                    context
                        .replies
                        .push_back(UIEvent::StatusEvent(StatusEvent::UpdateStatus(
                            self.children[self.cursor_pos].get_status(context),
                        )));
                    self.set_dirty(true);
                }
                return true;
            }
            UIEvent::Input(ref key) if shortcut!(key == shortcuts["general"]["next_tab"]) => {
                self.cursor_pos = (self.cursor_pos + 1) % self.children.len();
                let mut children_maps = self.children[self.cursor_pos].get_shortcuts(context);
                children_maps.extend(self.get_shortcuts(context));
                self.help_curr_views = children_maps;
                context
                    .replies
                    .push_back(UIEvent::StatusEvent(StatusEvent::UpdateStatus(
                        self.children[self.cursor_pos].get_status(context),
                    )));
                self.set_dirty(true);
                return true;
            }
            UIEvent::Input(ref key) if shortcut!(key == shortcuts["general"]["toggle_help"]) => {
                if self.show_shortcuts {
                    /* children below the shortcut overlay must be redrawn */
                    self.set_dirty(true);
                }
                self.show_shortcuts = !self.show_shortcuts;
                self.dirty = true;
                return true;
            }
            UIEvent::Action(Tab(New(ref mut e))) if e.is_some() => {
                self.add_component(e.take().unwrap());
                self.cursor_pos = self.children.len() - 1;
                self.children[self.cursor_pos].set_dirty(true);
                let mut children_maps = self.children[self.cursor_pos].get_shortcuts(context);
                children_maps.extend(self.get_shortcuts(context));
                self.help_curr_views = children_maps;
                return true;
            }
            UIEvent::Action(Tab(Close)) => {
                if self.pinned > self.cursor_pos {
                    return true;
                }
                let id = self.children[self.cursor_pos].id();
                self.children[self.cursor_pos].kill(id, context);
                let mut children_maps = self.children[self.cursor_pos].get_shortcuts(context);
                children_maps.extend(self.get_shortcuts(context));
                self.help_curr_views = children_maps;
                self.set_dirty(true);
                return true;
            }
            UIEvent::Action(Tab(Kill(id))) => {
                if self.pinned > self.cursor_pos {
                    return true;
                }
                if let Some(c_idx) = self.children.iter().position(|x| x.id() == *id) {
                    self.children.remove(c_idx);
                    self.cursor_pos = 0;
                    self.set_dirty(true);
                    let mut children_maps = self.children[self.cursor_pos].get_shortcuts(context);
                    children_maps.extend(self.get_shortcuts(context));
                    self.help_curr_views = children_maps;
                    return true;
                } else {
                    debug!(
                        "DEBUG: Child component with id {:?} not found.\nList: {:?}",
                        id, self.children
                    );
                }
            }
            UIEvent::Action(Action::Listing(ListingAction::Search(pattern)))
                if self.show_shortcuts =>
            {
                self.help_search = Some(SearchPattern {
                    pattern: pattern.to_string(),
                    positions: vec![],
                    cursor: 0,
                    movement: Some(PageMovement::Home),
                });
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Char('n')) if self.show_shortcuts && self.help_search.is_some() => {
                if let Some(ref mut search) = self.help_search {
                    search.movement = Some(PageMovement::Down(1));
                    search.cursor += 1;
                } else {
                    unsafe {
                        std::hint::unreachable_unchecked();
                    }
                }
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Char('N')) if self.show_shortcuts && self.help_search.is_some() => {
                if let Some(ref mut search) = self.help_search {
                    search.movement = Some(PageMovement::Up(1));
                    search.cursor = search.cursor.saturating_sub(1);
                } else {
                    unsafe {
                        std::hint::unreachable_unchecked();
                    }
                }
                self.dirty = true;
                return true;
            }
            UIEvent::Input(Key::Esc) if self.show_shortcuts && self.help_search.is_some() => {
                self.help_search = None;
                self.dirty = true;
                return true;
            }
            UIEvent::Resize => {
                self.dirty = true;
            }
            UIEvent::Input(ref key)
                if self.show_shortcuts
                    && shortcut!(
                        key == shortcuts[super::listing::Listing::DESCRIPTION]["search"]
                    ) =>
            {
                context
                    .replies
                    .push_back(UIEvent::CmdInput(Key::Paste("search ".to_string())));
                context
                    .replies
                    .push_back(UIEvent::ChangeMode(UIMode::Command));
                return true;
            }
            UIEvent::Input(ref key) if self.show_shortcuts => {
                match key {
                    _ if shortcut!(key == shortcuts["general"]["scroll_up"]) => {
                        self.help_screen_cursor.1 = self.help_screen_cursor.1.saturating_sub(1);
                    }
                    _ if shortcut!(key == shortcuts["general"]["scroll_down"]) => {
                        self.help_screen_cursor.1 = self.help_screen_cursor.1 + 1;
                    }
                    _ if shortcut!(key == shortcuts["general"]["scroll_left"]) => {
                        self.help_screen_cursor.0 = self.help_screen_cursor.0.saturating_sub(1);
                    }
                    _ if shortcut!(key == shortcuts["general"]["scroll_right"]) => {
                        self.help_screen_cursor.0 = self.help_screen_cursor.0 + 1;
                    }
                    _ => {
                        /* ignore, don't pass to components below the shortcut panel */
                        return false;
                    }
                }
                self.dirty = true;
                return true;
            }
            _ => {}
        }
        let c = self.cursor_pos;
        if let UIEvent::Input(_) | UIEvent::CmdInput(_) | UIEvent::EmbedInput(_) = event {
            self.children[c].process_event(event, context)
        } else {
            self.children[c].process_event(event, context)
                || self.children.iter_mut().enumerate().any(|(idx, child)| {
                    if idx == c {
                        return false;
                    }
                    child.process_event(event, context)
                })
        }
    }
    fn is_dirty(&self) -> bool {
        self.dirty || self.children[self.cursor_pos].is_dirty()
    }
    fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
        self.children[self.cursor_pos].set_dirty(value);
    }

    fn id(&self) -> ComponentId {
        self.id
    }
    fn set_id(&mut self, id: ComponentId) {
        self.id = id;
    }

    fn get_shortcuts(&self, context: &Context) -> ShortcutMaps {
        let mut map = ShortcutMaps::default();
        map.insert("general", context.settings.shortcuts.general.key_values());
        map
    }

    fn can_quit_cleanly(&mut self, context: &Context) -> bool {
        for (i, c) in self.children.iter_mut().enumerate() {
            if !c.can_quit_cleanly(context) {
                self.cursor_pos = i;
                self.set_dirty(true);
                return false;
            }
        }
        true
    }
}

#[derive(Debug, Copy, PartialEq, Clone)]
enum SelectorCursor {
    /// Cursor is at an entry
    Entry(usize),
    /// Cursor is located on the Ok button
    Ok,
    /// Cursor is located on the Cancel button
    Cancel,
}

/// Shows a little window with options for user to select.
///
/// Instantiate with Selector::new(). Set single_only to true if user should only choose one of the
/// options. After passing input events to this component, check Selector::is_done to see if the
/// user has finalised their choices. Collect the choices by consuming the Selector with
/// Selector::collect()
pub struct Selector<T: 'static + PartialEq + Debug + Clone + Sync + Send, F: 'static + Sync + Send>
{
    /// allow only one selection
    single_only: bool,
    entries: Vec<(T, bool)>,
    pub content: CellBuffer,

    cursor: SelectorCursor,

    /// If true, user has finished their selection
    done: bool,
    done_fn: F,
    dirty: bool,
    id: ComponentId,
}

pub type UIConfirmationDialog = Selector<
    bool,
    Option<Box<dyn FnOnce(ComponentId, bool) -> Option<UIEvent> + 'static + Sync + Send>>,
>;

pub type UIDialog<T> = Selector<
    T,
    Option<Box<dyn FnOnce(ComponentId, &[T]) -> Option<UIEvent> + 'static + Sync + Send>>,
>;

impl<T: 'static + PartialEq + Debug + Clone + Sync + Send, F: 'static + Sync + Send> fmt::Debug
    for Selector<T, F>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt("Selector", f)
    }
}

impl<T: 'static + PartialEq + Debug + Clone + Sync + Send, F: 'static + Sync + Send> fmt::Display
    for Selector<T, F>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt("Selector", f)
    }
}

impl<T: 'static + PartialEq + Debug + Clone + Sync + Send, F: 'static + Sync + Send> PartialEq
    for Selector<T, F>
{
    fn eq(&self, other: &Selector<T, F>) -> bool {
        self.entries == other.entries
    }
}

impl<T: 'static + PartialEq + Debug + Clone + Sync + Send> Component for UIDialog<T> {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        let (width, height) = self.content.size();
        copy_area_with_break(grid, &self.content, area, ((0, 0), (width, height)));
        context.dirty_areas.push_back(area);
    }
    fn process_event(&mut self, event: &mut UIEvent, context: &mut Context) -> bool {
        let (width, height) = self.content.size();
        let shortcuts = self.get_shortcuts(context);
        let mut highlighted_attrs = crate::conf::value(context, "widgets.options.highlighted");
        if !context.settings.terminal.use_color() {
            highlighted_attrs.attrs |= Attr::REVERSE;
        }
        match (event, self.cursor) {
            (UIEvent::Input(Key::Char('\n')), _) if self.single_only => {
                /* User can only select one entry, so Enter key finalises the selection */
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Char('\n')), SelectorCursor::Entry(c)) if !self.single_only => {
                /* User can select multiple entries, so Enter key toggles the entry under the
                 * cursor */
                self.entries[c].1 = !self.entries[c].1;
                if self.entries[c].1 {
                    write_string_to_grid(
                        "x",
                        &mut self.content,
                        Color::Default,
                        Color::Default,
                        Attr::DEFAULT,
                        ((3, c + 2), (width - 2, c + 2)),
                        None,
                    );
                } else {
                    write_string_to_grid(
                        " ",
                        &mut self.content,
                        Color::Default,
                        Color::Default,
                        Attr::DEFAULT,
                        ((3, c + 2), (width - 2, c + 2)),
                        None,
                    );
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(Key::Char('\n')), SelectorCursor::Ok) if !self.single_only => {
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Esc), _) => {
                for e in self.entries.iter_mut() {
                    e.1 = false;
                }
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Char('\n')), SelectorCursor::Cancel) if !self.single_only => {
                for e in self.entries.iter_mut() {
                    e.1 = false;
                }
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Up), SelectorCursor::Entry(c)) if c > 0 => {
                if self.single_only {
                    // Redraw selection
                    for c in self.content.row_iter(2..(width - 2), c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..(width - 2), c + 1) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                    self.entries[c].1 = false;
                    self.entries[c - 1].1 = true;
                } else {
                    // Redraw cursor
                    for c in self.content.row_iter(2..4, c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..4, c + 1) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                }
                self.cursor = SelectorCursor::Entry(c - 1);
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Ok)
            | (UIEvent::Input(ref key), SelectorCursor::Cancel)
                if shortcut!(key == shortcuts["general"]["scroll_up"]) =>
            {
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..(width - 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(Color::Default)
                        .set_bg(Color::Default)
                        .set_attrs(Attr::DEFAULT);
                }
                let c = self.entries.len().saturating_sub(1);
                self.cursor = SelectorCursor::Entry(c);
                let mut highlighted_attrs =
                    crate::conf::value(context, "widgets.options.highlighted");
                if !context.settings.terminal.use_color() {
                    highlighted_attrs.attrs |= Attr::REVERSE;
                }
                for c in self.content.row_iter(2..4, c + 2) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Entry(c))
                if c < self.entries.len().saturating_sub(1)
                    && shortcut!(key == shortcuts["general"]["scroll_down"]) =>
            {
                if self.single_only {
                    // Redraw selection
                    for c in self.content.row_iter(2..(width - 2), c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..(width - 2), c + 3) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                    self.entries[c].1 = false;
                    self.entries[c + 1].1 = true;
                } else {
                    // Redraw cursor
                    for c in self.content.row_iter(2..4, c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..4, c + 3) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                }
                self.cursor = SelectorCursor::Entry(c + 1);
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Entry(c))
                if !self.single_only && shortcut!(key == shortcuts["general"]["scroll_down"]) =>
            {
                self.cursor = SelectorCursor::Ok;
                for c in self.content.row_iter(2..4, c + 2) {
                    self.content[c]
                        .set_fg(Color::Default)
                        .set_bg(Color::Default)
                        .set_attrs(Attr::DEFAULT);
                }
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..((width - "OK    Cancel".len()) / 2 + 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Ok)
                if shortcut!(key == shortcuts["general"]["scroll_right"]) =>
            {
                self.cursor = SelectorCursor::Cancel;
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..((width - "OK    Cancel".len()) / 2 + 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(Color::Default)
                        .set_bg(Color::Default)
                        .set_attrs(Attr::DEFAULT);
                }
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2 + 6)
                        ..((width - "OK    Cancel".len()) / 2 + 11),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Cancel)
                if shortcut!(key == shortcuts["general"]["scroll_left"]) =>
            {
                self.cursor = SelectorCursor::Ok;
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..((width - "OK    Cancel".len()) / 2 + 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                change_colors(
                    &mut self.content,
                    (
                        ((width - "OK    Cancel".len()) / 2 + 6, height - 3),
                        ((width - "OK    Cancel".len()) / 2 + 11, height - 3),
                    ),
                    Color::Default,
                    Color::Default,
                );
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), _)
                if shortcut!(key == shortcuts["general"]["scroll_left"])
                    || shortcut!(key == shortcuts["general"]["scroll_right"])
                    || shortcut!(key == shortcuts["general"]["scroll_up"])
                    || shortcut!(key == shortcuts["general"]["scroll_down"]) =>
            {
                return true
            }
            _ => {}
        }

        false
    }
    fn get_shortcuts(&self, context: &Context) -> ShortcutMaps {
        let mut map = ShortcutMaps::default();
        map.insert("general", context.settings.shortcuts.general.key_values());
        map
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

impl Component for UIConfirmationDialog {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        let (width, height) = self.content.size();
        copy_area_with_break(grid, &self.content, area, ((0, 0), (width, height)));
        context.dirty_areas.push_back(area);
    }
    fn process_event(&mut self, event: &mut UIEvent, context: &mut Context) -> bool {
        let (width, height) = self.content.size();
        let shortcuts = self.get_shortcuts(context);
        let mut highlighted_attrs = crate::conf::value(context, "widgets.options.highlighted");
        if !context.settings.terminal.use_color() {
            highlighted_attrs.attrs |= Attr::REVERSE;
        }
        match (event, self.cursor) {
            (UIEvent::Input(Key::Char('\n')), _) if self.single_only => {
                /* User can only select one entry, so Enter key finalises the selection */
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Char('\n')), SelectorCursor::Entry(c)) if !self.single_only => {
                /* User can select multiple entries, so Enter key toggles the entry under the
                 * cursor */
                self.entries[c].1 = !self.entries[c].1;
                if self.entries[c].1 {
                    write_string_to_grid(
                        "x",
                        &mut self.content,
                        Color::Default,
                        Color::Default,
                        Attr::DEFAULT,
                        ((3, c + 2), (width - 2, c + 2)),
                        None,
                    );
                } else {
                    write_string_to_grid(
                        " ",
                        &mut self.content,
                        Color::Default,
                        Color::Default,
                        Attr::DEFAULT,
                        ((3, c + 2), (width - 2, c + 2)),
                        None,
                    );
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(Key::Char('\n')), SelectorCursor::Ok) if !self.single_only => {
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Esc), _) => {
                for e in self.entries.iter_mut() {
                    e.1 = false;
                }
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Char('\n')), SelectorCursor::Cancel) if !self.single_only => {
                for e in self.entries.iter_mut() {
                    e.1 = false;
                }
                self.done = true;
                if let Some(event) = self.done() {
                    context.replies.push_back(event);
                    context.replies.push_back(UIEvent::ComponentKill(self.id));
                }
                return true;
            }
            (UIEvent::Input(Key::Up), SelectorCursor::Entry(c)) if c > 0 => {
                if self.single_only {
                    // Redraw selection
                    for c in self.content.row_iter(2..(width - 2), c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..(width - 2), c + 1) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                    self.entries[c].1 = false;
                    self.entries[c - 1].1 = true;
                } else {
                    // Redraw cursor
                    for c in self.content.row_iter(2..4, c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..4, c + 1) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                }
                self.cursor = SelectorCursor::Entry(c - 1);
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Ok)
            | (UIEvent::Input(ref key), SelectorCursor::Cancel)
                if shortcut!(key == shortcuts["general"]["scroll_up"]) =>
            {
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..(width - 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(Color::Default)
                        .set_bg(Color::Default)
                        .set_attrs(Attr::DEFAULT);
                }
                let c = self.entries.len().saturating_sub(1);
                self.cursor = SelectorCursor::Entry(c);
                let mut highlighted_attrs =
                    crate::conf::value(context, "widgets.options.highlighted");
                if !context.settings.terminal.use_color() {
                    highlighted_attrs.attrs |= Attr::REVERSE;
                }
                for c in self.content.row_iter(2..4, c + 2) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Entry(c))
                if c < self.entries.len().saturating_sub(1)
                    && shortcut!(key == shortcuts["general"]["scroll_down"]) =>
            {
                if self.single_only {
                    // Redraw selection
                    for c in self.content.row_iter(2..(width - 2), c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..(width - 2), c + 3) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                    self.entries[c].1 = false;
                    self.entries[c + 1].1 = true;
                } else {
                    // Redraw cursor
                    for c in self.content.row_iter(2..4, c + 2) {
                        self.content[c]
                            .set_fg(Color::Default)
                            .set_bg(Color::Default)
                            .set_attrs(Attr::DEFAULT);
                    }
                    for c in self.content.row_iter(2..4, c + 3) {
                        self.content[c]
                            .set_fg(highlighted_attrs.fg)
                            .set_bg(highlighted_attrs.bg)
                            .set_attrs(highlighted_attrs.attrs);
                    }
                }
                self.cursor = SelectorCursor::Entry(c + 1);
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Entry(c))
                if !self.single_only && shortcut!(key == shortcuts["general"]["scroll_down"]) =>
            {
                self.cursor = SelectorCursor::Ok;
                for c in self.content.row_iter(2..4, c + 2) {
                    self.content[c]
                        .set_fg(Color::Default)
                        .set_bg(Color::Default)
                        .set_attrs(Attr::DEFAULT);
                }
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..((width - "OK    Cancel".len()) / 2 + 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Ok)
                if shortcut!(key == shortcuts["general"]["scroll_right"]) =>
            {
                self.cursor = SelectorCursor::Cancel;
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..((width - "OK    Cancel".len()) / 2 + 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(Color::Default)
                        .set_bg(Color::Default)
                        .set_attrs(Attr::DEFAULT);
                }
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2 + 6)
                        ..((width - "OK    Cancel".len()) / 2 + 11),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), SelectorCursor::Cancel)
                if shortcut!(key == shortcuts["general"]["scroll_left"]) =>
            {
                self.cursor = SelectorCursor::Ok;
                for c in self.content.row_iter(
                    ((width - "OK    Cancel".len()) / 2)..((width - "OK    Cancel".len()) / 2 + 1),
                    height - 3,
                ) {
                    self.content[c]
                        .set_fg(highlighted_attrs.fg)
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
                change_colors(
                    &mut self.content,
                    (
                        ((width - "OK    Cancel".len()) / 2 + 6, height - 3),
                        ((width - "OK    Cancel".len()) / 2 + 11, height - 3),
                    ),
                    Color::Default,
                    Color::Default,
                );
                self.dirty = true;
                return true;
            }
            (UIEvent::Input(ref key), _)
                if shortcut!(key == shortcuts["general"]["scroll_left"])
                    || shortcut!(key == shortcuts["general"]["scroll_right"])
                    || shortcut!(key == shortcuts["general"]["scroll_up"])
                    || shortcut!(key == shortcuts["general"]["scroll_down"]) =>
            {
                return true
            }
            _ => {}
        }

        false
    }
    fn get_shortcuts(&self, context: &Context) -> ShortcutMaps {
        let mut map = ShortcutMaps::default();
        map.insert("general", context.settings.shortcuts.general.key_values());
        map
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

impl<T: PartialEq + Debug + Clone + Sync + Send, F: 'static + Sync + Send> Selector<T, F> {
    pub fn new(
        title: &str,
        entries: Vec<(T, String)>,
        single_only: bool,
        done_fn: F,
        context: &Context,
    ) -> Selector<T, F> {
        let width = std::cmp::max(
            "OK    Cancel".len(),
            std::cmp::max(
                entries
                    .iter()
                    .max_by_key(|e| e.1.len())
                    .map(|v| v.1.len())
                    .unwrap_or(0),
                title.len(),
            ),
        ) + 7;
        let height = entries.len()
            + 4
            + if single_only {
                0
            } else {
                /* Extra room for buttons Okay/Cancel */
                3
            };
        let mut content =
            CellBuffer::new_with_context(width, height, Cell::with_char(' '), context);
        let ascii_drawing = context.settings.terminal.ascii_drawing;
        write_string_to_grid(
            if ascii_drawing { "+-" } else { "┏━" },
            &mut content,
            Color::Byte(8),
            Color::Default,
            Attr::DEFAULT,
            ((0, 0), (width - 1, 0)),
            None,
        );
        let (x, _) = write_string_to_grid(
            title,
            &mut content,
            Color::Default,
            Color::Default,
            Attr::DEFAULT,
            ((2, 0), (width - 1, 0)),
            None,
        );
        for i in 1..(width - title.len() - 1) {
            write_string_to_grid(
                if ascii_drawing { "-" } else { "━" },
                &mut content,
                Color::Byte(8),
                Color::Default,
                Attr::DEFAULT,
                ((x + i, 0), (width - 1, 0)),
                None,
            );
        }
        write_string_to_grid(
            if ascii_drawing { "+" } else { "┓" },
            &mut content,
            Color::Byte(8),
            Color::Default,
            Attr::DEFAULT,
            ((width - 1, 0), (width - 1, 0)),
            None,
        );
        write_string_to_grid(
            if ascii_drawing { "+" } else { "┗" },
            &mut content,
            Color::Byte(8),
            Color::Default,
            Attr::DEFAULT,
            ((0, height - 1), (width - 1, height - 1)),
            None,
        );
        write_string_to_grid(
            &if ascii_drawing {
                "-".repeat(width - 2)
            } else {
                "━".repeat(width - 2)
            },
            &mut content,
            Color::Byte(8),
            Color::Default,
            Attr::DEFAULT,
            ((1, height - 1), (width - 2, height - 1)),
            None,
        );
        write_string_to_grid(
            if ascii_drawing { "+" } else { "┛" },
            &mut content,
            Color::Byte(8),
            Color::Default,
            Attr::DEFAULT,
            ((width - 1, height - 1), (width - 1, height - 1)),
            None,
        );
        for i in 1..height - 1 {
            write_string_to_grid(
                if ascii_drawing { "|" } else { "┃" },
                &mut content,
                Color::Byte(8),
                Color::Default,
                Attr::DEFAULT,
                ((0, i), (width - 1, i)),
                None,
            );
            write_string_to_grid(
                if ascii_drawing { "|" } else { "┃" },
                &mut content,
                Color::Byte(8),
                Color::Default,
                Attr::DEFAULT,
                ((width - 1, i), (width - 1, i)),
                None,
            );
        }
        let mut highlighted_attrs = crate::conf::value(context, "widgets.options.highlighted");
        if !context.settings.terminal.use_color() {
            highlighted_attrs.attrs |= Attr::REVERSE;
        }
        if single_only {
            for (i, e) in entries.iter().enumerate() {
                write_string_to_grid(
                    &e.1,
                    &mut content,
                    Color::Default,
                    if i == 0 {
                        highlighted_attrs.bg
                    } else {
                        Color::Default
                    },
                    if i == 0 {
                        highlighted_attrs.attrs
                    } else {
                        Attr::DEFAULT
                    },
                    ((2, i + 2), (width - 1, i + 2)),
                    None,
                );
            }
        } else {
            for (i, e) in entries.iter().enumerate() {
                write_string_to_grid(
                    &format!("[ ] {}", e.1),
                    &mut content,
                    Color::Default,
                    Color::Default,
                    Attr::DEFAULT,
                    ((2, i + 2), (width - 1, i + 2)),
                    None,
                );
                if i == 0 {
                    content[(2, i + 2)]
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                    content[(3, i + 2)]
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                    content[(4, i + 2)]
                        .set_bg(highlighted_attrs.bg)
                        .set_attrs(highlighted_attrs.attrs);
                }
            }
            write_string_to_grid(
                "OK    Cancel",
                &mut content,
                Color::Default,
                Color::Default,
                Attr::BOLD,
                (
                    ((width - "OK    Cancel".len()) / 2, height - 3),
                    (width - 1, height - 3),
                ),
                None,
            );
        }
        let mut identifiers: Vec<(T, bool)> =
            entries.into_iter().map(|(id, _)| (id, false)).collect();
        if single_only {
            /* set default option */
            identifiers[0].1 = true;
        }

        Selector {
            single_only,
            entries: identifiers,
            content,
            cursor: SelectorCursor::Entry(0),
            done: false,
            done_fn,
            dirty: true,
            id: ComponentId::new_v4(),
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn collect(self) -> Vec<T> {
        self.entries
            .into_iter()
            .filter(|v| v.1)
            .map(|(id, _)| id)
            .collect()
    }
}

impl<T: 'static + PartialEq + Debug + Clone + Sync + Send> UIDialog<T> {
    fn done(&mut self) -> Option<UIEvent> {
        let Self {
            ref mut done_fn,
            ref mut entries,
            ref id,
            ..
        } = self;
        done_fn.take().and_then(|done_fn| {
            debug!(done_fn(
                *id,
                entries
                    .iter()
                    .filter(|v| v.1)
                    .map(|(id, _)| id)
                    .cloned()
                    .collect::<Vec<_>>()
                    .as_slice(),
            ))
        })
    }
}

impl UIConfirmationDialog {
    fn done(&mut self) -> Option<UIEvent> {
        let Self {
            ref mut done_fn,
            ref mut entries,
            ref id,
            ..
        } = self;
        done_fn.take().and_then(|done_fn| {
            done_fn(
                *id,
                entries
                    .iter()
                    .filter(|v| v.1)
                    .map(|(id, _)| id)
                    .cloned()
                    .any(core::convert::identity),
            )
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawBuffer {
    pub buf: CellBuffer,
    title: Option<String>,
    cursor: (usize, usize),
    dirty: bool,
}

impl fmt::Display for RawBuffer {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt("Raw buffer", f)
    }
}

impl Component for RawBuffer {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if self.dirty {
            let (width, height) = self.buf.size();
            let (cols, rows) = (width!(area), height!(area));
            self.cursor = (
                std::cmp::min(width.saturating_sub(cols), self.cursor.0),
                std::cmp::min(height.saturating_sub(rows), self.cursor.1),
            );
            clear_area(grid, area, crate::conf::value(context, "theme_default"));
            copy_area(
                grid,
                &self.buf,
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
            self.dirty = false;
        }
    }
    fn process_event(&mut self, event: &mut UIEvent, _context: &mut Context) -> bool {
        match *event {
            UIEvent::Input(Key::Left) => {
                self.cursor.0 = self.cursor.0.saturating_sub(1);
                self.dirty = true;
                true
            }
            UIEvent::Input(Key::Right) => {
                self.cursor.0 = self.cursor.0 + 1;
                self.dirty = true;
                true
            }
            UIEvent::Input(Key::Up) => {
                self.cursor.1 = self.cursor.1.saturating_sub(1);
                self.dirty = true;
                true
            }
            UIEvent::Input(Key::Down) => {
                self.cursor.1 = self.cursor.1 + 1;
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
    }

    fn id(&self) -> ComponentId {
        ComponentId::nil()
    }
}

impl RawBuffer {
    pub fn new(buf: CellBuffer, title: Option<String>) -> Self {
        RawBuffer {
            buf,
            title,
            dirty: true,
            cursor: (0, 0),
        }
    }
    pub fn title(&self) -> &str {
        self.title
            .as_ref()
            .map(String::as_str)
            .unwrap_or("untitled")
    }
}
