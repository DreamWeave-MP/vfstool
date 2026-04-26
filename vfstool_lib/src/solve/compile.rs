// SPDX-License-Identifier: GPL-3.0-only
use super::types::{CompiledConstraint, OrderConstraint};
use crate::{analysis::LayerIndex, matchers::CompiledGlob};
use ahash::AHashMap;
use std::{io, path::PathBuf};

pub(super) fn source_lookup(layer: &LayerIndex) -> io::Result<AHashMap<PathBuf, usize>> {
    let mut lookup = AHashMap::new();
    for (idx, source) in layer.sources.iter().enumerate() {
        if lookup.insert(source.path.clone(), idx).is_some() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate source path: {}", source.path.display()),
            ));
        }
    }
    Ok(lookup)
}

pub(super) fn resolve_current_order(
    layer: &LayerIndex,
    current_order: &[PathBuf],
    source_lookup: &AHashMap<PathBuf, usize>,
) -> io::Result<Vec<usize>> {
    if current_order.is_empty() {
        return Ok((0..layer.sources.len()).collect());
    }

    if current_order.len() != layer.sources.len() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "current_order length does not match source count",
        ));
    }

    let mut seen = vec![false; layer.sources.len()];
    let mut indices = Vec::with_capacity(current_order.len());
    for path in current_order {
        let idx = source_lookup.get(path).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("unknown source in current_order: {}", path.display()),
            )
        })?;

        if seen[idx] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate source in current_order: {}", path.display()),
            ));
        }
        seen[idx] = true;
        indices.push(idx);
    }

    Ok(indices)
}

pub(super) fn source_index(
    source_lookup: &AHashMap<PathBuf, usize>,
    path: &PathBuf,
) -> io::Result<usize> {
    source_lookup.get(path).copied().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("unknown source in constraint: {}", path.display()),
        )
    })
}

pub(super) fn compile_glob(field: &str, glob: &str) -> io::Result<CompiledGlob> {
    CompiledGlob::new(glob).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid {field} '{glob}': {err}"),
        )
    })
}

pub(super) fn compile_constraints(
    layer: &LayerIndex,
    constraints: &[OrderConstraint],
    keys: &[PathBuf],
    source_lookup: &AHashMap<PathBuf, usize>,
) -> io::Result<Vec<CompiledConstraint>> {
    let mut compiled = Vec::with_capacity(constraints.len());
    for (constraint_index, constraint) in constraints.iter().enumerate() {
        let item = match constraint {
            OrderConstraint::SourceBefore { a, b } => CompiledConstraint::SourceBefore {
                constraint_index,
                a_idx: source_index(source_lookup, a)?,
                b_idx: source_index(source_lookup, b)?,
                a: a.clone(),
                b: b.clone(),
            },
            OrderConstraint::SourceAfter { a, b } => CompiledConstraint::SourceAfter {
                constraint_index,
                a_idx: source_index(source_lookup, a)?,
                b_idx: source_index(source_lookup, b)?,
                a: a.clone(),
                b: b.clone(),
            },
            OrderConstraint::WinnerMustBe {
                path_glob,
                source_glob,
            } => {
                let path_glob_re = compile_glob("path_glob", path_glob)?;
                let matched_key_indices = keys
                    .iter()
                    .enumerate()
                    .filter_map(|(key_idx, key)| {
                        if path_glob_re.is_match(key) {
                            Some(key_idx)
                        } else {
                            None
                        }
                    })
                    .collect();
                let source_glob_re = compile_glob("source_glob", source_glob)?;
                let allowed_sources = layer
                    .sources
                    .iter()
                    .map(|source| source_glob_re.is_match(&source.path))
                    .collect();

                CompiledConstraint::WinnerMustBe {
                    constraint_index,
                    path_glob: path_glob.clone(),
                    source_glob: source_glob.clone(),
                    matched_key_indices,
                    allowed_sources,
                }
            }
        };
        compiled.push(item);
    }
    Ok(compiled)
}

pub(super) fn precedence_edges(constraints: &[CompiledConstraint]) -> Vec<(usize, usize)> {
    constraints
        .iter()
        .filter_map(|constraint| match constraint {
            CompiledConstraint::SourceBefore { a_idx, b_idx, .. } => Some((*a_idx, *b_idx)),
            CompiledConstraint::SourceAfter { a_idx, b_idx, .. } => Some((*b_idx, *a_idx)),
            CompiledConstraint::WinnerMustBe { .. } => None,
        })
        .collect()
}
