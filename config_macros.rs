/*
 * meli -
 *
 * Copyright  Manos Pitsidianakis
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

use std::fs::File;
use std::io::prelude::*;
use std::process::{Command, Stdio};

use quote::{format_ident, quote};

// Write ConfigStructOverride to overrides.rs
pub fn override_derive(filenames: &[(&str, &str)]) {
    let mut output_file =
        File::create("src/conf/overrides.rs").expect("Unable to open output file");
    let mut output_string = r##"/*
 * meli - conf/overrides.rs
 *
 * Copyright 2020 Manos Pitsidianakis
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

//! This module is automatically generated by build.rs.
use super::*;

"##
    .to_string();

    'file_loop: for (filename, ident) in filenames {
        println!("cargo:rerun-if-changed={}", filename);
        let mut file = File::open(&filename).expect(&format!("Unable to open file `{}`", filename));

        let mut src = String::new();
        file.read_to_string(&mut src).expect("Unable to read file");

        let syntax = syn::parse_file(&src).expect("Unable to parse file");
        if syntax.items.iter().any(|item| {
            if let syn::Item::Struct(s) = item {
                if s.ident.to_string().ends_with("Override") {
                    println!("ident {} exists, skipping {}", ident, filename);
                    return true;
                }
            }
            false
        }) {
            continue 'file_loop;
        }

        for item in syntax.items.iter() {
            if let syn::Item::Struct(s) = item {
                if s.ident != ident {
                    continue;
                }
                if s.ident.to_string().ends_with("Override") {
                    unreachable!();
                }
                let override_ident: syn::Ident = format_ident!("{}Override", s.ident);
                let mut field_tokentrees = vec![];
                let mut field_idents = vec![];
                for f in &s.fields {
                    let ident = &f.ident;
                    let ty = &f.ty;
                    let attrs = f
                        .attrs
                        .iter()
                        .filter_map(|f| {
                            let mut new_attr = f.clone();
                            if let quote::__private::TokenTree::Group(g) =
                                f.tokens.clone().into_iter().next().unwrap()
                            {
                                let attr_inner_value = f.tokens.to_string();
                                if !attr_inner_value.starts_with("( default")
                                    && !attr_inner_value.starts_with("( default =")
                                {
                                    return None;
                                }
                                if attr_inner_value.starts_with("( default =") {
                                    let rest = g.stream().clone().into_iter().skip(4);
                                    new_attr.tokens = quote! { ( #(#rest)*) };
                                    if new_attr.tokens.to_string().as_str() == "( )" {
                                        return None;
                                    }
                                } else if attr_inner_value.starts_with("( default") {
                                    let rest = g.stream().clone().into_iter().skip(2);
                                    new_attr.tokens = quote! { ( #(#rest)*) };
                                    if new_attr.tokens.to_string().as_str() == "( )" {
                                        return None;
                                    }
                                }
                            }

                            Some(new_attr)
                        })
                        .collect::<Vec<_>>();
                    let t = quote! {
                        #(#attrs)*
                        #[serde(default)]
                        pub #ident : Option<#ty>
                    };
                    field_idents.push(ident);
                    field_tokentrees.push(t);
                }
                //let fields = &s.fields;

                let literal_struct = quote! {
                    #[derive(Debug, Serialize, Deserialize, Clone)]
                    pub struct #override_ident {
                        #(#field_tokentrees),*
                    }


                    impl Default for #override_ident {
                        fn default() -> Self {
                            #override_ident {
                                #(#field_idents: None),*
                            }
                        }
                    }
                };
                output_string.extend(literal_struct.to_string().chars());
                output_string.push_str("\n\n");
            }
        }
    }

    /*
    let mut rustfmt = Command::new("rustfmt")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to execute rustfmt");

    {
        // limited borrow of stdin
        let stdin = rustfmt
            .stdin
            .as_mut()
            .expect("failed to get rustfmt stdin");
        stdin
            .write_all(output_string.as_bytes())
            .expect("failed to write to rustfmit stdin");
    }

    let output = rustfmt
        .wait_with_output()
        .expect("failed to wait on rustfmt child");
    if !output.stderr.is_empty() {
        panic!(format!("{}", String::from_utf8_lossy(&output.stderr)));
    }

    output_file.write_all(&output.stdout).unwrap();
    */
    output_file.write_all(output_string.as_bytes()).unwrap();
}
