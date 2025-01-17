/*
 * meli - melib crate.
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

#[cfg(feature = "unicode_algorithms")]
include!("src/text_processing/types.rs");

fn main() -> Result<(), std::io::Error> {
    #[cfg(feature = "unicode_algorithms")]
    {
        /* Line break tables */
        use std::fs::File;
        use std::io::prelude::*;
        use std::io::BufReader;
        use std::path::Path;
        use std::process::{Command, Stdio};
        const LINE_BREAK_TABLE_URL: &str =
            "http://www.unicode.org/Public/UCD/latest/ucd/LineBreak.txt";

        let mod_path = Path::new("src/text_processing/tables.rs");
        if mod_path.exists() {
            eprintln!(
                "{} already exists, delete it if you want to replace it.",
                mod_path.display()
            );
            std::process::exit(0);
        }
        let mut child = Command::new("curl")
            .args(&["-o", "-", LINE_BREAK_TABLE_URL])
            .stdout(Stdio::piped())
            .stdin(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;

        let buf_reader = BufReader::new(child.stdout.take().unwrap());

        let mut line_break_table: Vec<(u32, u32, LineBreakClass)> = Vec::with_capacity(3800);
        for line in buf_reader.lines() {
            let line = line.unwrap();
            if line.starts_with('#') || line.starts_with(' ') || line.is_empty() {
                continue;
            }
            let tokens: &str = line.split_whitespace().next().unwrap();

            let semicolon_idx: usize = tokens.chars().position(|c| c == ';').unwrap();
            /* LineBreak.txt list is ascii encoded so we can assume each char takes one byte: */
            let chars_str: &str = &tokens[..semicolon_idx];

            let mut codepoint_iter = chars_str.split("..");

            let first_codepoint: u32 =
                u32::from_str_radix(std::dbg!(codepoint_iter.next().unwrap()), 16).unwrap();

            let sec_codepoint: u32 = codepoint_iter
                .next()
                .map(|v| u32::from_str_radix(std::dbg!(v), 16).unwrap())
                .unwrap_or(first_codepoint);
            let class = &tokens[semicolon_idx + 1..semicolon_idx + 1 + 2];
            line_break_table.push((first_codepoint, sec_codepoint, LineBreakClass::from(class)));
        }
        child.wait()?;

        let mut file = File::create(&mod_path)?;
        file.write_all(
            br#"/*
 * meli - text_processing crate.
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

use super::types::LineBreakClass::{self, *};

pub const LINE_BREAK_RULES: &[(u32, u32, LineBreakClass)] = &[
"#,
        )
        .unwrap();
        for l in &line_break_table {
            file.write_all(format!("    (0x{:X}, 0x{:X}, {:?}),\n", l.0, l.1, l.2).as_bytes())
                .unwrap();
        }
        file.write_all(b"];").unwrap();
    }
    Ok(())
}
