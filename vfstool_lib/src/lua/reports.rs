// SPDX-License-Identifier: MIT OR Apache-2.0
//! Report tables: plain tables with camelCase fields, built on the stack for rows and through
//! pinned tables for nested reports. Indices are 1-based here; byte sizes are Luau integers.

use std::path::{Path, PathBuf};

use l3i::{
    Result,
    convert::Integer,
    stack::{Scope, ValueView},
    userdata::push_owned,
    value::Table,
};

use super::{VfsFileHandle, index_to_lua, path_bytes, source_kind_name, views::push_record};
use crate::{
    ArchiveEntry, ArchiveInfo, AssetClass, ConflictSourceEntry, ConflictsReport, DiffReport,
    DirectoryDiff, DriftKind, DriftReport, DuplicateReport, ExplainReport, LayerProvider,
    MaterializationAction, MaterializationIssue, MaterializationPlan, MaterializationSkip,
    SemanticConflictReport, SemanticDelta, SemanticProvider, SemanticRelation, ShadowedReport,
    SourceContributionReport, SourceMeta, VfsFile, VfsLock, VfsLockEntry, VfsProvider,
    VfsProviderRecord, analysis::ProvenanceChain,
};

// ---------------------------------------------------------------------------------------------
// Stack-built rows
// ---------------------------------------------------------------------------------------------

/// `{ path = string, kind = string }` left on top of `scope`.
pub(super) fn push_source_meta<'s>(
    scope: &'s impl Scope,
    source: &SourceMeta,
) -> Result<ValueView<'s>> {
    push_record(scope, 2, |frame, table| {
        frame.push(path_bytes(&source.path))?;
        table.raw_set(frame, "path")?;
        frame.push(source_kind_name(source.kind))?;
        table.raw_set(frame, "kind")
    })
}

/// A provider record row.
pub(super) fn push_provider_record<'s>(
    scope: &'s impl Scope,
    record: &VfsProviderRecord,
) -> Result<ValueView<'s>> {
    push_record(scope, 5, |frame, table| {
        frame.push(&index_to_lua(record.source_index))?;
        table.raw_set(frame, "sourceIndex")?;
        push_source_meta(frame, &record.source)?;
        table.raw_set(frame, "source")?;
        frame.push(path_bytes(&record.key))?;
        table.raw_set(frame, "key")?;
        frame.push(path_bytes(&record.original_path))?;
        table.raw_set(frame, "originalPath")?;
        frame.push(record.resolved_path.as_str())?;
        table.raw_set(frame, "resolvedPath")
    })
}

/// The file row a tree node lists: `{ path, isLoose, isArchive, parentArchivePath, parentArchiveName, file }`.
pub(super) fn push_file_row<'s>(scope: &'s impl Scope, file: &VfsFile) -> Result<ValueView<'s>> {
    push_record(scope, 6, |frame, table| {
        frame.push(file.path_bytes())?;
        table.raw_set(frame, "path")?;
        frame.push(&file.is_loose())?;
        table.raw_set(frame, "isLoose")?;
        frame.push(&file.is_archive())?;
        table.raw_set(frame, "isArchive")?;
        frame.push(&file.parent_archive_path())?;
        table.raw_set(frame, "parentArchivePath")?;
        frame.push(&file.parent_archive_name())?;
        table.raw_set(frame, "parentArchiveName")?;
        push_owned(frame, VfsFileHandle(file.clone()))?;
        table.raw_set(frame, "file")
    })
}

/// `{ source = { path, kind }, file = VfsFile }`.
pub(super) fn push_provider<'s>(
    scope: &'s impl Scope,
    provider: &VfsProvider,
) -> Result<ValueView<'s>> {
    push_record(scope, 2, |frame, table| {
        push_source_meta(frame, &provider.source)?;
        table.raw_set(frame, "source")?;
        push_owned(frame, VfsFileHandle(provider.file.clone()))?;
        table.raw_set(frame, "file")
    })
}

// ---------------------------------------------------------------------------------------------
// Pinned tables
// ---------------------------------------------------------------------------------------------

/// An array table whose element `i` is pushed by `push(i)`.
fn array(
    scope: &impl Scope,
    len: usize,
    mut push: impl FnMut(&l3i::stack::Frame<'_>, usize) -> Result<()>,
) -> Result<Table> {
    let table = Table::new(scope, len, 0)?;
    scope.with_frame(|frame| {
        let view = table.push_to(frame)?;
        for index in 0..len {
            push(frame, index)?;
            view.raw_set_index(frame, (index + 1) as i64)?;
        }
        Ok(())
    })?;
    Ok(table)
}

pub(super) fn source_meta(scope: &impl Scope, source: &SourceMeta) -> Result<Table> {
    let table = Table::new(scope, 0, 2)?;
    table.set(scope, "path", path_bytes(&source.path))?;
    table.set(scope, "kind", source_kind_name(source.kind))?;
    Ok(table)
}

pub(super) fn path_list<'a>(
    scope: &impl Scope,
    paths: impl ExactSizeIterator<Item = &'a PathBuf>,
) -> Result<Table> {
    let paths: Vec<&PathBuf> = paths.collect();
    array(scope, paths.len(), |frame, i| {
        frame.push(path_bytes(paths[i])).map(drop)
    })
}

fn string_list(scope: &impl Scope, values: &[String]) -> Result<Table> {
    array(scope, values.len(), |frame, i| {
        frame.push(values[i].as_str()).map(drop)
    })
}

pub(super) fn sources(scope: &impl Scope, sources: &[SourceMeta]) -> Result<Table> {
    array(scope, sources.len(), |frame, i| {
        push_source_meta(frame, &sources[i]).map(drop)
    })
}

pub(super) fn indices(scope: &impl Scope, indices: &[usize]) -> Result<Table> {
    array(scope, indices.len(), |frame, i| {
        frame.push(&index_to_lua(indices[i])).map(drop)
    })
}

pub(super) fn provider_records(scope: &impl Scope, records: &[VfsProviderRecord]) -> Result<Table> {
    array(scope, records.len(), |frame, i| {
        push_provider_record(frame, &records[i]).map(drop)
    })
}

pub(super) fn explain(scope: &impl Scope, report: &ExplainReport) -> Result<Table> {
    let table = Table::new(scope, 0, 3)?;
    table.set(scope, "key", path_bytes(&report.key))?;
    scope.with_frame(|frame| {
        let view = table.push_to(frame)?;
        push_provider_record(frame, &report.winner)?;
        view.raw_set(frame, "winner")
    })?;
    table.set(
        scope,
        "overridden",
        &provider_records(scope, &report.overridden)?,
    )?;
    Ok(table)
}

pub(super) fn duplicates(scope: &impl Scope, report: &DuplicateReport) -> Result<Table> {
    let entries = array(scope, report.entries.len(), |frame, i| {
        let entry = &report.entries[i];
        let providers = provider_records(frame, &entry.providers)?;
        push_record(frame, 3, |frame, table| {
            frame.push(path_bytes(&entry.key))?;
            table.raw_set(frame, "key")?;
            providers.push_to(frame)?;
            table.raw_set(frame, "providers")?;
            frame.push(&index_to_lua(entry.winner_index))?;
            table.raw_set(frame, "winnerIndex")
        })
        .map(drop)
    })?;
    let table = Table::new(scope, 0, 1)?;
    table.set(scope, "entries", &entries)?;
    Ok(table)
}

pub(super) fn archive_infos(scope: &impl Scope, infos: &[ArchiveInfo]) -> Result<Table> {
    array(scope, infos.len(), |frame, i| {
        let info = &infos[i];
        push_record(frame, 4, |frame, table| {
            frame.push(&index_to_lua(info.source_index))?;
            table.raw_set(frame, "sourceIndex")?;
            frame.push(path_bytes(&info.path))?;
            table.raw_set(frame, "path")?;
            frame.push(&(info.entry_count as f64))?;
            table.raw_set(frame, "entryCount")?;
            frame.push(&(info.winning_entry_count as f64))?;
            table.raw_set(frame, "winningEntryCount")
        })
        .map(drop)
    })
}

pub(super) fn archive_entries(scope: &impl Scope, entries: &[ArchiveEntry]) -> Result<Table> {
    array(scope, entries.len(), |frame, i| {
        let entry = &entries[i];
        push_record(frame, 4, |frame, table| {
            frame.push(path_bytes(&entry.key))?;
            table.raw_set(frame, "key")?;
            frame.push(path_bytes(&entry.archive_path))?;
            table.raw_set(frame, "archivePath")?;
            frame.push(path_bytes(&entry.original_path))?;
            table.raw_set(frame, "originalPath")?;
            frame.push(&entry.wins)?;
            table.raw_set(frame, "wins")
        })
        .map(drop)
    })
}

pub(super) fn source_contributions(
    scope: &impl Scope,
    report: &SourceContributionReport,
) -> Result<Table> {
    let sources = array(scope, report.sources.len(), |frame, i| {
        let row = &report.sources[i];
        push_record(frame, 9, |frame, table| {
            frame.push(&index_to_lua(row.source_index))?;
            table.raw_set(frame, "sourceIndex")?;
            push_source_meta(frame, &row.source)?;
            table.raw_set(frame, "source")?;
            for (name, count) in [
                ("winningFiles", row.winning_files),
                ("overridingFiles", row.overriding_files),
                ("overriddenFiles", row.overridden_files),
                ("uniqueFiles", row.unique_files),
                ("duplicateFiles", row.duplicate_files),
                ("looseFiles", row.loose_files),
                ("archiveFiles", row.archive_files),
            ] {
                frame.push(&(count as f64))?;
                table.raw_set(frame, name)?;
            }
            Ok(())
        })
        .map(drop)
    })?;
    let table = Table::new(scope, 0, 1)?;
    table.set(scope, "sources", &sources)?;
    Ok(table)
}

fn materialization_row(
    frame: &l3i::stack::Frame<'_>,
    kind: &str,
    key: &Path,
    source: Option<&PathBuf>,
    archive: Option<&PathBuf>,
    dest: Option<&PathBuf>,
) -> Result<()> {
    push_record(frame, 5, |frame, table| {
        frame.push(kind)?;
        table.raw_set(frame, "kind")?;
        frame.push(path_bytes(key))?;
        table.raw_set(frame, "key")?;
        for (name, path) in [("source", source), ("archive", archive), ("dest", dest)] {
            frame.push(&path.map(|path| path_bytes(path).to_vec()))?;
            table.raw_set(frame, name)?;
        }
        Ok(())
    })
    .map(drop)
}

pub(super) fn materialization_plan(
    scope: &impl Scope,
    plan: &MaterializationPlan,
) -> Result<Table> {
    let actions = array(scope, plan.actions.len(), |frame, i| {
        match &plan.actions[i] {
            MaterializationAction::Hardlink { key, source, dest } => {
                materialization_row(frame, "hardlink", key, Some(source), None, Some(dest))
            }
            MaterializationAction::Symlink { key, source, dest } => {
                materialization_row(frame, "symlink", key, Some(source), None, Some(dest))
            }
            MaterializationAction::Copy { key, source, dest } => {
                materialization_row(frame, "copy", key, Some(source), None, Some(dest))
            }
            MaterializationAction::ExtractArchive { key, archive, dest } => materialization_row(
                frame,
                "extractArchive",
                key,
                None,
                Some(archive),
                Some(dest),
            ),
            MaterializationAction::SkipArchiveFile { key, archive } => {
                materialization_row(frame, "skipArchiveFile", key, None, Some(archive), None)
            }
        }
    })?;
    let issues = array(scope, plan.issues.len(), |frame, i| match &plan.issues[i] {
        MaterializationIssue::MissingLooseSource { key, source } => {
            materialization_row(frame, "missingLooseSource", key, Some(source), None, None)
        }
        MaterializationIssue::FileDirectoryConflict { key, dest } => {
            materialization_row(frame, "fileDirectoryConflict", key, None, None, Some(dest))
        }
        MaterializationIssue::UnsafeDestination { key, dest } => {
            materialization_row(frame, "unsafeDestination", key, None, None, Some(dest))
        }
    })?;
    let table = Table::new(scope, 0, 2)?;
    table.set(scope, "actions", &actions)?;
    table.set(scope, "issues", &issues)?;
    Ok(table)
}

/// `{ { kind, key, archive?, source?, error? } }`: the winners a write-out left out.
pub(super) fn materialization_skips(
    scope: &impl Scope,
    skipped: &[MaterializationSkip],
) -> Result<Table> {
    array(scope, skipped.len(), |frame, i| {
        let (kind, key, archive, source, error) = match &skipped[i] {
            MaterializationSkip::ArchiveEntry { key, archive } => {
                ("archiveEntry", key, Some(archive), None, None)
            }
            MaterializationSkip::ArchiveFile { key, archive } => {
                ("archiveFile", key, Some(archive), None, None)
            }
            MaterializationSkip::MissingLooseSource { key, source } => {
                ("missingLooseSource", key, None, Some(source), None)
            }
            MaterializationSkip::UnreadableArchiveEntry {
                key,
                archive,
                error,
            } => (
                "unreadableArchiveEntry",
                key,
                Some(archive),
                None,
                Some(error.as_str()),
            ),
        };
        push_record(frame, 5, |frame, table| {
            frame.push(kind)?;
            table.raw_set(frame, "kind")?;
            frame.push(path_bytes(key))?;
            table.raw_set(frame, "key")?;
            for (name, path) in [("archive", archive), ("source", source)] {
                frame.push(&path.map(|path| path_bytes(path).to_vec()))?;
                table.raw_set(frame, name)?;
            }
            frame.push(&error)?;
            table.raw_set(frame, "error")
        })
        .map(drop)
    })
}

pub(super) fn directory_diff(scope: &impl Scope, diff: &DirectoryDiff<'_>) -> Result<Table> {
    let conflicts = array(scope, diff.conflicts.len(), |frame, i| {
        let (key, incoming, current) = &diff.conflicts[i];
        push_record(frame, 3, |frame, table| {
            frame.push(path_bytes(key))?;
            table.raw_set(frame, "key")?;
            push_owned(frame, VfsFileHandle(incoming.clone()))?;
            table.raw_set(frame, "incoming")?;
            push_owned(frame, VfsFileHandle((*current).clone()))?;
            table.raw_set(frame, "current")
        })
        .map(drop)
    })?;
    let additions = array(scope, diff.additions.len(), |frame, i| {
        let (key, file) = &diff.additions[i];
        push_record(frame, 2, |frame, table| {
            frame.push(path_bytes(key))?;
            table.raw_set(frame, "key")?;
            push_owned(frame, VfsFileHandle(file.clone()))?;
            table.raw_set(frame, "file")
        })
        .map(drop)
    })?;
    let unreadable = array(scope, diff.unreadable.len(), |frame, i| {
        let (path, error) = &diff.unreadable[i];
        push_record(frame, 2, |frame, table| {
            frame.push(path_bytes(path))?;
            table.raw_set(frame, "path")?;
            frame.push(error.to_string().as_str())?;
            table.raw_set(frame, "error")
        })
        .map(drop)
    })?;
    let table = Table::new(scope, 0, 3)?;
    table.set(scope, "conflicts", &conflicts)?;
    table.set(scope, "additions", &additions)?;
    table.set(scope, "unreadable", &unreadable)?;
    Ok(table)
}

pub(super) fn removed_providers(
    scope: &impl Scope,
    removed: &[(crate::NormalizedPath, VfsProvider)],
) -> Result<Table> {
    array(scope, removed.len(), |frame, i| {
        let (key, provider) = &removed[i];
        push_record(frame, 2, |frame, table| {
            frame.push(key.as_bytes())?;
            table.raw_set(frame, "key")?;
            push_provider(frame, provider)?;
            table.raw_set(frame, "provider")
        })
        .map(drop)
    })
}

pub(super) fn providers_list(scope: &impl Scope, providers: &[VfsProvider]) -> Result<Table> {
    array(scope, providers.len(), |frame, i| {
        push_provider(frame, &providers[i]).map(drop)
    })
}

pub(super) fn removed_files(
    scope: &impl Scope,
    removed: &[(crate::NormalizedPath, VfsFile)],
) -> Result<Table> {
    array(scope, removed.len(), |frame, i| {
        let (key, file) = &removed[i];
        push_record(frame, 2, |frame, table| {
            frame.push(key.as_bytes())?;
            table.raw_set(frame, "key")?;
            push_owned(frame, VfsFileHandle(file.clone()))?;
            table.raw_set(frame, "file")
        })
        .map(drop)
    })
}

pub(super) fn layer_providers(scope: &impl Scope, providers: &[LayerProvider]) -> Result<Table> {
    array(scope, providers.len(), |frame, i| {
        let provider = &providers[i];
        push_record(frame, 5, |frame, table| {
            frame.push(&index_to_lua(provider.source_index))?;
            table.raw_set(frame, "sourceIndex")?;
            frame.push(&index_to_lua(provider.provider_index))?;
            table.raw_set(frame, "providerIndex")?;
            push_source_meta(frame, &provider.source)?;
            table.raw_set(frame, "source")?;
            frame.push(path_bytes(&provider.key))?;
            table.raw_set(frame, "key")?;
            frame.push(path_bytes(&provider.original_path))?;
            table.raw_set(frame, "originalPath")
        })
        .map(drop)
    })
}

/// An optional byte size as a Luau integer.
fn size(value: Option<u64>) -> Option<Integer> {
    value.map(|size| Integer(i64::try_from(size).unwrap_or(i64::MAX)))
}

pub(super) fn provenance(scope: &impl Scope, chain: &ProvenanceChain) -> Result<Table> {
    let providers = array(scope, chain.providers.len(), |frame, i| {
        let provider = &chain.providers[i];
        push_record(frame, 4, |frame, table| {
            push_source_meta(frame, &provider.source)?;
            table.raw_set(frame, "source")?;
            frame.push(provider.resolved_path.as_str())?;
            table.raw_set(frame, "resolvedPath")?;
            frame.push(&provider.hash_blake3)?;
            table.raw_set(frame, "hashBlake3")?;
            frame.push(&size(provider.size))?;
            table.raw_set(frame, "size")
        })
        .map(drop)
    })?;
    let table = Table::new(scope, 0, 3)?;
    table.set(scope, "key", path_bytes(&chain.key))?;
    table.set(scope, "winner", &source_meta(scope, &chain.winner)?)?;
    table.set(scope, "providers", &providers)?;
    Ok(table)
}

pub(super) fn lock_entries(scope: &impl Scope, entries: &[VfsLockEntry]) -> Result<Table> {
    array(scope, entries.len(), |frame, i| {
        let entry = &entries[i];
        push_record(frame, 6, |frame, table| {
            frame.push(path_bytes(&entry.key))?;
            table.raw_set(frame, "key")?;
            frame.push(path_bytes(&entry.winner_source))?;
            table.raw_set(frame, "winnerSource")?;
            frame.push(source_kind_name(entry.winner_kind))?;
            table.raw_set(frame, "winnerKind")?;
            frame.push(&entry.winner_hash_blake3)?;
            table.raw_set(frame, "winnerHashBlake3")?;
            frame.push(&size(entry.winner_size))?;
            table.raw_set(frame, "winnerSize")?;
            frame.push(&(entry.provider_count as f64))?;
            table.raw_set(frame, "providerCount")
        })
        .map(drop)
    })
}

pub(super) fn lock(scope: &impl Scope, lock: &VfsLock) -> Result<Table> {
    let table = Table::new(scope, 0, 2)?;
    table.set(scope, "schemaVersion", &f64::from(lock.schema_version))?;
    table.set(scope, "entries", &lock_entries(scope, &lock.entries)?)?;
    Ok(table)
}

fn drift_kind_name(kind: DriftKind) -> &'static str {
    match kind {
        DriftKind::Added => "added",
        DriftKind::Removed => "removed",
        DriftKind::WinnerSourceChanged => "winnerSourceChanged",
        DriftKind::WinnerHashChanged => "winnerHashChanged",
        DriftKind::ProviderCountChanged => "providerCountChanged",
    }
}

pub(super) fn drift(scope: &impl Scope, report: &DriftReport) -> Result<Table> {
    let entries = array(scope, report.entries.len(), |frame, i| {
        let entry = &report.entries[i];
        push_record(frame, 2, |frame, table| {
            frame.push(path_bytes(&entry.key))?;
            table.raw_set(frame, "key")?;
            frame.push(drift_kind_name(entry.kind))?;
            table.raw_set(frame, "kind")
        })
        .map(drop)
    })?;
    let counts = Table::new(scope, 0, report.counts.len())?;
    for (kind, count) in &report.counts {
        counts.set(scope, drift_kind_name(*kind), &(*count as f64))?;
    }
    let table = Table::new(scope, 0, 2)?;
    table.set(scope, "entries", &entries)?;
    table.set(scope, "counts", &counts)?;
    Ok(table)
}

pub(super) fn asset_class_name(class: AssetClass) -> &'static str {
    match class {
        AssetClass::Ini => "ini",
        AssetClass::Toml => "toml",
        AssetClass::Json => "json",
        AssetClass::LuaScript => "luaScript",
        AssetClass::MwScriptLike => "mwScriptLike",
        AssetClass::Text => "text",
        AssetClass::Binary => "binary",
        AssetClass::Unknown => "unknown",
    }
}

fn semantic_delta(scope: &impl Scope, delta: &SemanticDelta) -> Result<Table> {
    let table = Table::new(scope, 0, 2)?;
    match delta {
        SemanticDelta::NoOpEquivalent => table.set(scope, "kind", "noOpEquivalent")?,
        SemanticDelta::CosmeticOnly => table.set(scope, "kind", "cosmeticOnly")?,
        SemanticDelta::BehaviorChanging { change_summary } => {
            table.set(scope, "kind", "behaviorChanging")?;
            table.set(scope, "changeSummary", &string_list(scope, change_summary)?)?;
        }
        SemanticDelta::Unknown => table.set(scope, "kind", "unknown")?,
    }
    Ok(table)
}

pub(super) fn semantic_pair(
    scope: &impl Scope,
    class: AssetClass,
    delta: &SemanticDelta,
) -> Result<Table> {
    let table = Table::new(scope, 0, 2)?;
    table.set(scope, "assetClass", asset_class_name(class))?;
    table.set(scope, "delta", &semantic_delta(scope, delta)?)?;
    Ok(table)
}

fn semantic_relation_name(relation: SemanticRelation) -> &'static str {
    match relation {
        SemanticRelation::IdenticalToWinner => "identicalToWinner",
        SemanticRelation::DifferentFromWinner => "differentFromWinner",
        SemanticRelation::Unknown => "unknown",
    }
}

fn semantic_provider(scope: &impl Scope, provider: &SemanticProvider) -> Result<Table> {
    let table = Table::new(scope, 0, 5)?;
    table.set(scope, "source", &source_meta(scope, &provider.source)?)?;
    table.set(scope, "relation", semantic_relation_name(provider.relation))?;
    table.set(scope, "hashBlake3", &provider.hash_blake3)?;
    table.set(scope, "size", &size(provider.size))?;
    match &provider.semantic_delta_to_winner {
        Some(delta) => table.set(
            scope,
            "semanticDeltaToWinner",
            &semantic_delta(scope, delta)?,
        )?,
        None => table.set(scope, "semanticDeltaToWinner", &())?,
    }
    Ok(table)
}

pub(super) fn semantic_conflicts(
    scope: &impl Scope,
    report: &SemanticConflictReport,
) -> Result<Table> {
    let entries = array(scope, report.entries.len(), |frame, i| {
        let entry = &report.entries[i];
        let winner = source_meta(frame, &entry.winner)?;
        let providers = array(frame, entry.providers.len(), |frame, j| {
            semantic_provider(frame, &entry.providers[j])?
                .push_to(frame)
                .map(drop)
        })?;
        push_record(frame, 6, |frame, table| {
            frame.push(path_bytes(&entry.key))?;
            table.raw_set(frame, "key")?;
            winner.push_to(frame)?;
            table.raw_set(frame, "winner")?;
            providers.push_to(frame)?;
            table.raw_set(frame, "providers")?;
            frame.push(asset_class_name(entry.asset_class))?;
            table.raw_set(frame, "assetClass")?;
            frame.push(&entry.all_identical)?;
            table.raw_set(frame, "allIdentical")?;
            frame.push(&(entry.distinct_versions as f64))?;
            table.raw_set(frame, "distinctVersions")
        })
        .map(drop)
    })?;
    let table = Table::new(scope, 0, 1)?;
    table.set(scope, "entries", &entries)?;
    Ok(table)
}

pub(super) fn conflicts_report(scope: &impl Scope, report: &ConflictsReport) -> Result<Table> {
    let sources = array(scope, report.sources.len(), |frame, i| {
        let ConflictSourceEntry {
            path,
            overrides,
            overridden_by,
        } = &report.sources[i];
        let overrides = path_list(frame, overrides.iter())?;
        let overridden_by = path_list(frame, overridden_by.iter())?;
        push_record(frame, 3, |frame, table| {
            frame.push(path_bytes(path))?;
            table.raw_set(frame, "path")?;
            overrides.push_to(frame)?;
            table.raw_set(frame, "overrides")?;
            overridden_by.push_to(frame)?;
            table.raw_set(frame, "overriddenBy")
        })
        .map(drop)
    })?;
    let table = Table::new(scope, 0, 1)?;
    table.set(scope, "sources", &sources)?;
    Ok(table)
}

pub(super) fn shadowed_report(scope: &impl Scope, report: &ShadowedReport) -> Result<Table> {
    let sources = array(scope, report.sources.len(), |frame, i| {
        let source = &report.sources[i];
        let files = path_list(frame, source.shadowed_files.iter())?;
        push_record(frame, 2, |frame, table| {
            frame.push(path_bytes(&source.path))?;
            table.raw_set(frame, "path")?;
            files.push_to(frame)?;
            table.raw_set(frame, "shadowedFiles")
        })
        .map(drop)
    })?;
    let table = Table::new(scope, 0, 1)?;
    table.set(scope, "sources", &sources)?;
    Ok(table)
}

pub(super) fn diff_report(scope: &impl Scope, report: &DiffReport) -> Result<Table> {
    let table = Table::new(scope, 0, 6)?;
    table.set(scope, "sourceA", path_bytes(&report.source_a))?;
    table.set(scope, "sourceB", path_bytes(&report.source_b))?;
    table.set(scope, "higherPriority", path_bytes(&report.higher_priority))?;
    table.set(scope, "shared", &string_list(scope, &report.shared)?)?;
    table.set(scope, "onlyInA", &string_list(scope, &report.only_in_a)?)?;
    table.set(scope, "onlyInB", &string_list(scope, &report.only_in_b)?)?;
    Ok(table)
}

pub(super) fn copied(scope: &impl Scope, copied: &[(PathBuf, PathBuf)]) -> Result<Table> {
    array(scope, copied.len(), |frame, i| {
        let (relative, destination) = &copied[i];
        push_record(frame, 2, |frame, table| {
            frame.push(path_bytes(relative))?;
            table.raw_set(frame, "relativePath")?;
            frame.push(path_bytes(destination))?;
            table.raw_set(frame, "destinationPath")
        })
        .map(drop)
    })
}
