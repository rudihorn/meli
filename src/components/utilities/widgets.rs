/*
 * meli
 *
 * Copyright 2017-2020 Manos Pitsidianakis
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
use std::borrow::Cow;
use std::collections::HashMap;

type AutoCompleteFn = Box<dyn Fn(&Context, &str) -> Vec<AutoCompleteEntry> + Send + Sync>;

#[derive(Debug, PartialEq)]
enum FormFocus {
    Fields,
    Buttons,
    TextInput,
}

type Cursor = usize;

impl Default for FormFocus {
    fn default() -> FormFocus {
        FormFocus::Fields
    }
}

pub enum Field {
    Text(UText, Option<(AutoCompleteFn, AutoComplete)>),
    Choice(Vec<Cow<'static, str>>, Cursor),
}

impl Debug for Field {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Text(s, _) => fmt::Debug::fmt(s, f),
            k => fmt::Debug::fmt(k, f),
        }
    }
}

use crate::Field::*;

impl Default for Field {
    fn default() -> Field {
        Field::Text(UText::new(String::with_capacity(256)), None)
    }
}

impl Field {
    pub fn as_str(&self) -> &str {
        match self {
            Text(ref s, _) => s.as_str(),
            Choice(ref v, cursor) => {
                if v.is_empty() {
                    ""
                } else {
                    v[*cursor].as_ref()
                }
            }
        }
    }

    pub fn cursor(&self) -> usize {
        match self {
            Text(ref s, _) => s.grapheme_pos(),
            Choice(_, ref cursor) => *cursor,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.as_str().is_empty()
    }

    pub fn into_string(self) -> String {
        match self {
            Text(s, _) => s.into_string(),
            Choice(mut v, cursor) => v.remove(cursor).to_string(),
        }
    }

    pub fn clear(&mut self) {
        match self {
            Text(s, _) => s.clear(),
            Choice(_, _) => {}
        }
    }

    pub fn draw_cursor(
        &mut self,
        grid: &mut CellBuffer,
        area: Area,
        secondary_area: Area,
        context: &mut Context,
    ) {
        let upper_left = upper_left!(area);
        match self {
            Text(ref term, auto_complete_fn) => {
                change_colors(
                    grid,
                    (
                        pos_inc(upper_left, (term.grapheme_pos(), 0)),
                        (pos_inc(upper_left, (term.grapheme_pos(), 0))),
                    ),
                    Color::Default,
                    Color::Byte(248),
                );
                if term.grapheme_len() <= 2 {
                    return;
                }
                if let Some((auto_complete_fn, auto_complete)) = auto_complete_fn {
                    let entries = auto_complete_fn(context, term.as_str());
                    auto_complete.set_suggestions(entries);
                    auto_complete.draw(grid, secondary_area, context);
                }
            }
            Choice(_, _cursor) => {}
        }
    }
}

impl Component for Field {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        let theme_attr = crate::conf::value(context, "widgets.form.field");
        write_string_to_grid(
            self.as_str(),
            grid,
            theme_attr.fg,
            theme_attr.bg,
            theme_attr.attrs,
            area,
            None,
        );
    }
    fn process_event(&mut self, event: &mut UIEvent, context: &mut Context) -> bool {
        match *event {
            UIEvent::InsertInput(Key::Char('\t')) => {
                if let Text(ref mut s, Some((_, auto_complete))) = self {
                    if let Some(suggestion) = auto_complete.get_suggestion() {
                        *s = UText::new(suggestion);
                        let len = s.as_str().len();
                        s.set_cursor(len);
                        return true;
                    }
                }
                if let Text(ref mut s, _) = self {
                    s.insert_char(' ');
                }
                return true;
            }
            UIEvent::InsertInput(Key::Char('\n')) => {
                if let Text(ref mut s, Some((_, auto_complete))) = self {
                    if let Some(suggestion) = auto_complete.get_suggestion() {
                        *s = UText::new(suggestion);
                        let len = s.as_str().len();
                        s.set_cursor(len);
                    }
                }
                context
                    .replies
                    .push_back(UIEvent::ChangeMode(UIMode::Normal));
                return true;
            }
            UIEvent::InsertInput(Key::Up) => {
                if let Text(_, Some((_, auto_complete))) = self {
                    auto_complete.dec_cursor();
                } else {
                    return false;
                }
            }
            UIEvent::InsertInput(Key::Down) => {
                if let Text(_, Some((_, auto_complete))) = self {
                    auto_complete.inc_cursor();
                } else {
                    return false;
                }
            }
            UIEvent::InsertInput(Key::Right) => match self {
                Text(ref mut s, _) => {
                    s.cursor_inc();
                }
                Choice(ref vec, ref mut cursor) => {
                    *cursor = if *cursor == vec.len().saturating_sub(1) {
                        0
                    } else {
                        *cursor + 1
                    };
                }
            },
            UIEvent::InsertInput(Key::Left) => match self {
                Text(ref mut s, _) => {
                    s.cursor_dec();
                }
                Choice(_, ref mut cursor) => {
                    if *cursor == 0 {
                        return false;
                    } else {
                        *cursor -= 1;
                    }
                }
            },
            UIEvent::InsertInput(Key::Char(k)) => {
                if let Text(ref mut s, _) = self {
                    s.insert_char(k);
                }
            }
            UIEvent::InsertInput(Key::Paste(ref p)) => {
                if let Text(ref mut s, _) = self {
                    for c in p.chars() {
                        s.insert_char(c);
                    }
                }
            }
            UIEvent::InsertInput(Key::Backspace) | UIEvent::InsertInput(Key::Ctrl('h')) => {
                if let Text(ref mut s, auto_complete) = self {
                    s.backspace();
                    if let Some(ac) = auto_complete.as_mut() {
                        ac.1.set_suggestions(Vec::new());
                    }
                }
            }
            UIEvent::InsertInput(Key::Ctrl('a')) => {
                if let Text(ref mut s, _) = self {
                    s.set_cursor(0);
                }
            }
            UIEvent::InsertInput(Key::Ctrl('b')) => {
                /* Backward one character */
                if let Text(ref mut s, _) = self {
                    s.cursor_dec();
                }
            }
            UIEvent::InsertInput(Key::Ctrl('f')) => {
                /* Forward one character */
                if let Text(ref mut s, _) = self {
                    s.cursor_inc();
                }
            }
            UIEvent::InsertInput(Key::Ctrl('w')) => {
                /* Cut previous word */
                if let Text(ref mut s, _) = self {
                    while s.as_str()[..s.cursor_pos()]
                        .last_grapheme()
                        .map(|(_, graph)| !graph.is_empty() && graph.trim().is_empty())
                        .unwrap_or(false)
                    {
                        s.backspace();
                    }
                    while s.as_str()[..s.cursor_pos()]
                        .last_grapheme()
                        .map(|(_, graph)| !graph.is_empty() && !graph.trim().is_empty())
                        .unwrap_or(false)
                    {
                        s.backspace();
                    }
                }
            }
            UIEvent::InsertInput(Key::Ctrl('u')) => {
                if let Text(ref mut s, _) = self {
                    s.cut_left()
                }
            }
            UIEvent::InsertInput(Key::Ctrl('e')) => {
                if let Text(ref mut s, _) = self {
                    s.set_cursor(s.as_str().len());
                }
            }
            /* TODO: add rest of readline shortcuts */
            _ => {
                return false;
            }
        }
        self.set_dirty(true);
        true
    }
    fn is_dirty(&self) -> bool {
        false
    }
    fn set_dirty(&mut self, _value: bool) {}

    fn id(&self) -> ComponentId {
        ComponentId::nil()
    }
    fn set_id(&mut self, _id: ComponentId) {}
}

impl fmt::Display for Field {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match self {
                Text(ref s, _) => s.as_str(),
                Choice(ref v, ref cursor) => v[*cursor].as_ref(),
            }
        )
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum FormButtonActions {
    Accept,
    Reset,
    Cancel,
    Other(&'static str),
}

impl Default for FormButtonActions {
    fn default() -> Self {
        FormButtonActions::Cancel
    }
}

#[derive(Debug, Default)]
pub struct FormWidget<T>
where
    T: 'static + std::fmt::Debug + Copy + Default + Send + Sync,
{
    fields: HashMap<Cow<'static, str>, Field>,
    layout: Vec<Cow<'static, str>>,
    buttons: ButtonWidget<T>,

    field_name_max_length: usize,
    cursor: usize,
    focus: FormFocus,
    hide_buttons: bool,
    dirty: bool,
    id: ComponentId,
}

impl<T: 'static + std::fmt::Debug + Copy + Default + Send + Sync> fmt::Display for FormWidget<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt("", f)
    }
}

impl<T: 'static + std::fmt::Debug + Copy + Default + Send + Sync> FormWidget<T> {
    pub fn new(action: (Cow<'static, str>, T)) -> FormWidget<T> {
        FormWidget {
            buttons: ButtonWidget::new(action),
            focus: FormFocus::Fields,
            hide_buttons: false,
            id: ComponentId::new_v4(),
            dirty: true,
            ..Default::default()
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, new_cursor: usize) {
        self.cursor = new_cursor;
    }

    pub fn hide_buttons(&mut self) {
        self.hide_buttons = true;
        self.buttons.set_dirty(false);
    }

    pub fn len(&self) -> usize {
        self.layout.len()
    }

    pub fn add_button(&mut self, val: (Cow<'static, str>, T)) {
        self.buttons.push(val);
    }

    pub fn push_choices(&mut self, value: (Cow<'static, str>, Vec<Cow<'static, str>>)) {
        self.field_name_max_length = std::cmp::max(self.field_name_max_length, value.0.len());
        self.layout.push(value.0.clone());
        self.fields.insert(value.0, Choice(value.1, 0));
    }
    pub fn push_cl(&mut self, value: (Cow<'static, str>, String, AutoCompleteFn)) {
        self.field_name_max_length = std::cmp::max(self.field_name_max_length, value.0.len());
        self.layout.push(value.0.clone());
        self.fields.insert(
            value.0,
            Text(
                UText::new(value.1),
                Some((value.2, AutoComplete::new(Vec::new()))),
            ),
        );
    }
    pub fn push(&mut self, value: (Cow<'static, str>, String)) {
        self.field_name_max_length = std::cmp::max(self.field_name_max_length, value.0.len());
        self.layout.push(value.0.clone());
        self.fields.insert(value.0, Text(UText::new(value.1), None));
    }

    pub fn insert(&mut self, index: usize, value: (Cow<'static, str>, Field)) {
        self.layout.insert(index, value.0.clone());
        self.fields.insert(value.0, value.1);
    }

    pub fn values(&self) -> &HashMap<Cow<'static, str>, Field> {
        &self.fields
    }

    pub fn values_mut(&mut self) -> &mut HashMap<Cow<'static, str>, Field> {
        &mut self.fields
    }

    pub fn collect(self) -> Option<HashMap<Cow<'static, str>, Field>> {
        if self.buttons_result().is_some() {
            Some(self.fields)
        } else {
            None
        }
    }
    pub fn buttons_result(&self) -> Option<T> {
        self.buttons.result
    }
}

impl<T: 'static + std::fmt::Debug + Copy + Default + Send + Sync> Component for FormWidget<T> {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        let upper_left = upper_left!(area);
        let bottom_right = bottom_right!(area);

        if self.is_dirty() {
            let theme_default = crate::conf::value(context, "theme_default");
            clear_area(
                grid,
                (
                    upper_left,
                    set_y(bottom_right, get_y(upper_left) + self.layout.len()),
                ),
                theme_default,
            );
            let label_attrs = crate::conf::value(context, "widgets.form.label");

            for (i, k) in self.layout.iter().enumerate() {
                let v = self.fields.get_mut(k).unwrap();
                /* Write field label */
                write_string_to_grid(
                    k.as_ref(),
                    grid,
                    label_attrs.fg,
                    label_attrs.bg,
                    label_attrs.attrs,
                    (
                        pos_inc(upper_left, (1, i)),
                        set_y(bottom_right, i + get_y(upper_left)),
                    ),
                    None,
                );
                /* draw field */
                v.draw(
                    grid,
                    (
                        pos_inc(upper_left, (self.field_name_max_length + 3, i)),
                        set_y(bottom_right, i + get_y(upper_left)),
                    ),
                    context,
                );

                /* Highlight if necessary */
                if i == self.cursor {
                    if self.focus == FormFocus::Fields {
                        let mut field_attrs =
                            crate::conf::value(context, "widgets.form.highlighted");
                        if !context.settings.terminal.use_color() {
                            field_attrs.attrs |= Attr::REVERSE;
                        }
                        for row in grid.bounds_iter((
                            pos_inc(upper_left, (0, i)),
                            (get_x(bottom_right).saturating_sub(1), i + get_y(upper_left)),
                        )) {
                            for c in row {
                                grid[c]
                                    .set_fg(field_attrs.fg)
                                    .set_bg(field_attrs.bg)
                                    .set_attrs(field_attrs.attrs);
                            }
                        }
                    }
                    if self.focus == FormFocus::TextInput {
                        v.draw_cursor(
                            grid,
                            (
                                pos_inc(upper_left, (self.field_name_max_length + 3, i)),
                                (
                                    get_x(upper_left) + self.field_name_max_length + 3,
                                    i + get_y(upper_left),
                                ),
                            ),
                            (
                                pos_inc(upper_left, (self.field_name_max_length + 3, i + 1)),
                                bottom_right,
                            ),
                            context,
                        );
                    }
                }
            }

            let length = self.layout.len();
            clear_area(
                grid,
                (
                    pos_inc(upper_left, (0, length)),
                    set_y(bottom_right, length + 2 + get_y(upper_left)),
                ),
                theme_default,
            );
            if !self.hide_buttons {
                self.buttons.draw(
                    grid,
                    (
                        pos_inc(upper_left, (1, length + 3)),
                        set_y(bottom_right, length + 3 + get_y(upper_left)),
                    ),
                    context,
                );
            }
            clear_area(
                grid,
                (
                    set_y(upper_left, length + 4 + get_y(upper_left)),
                    bottom_right,
                ),
                theme_default,
            );
            self.set_dirty(false);
            context.dirty_areas.push_back(area);
        }
    }
    fn process_event(&mut self, event: &mut UIEvent, context: &mut Context) -> bool {
        if self.focus == FormFocus::Buttons && self.buttons.process_event(event, context) {
            return true;
        }

        match *event {
            UIEvent::Input(Key::Up) if self.focus == FormFocus::Buttons => {
                self.focus = FormFocus::Fields;
                self.buttons.set_focus(false);
                self.set_dirty(true);
                return true;
            }
            UIEvent::InsertInput(Key::Up) if self.focus == FormFocus::TextInput => {
                let field = self.fields.get_mut(&self.layout[self.cursor]).unwrap();
                field.process_event(event, context);
                self.set_dirty(true);
                return true;
            }
            UIEvent::Input(Key::Up) => {
                self.cursor = self.cursor.saturating_sub(1);
                self.set_dirty(true);
                return true;
            }
            UIEvent::InsertInput(Key::Down) if self.focus == FormFocus::TextInput => {
                let field = self.fields.get_mut(&self.layout[self.cursor]).unwrap();
                field.process_event(event, context);
                self.set_dirty(true);
                return true;
            }
            UIEvent::Input(Key::Down) if self.cursor < self.layout.len().saturating_sub(1) => {
                self.cursor += 1;
                self.set_dirty(true);
                return true;
            }
            UIEvent::Input(Key::Down) if self.focus == FormFocus::Fields => {
                self.focus = FormFocus::Buttons;
                self.buttons.set_focus(true);
                self.set_dirty(true);
                if self.hide_buttons {
                    return false;
                }
                return true;
            }
            UIEvent::InsertInput(Key::Char('\t')) if self.focus == FormFocus::TextInput => {
                let field = self.fields.get_mut(&self.layout[self.cursor]).unwrap();
                field.process_event(event, context);
                self.set_dirty(true);
                return true;
            }
            UIEvent::Input(Key::Char('\n')) if self.focus == FormFocus::Fields => {
                self.focus = FormFocus::TextInput;
                context
                    .replies
                    .push_back(UIEvent::ChangeMode(UIMode::Insert));
                self.set_dirty(true);
                return true;
            }
            UIEvent::InsertInput(Key::Right) if self.focus == FormFocus::TextInput => {
                let field = self.fields.get_mut(&self.layout[self.cursor]).unwrap();
                field.process_event(event, context);
                self.set_dirty(true);
                return true;
            }
            UIEvent::InsertInput(Key::Left) if self.focus == FormFocus::TextInput => {
                let field = self.fields.get_mut(&self.layout[self.cursor]).unwrap();
                if !field.process_event(event, context) {
                    self.focus = FormFocus::Fields;
                    context
                        .replies
                        .push_back(UIEvent::ChangeMode(UIMode::Normal));
                }
                self.set_dirty(true);
                return true;
            }
            UIEvent::ChangeMode(UIMode::Normal) if self.focus == FormFocus::TextInput => {
                self.focus = FormFocus::Fields;
                return false;
            }
            UIEvent::InsertInput(Key::Backspace) if self.focus == FormFocus::TextInput => {
                let field = self.fields.get_mut(&self.layout[self.cursor]).unwrap();
                field.process_event(event, context);
                self.set_dirty(true);
                return true;
            }
            UIEvent::InsertInput(_) if self.focus == FormFocus::TextInput => {
                let field = self.fields.get_mut(&self.layout[self.cursor]).unwrap();
                field.process_event(event, context);
                self.set_dirty(true);
                return true;
            }
            _ => {}
        }
        false
    }
    fn is_dirty(&self) -> bool {
        self.dirty || self.buttons.is_dirty()
    }
    fn set_dirty(&mut self, value: bool) {
        self.dirty = value;
        self.buttons.set_dirty(value);
    }

    fn id(&self) -> ComponentId {
        self.id
    }
    fn set_id(&mut self, id: ComponentId) {
        self.id = id;
    }
}

#[derive(Debug, Default)]
pub struct ButtonWidget<T>
where
    T: 'static + std::fmt::Debug + Copy + Default + Send + Sync,
{
    buttons: HashMap<Cow<'static, str>, T>,
    layout: Vec<Cow<'static, str>>,

    result: Option<T>,
    cursor: usize,
    /// Is the button widget focused, i.e do we need to draw the highlighting?
    focus: bool,
    dirty: bool,
    id: ComponentId,
}

impl<T> fmt::Display for ButtonWidget<T>
where
    T: 'static + std::fmt::Debug + Copy + Default + Send + Sync,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt("", f)
    }
}

impl<T> ButtonWidget<T>
where
    T: 'static + std::fmt::Debug + Copy + Default + Send + Sync,
{
    pub fn new(init_val: (Cow<'static, str>, T)) -> ButtonWidget<T> {
        ButtonWidget {
            layout: vec![init_val.0.clone()],
            buttons: vec![init_val].into_iter().collect(),
            result: None,
            cursor: 0,
            focus: false,
            dirty: true,
            id: ComponentId::new_v4(),
        }
    }

    pub fn push(&mut self, value: (Cow<'static, str>, T)) {
        self.layout.push(value.0.clone());
        self.buttons.insert(value.0, value.1);
    }

    pub fn is_resolved(&self) -> bool {
        self.result.is_some()
    }

    pub fn result(&self) -> Option<T> {
        self.result
    }

    pub fn set_focus(&mut self, new_val: bool) {
        self.focus = new_val;
    }

    pub fn set_cursor(&mut self, new_val: usize) {
        if self.buttons.is_empty() {
            return;
        }
        self.cursor = new_val % self.buttons.len();
    }
}

impl<T> Component for ButtonWidget<T>
where
    T: 'static + std::fmt::Debug + Copy + Default + Send + Sync,
{
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if self.dirty {
            let theme_default = crate::conf::value(context, "theme_default");
            clear_area(grid, area, theme_default);
            let upper_left = upper_left!(area);

            let mut len = 0;
            for (i, k) in self.layout.iter().enumerate() {
                let cur_len = k.len();
                write_string_to_grid(
                    k.as_ref(),
                    grid,
                    theme_default.fg,
                    if i == self.cursor && self.focus {
                        Color::Byte(246)
                    } else {
                        theme_default.bg
                    },
                    Attr::BOLD,
                    (
                        pos_inc(upper_left, (len, 0)),
                        pos_inc(upper_left, (cur_len + len, 0)),
                    ),
                    None,
                );
                len += cur_len + 3;
            }
            self.dirty = false;
        }
    }
    fn process_event(&mut self, event: &mut UIEvent, _context: &mut Context) -> bool {
        match *event {
            UIEvent::Input(Key::Char('\n')) => {
                self.result = Some(
                    self.buttons
                        .remove(&self.layout[self.cursor])
                        .unwrap_or_default(),
                );
                self.set_dirty(true);
                return true;
            }
            UIEvent::Input(Key::Left) => {
                self.cursor = self.cursor.saturating_sub(1);
                self.set_dirty(true);
                return true;
            }
            UIEvent::Input(Key::Right) if self.cursor < self.layout.len().saturating_sub(1) => {
                self.cursor += 1;
                self.set_dirty(true);
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

#[derive(Debug, PartialEq, Clone)]
pub struct AutoCompleteEntry {
    pub entry: String,
    pub description: String,
}

impl AutoCompleteEntry {
    pub fn as_str(&self) -> &str {
        self.entry.as_str()
    }
}

impl From<String> for AutoCompleteEntry {
    fn from(val: String) -> Self {
        AutoCompleteEntry {
            entry: val,
            description: String::new(),
        }
    }
}

impl From<&(&str, &str, TokenStream)> for AutoCompleteEntry {
    fn from(val: &(&str, &str, TokenStream)) -> Self {
        let (a, b, _) = val;
        AutoCompleteEntry {
            entry: a.to_string(),
            description: b.to_string(),
        }
    }
}

impl From<(String, String)> for AutoCompleteEntry {
    fn from(val: (String, String)) -> Self {
        let (a, b) = val;
        AutoCompleteEntry {
            entry: a,
            description: b,
        }
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct AutoComplete {
    entries: Vec<AutoCompleteEntry>,
    content: CellBuffer,
    cursor: usize,

    dirty: bool,
    id: ComponentId,
}

impl fmt::Display for AutoComplete {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt("AutoComplete", f)
    }
}

impl Component for AutoComplete {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if self.entries.is_empty() {
            return;
        };
        self.dirty = false;

        let (upper_left, bottom_right) = area;
        let rows = get_y(bottom_right) - get_y(upper_left);
        if rows == 0 {
            return;
        }
        let page_no = (self.cursor.saturating_sub(1)).wrapping_div(rows);
        let top_idx = page_no * rows;
        let x_offset = if rows < self.entries.len() { 1 } else { 0 };

        let (width, height) = self.content.size();
        clear_area(grid, area, crate::conf::value(context, "theme_default"));
        copy_area(
            grid,
            &self.content,
            (upper_left, pos_dec(bottom_right, (x_offset, 0))),
            (
                (0, top_idx),
                (width.saturating_sub(1), height.saturating_sub(1)),
            ),
        );
        /* Highlight cursor */
        if self.cursor > 0 {
            change_colors(
                grid,
                (
                    pos_inc(upper_left, (0, (self.cursor - 1) % rows)),
                    (
                        std::cmp::min(
                            get_x(upper_left) + width.saturating_sub(1),
                            get_x(bottom_right),
                        )
                        .saturating_sub(x_offset),
                        get_y(pos_inc(upper_left, (0, (self.cursor - 1) % rows))),
                    ),
                ),
                Color::Default,
                Color::Byte(246),
            );
        }
        if rows < self.entries.len() {
            ScrollBar { show_arrows: false }.draw(
                grid,
                (
                    set_y(pos_dec(bottom_right, (x_offset, 0)), get_y(upper_left)),
                    pos_dec(bottom_right, (x_offset, 0)),
                ),
                context,
                self.cursor.saturating_sub(1),
                rows,
                self.entries.len(),
            );
        }
        context.dirty_areas.push_back(area);
    }
    fn process_event(&mut self, _event: &mut UIEvent, _context: &mut Context) -> bool {
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

impl AutoComplete {
    pub fn new(entries: Vec<AutoCompleteEntry>) -> Self {
        let mut ret = AutoComplete {
            entries: Vec::new(),
            content: CellBuffer::default(),
            cursor: 0,
            dirty: true,
            id: ComponentId::new_v4(),
        };
        ret.set_suggestions(entries);
        ret
    }

    pub fn set_suggestions(&mut self, entries: Vec<AutoCompleteEntry>) -> bool {
        if entries.len() == self.entries.len() && entries == self.entries {
            return false;
        }

        let mut content = CellBuffer::new(
            entries
                .iter()
                .map(|a| a.entry.grapheme_len() + a.description.grapheme_len() + 2)
                .max()
                .unwrap_or(0)
                + 1,
            entries.len(),
            Cell::with_style(Color::Byte(23), Color::Byte(7), Attr::DEFAULT),
        );
        let width = content.cols();
        for (i, e) in entries.iter().enumerate() {
            let (x, _) = write_string_to_grid(
                &e.entry,
                &mut content,
                Color::Byte(23),
                Color::Byte(7),
                Attr::DEFAULT,
                ((0, i), (width - 1, i)),
                None,
            );
            write_string_to_grid(
                &e.description,
                &mut content,
                Color::Byte(23),
                Color::Byte(7),
                Attr::ITALICS,
                ((x + 2, i), (width - 1, i)),
                None,
            );
            write_string_to_grid(
                "▒",
                &mut content,
                Color::Byte(23),
                Color::Byte(7),
                Attr::DEFAULT,
                ((width - 1, i), (width - 1, i)),
                None,
            );
        }
        self.content = content;
        self.entries = entries;
        self.cursor = 0;
        true
    }

    pub fn inc_cursor(&mut self) {
        if self.cursor < self.entries.len() {
            self.cursor += 1;
            self.set_dirty(true);
        }
    }
    pub fn dec_cursor(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
        self.set_dirty(true);
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, val: usize) {
        debug_assert!(val <= self.entries.len());
        self.cursor = val;
    }

    pub fn get_suggestion(&mut self) -> Option<String> {
        if self.entries.is_empty() || self.cursor == 0 {
            return None;
        }
        let ret = self.entries.remove(self.cursor - 1);
        self.entries.clear();
        self.cursor = 0;
        self.content.empty();
        Some(ret.entry)
    }

    pub fn suggestions(&self) -> &Vec<AutoCompleteEntry> {
        &self.entries
    }
}

#[derive(Default)]
pub struct ScrollBar {
    pub show_arrows: bool,
}

impl ScrollBar {
    pub fn set_show_arrows(&mut self, new_val: bool) -> &mut Self {
        self.show_arrows = new_val;
        self
    }

    pub fn draw(
        &mut self,
        grid: &mut CellBuffer,
        area: Area,
        context: &Context,
        pos: usize,
        visible_rows: usize,
        length: usize,
    ) {
        if length == 0 {
            return;
        }
        let mut height = height!(area);
        if height < 3 {
            return;
        }
        clear_area(grid, area, crate::conf::value(context, "theme_default"));

        let visible_ratio: f64 = (std::cmp::min(visible_rows, length) as f64) / (length as f64);
        let scrollbar_height = std::cmp::min(
            height.saturating_sub(1),
            std::cmp::max((visible_ratio * (height as f64)) as usize, 1),
        );
        if self.show_arrows {
            height -= 3;
        }

        let scrollbar_offset = {
            let temp = (((pos as f64) / (length as f64)) * (height as f64)) as usize;
            if scrollbar_height + temp > height {
                height.saturating_sub(scrollbar_height)
            } else {
                temp
            }
        };
        let (mut upper_left, bottom_right) = area;

        if self.show_arrows {
            grid[upper_left]
                .set_ch('▄')
                .set_fg(crate::conf::value(context, "widgets.options.highlighted").bg);
            upper_left = pos_inc(upper_left, (0, 1));
        }

        upper_left = pos_inc(upper_left, (0, scrollbar_offset));
        for _ in 0..scrollbar_height {
            grid[upper_left].set_bg(crate::conf::value(context, "widgets.options.highlighted").bg);
            upper_left = pos_inc(upper_left, (0, 1));
        }
        if self.show_arrows {
            grid[pos_dec(bottom_right, (0, 1))]
                .set_ch('▀')
                .set_fg(crate::conf::value(context, "widgets.options.highlighted").bg)
                .set_bg(crate::conf::value(context, "theme_default").bg);
        }
    }

    pub fn draw_horizontal(
        &mut self,
        grid: &mut CellBuffer,
        area: Area,
        context: &Context,
        pos: usize,
        visible_cols: usize,
        length: usize,
    ) {
        if length == 0 {
            return;
        }
        let mut width = width!(area);
        if width < 3 {
            return;
        }
        clear_area(grid, area, crate::conf::value(context, "theme_default"));

        let visible_ratio: f64 = (std::cmp::min(visible_cols, length) as f64) / (length as f64);
        let scrollbar_width = std::cmp::max((visible_ratio * (width as f64)) as usize, 1);
        if self.show_arrows {
            width -= 3;
        }
        let scrollbar_offset = {
            let temp = (((pos as f64) / (length as f64)) * (width as f64)) as usize;
            if scrollbar_width + temp > width {
                width.saturating_sub(scrollbar_width)
            } else {
                temp
            }
        };
        let (mut upper_left, bottom_right) = area;

        if self.show_arrows {
            grid[upper_left]
                .set_ch('▐')
                .set_fg(crate::conf::value(context, "widgets.options.highlighted").bg);
            upper_left = pos_inc(upper_left, (1, 0));
        }

        upper_left = pos_inc(upper_left, (scrollbar_offset, 0));
        for _ in 0..=scrollbar_width {
            grid[upper_left]
                .set_ch('█')
                .set_fg(crate::conf::value(context, "widgets.options.highlighted").bg);
            upper_left = pos_inc(upper_left, (1, 0));
        }
        if self.show_arrows {
            grid[pos_dec(bottom_right, (1, 0))]
                .set_ch('▌')
                .set_fg(crate::conf::value(context, "widgets.options.highlighted").bg)
                .set_bg(crate::conf::value(context, "theme_default").bg);
        }
    }
}

#[derive(Debug)]
pub struct ProgressSpinner {
    timer: crate::jobs::Timer,
    stage: usize,
    pub kind: std::result::Result<usize, Vec<String>>,
    pub width: usize,
    active: bool,
    dirty: bool,
    id: ComponentId,
}

impl ProgressSpinner {
    pub const KINDS: [&'static [&'static str]; 30] = [
        &["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"],
        &["⣀", "⣄", "⣤", "⣦", "⣶", "⣷", "⣿"],
        &["⣀", "⣄", "⣆", "⣇", "⣧", "⣷", "⣿"],
        &["○", "◔", "◐", "◕", "⬤"],
        &["□", "◱", "◧", "▣", "■"],
        &["□", "◱", "▨", "▩", "■"],
        &["□", "◱", "▥", "▦", "■"],
        &["░", "▒", "▓", "█"],
        &["░", "█"],
        &["⬜", "⬛"],
        &["▱", "▰"],
        &["▭", "◼"],
        &["▯", "▮"],
        &["◯", "⬤"],
        &["⚪", "⚫"],
        &["▖", "▗", "▘", "▝", "▞", "▚", "▙", "▟", "▜", "▛"],
        &["|", "/", "-", "\\"],
        &[".", "o", "O", "@", "*"],
        &["◡◡", "⊙⊙", "◠◠", "⊙⊙"],
        &["◜ ", " ◝", " ◞", "◟ "],
        &["←", "↖", "↑", "↗", "→", "↘", "↓", "↙"],
        &["▁", "▃", "▄", "▅", "▆", "▇", "█", "▇", "▆", "▅", "▄", "▃"],
        &[
            "▉", "▊", "▋", "▌", "▍", "▎", "▏", "▎", "▍", "▌", "▋", "▊", "▉",
        ],
        &["▖", "▘", "▝", "▗"],
        &["▌", "▀", "▐", "▄"],
        &["┤", "┘", "┴", "└", "├", "┌", "┬", "┐"],
        &["◢", "◣", "◤", "◥"],
        &["⠁", "⠂", "⠄", "⡀", "⢀", "⠠", "⠐", "⠈"],
        &["⢎⡰", "⢎⡡", "⢎⡑", "⢎⠱", "⠎⡱", "⢊⡱", "⢌⡱", "⢆⡱"],
        &[".", "o", "O", "°", "O", "o", "."],
    ];

    const INTERVAL: std::time::Duration = std::time::Duration::from_millis(50);

    pub fn new(kind: usize, context: &Context) -> Self {
        let timer = context
            .job_executor
            .clone()
            .create_timer(Self::INTERVAL, Self::INTERVAL);
        let kind = kind % Self::KINDS.len();
        let width = Self::KINDS[kind]
            .iter()
            .map(|f| f.grapheme_len())
            .max()
            .unwrap_or(0);
        ProgressSpinner {
            timer,
            stage: 0,
            kind: Ok(kind),
            width,
            dirty: true,
            active: false,
            id: ComponentId::new_v4(),
        }
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn set_kind(&mut self, kind: usize) {
        self.stage = 0;
        self.width = Self::KINDS[kind % Self::KINDS.len()]
            .iter()
            .map(|f| f.grapheme_len())
            .max()
            .unwrap_or(0);
        self.kind = Ok(kind % Self::KINDS.len());
        self.dirty = true;
    }

    pub fn set_custom_kind(&mut self, custom: Vec<String>) {
        self.stage = 0;
        self.width = custom.iter().map(|f| f.grapheme_len()).max().unwrap_or(0);
        if self.width == 0 {
            self.stop();
        }
        self.kind = Err(custom);
        self.dirty = true;
    }

    pub fn start(&mut self) {
        if self.width == 0 {
            return;
        }
        self.active = true;
        self.timer.rearm();
    }

    pub fn stop(&mut self) {
        self.active = false;
        self.stage = 0;
        self.timer.disable();
    }
}

impl Drop for ProgressSpinner {
    fn drop(&mut self) {
        self.stop();
    }
}

impl fmt::Display for ProgressSpinner {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "progress bar")
    }
}

impl Component for ProgressSpinner {
    fn draw(&mut self, grid: &mut CellBuffer, area: Area, context: &mut Context) {
        if self.dirty {
            let theme_attr = crate::conf::value(context, "status.bar");
            clear_area(grid, area, theme_attr);
            if self.active {
                write_string_to_grid(
                    match self.kind.as_ref() {
                        Ok(kind) => Self::KINDS[*kind][self.stage].as_ref(),
                        Err(custom) => custom[self.stage].as_ref(),
                    },
                    grid,
                    theme_attr.fg,
                    theme_attr.bg,
                    theme_attr.attrs,
                    area,
                    None,
                );
            }
            context.dirty_areas.push_back(area);
            self.dirty = false;
        }
    }

    fn process_event(&mut self, event: &mut UIEvent, _context: &mut Context) -> bool {
        match event {
            UIEvent::Timer(id) if *id == self.timer.id() => {
                match self.kind.as_ref() {
                    Ok(kind) => {
                        self.stage = (self.stage + 1).wrapping_rem(Self::KINDS[*kind].len());
                    }
                    Err(custom) => {
                        self.stage = (self.stage + 1).wrapping_rem(custom.len());
                    }
                }
                self.dirty = true;
                true
            }
            _ => false,
        }
    }

    fn set_dirty(&mut self, new_val: bool) {
        self.dirty = new_val;
    }

    fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn id(&self) -> ComponentId {
        self.id
    }

    fn set_id(&mut self, id: ComponentId) {
        self.id = id;
    }
}
