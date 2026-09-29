// SPDX-License-Identifier: MIT OR Apache-2.0
//! `vfstool.serialize(value, format)`: a Lua value tree as JSON, YAML, or TOML.
//!
//! Tables whose keys are exactly `1..n` are arrays (an empty table is an empty array, as the
//! previous binding serialized it); other tables are objects with string keys. The module's
//! sequence views serialize as the arrays their `toTable()` would give. Strings must be UTF-8
//! for these text formats.

use l3i::{
    Error, Result,
    bind::Call,
    sequence::{Sequence, SequenceItem, SequenceSource},
    stack::{Frame, Scope, Type, ValueView},
    userdata::receiver,
};
use serde_json::{Map, Number, Value};

use super::views::{Entries, Keys, ProviderRecords, Providers};
use crate::{SerializeType, serialize_value};

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

/// The sequence view at `view` materialized as an array, if it is one of this module's.
fn sequence_items<S: SequenceSource>(
    frame: &Frame<'_>,
    view: ValueView<'_>,
    depth: usize,
) -> Result<Option<Value>> {
    let Some(sequence) = receiver::<Sequence<S>>(view) else {
        return Ok(None);
    };
    let mut items = Vec::with_capacity(sequence.0.len());
    for index in 0..sequence.0.len() {
        if let Some(item) = sequence.0.get(index) {
            let value = frame.with_frame(|step| {
                item.push_item(step)?;
                convert(step, step.top_value(), depth + 1)
            })?;
            items.push(value);
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
                "dream.vfs: serialize: userdata other than a sequence view cannot be serialized",
            ))
        }
        other => Err(Error::runtime(format!(
            "dream.vfs: serialize: cannot serialize a {}",
            other.name()
        ))),
    }
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
        for index in 1..=border {
            let value = frame.with_frame(|step| {
                let item = table.raw_get_index(step, index as i64)?;
                convert(step, item, depth + 1)
            })?;
            items.push(value);
        }
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
