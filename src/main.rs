use std::{
    fs::File as StdFile,
    io::{Result, Write},
    path::PathBuf,
};
use vfstool::{SerializeType, display_tree::VFSSerialize, vfs::VFS};

fn main() -> Result<()> {
    let mut vfs = VFS::new();

    let data_directories =
        openmw_cfg::get_data_dirs(&openmw_cfg::get_config().expect("")).expect("");

    let data_paths: Vec<PathBuf> = data_directories
        .iter()
        .map(|str| PathBuf::from(str))
        .collect();

    vfs.add_files_from_directories(data_paths);

    // vfs.add_files_from_directories(&[mw_dir.clone(), sw_dir, pfp_dir]);
    // dbg!(&vfs);

    // Perform a lookup
    // if let Some(file) = vfs.get_file(&"music/explore/mx_ExPlOrE_2.mp3") {
    //     println!("Found file: {}", file.get_path().display());
    //     // Open the file
    //     let mut file_stream = file.open().expect("Failed to open file");
    //     let mut contents = Vec::new();
    //     file_stream
    //         .read_to_end(&mut contents)
    //         .expect("Failed to read file");
    //     println!("File contents: {}", contents.len());
    // } else {
    //     println!("File not found.");
    // }

    // let prefix = "music/explore";
    // vfs.par_paths_with(prefix).for_each(|(path, file)| {
    //     let mut fd = file.open().expect("");
    //     let mut contents = Vec::new();
    //     fd.read_to_end(&mut contents).expect("");
    //     println!(
    //         "Found prefix-matching file in VFS: {} of size {}",
    //         path.display(),
    //         contents.len()
    //     );
    // });

    // let explore_tracks: Vec<&Arc<VfsFile>> =
    //     vfs.par_paths_with(prefix).map(|(_, file)| file).collect();
    // let rng = std::time::SystemTime::now().elapsed().unwrap().as_secs() as usize; // Get the elapsed time in seconds
    // let random_index = rng % explore_tracks.len();

    // // let random_index = rand::random::<usize>() % explore_tracks.len();
    // let random_track = explore_tracks[random_index];

    // let mut fd = random_track.open().expect("");
    // let mut contents = Vec::new();
    // fd.read_to_end(&mut contents).expect("");

    // println!(
    //     "Picked random explore track from VFS: {} of size {}",
    //     random_track.path.display(),
    //     contents.len()
    // );

    // for (path, file) in vfs.paths_with(prefix) {
    //     let mut fd = file.open().expect("");
    //     let mut contents = Vec::new();
    //     fd.read_to_end(&mut contents).expect("");
    //     println!(
    //         "Found prefix-matching file in VFS: {} of size {}",
    //         path.display(),
    //         contents.len()
    //     );
    // }

    // let prefix = "explore/";
    // for (path, file) in vfs.paths_matching(prefix) {
    //     let mut fd = file.open().expect("");
    //     let mut contents = Vec::new();
    //     fd.read_to_end(&mut contents).expect("");
    //     println!(
    //         "Found fuzzy matching file in VFS: {} of size {}",
    //         path.display(),
    //         contents.len()
    //     );
    // }

    // let suffix = ".bik";
    // vfs.par_paths_matching(suffix).for_each(|(path, file)| {
    //     let mut fd = file.open().expect("");
    //     let mut contents = Vec::new();
    //     fd.read_to_end(&mut contents).expect("");
    //     println!(
    //         "Found suffixed file in VFS: {} of size {} at true path: {}",
    //         path.display(),
    //         contents.len(),
    //         file.path.display(),
    //     );
    // });

    // dbg!(&vfs.tree(false));
    let tree = vfs.tree(false);
    // let filter_tree = vfs.tree_filtered(false, |dir| dir.contains("core"), |_| true);
    // dbg!(&filter_tree);
    let filter_tree = vfs.tree_filtered(true, |file| {
        let file_string = file.path.to_string_lossy().to_string().to_ascii_lowercase();

        // file_string.contains("oaabdata")
        file_string.contains("mwse")
            && (file_string.contains(".json") || file_string.contains(".lua"))
    });

    // let toml = toml::to_string_pretty(&filter_tree).expect("Failed to serialize to toml!");
    // let mut file =
    //     StdFile::create("lua.toml").expect("Failed to create new file for filtered tree!");
    // let _ = write!(file, "{}", &toml);
    // dbg!(&filter_tree);

    let json = serde_json::to_string_pretty(&tree).unwrap();
    let mut file = StdFile::create("vfs.json").expect("");
    let _ = write!(file, "{}", &json);

    // let toml = toml::to_string_pretty(&tree).unwrap();
    // let mut file = StdFile::create("vfs.toml").expect("");
    // let _ = write!(file, "{}", &toml);

    let yaml = serde_yaml_with_quirks::to_string(&tree).unwrap();
    let mut file = StdFile::create("vfs.yaml").expect("");
    let _ = write!(file, "{}", &yaml);

    let toml = toml::to_string_pretty(&tree).unwrap();
    let mut file = StdFile::create("vfs.toml").expect("");
    let _ = write!(file, "{}", &toml);

    filter_tree.to_serialized("lua.toml", SerializeType::Toml)?;

    // let _ = write!(file, "{}", vfs.display_filtered(false, |_| true, |_| true));

    // let _ = file = StdFile::create("meshes.txt").expect("");
    // let _ = write!(
    //     file,
    //     "{}",
    //     vfs.display_filtered(
    //         true,
    //         |dir| dir.contains("meshes"),
    //         |file| file.contains("hlaalu")
    //     )
    // );

    // let _ = file = StdFile::create("absolute_meshes.txt").expect("");
    // let _ = write!(
    //     file,
    //     "{}",
    //     vfs.display_filtered(
    //         false,
    //         |dir| dir.contains("meshes"),
    //         |file| file.contains("hlaalu")
    //     )
    // );

    // let _ = file = StdFile::create("lua_mods.txt").expect("");
    // let _ = write!(
    //     file,
    //     "{}",
    //     vfs.display_filtered(false, |_| true, |file| file.contains("omwscripts"))
    // );

    // let _ = file = StdFile::create("solthas.txt").expect("");
    // let _ = write!(
    //     file,
    //     "{}",
    //     vfs.display_filtered(
    //         true,
    //         |dir| dir.contains("scripts") || dir.contains("shaders"),
    //         |_| true
    //     )
    // );

    // let filter_tree = vfs.tree_filtered(
    //     true,
    //     |dir| dir.contains("textures"),
    //     |file| file.contains("selkath"),
    // );
    // dbg!(&filter_tree);

    // let filter_tree = vfs.tree_filtered(
    //     true,
    //     |dir| dir.contains("meshes"),
    //     |file| file.contains("wookie"),
    // );
    // dbg!(&filter_tree);

    // let filter_tree = vfs.tree_filtered(
    //     true,
    //     |dir| dir.contains("icons"),
    //     |file| file.contains("book"),
    // );
    // dbg!(&filter_tree);

    // let filter_tree = vfs.tree_filtered(
    //     true,
    //     |dir| dir.contains("music"),
    //     |file| file.contains("mx"),
    // );
    // dbg!(&filter_tree);

    // let filter_tree = vfs.tree_filtered(true, |dir| dir.contains("video"), |_| true);
    // dbg!(&filter_tree);

    Ok(())
}
