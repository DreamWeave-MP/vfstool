use dw_vfs_lib::{SerializeType, vfs::VFS};
use std::{io::Result, path::PathBuf};

fn main() -> Result<()> {
    let config = openmw_cfg::get_config().expect("[ CRITICAL ERROR ]: FAILED TO READ OPENMW_CFG!");

    let data_directories = openmw_cfg::get_data_dirs(&config)
        .expect("[CRITICAL ERROR ]: FAILED TO GET DATA DIRECTORIES FROM OPENMW.CFG!");

    let data_paths: Vec<PathBuf> = data_directories.iter().map(PathBuf::from).collect();

    // Collect archives from openmw.cfg, in order
    let archives = config
        .general_section()
        .iter()
        .filter_map(|(k, v)| match k == "fallback-archive" {
            false => None,
            true => Some(v),
        })
        .collect();

    let vfs = VFS::from_directories(data_paths, Some(archives));

    let filtered = vfs.tree_filtered(false, |vfs_file| vfs_file.is_archive());

    VFS::serialize_from_tree(&filtered, "in_archives.yaml", SerializeType::Yaml)?;

    Ok(())
}
