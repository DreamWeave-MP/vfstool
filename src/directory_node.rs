use crate::{DisplayTree, VfsFile};
use serde::{Serialize, Serializer, ser::SerializeMap};
use std::{borrow::Cow, collections::BTreeMap, sync::Arc};

pub trait VFSDirectory {
    fn sort(&mut self);

    fn filter<F>(&mut self, file_filter: &F)
    where
        F: Fn(&Arc<VfsFile>) -> bool;
}

impl VFSDirectory for DirectoryNode {
    fn sort(&mut self) {
        self.files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        self.subdirs.values_mut().for_each(|dir| dir.sort());
    }

    fn filter<F>(&mut self, file_filter: &F)
    where
        F: Fn(&Arc<VfsFile>) -> bool,
    {
        self.files.retain(file_filter);
        self.subdirs.retain(|_path, subdir| {
            subdir.filter(file_filter);
            !subdir.files.is_empty() || !subdir.subdirs.is_empty()
        });
    }
}

#[derive(Debug)]
pub struct DirectoryNode {
    pub files: Vec<Arc<VfsFile>>,
    pub subdirs: DisplayTree,
}

impl DirectoryNode {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            subdirs: BTreeMap::new(),
        }
    }
}

impl Serialize for DirectoryNode {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(
            self.subdirs.len() + if self.files.is_empty() { 0 } else { 1 },
        ))?;

        if !self.files.is_empty() {
            map.serialize_entry(
                ".",
                &self
                    .files
                    .iter()
                    .map(|file| file.file_name().unwrap_or_default().to_string_lossy())
                    .collect::<Vec<Cow<'_, str>>>(),
            )?;
        }

        for (dir_name, subdir) in &self.subdirs {
            let dir_key = dir_name.file_name().unwrap_or_default().to_string_lossy();

            map.serialize_entry(&dir_key, subdir)?;
        }

        map.end()
    }
}
