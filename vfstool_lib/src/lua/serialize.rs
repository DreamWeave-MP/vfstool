// SPDX-License-Identifier: MIT OR Apache-2.0
//! `vfstool.serialize(value, format)`: a Lua value tree as JSON, YAML, or TOML.
//!
//! Tables whose keys are exactly `1..n` are arrays (an empty table is an empty array, as the
//! previous binding serialized it); other tables are objects with string keys. The module's
//! sequence views serialize as the arrays their `toTable()` would give, a tree stream as its
//! `toTable()` shape, and a `VfsFile` as the plain fields of its file row. Strings must be UTF-8
//! for these text formats.

use l3i::{
    Error, Result,
    bind::Call,
    sequence::{Sequence, SequenceItem, SequenceSource, Stream},
    stack::{Frame, Scope, Type, ValueView},
    userdata::receiver,
};
use serde_json::{Map, Number, Value};

use super::{
    VfsFileHandle,
    views::{Entries, Keys, ProviderRecords, Providers, TreeWalk},
};
use crate::{DirectoryNode, DisplayTree, SerializeType, VfsFile, serialize_value};

const MAX_DEPTH: usize = 128;

pub(super) fn format(name: &str) -> Result<SerializeType> {
    match name {
        "json" => Ok(SerializeType::Json),
        "yaml" => Ok(SerializeType::Yaml),
        "toml" => Ok(SerializeType::Toml),
        other => Err(Error::runtime(format!(
            "dream.vfs: unknown serialization format '{other}' (expected json, yaml, or toml)"
        ))),
    }
}

pub(super) fn serialize(call: &Call<'_>, value: ValueView<'_>, name: &str) -> Result<String> {
    let format = format(name)?;
    let value = call.with_frame(|frame| convert(frame, value, 0))?;
    serialize_value(&value, format).map_err(super::io_error)
}

/// The sequence view at `view` materialized as an array, if it is one of this module's. One
/// frame serves every item: each is pushed, converted, and popped in turn.
fn sequence_items<S: SequenceSource>(
    frame: &Frame<'_>,
    view: ValueView<'_>,
    depth: usize,
) -> Result<Option<Value>> {
    let Some(sequence) = receiver::<Sequence<S>>(view) else {
        return Ok(None);
    };
    let mut items = Vec::with_capacity(sequence.0.len());
    let mut step = frame.frame();
    for index in 0..sequence.0.len() {
        if let Some(item) = sequence.0.get(index) {
            item.push_item(&step)?;
            items.push(convert(&step, step.top_value(), depth + 1)?);
            step.pop(1);
        }
    }
    Ok(Some(Value::Array(items)))
}

fn convert(frame: &Frame<'_>, view: ValueView<'_>, depth: usize) -> Result<Value> {
    if depth > MAX_DEPTH {
        return Err(Error::runtime(
            "dream.vfs: serialize: value nests deeper than 128 levels",
        ));
    }
    match view.type_of() {
        Type::Nil | Type::None => Ok(Value::Null),
        Type::Boolean => view.read::<bool>().map(Value::Bool),
        Type::Integer => view
            .read::<l3i::convert::Integer>()
            .map(|value| Value::Number(value.0.into())),
        Type::Number => {
            let number = view.read::<f64>()?;
            if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 {
                return Ok(Value::Number((number as i64).into()));
            }
            Number::from_f64(number).map(Value::Number).ok_or_else(|| {
                Error::runtime("dream.vfs: serialize: a non-finite number has no representation")
            })
        }
        Type::String => {
            let bytes = view.read::<&[u8]>()?;
            std::str::from_utf8(bytes)
                .map(|text| Value::String(text.to_owned()))
                .map_err(|_| Error::runtime("dream.vfs: serialize: strings must be valid UTF-8"))
        }
        Type::Table => table(frame, view, depth),
        Type::Userdata => {
            if let Some(file) = receiver::<VfsFileHandle>(view) {
                return file_object(&file.file);
            }
            if let Some(walk) = receiver::<Stream<TreeWalk>>(view) {
                return tree_object(walk.0.tree());
            }
            let probes = [
                sequence_items::<Keys>(frame, view, depth)?,
                sequence_items::<Entries>(frame, view, depth)?,
                sequence_items::<Providers>(frame, view, depth)?,
                sequence_items::<ProviderRecords>(frame, view, depth)?,
            ];
            if let Some(value) = probes.into_iter().flatten().next() {
                return Ok(value);
            }
            Err(Error::runtime(
                "dream.vfs: serialize: userdata other than a VfsFile, a sequence view or a tree cannot be serialized",
            ))
        }
        other => Err(Error::runtime(format!(
            "dream.vfs: serialize: cannot serialize a {}",
            other.name()
        ))),
    }
}

fn utf8(bytes: &[u8]) -> Result<&str> {
    std::str::from_utf8(bytes)
        .map_err(|_| Error::runtime("dream.vfs: serialize: strings must be valid UTF-8"))
}

/// A file as the fields of its file row, the table a tree's `toTable()` lists it by, without the
/// handle: `path`, `isLoose`, `isArchive`, and for an archive entry `parentArchivePath` and
/// `parentArchiveName`.
fn file_object(file: &VfsFile) -> Result<Value> {
    let mut object = Map::new();
    object.insert(
        "path".to_owned(),
        Value::String(utf8(file.path_bytes())?.to_owned()),
    );
    object.insert("isLoose".to_owned(), Value::Bool(file.is_loose()));
    object.insert("isArchive".to_owned(), Value::Bool(file.is_archive()));
    if let Some(archive) = file.parent_archive_path() {
        object.insert("parentArchivePath".to_owned(), Value::String(archive));
    }
    if let Some(name) = file.parent_archive_name() {
        object.insert("parentArchiveName".to_owned(), Value::String(name));
    }
    Ok(Value::Object(object))
}

/// A tree as its `toTable()` shape, `{ [root] = { files, subdirs } }`, each file as
/// [`file_object`].
fn tree_object(tree: &DisplayTree) -> Result<Value> {
    let mut object = Map::new();
    for (name, node) in tree {
        object.insert(name.to_string_lossy().into_owned(), node_object(node)?);
    }
    Ok(Value::Object(object))
}

fn node_object(node: &DirectoryNode) -> Result<Value> {
    let files = node.files.iter().map(file_object).collect::<Result<_>>()?;
    let mut object = Map::new();
    object.insert("files".to_owned(), Value::Array(files));
    object.insert("subdirs".to_owned(), tree_object(&node.subdirs)?);
    Ok(Value::Object(object))
}

fn table(frame: &Frame<'_>, view: ValueView<'_>, depth: usize) -> Result<Value> {
    let table = view.as_table()?;
    let border = table.raw_len();
    let mut entries = 0usize;
    table.for_each(frame, |_, _, _| {
        entries += 1;
        Ok(())
    })?;
    if entries == border {
        // Keys are exactly 1..border: an array (empty included).
        let mut items = Vec::with_capacity(border);
        table.for_each_array(frame, |step, _, item| {
            items.push(convert(step, item, depth + 1)?);
            Ok(())
        })?;
        return Ok(Value::Array(items));
    }
    let mut object = Map::new();
    table.for_each(frame, |step, key, value| {
        let name = match key.type_of() {
            Type::String => std::str::from_utf8(key.read::<&[u8]>()?)
                .map(str::to_owned)
                .map_err(|_| {
                    Error::runtime("dream.vfs: serialize: table keys must be valid UTF-8")
                })?,
            Type::Number => key.read::<f64>()?.to_string(),
            Type::Integer => key.read::<l3i::convert::Integer>()?.0.to_string(),
            other => {
                return Err(Error::runtime(format!(
                    "dream.vfs: serialize: a {} cannot be an object key",
                    other.name()
                )));
            }
        };
        object.insert(name, convert(step, value, depth + 1)?);
        Ok(())
    })?;
    Ok(Value::Object(object))
}
