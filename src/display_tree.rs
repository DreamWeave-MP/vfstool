use std::{
    collections::BTreeMap,
    fs::File,
    io::{Error, ErrorKind, Result, Write},
    path::{Path, PathBuf},
};

use crate::DirectoryNode;

pub type DisplayTree = BTreeMap<PathBuf, DirectoryNode>;

pub enum SerializeType {
    Json,
    Yaml,
    Toml,
}

#[doc(hidden)]
mod _vfs_serialize {
    pub trait PrivateVFSSerialize {}
    impl PrivateVFSSerialize for super::DisplayTree {}
}

pub trait VFSSerialize: _vfs_serialize::PrivateVFSSerialize {
    fn to_serialized<P: AsRef<Path>>(&self, file_name: P, write_type: SerializeType) -> Result<()>;
}

impl VFSSerialize for DisplayTree {
    fn to_serialized<P: AsRef<Path>>(&self, file_name: P, write_type: SerializeType) -> Result<()> {
        fn to_io_error<E: std::fmt::Display>(err: E) -> Error {
            Error::new(ErrorKind::InvalidData, err.to_string())
        }

        let serialized_content = match write_type {
            SerializeType::Json => serde_json::to_string_pretty(&self).map_err(to_io_error)?,
            SerializeType::Yaml => serde_yaml_with_quirks::to_string(&self).map_err(to_io_error)?,
            SerializeType::Toml => toml::to_string_pretty(&self).map_err(to_io_error)?,
        };

        let mut output_file = File::create(file_name)?;
        write!(output_file, "{}", serialized_content)?;

        Ok(())
    }
}
