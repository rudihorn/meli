/*
 * meli - mailbox threading module.
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

/*!
 * Threading algorithm
 */

use error::Result;
use mailbox::email::*;
use mailbox::Mailbox;

extern crate fnv;
use self::fnv::FnvHashMap;
use std::borrow::Cow;
use std::ops::{Index, };
use std::str::FromStr;
use std::result::Result as StdResult;
use std::cell::{Ref, RefCell};
use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum SortOrder {
    Asc,
    Desc,
}

#[derive(Debug, Clone, PartialEq, Copy)]
pub enum SortField {
    Subject,
    Date,
}

impl Default for SortField {
    fn default() -> Self {
        SortField::Date
    }
}

impl Default for SortOrder {
    fn default() -> Self {
        SortOrder::Desc
    }
}

impl FromStr for SortField {
    type Err = ();
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        match s.trim() {
            "subject" | "s" | "sub" | "sbj" | "subj" => Ok(SortField::Subject),
            "date" | "d" => Ok(SortField::Date),
            _ => Err(()),
        }
    }
}

impl FromStr for SortOrder {
    type Err = ();
    fn from_str(s: &str) -> StdResult<Self, Self::Err> {
        match s.trim() {
            "asc" => Ok(SortOrder::Asc),
            "desc" => Ok(SortOrder::Desc),
            _ => Err(()),
        }
    }
}

type UnixTimestamp = u64;

/// A `Container` struct is needed to describe the thread tree forest during creation of threads.
/// Because of Rust's memory model, we store indexes of Envelopes inside a collection instead of
/// references and every reference is passed through the `Container` owner (a `Vec<Container>`).
//
/// `message` refers to a `Envelope` entry in a `Vec`. If it's empty, the `Container` is
/// nonexistent in our `Mailbox` but we know it exists (for example we have a copy
/// of a reply to a mail but we don't have its copy.
#[derive(Clone, Copy, Debug)]
pub struct Container {
    id: usize,
    message: Option<usize>,
    parent: Option<usize>,
    first_child: Option<usize>,
    next_sibling: Option<usize>,
    date: UnixTimestamp,
    indentation: usize,
    show_subject: bool,
}

#[derive(Clone, Debug)]
struct ContainerTree {
    id: usize,
    children: Option<Vec<ContainerTree>>,
    len: usize,
    has_unseen: bool,
}

impl ContainerTree {
    fn new(id: usize) -> Self {
        ContainerTree {
            id,
            children: None,
            len: 1,
            has_unseen: false,
        }
    }
}


#[derive(Clone, Debug, Default)]
pub struct Threads {
    containers: Vec<Container>,
    threaded_collection: Vec<usize>,
    root_set: Vec<usize>,
    tree: RefCell<Vec<ContainerTree>>,
    sort: RefCell<(SortField, SortOrder)>,
    subsort: RefCell<(SortField, SortOrder)>,
}

pub struct ThreadIterator<'a> {
    pos: usize,
    stack: Vec<usize>,
    tree: Ref<'a ,Vec<ContainerTree>>,
}
impl<'a> Iterator for ThreadIterator<'a> {
    type Item  = usize;
    fn next(&mut self) -> Option<usize> {
        {
            let mut tree = &(*self.tree);
            for i in &self.stack {
                tree = tree[*i].children.as_ref().unwrap();
            }
            if self.pos == tree.len() {
                if self.stack.is_empty() {
                    return None;
                }
                self.pos = self.stack.pop().unwrap() + 1;
            } else {
                debug_assert!(self.pos < tree.len());
                let ret = tree[self.pos].id;
                if tree[self.pos].children.is_some() {
                    self.stack.push(self.pos);
                    self.pos = 0;
                    return Some(ret);
                }
                self.pos += 1;
                return Some(ret);
            }
        }
        return self.next();
    }
}

impl<'a> IntoIterator for &'a Threads {
    type Item = usize;
    type IntoIter = ThreadIterator<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ThreadIterator { pos: 0, stack: Vec::new(), tree: self.tree.borrow() }
    }
}


pub struct RootIterator<'a> {
    pos: usize,
    tree: Ref<'a ,Vec<ContainerTree>>,
}

impl<'a> Iterator for RootIterator<'a> {
    type Item  = (usize, usize, bool);
    fn next(&mut self) -> Option<(usize, usize, bool)> {
        if self.pos == self.tree.len() {
            return None;
        }
        let node = &self.tree[self.pos];
        self.pos += 1;
        return Some((node.id, node.len, node.has_unseen));
    }
}

impl Threads {
    pub fn root_len(&self) -> usize {
        self.tree.borrow().len()
    }
    pub fn root_set(&self) -> &Vec<usize> {
        &self.root_set
    }
    pub fn root_set_iter(&self) ->  RootIterator {
        RootIterator { pos: 0, tree: self.tree.borrow() }
    }
    pub fn thread_to_mail(&self, i: usize) -> usize {
        let thread = self.containers[self.threaded_collection[i]];
        thread.message().unwrap()
    }
    pub fn threaded_collection(&self) -> &Vec<usize> {
        &self.threaded_collection
    }
    pub fn containers(&self) -> &Vec<Container> {
        &self.containers
    }

    fn inner_subsort_by(&self, subsort: (SortField, SortOrder), collection: &[Envelope]) {
        let tree = &mut self.tree.borrow_mut();
        let containers = &self.containers;
        for mut t in tree.iter_mut() {
            if let Some(ref mut c ) = t.children {
                c.sort_by(|a, b| { match subsort {
                    (SortField::Date, SortOrder::Desc) => {
                        let a = containers[a.id];
                        let b = containers[b.id];
                        b.date.cmp(&a.date)
                    }
                    (SortField::Date, SortOrder::Asc) => {
                        let a = containers[a.id];
                        let b = containers[b.id];
                        a.date.cmp(&b.date)
                    }
                    (SortField::Subject, SortOrder::Desc) => {
                        let a = containers[a.id].message();
                        let b = containers[b.id].message();

                        if a.is_none() || b.is_none() {
                            return Ordering::Equal;
                        }
                        let ma = &collection[a.unwrap()];
                        let mb = &collection[b.unwrap()];
                        ma.subject().cmp(&mb.subject())
                    }
                    (SortField::Subject, SortOrder::Asc) => {
                        let a = containers[a.id].message();
                        let b = containers[b.id].message();

                        if a.is_none() || b.is_none() {
                            return Ordering::Equal;
                        }
                        let ma = &collection[a.unwrap()];
                        let mb = &collection[b.unwrap()];
                        mb.subject().cmp(&ma.subject())
                    }
                }
                });
            }
        }
    }

    fn inner_sort_by(&self, sort: (SortField, SortOrder), collection: &[Envelope]) {
        let tree = &mut self.tree.borrow_mut();
        let containers = &self.containers;
        tree.sort_by(|a, b| { match sort {
            (SortField::Date, SortOrder::Desc) => {
                let a = containers[a.id];
                let b = containers[b.id];
                b.date.cmp(&a.date)

            }
            (SortField::Date, SortOrder::Asc) => {
                let a = containers[a.id];
                let b = containers[b.id];
                a.date.cmp(&b.date)
            }
            (SortField::Subject, SortOrder::Desc) => {
                let a = containers[a.id].message();
                let b = containers[b.id].message();

                if a.is_none() || b.is_none() {
                    return Ordering::Equal;
                }
                let ma = &collection[a.unwrap()];
                let mb = &collection[b.unwrap()];
                ma.subject().cmp(&mb.subject())
            }
            (SortField::Subject, SortOrder::Asc) => {
                let a = containers[a.id].message();
                let b = containers[b.id].message();

                if a.is_none() || b.is_none() {
                    return Ordering::Equal;
                }
                let ma = &collection[a.unwrap()];
                let mb = &collection[b.unwrap()];
                mb.subject().cmp(&ma.subject())
            }
        }
        });
    }
    pub fn sort_by(&self, sort: (SortField, SortOrder), subsort: (SortField, SortOrder), collection: &[Envelope]) {
        if *self.sort.borrow() != sort {
            self.inner_sort_by(sort, collection);
            *self.sort.borrow_mut() = sort;
        }
        if *self.subsort.borrow() != subsort {
            self.inner_subsort_by(subsort, collection);
            *self.subsort.borrow_mut() = subsort;
        }

    }

    pub fn build_collection(&mut self, collection: &[Envelope]) {
        fn build_threaded(
            tree: &mut ContainerTree,
            containers: &mut Vec<Container>,
            indentation: usize,
            threaded: &mut Vec<usize>,
            i: usize,
            root_subject_idx: usize,
            collection: &[Envelope],
            ) {
            let thread = containers[i];
            if let Some(msg_idx) = containers[root_subject_idx].message() {
                let root_subject = collection[msg_idx].subject();
                /* If the Container has no Message, but does have children, remove this container but
                 * promote its children to this level (that is, splice them in to the current child
                 * list.) */
                if indentation > 0 && thread.has_message() {
                    let subject = collection[thread.message().unwrap()].subject();
                    tree.has_unseen = !collection[thread.message().unwrap()].is_seen();
                    if subject == root_subject
                        || subject.starts_with("Re: ")
                            && subject.as_ref().ends_with(root_subject.as_ref())
                            {
                                containers[i].set_show_subject(false);
                            }
                }
            }
            if thread.has_parent() && !containers[thread.parent().unwrap()].has_message() {
                containers[i].parent = None;
            }
            let indentation = if thread.has_message() {
                containers[i].set_indentation(indentation);
                if !threaded.contains(&i) {
                    threaded.push(i);
                }
                indentation + 1
            } else if indentation > 0 {
                indentation
            } else {
                indentation + 1
            };

            if thread.has_children() {
                let mut child_vec = Vec::new();

                let mut fc = thread.first_child().unwrap();

                loop {
                    let mut new_child_tree = ContainerTree::new(fc);
                    build_threaded(&mut new_child_tree, containers, indentation, threaded, fc, i, collection);
                    tree.has_unseen |= new_child_tree.has_unseen;
                    child_vec.push(new_child_tree);
                    let thread_ = containers[fc];
                    if !thread_.has_sibling() {
                        break;
                    }
                    fc = thread_.next_sibling().unwrap();
                }
                tree.len = child_vec.iter().map(|c| c.len).sum();
                tree.children = Some(child_vec);
            }
        }
        let mut tree = Vec::new();
        for i in &self.root_set {
            let mut tree_node = ContainerTree::new(*i);
            build_threaded(
                &mut tree_node,
                &mut self.containers,
                0,
                &mut self.threaded_collection,
                *i,
                *i,
                collection,
                );
            tree.push(tree_node);
        }
        self.tree.replace(tree);
        self.inner_sort_by(*self.sort.borrow(), collection);
        self.inner_subsort_by(*self.subsort.borrow(), collection);
    }
}

impl Index<usize> for Threads {
    type Output = Container;

    fn index(&self, index: usize) -> &Container {
        self.containers.get(index).expect("thread index out of bounds")
    }
}


impl Container {
    pub fn date(&self) -> UnixTimestamp {
        self.date
    }
    pub fn message(&self) -> Option<usize> {
        self.message
    }
    pub fn parent(&self) -> Option<usize> {
        self.parent
    }
    pub fn has_parent(&self) -> bool {
        self.parent.is_some()
    }
    pub fn first_child(&self) -> Option<usize> {
        self.first_child
    }
    pub fn next_sibling(&self) -> Option<usize> {
        self.next_sibling
    }
    pub fn has_children(&self) -> bool {
        self.first_child.is_some()
    }
    pub fn has_sibling(&self) -> bool {
        self.next_sibling.is_some()
    }
    pub fn has_message(&self) -> bool {
        self.message.is_some()
    }
    fn set_indentation(&mut self, i: usize) {
        self.indentation = i;
    }
    pub fn indentation(&self) -> usize {
        self.indentation
    }
    fn is_descendant(&self, threads: &[Container], other: &Container) -> bool {
        if self == other {
            return true;
        }

        if let Some(v) = self.first_child {
            if threads[v].is_descendant(threads, other) {
                return true;
            }
        };
        if let Some(v) = self.next_sibling {
            if threads[v].is_descendant(threads, other) {
                return true;
            }
        };
        false
    }
    fn set_show_subject(&mut self, set: bool) -> () {
        self.show_subject = set;
    }
    pub fn show_subject(&self) -> bool {
        self.show_subject
    }
}

impl PartialEq for Container {
    fn eq(&self, other: &Container) -> bool {
        match (self.message, other.message) {
            (Some(s), Some(o)) => s == o,
            _ => self.id == other.id,
        }
    }
}

fn build_collection(
    threads: &mut Vec<Container>,
    id_table: &mut FnvHashMap<Cow<str>, usize>,
    collection: &mut [Envelope],
    ) -> () {
    for (i, x) in collection.iter_mut().enumerate() {
        let x_index; /* x's index in threads */
        let m_id = x.message_id_raw().into_owned();
        let m_id = Cow::from(m_id);
        if id_table.contains_key(&m_id) {
            let t = id_table[&m_id];
            /* the already existing Container should be empty, since we're
             * seeing this message for the first time */
            if threads[t].message.is_some() {
                /* skip duplicate message-id, but this should be handled instead */
                continue;
            }
            x_index = t;
            /* Store this message in the Container's message slot.  */
            threads[t].date = x.date();
            x.set_thread(t);
            threads[t].message = Some(i);
        } else {
            /* Create a new Container object holding this message */
            x_index = threads.len();
            threads.push(Container {
                message: Some(i),
                id: x_index,
                parent: None,
                first_child: None,
                next_sibling: None,
                date: x.date(),
                indentation: 0,
                show_subject: true,
            });
            x.set_thread(x_index);
            id_table.insert(m_id, x_index);
        }
        /* For each element in the message's References field:
         *
         * Find a Container object for the given Message-ID:
         * If there's one in id_table use that;
         * Otherwise, make (and index) one with a null Message
         *
         * Link the References field's Container together in the order implied by the References header.
         * If they are already linked, don't change the existing links.
         * Do not add a link if adding that link would introduce a loop: that is, before asserting A->B, search down the children of B to see if A is reachable, and also search down the children of A to see if B is reachable. If either is already reachable as a child of the other, don't add the link.
         */
        let mut curr_ref = x_index;
        let mut iasf = 0;
        for &r in x.references().iter().rev() {
            if iasf == 1 {
                continue;
            }
            iasf += 1;
            let r = String::from_utf8_lossy(r.raw());
            let parent_id = if id_table.contains_key(&r) {
                let p = id_table[r.as_ref()];
                if !(threads[p].is_descendant(threads, &threads[curr_ref])
                     || threads[curr_ref].is_descendant(threads, &threads[p]))
                {
                    threads[curr_ref].parent = Some(p);
                    if threads[p].first_child.is_none() {
                        threads[p].first_child = Some(curr_ref);
                    } else {
                        let mut child_iter = threads[p].first_child.unwrap();
                        while threads[child_iter].next_sibling.is_some() {
                            threads[child_iter].parent = Some(p);
                            child_iter = threads[child_iter].next_sibling.unwrap();
                        }
                        threads[child_iter].next_sibling = Some(curr_ref);
                        threads[child_iter].parent = Some(p);
                    }
                }
                p
            } else {
                let idx = threads.len();
                threads.push(Container {
                    message: None,
                    id: idx,
                    parent: None,
                    first_child: Some(curr_ref),
                    next_sibling: None,
                    date: x.date(),
                    indentation: 0,
                    show_subject: true,
                });
                if threads[curr_ref].parent.is_none() {
                    threads[curr_ref].parent = Some(idx);
                }
                /* Can't avoid copy here since we have different lifetimes */
                id_table.insert(Cow::from(r.into_owned()), idx);
                idx
            };
            /* update thread date */
            let mut parent_iter = parent_id;
            'date: loop {
                let p = &mut threads[parent_iter];
                if p.date < x.date() {
                    p.date = x.date();
                }
                match p.parent {
                    Some(p) => {
                        parent_iter = p;
                    }
                    None => {
                        break 'date;
                    }
                }
            }
            curr_ref = parent_id;
        }
    }
}

/// Builds threads from a collection.
pub fn build_threads(
    collection: &mut Vec<Envelope>,
    sent_folder: &Option<Result<Mailbox>>,
    ) -> Threads {
    /* To reconstruct thread information from the mails we need: */

    /* a vector to hold thread members */
    let mut threads: Vec<Container> = Vec::with_capacity((collection.len() as f64 * 1.2) as usize);
    /* A hash table of Message IDs */
    let mut id_table: FnvHashMap<Cow<str>, usize> =
        FnvHashMap::with_capacity_and_hasher(collection.len(), Default::default());

    /* Add each message to id_table and threads, and link them together according to the
     * References / In-Reply-To headers */
    build_collection(&mut threads, &mut id_table, collection);
    let mut idx = collection.len();
    let mut tidx = threads.len();
    /* Link messages from Sent folder if they are relevant to this folder.
     * This means that
     *  - if a message from Sent is a reply to a message in this folder, we will
     *    add it to the threading (but not the collection; non-threading users shouldn't care
     *    about this)
     *  - if a message in this folder is a reply to a message we sent, we will add it to the
     *    threading
     */

    if let Some(ref sent_box) = *sent_folder {
        if sent_box.is_ok() {
            let sent_mailbox = sent_box.as_ref();
            let sent_mailbox = sent_mailbox.unwrap();

            for x in &sent_mailbox.collection {
                let m_id = x.message_id_raw();
                let x_r_id = x.in_reply_to_raw();
                if id_table.contains_key(&m_id)
                    || (!x.in_reply_to_raw().is_empty()
                        && id_table.contains_key(&x.in_reply_to_raw()))
                    {
                        let mut x: Envelope = (*x).clone();
                        if id_table.contains_key(&m_id) {
                            let c = id_table[&m_id];
                            if threads[c].message.is_some() {
                                /* skip duplicate message-id, but this should be handled instead */
                                continue;
                            }
                            threads[c].message = Some(idx);
                            assert!(threads[c].has_children());
                            threads[c].date = x.date();
                            x.set_thread(c);
                        } else if !x.in_reply_to_raw().is_empty()
                            && id_table.contains_key(&x.in_reply_to_raw())
                            {
                                let p = id_table[&x_r_id];
                                let c = if id_table.contains_key(&m_id) {
                                    id_table[&m_id]
                                } else {
                                    threads.push(Container {
                                        message: Some(idx),
                                        id: tidx,
                                        parent: Some(p),
                                        first_child: None,
                                        next_sibling: None,
                                        date: x.date(),
                                        indentation: 0,
                                        show_subject: true,
                                    });
                                    id_table.insert(Cow::from(m_id.into_owned()), tidx);
                                    x.set_thread(tidx);
                                    tidx += 1;
                                    tidx - 1
                                };
                                threads[c].parent = Some(p);
                                if threads[p].is_descendant(&threads, &threads[c])
                                    || threads[c].is_descendant(&threads, &threads[p])
                                    {
                                        continue;
                                    }
                                if threads[p].first_child.is_none() {
                                    threads[p].first_child = Some(c);
                                } else {
                                    let mut fc = threads[p].first_child.unwrap();
                                    while threads[fc].next_sibling.is_some() {
                                        threads[fc].parent = Some(p);
                                        fc = threads[fc].next_sibling.unwrap();
                                    }
                                    threads[fc].next_sibling = Some(c);
                                    threads[fc].parent = Some(p);
                                }
                                /* update thread date */
                                let mut parent_iter = p;
                                'date: loop {
                                    let p = &mut threads[parent_iter];
                                    if p.date < x.date() {
                                        p.date = x.date();
                                    }
                                    match p.parent {
                                        Some(p) => {
                                            parent_iter = p;
                                        }
                                        None => {
                                            break 'date;
                                        }
                                    }
                                }
                            }
                        collection.push(x);
                        idx += 1;
                    }
            }
        }
    }
    /* Walk over the elements of id_table, and gather a list of the Container objects that have
     * no parents. These are the root messages of each thread */
    let mut root_set = Vec::with_capacity(collection.len());
    'root_set: for v in id_table.values() {
        if threads[*v].parent.is_none() {
            if !threads[*v].has_message()
                && threads[*v].has_children()
                    && !threads[threads[*v].first_child.unwrap()].has_sibling()
                    {
                        /* Do not promote the children if doing so would promote them to the root set
                         * -- unless there is only one child, in which case, do. */
                        root_set.push(threads[*v].first_child.unwrap());

                        continue 'root_set;
                    }
            root_set.push(*v);
        }
    }
    root_set.sort_by(|a, b| threads[*b].date.cmp(&threads[*a].date));

    /* Group messages together by thread in a collection so we can print them together */
    let threaded_collection: Vec<usize> = Vec::with_capacity(collection.len());

    let mut t = Threads {
        containers: threads,
        threaded_collection,
        root_set,
        ..Default::default()
    };
    t.build_collection(&collection);
    t
}
