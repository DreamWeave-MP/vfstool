// SPDX-License-Identifier: MIT OR Apache-2.0
//! Sequence and stream views over the collections scripts iterate: keys, entries, providers,
//! provider records, and the directory tree. The backing collection stays native; only the
//! item a script touches is pushed.

use std::{cell::Cell, rc::Rc};

use l3i::{
    Result,
    bind::Call,
    convert::Push,
    extension::{ExtensionDescriptor, TagPolicy},
    sequence::{Sequence, SequenceSource, Stream, StreamSource},
    stack::{Frame, Scope, TableView, ValueView},
    userdata::push_owned,
    value::Table,
};

use super::{VfsFileHandle, path_bytes, reports};
use crate::{DisplayTree, NormalizedPath, VfsFile, VfsProvider, VfsProviderRecord};

/// Builds a small record table on `scope` with `fill` and leaves it as the pushed value.
pub(super) fn push_record<S: Scope>(
    scope: &S,
    fields: usize,
    fill: impl FnOnce(&Frame<'_>, &TableView<'_>) -> Result<()>,
) -> Result<ValueView<'_>> {
    let mut frame = scope.frame();
    {
        let table = frame.push_table(0, fields)?;
        fill(&frame, &table)?;
    }
    // The table is the frame's only remaining value: every field set consumed its own push.
    frame.preserve_top_and_release();
    Ok(scope.top_value())
}

/// A packed list of byte strings: one blob and one end offset per key, so a view over 5000
/// keys is two allocations and an item is a slice of the blob.
#[derive(Clone, Debug, Default)]
pub struct KeyBlob {
    bytes: Rc<[u8]>,
    ends: Rc<[u32]>,
}

impl KeyBlob {
    /// Packs `keys` in the order given.
    pub fn new<'a>(keys: impl IntoIterator<Item = &'a [u8]>) -> Self {
        let mut bytes = Vec::new();
        let mut ends = Vec::new();
        for key in keys {
            bytes.extend_from_slice(key);
            ends.push(u32::try_from(bytes.len()).expect("a key list under 4 GiB"));
        }
        KeyBlob {
            bytes: bytes.into(),
            ends: ends.into(),
        }
    }

    /// Sorted keys of a VFS or an index.
    pub fn sorted(keys: impl IntoIterator<Item = NormalizedPath>) -> Self {
        let mut keys: Vec<NormalizedPath> = keys.into_iter().collect();
        keys.sort_unstable();
        KeyBlob::new(keys.iter().map(NormalizedPath::as_bytes))
    }

    /// Number of keys.
    pub fn len(&self) -> usize {
        self.ends.len()
    }

    /// Whether there are no keys.
    pub fn is_empty(&self) -> bool {
        self.ends.is_empty()
    }

    /// The bytes of key `index` (0-based).
    pub fn get(&self, index: usize) -> Option<&[u8]> {
        let end = *self.ends.get(index)? as usize;
        let start = if index == 0 {
            0
        } else {
            self.ends[index - 1] as usize
        };
        Some(&self.bytes[start..end])
    }

    fn item(&self, index: usize) -> Option<KeyItem> {
        self.get(index).map(|_| KeyItem {
            blob: Rc::clone(&self.bytes),
            ends: Rc::clone(&self.ends),
            index,
        })
    }
}

/// One key of a [`KeyBlob`], pushed as a Lua string straight from the blob.
pub struct KeyItem {
    blob: Rc<[u8]>,
    ends: Rc<[u32]>,
    index: usize,
}

impl KeyItem {
    fn bytes(&self) -> &[u8] {
        let end = self.ends[self.index] as usize;
        let start = if self.index == 0 {
            0
        } else {
            self.ends[self.index - 1] as usize
        };
        &self.blob[start..end]
    }
}

impl Push for KeyItem {
    fn push_into<'s, S: Scope>(&self, scope: &'s S) -> Result<ValueView<'s>> {
        self.bytes().push_into(scope)
    }
}

/// `dream.vfs.Keys`: sorted VFS keys as strings.
#[derive(Clone, Debug, Default)]
pub struct Keys(pub KeyBlob);

impl SequenceSource for Keys {
    const NAME: &'static str = "dream.vfs.Keys";
    type Item = KeyItem;
    fn len(&self) -> usize {
        self.0.len()
    }
    fn get(&self, index: usize) -> Option<KeyItem> {
        self.0.item(index)
    }
}

/// `dream.vfs.Entries`: `(key, file)` pairs, each pushed as `{ key = string, file = VfsFile }`.
#[derive(Clone, Debug, Default)]
pub struct Entries {
    keys: KeyBlob,
    files: Rc<[VfsFile]>,
}

impl Entries {
    /// Entries in the order given.
    pub fn new(entries: impl IntoIterator<Item = (NormalizedPath, VfsFile)>) -> Self {
        let (keys, files): (Vec<NormalizedPath>, Vec<VfsFile>) = entries.into_iter().unzip();
        Entries {
            keys: KeyBlob::new(keys.iter().map(NormalizedPath::as_bytes)),
            files: files.into(),
        }
    }

    /// Entries sorted by key.
    pub fn sorted(entries: impl IntoIterator<Item = (NormalizedPath, VfsFile)>) -> Self {
        let mut entries: Vec<(NormalizedPath, VfsFile)> = entries.into_iter().collect();
        entries.sort_unstable_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        Entries::new(entries)
    }

    /// Borrowed `(key, file)` pairs, cloned into a view.
    pub fn cloned<'a>(
        entries: impl IntoIterator<Item = (&'a NormalizedPath, &'a VfsFile)>,
    ) -> Self {
        Entries::sorted(
            entries
                .into_iter()
                .map(|(key, file)| (key.clone(), file.clone())),
        )
    }
}

/// One entry row: `{ key = string, file = VfsFile }`.
pub struct EntryRow {
    key: KeyItem,
    file: VfsFile,
}

impl Push for EntryRow {
    fn push_into<'s, S: Scope>(&self, scope: &'s S) -> Result<ValueView<'s>> {
        push_record(scope, 2, |frame, table| {
            self.key.push_into(frame)?;
            table.raw_set(frame, "key")?;
            push_owned(frame, VfsFileHandle(self.file.clone()))?;
            table.raw_set(frame, "file")
        })
    }
}

impl SequenceSource for Entries {
    const NAME: &'static str = "dream.vfs.Entries";
    type Item = EntryRow;
    fn len(&self) -> usize {
        self.files.len()
    }
    fn get(&self, index: usize) -> Option<EntryRow> {
        Some(EntryRow {
            key: self.keys.item(index)?,
            file: self.files.get(index)?.clone(),
        })
    }
}

/// `dream.vfs.Providers`: a key's providers low to high priority, each pushed as
/// `{ source = { path, kind }, file = VfsFile }`.
#[derive(Clone, Debug)]
pub struct Providers(pub Rc<[VfsProvider]>);

/// One provider row.
pub struct ProviderRow(VfsProvider);

impl Push for ProviderRow {
    fn push_into<'s, S: Scope>(&self, scope: &'s S) -> Result<ValueView<'s>> {
        push_record(scope, 2, |frame, table| {
            reports::push_source_meta(frame, &self.0.source)?;
            table.raw_set(frame, "source")?;
            push_owned(frame, VfsFileHandle(self.0.file.clone()))?;
            table.raw_set(frame, "file")
        })
    }
}

impl SequenceSource for Providers {
    const NAME: &'static str = "dream.vfs.Providers";
    type Item = ProviderRow;
    fn len(&self) -> usize {
        self.0.len()
    }
    fn get(&self, index: usize) -> Option<ProviderRow> {
        self.0.get(index).cloned().map(ProviderRow)
    }
}

/// `dream.vfs.ProviderRecords`: provider report rows for a key, low to high priority.
#[derive(Clone, Debug)]
pub struct ProviderRecords(pub Rc<[VfsProviderRecord]>);

/// One provider record row.
pub struct ProviderRecordRow(VfsProviderRecord);

impl Push for ProviderRecordRow {
    fn push_into<'s, S: Scope>(&self, scope: &'s S) -> Result<ValueView<'s>> {
        reports::push_provider_record(scope, &self.0)
    }
}

impl SequenceSource for ProviderRecords {
    const NAME: &'static str = "dream.vfs.ProviderRecords";
    type Item = ProviderRecordRow;
    fn len(&self) -> usize {
        self.0.len()
    }
    fn get(&self, index: usize) -> Option<ProviderRecordRow> {
        self.0.get(index).cloned().map(ProviderRecordRow)
    }
}

/// `dream.vfs.Tree`: a sorted directory tree walked depth first, one `{ dir, file }` row per
/// file, with `toTable()` for the nested `{ files, subdirs }` shape.
pub struct TreeWalk {
    tree: Rc<DisplayTree>,
    rows: Rc<[TreeRow]>,
}

/// One file of the walk: the directory it sits in (as displayed) and the file.
#[derive(Clone)]
pub struct TreeRow {
    dir: Rc<str>,
    file: VfsFile,
}

impl TreeWalk {
    /// Flattens `tree` for iteration; the tree itself stays for `toTable`.
    #[must_use]
    pub fn new(tree: DisplayTree) -> Self {
        let mut rows = Vec::new();
        for (root, node) in &tree {
            walk(root, node, &mut rows);
        }
        TreeWalk {
            tree: Rc::new(tree),
            rows: rows.into(),
        }
    }

    /// The tree the rows were flattened from.
    #[must_use]
    pub fn tree(&self) -> &DisplayTree {
        &self.tree
    }
}

fn walk(dir: &std::path::Path, node: &crate::DirectoryNode, rows: &mut Vec<TreeRow>) {
    let display: Rc<str> = dir.to_string_lossy().into();
    for file in &node.files {
        rows.push(TreeRow {
            dir: Rc::clone(&display),
            file: file.clone(),
        });
    }
    for (name, child) in &node.subdirs {
        walk(&dir.join(name), child, rows);
    }
}

impl Push for TreeRow {
    fn push_into<'s, S: Scope>(&self, scope: &'s S) -> Result<ValueView<'s>> {
        push_record(scope, 2, |frame, table| {
            frame.push(&*self.dir)?;
            table.raw_set(frame, "dir")?;
            push_owned(frame, VfsFileHandle(self.file.clone()))?;
            table.raw_set(frame, "file")
        })
    }
}

/// One loop's private position in a [`TreeWalk`].
pub struct TreeCursor {
    rows: Rc<[TreeRow]>,
    index: Cell<usize>,
}

impl StreamSource for TreeWalk {
    const NAME: &'static str = "dream.vfs.Tree";
    type Item = TreeRow;
    type Cursor = TreeCursor;
    fn open(&self) -> TreeCursor {
        TreeCursor {
            rows: Rc::clone(&self.rows),
            index: Cell::new(0),
        }
    }
    fn next(cursor: &TreeCursor) -> Option<TreeRow> {
        let index = cursor.index.get();
        let row = cursor.rows.get(index)?.clone();
        cursor.index.set(index + 1);
        Some(row)
    }
}

/// The nested `{ [root] = { files = { VfsFile-row }, subdirs = {...} } }` shape.
fn tree_table(scope: &impl Scope, tree: &DisplayTree) -> Result<Table> {
    let table = Table::new(scope, 0, tree.len())?;
    for (root, node) in tree {
        let node = node_table(scope, node)?;
        table.set(scope, &root.to_string_lossy(), &node)?;
    }
    Ok(table)
}

fn node_table(scope: &impl Scope, node: &crate::DirectoryNode) -> Result<Table> {
    let table = Table::new(scope, 0, 2)?;
    let files = Table::new(scope, node.files.len(), 0)?;
    scope.with_frame(|frame| {
        let view = files.push_to(frame)?;
        for (index, file) in node.files.iter().enumerate() {
            reports::push_file_row(frame, file)?;
            view.raw_set_index(frame, (index + 1) as i64)?;
        }
        Ok(())
    })?;
    let subdirs = Table::new(scope, 0, node.subdirs.len())?;
    for (name, child) in &node.subdirs {
        let child = node_table(scope, child)?;
        subdirs.set(scope, &name.to_string_lossy(), &child)?;
    }
    table.set(scope, "files", &files)?;
    table.set(scope, "subdirs", &subdirs)?;
    Ok(table)
}

pub(super) fn describe(d: &mut ExtensionDescriptor) {
    d.sequence::<Keys>(Keys::NAME)
        .tag(TagPolicy::Preferred)
        .doc("Sorted VFS keys: #keys, keys[i], for _, key in keys, keys:toTable().");
    d.sequence::<Entries>(Entries::NAME)
        .tag(TagPolicy::Preferred)
        .doc("Resolved entries sorted by key, each { key = string, file = VfsFile }.");
    d.sequence::<Providers>(Providers::NAME)
        .tag(TagPolicy::Never)
        .doc("A key's providers low to high priority, each { source = { path, kind }, file = VfsFile }.");
    d.sequence::<ProviderRecords>(ProviderRecords::NAME)
        .tag(TagPolicy::Never)
        .doc("Provider report rows for a key, low to high priority.");
    let mut tree = d.stream::<TreeWalk>(TreeWalk::NAME);
    tree.tag(TagPolicy::Never)
        .doc("A sorted directory tree: for _, row in tree yields { dir = string, file = VfsFile }; toTable() is the nested { files, subdirs } shape.");
    tree.method("toTable", |walk: &Stream<TreeWalk>, call: &Call| {
        tree_table(call, &walk.0.tree)
    })
    .signature("(self): { [string]: any }")
    .doc("The nested tree as plain tables, the shape the previous binding returned.");
    tree.method("count", |walk: &Stream<TreeWalk>| walk.0.rows.len() as f64)
        .signature("(self): number");
}

/// Pushes a sequence of sorted keys.
pub(super) fn push_keys(scope: &impl Scope, keys: KeyBlob) -> Result<ValueView<'_>> {
    Sequence::push(scope, Keys(keys))
}

/// Pushes an entries view.
pub(super) fn push_entries(scope: &impl Scope, entries: Entries) -> Result<ValueView<'_>> {
    Sequence::push(scope, entries)
}

/// Pushes a tree stream.
pub(super) fn push_tree(scope: &impl Scope, tree: DisplayTree) -> Result<ValueView<'_>> {
    Stream::push(scope, TreeWalk::new(tree))
}

/// `bytes` of a path as the sequence expects them.
pub(super) fn key_blob_of_paths<'a>(
    paths: impl IntoIterator<Item = &'a std::path::Path>,
) -> KeyBlob {
    KeyBlob::new(paths.into_iter().map(path_bytes))
}
