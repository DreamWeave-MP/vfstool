use rayon::prelude::*;
use serde::Serialize;
use serde_yaml_with_quirks as Yaml;
use walkdir::{Error as WalkError, WalkDir};

// Implement file type enum
// Make DisplayTrees serializable

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    fs::File as StdFile,
    io::{self, Read, Seek},
    ops::Index,
    path::{Component, Path, PathBuf},
    sync::Arc,
};

// Owned
type VFSFiles = HashMap<PathBuf, Arc<VfsFile>>;

#[derive(Serialize, Debug)]
#[serde(untagged)]
enum FileOrDir {
    Dir(BTreeMap<String, FileOrDir>),
    Files(Vec<String>),
}
type DisplayTree<'a> = BTreeMap<Cow<'a, str>, Vec<Cow<'a, str>>>;
trait S3rialize {
    fn to_serialized(&self) -> FileOrDir;
}
impl S3rialize for DisplayTree<'_> {
    fn to_serialized(&self) -> FileOrDir {
        let mut root = BTreeMap::new();

        for (dir, files) in self {
            let mut parts = dir.split('/').filter(|s| !s.is_empty()).map(String::from);
            let mut current = &mut root;

            while let Some(part) = parts.next() {
                current = current
                    .entry(part)
                    .or_insert_with(|| FileOrDir::Dir(BTreeMap::new()))
                    .as_dir_mut()
                    .unwrap();
            }

            // Insert files as a list of strings, without the need for keys
            let file_list = files
                .iter()
                .map(|file| file.to_string())
                .collect::<Vec<String>>();
            current.insert("files".to_string(), FileOrDir::Files(file_list));
        }

        FileOrDir::Dir(root)
    }
}

impl FileOrDir {
    fn as_dir_mut(&mut self) -> Option<&mut BTreeMap<String, FileOrDir>> {
        match self {
            FileOrDir::Dir(map) => Some(map),
            _ => None,
        }
    }
}

type MaybeFile<'a> = Option<&'a Arc<VfsFile>>;
type VFSTuple<'a> = (&'a Path, &'a Arc<VfsFile>);

// Define a new trait that combines Read and Seek
trait ReadSeek: Read + Seek {}

// Explicitly implement the ReadSeek trait for std::fs::File
impl ReadSeek for StdFile {}

// This trait mimics the interface of OpenMW's `File`
trait File {
    fn open(&self) -> io::Result<Box<dyn ReadSeek>>;
    fn get_path(&self) -> &Path;
}

#[derive(Debug)]
// Struct representing a file in the VFS
struct VfsFile {
    path: PathBuf,
}

impl VfsFile {
    fn new(path: PathBuf) -> Self {
        VfsFile { path }
    }
}

impl File for VfsFile {
    fn open(&self) -> io::Result<Box<dyn ReadSeek>> {
        let file = StdFile::open(&self.path)?;
        Ok(Box::new(file))
    }

    fn get_path(&self) -> &Path {
        &self.path
    }
}
/// Sentinel VfsFile, representing an invalid path
impl Default for VfsFile {
    fn default() -> Self {
        VfsFile {
            path: PathBuf::new(),
        }
    }
}

impl PartialEq<VfsFile> for &VfsFile {
    fn eq(&self, other: &VfsFile) -> bool {
        self == other
    }
}

struct VFS {
    file_map: VFSFiles,
}

impl VFS {
    const DIR_PREFIX: &str = "├── ";
    const FILE_PREFIX: &str = "│   ├── ";

    pub fn new() -> Self {
        Self {
            file_map: HashMap::new(),
        }
    }

    /// Lowercase path and convert path separators to unix-style
    fn normalize_path<P: AsRef<Path>>(path: P) -> PathBuf {
        let path = path
            .as_ref()
            .to_string_lossy()
            .to_lowercase()
            .replace("\\", "/");
        PathBuf::from(path)
    }

    /// Looks up a file in the VFS after normalizing the path
    pub fn get_file<P: AsRef<Path>>(&self, path: P) -> MaybeFile {
        let normalized_path = Self::normalize_path(path.as_ref());
        self.file_map.get(&normalized_path)
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn paths_matching<S: AsRef<str>>(&self, substring: S) -> impl Iterator<Item = VFSTuple> {
        let normalized_substring = Self::normalize_path(substring.as_ref())
            .to_string_lossy()
            .into_owned();

        self.file_map.iter().filter_map(move |(path, file)| {
            if path.to_string_lossy().contains(&normalized_substring) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Given a substring, return an iterator over all paths that contain it.
    pub fn par_paths_matching<S: AsRef<str>>(
        &self,
        substring: S,
    ) -> impl ParallelIterator<Item = VFSTuple> {
        let normalized_substring = Self::normalize_path(substring.as_ref())
            .to_string_lossy()
            .into_owned();

        self.file_map.par_iter().filter_map(move |(path, file)| {
            if path.to_string_lossy().contains(&normalized_substring) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    pub fn paths_with<P: AsRef<Path>>(&self, prefix: P) -> impl Iterator<Item = VFSTuple> {
        let normalized_prefix = Self::normalize_path(&prefix);

        self.file_map.iter().filter_map(move |(path, file)| {
            if path.starts_with(&normalized_prefix) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Given a path prefix to a location in the VFS, return an iterator to *all* of its contents.
    pub fn par_paths_with<P: AsRef<Path>>(
        &self,
        prefix: P,
    ) -> impl ParallelIterator<Item = VFSTuple> {
        let normalized_prefix = Self::normalize_path(&prefix);

        self.file_map.par_iter().filter_map(move |(path, file)| {
            if path.starts_with(&normalized_prefix) {
                Some((path.as_path(), file))
            } else {
                None
            }
        })
    }

    /// Walkdir helper to filter out directories
    /// and somehow-nonexistent or inaccessible files
    fn valid_file(entry: Result<walkdir::DirEntry, WalkError>) -> Option<walkdir::DirEntry> {
        match entry {
            Err(_) => None,
            Ok(entry) => match entry.metadata().is_ok() && entry.file_type().is_file() {
                true => Some(entry),
                false => None,
            },
        }
    }

    /// Given some set which can be interpreted as a parallel iterator of paths,
    /// Load all of them into the VFS in parallel fashion
    pub fn add_files_from_directories(
        &mut self,
        search_dirs: impl IntoParallelIterator<Item = impl AsRef<Path> + Sync>,
    ) {
        self.file_map
            .par_extend(search_dirs.into_par_iter().flat_map(|dir| {
                let dir = dir.as_ref().to_path_buf();

                WalkDir::new(&dir)
                    .into_iter()
                    .filter_map(|entry| Self::valid_file(entry))
                    .par_bridge()
                    .map(move |entry| {
                        let path = entry.path().to_path_buf();

                        let normalized_path =
                            Self::normalize_path(&path.strip_prefix(&dir).unwrap_or(&path));

                        let vfs_file = VfsFile::new(path);
                        (normalized_path, Arc::new(vfs_file))
                    })
            }))
    }

    /// Returns a sorted version of the VFS contents as a binary tree
    /// Easier to display.
    pub fn tree(&self, relative: bool) -> DisplayTree {
        let mut tree: DisplayTree = BTreeMap::new();

        let mut paths: Vec<_> = match relative {
            true => self.file_map.keys().collect(),
            false => self.file_map.values().map(|entry| &entry.path).collect(),
        };

        paths.sort();

        paths.iter().for_each(|path| {
            let mut components = path.components();

            if let Some(Component::Normal(file)) = components.next_back() {
                let dir = components.as_path();

                let dir_str = if dir.as_os_str().is_empty() {
                    Cow::Borrowed("/")
                } else {
                    Cow::Owned(dir.to_string_lossy().into_owned())
                };

                let entry_str = Cow::Owned(file.to_string_lossy().into_owned());

                tree.entry(dir_str).or_default().push(entry_str);
            }
        });

        tree
    }

    /// Return a matching set of vfs entries from filter predicates for directories and files
    /// Might be empty.
    pub fn tree_filtered(
        &self,
        relative: bool,
        dir_filter: impl Fn(&str) -> bool,
        file_filter: impl Fn(&str) -> bool,
    ) -> DisplayTree {
        let mut tree = self.tree(relative);

        tree.retain(|dir, files| {
            files.retain(|file| file_filter(file));
            dir_filter(dir) && !files.is_empty()
        });

        tree
    }

    /// String formatter for the file tree
    /// Includes a newline, so caller is responsible for using the appropriate writer
    fn file_str<S: AsRef<str> + std::fmt::Display>(file: S) -> String {
        format!("{}{}\n", Self::FILE_PREFIX, file,)
    }

    fn dir_str<S: AsRef<str> + std::fmt::Display>(dir: S) -> String {
        format!("{}{}/\n", Self::DIR_PREFIX, dir,)
    }

    /// Returns the formatted file tree for a filtered subset
    pub fn display_filtered<'a>(
        &self,
        relative: bool,
        dir_filter: impl Fn(&str) -> bool,
        file_filter: impl Fn(&str) -> bool,
    ) -> String {
        let tree = self.tree(relative);
        let mut output = String::new();

        for (dir, mut files) in tree {
            if !dir_filter(&dir) {
                continue;
            }

            files.retain(|file| file_filter(file));

            if files.is_empty() {
                continue;
            } else if dir != "/" {
                output.push_str(&Self::dir_str(&dir));
            }

            files
                .iter()
                .for_each(|file| output.push_str(&Self::file_str(file)));
        }

        output
    }
}

impl std::fmt::Display for VFS {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "/")?;
        for (dir, files) in &self.tree(true) {
            if dir != "/" {
                write!(f, "{}", Self::dir_str(dir))?;
            }
            for file in files {
                write!(f, "{}", Self::file_str(file))?;
            }
        }
        Ok(())
    }
}

impl Index<&str> for VFS {
    type Output = VfsFile;

    fn index(&self, index: &str) -> &Self::Output {
        let normalized_path = Self::normalize_path(index);

        // If the path exists in the file_map, return the file, otherwise return a default value
        self.file_map
            .get(&normalized_path)
            .map(|file| file.as_ref()) // Dereference Arc<VfsFile> to &VfsFile
            .unwrap_or_else(|| {
                static DEFAULT_FILE: std::sync::OnceLock<VfsFile> = std::sync::OnceLock::new();
                DEFAULT_FILE.get_or_init(|| VfsFile::default())
            })
    }
}

use std::io::Write;

fn main() {
    let mut vfs = VFS::new();
    let mw_dir = PathBuf::from("/home/sk3shun-8/BethGames/Morrowind/Data Files/");
    let sw_dir = PathBuf::from("/home/sk3shun-8/openmw/umomwd/starwind-modded/TotalConversions/Starwindv3AStarWarsConversion/Starwind3.1/Data Files/");
    let pfp_dir = PathBuf::from(
        "/home/sk3shun-8/openmw/umomwd/total-overhaul/PatchesFixesandConsistency/PatchforPurists/",
    );

    let data_directories =
        openmw_cfg::get_data_dirs(&openmw_cfg::get_config().expect("")).expect("");

    let data_paths: Vec<PathBuf> = data_directories
        .iter()
        .map(|str| PathBuf::from(str))
        .collect();

    vfs.add_files_from_directories(data_paths);

    // vfs.add_files_from_directories(&[mw_dir.clone(), sw_dir, pfp_dir]);
    // dbg!(&vfs.file_map);

    // Perform a lookup
    if let Some(file) = vfs.get_file(&"music/explore/mx_ExPlOrE_2.mp3") {
        println!("Found file: {}", file.get_path().display());
        // Open the file
        let mut file_stream = file.open().expect("Failed to open file");
        let mut contents = Vec::new();
        file_stream
            .read_to_end(&mut contents)
            .expect("Failed to read file");
        println!("File contents: {}", contents.len());
    } else {
        println!("File not found.");
    }

    let prefix = "music/explore";
    vfs.par_paths_with(prefix).for_each(|(path, file)| {
        let mut fd = file.open().expect("");
        let mut contents = Vec::new();
        fd.read_to_end(&mut contents).expect("");
        println!(
            "Found prefix-matching file in VFS: {} of size {}",
            path.display(),
            contents.len()
        );
    });

    let explore_tracks: Vec<&Arc<VfsFile>> =
        vfs.par_paths_with(prefix).map(|(_, file)| file).collect();
    let rng = std::time::SystemTime::now().elapsed().unwrap().as_secs() as usize; // Get the elapsed time in seconds
    let random_index = rng % explore_tracks.len();

    // let random_index = rand::random::<usize>() % explore_tracks.len();
    let random_track = explore_tracks[random_index];

    let mut fd = random_track.open().expect("");
    let mut contents = Vec::new();
    fd.read_to_end(&mut contents).expect("");

    println!(
        "Picked random explore track from VFS: {} of size {}",
        random_track.path.display(),
        contents.len()
    );

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

    let suffix = ".bik";
    vfs.par_paths_matching(suffix).for_each(|(path, file)| {
        let mut fd = file.open().expect("");
        let mut contents = Vec::new();
        fd.read_to_end(&mut contents).expect("");
        println!(
            "Found suffixed file in VFS: {} of size {} at true path: {}",
            path.display(),
            contents.len(),
            file.path.display(),
        );
    });

    let mut file = StdFile::create("vfs.txt").expect("");
    let _ = write!(file, "{}", vfs);

    let _ = file = StdFile::create("meshes.txt").expect("");
    let _ = write!(
        file,
        "{}",
        vfs.display_filtered(
            true,
            |dir| dir.contains("meshes"),
            |file| file.contains("hlaalu")
        )
    );

    let _ = file = StdFile::create("absolute_meshes.txt").expect("");
    let _ = write!(
        file,
        "{}",
        vfs.display_filtered(
            false,
            |dir| dir.contains("meshes"),
            |file| file.contains("hlaalu")
        )
    );

    let _ = file = StdFile::create("solthas.txt").expect("");
    let _ = write!(
        file,
        "{}",
        vfs.display_filtered(false, |dir| true, |file| file.contains("omwscripts"))
    );

    let filter_tree = vfs.tree_filtered(
        true,
        |dir| dir.contains("textures"),
        |file| file.contains("selkath"),
    );
    dbg!(&filter_tree);

    let filter_tree = vfs.tree_filtered(
        true,
        |dir| dir.contains("meshes"),
        |file| file.contains("wookie"),
    );
    dbg!(&filter_tree);

    let filter_tree = vfs.tree_filtered(
        true,
        |dir| dir.contains("icons"),
        |file| file.contains("book"),
    );
    dbg!(&filter_tree);

    let filter_tree = vfs.tree_filtered(
        true,
        |dir| dir.contains("music"),
        |file| file.contains("mx"),
    );
    dbg!(&filter_tree);

    let filter_tree = vfs.tree_filtered(true, |dir| dir.contains("video"), |_| true);
    dbg!(&filter_tree);

    let tree = &vfs.tree(false);
    let serialized_tree = tree.to_serialized();

    let json = serde_json::to_string_pretty(&serialized_tree);
    if let Err(ref err) = json {
        eprintln!("Failed serializing vfs to json: {}", err.to_string())
    }

    let json_file = StdFile::create("vfs.json");
    match json_file {
        Err(err) => eprintln!(
            "Failed writing serialized VFS to json file: {}",
            err.to_string()
        ),
        Ok(mut file) => {
            write!(file, "{}", json.expect("")).expect("");
        }
    }

    // dbg!(&vfs.tree(false).to_serialized());

    // println!("{}", vfs);

    // for (path, file) in vfs.paths_matching(suffix) {
    //     let mut fd = file.open().expect("");
    //     let mut contents = Vec::new();
    //     fd.read_to_end(&mut contents).expect("");
    //     println!(
    //         "Found suffixed file in VFS: {} of size {} at true path: {}",
    //         path.display(),
    //         contents.len(),
    //         file.path.display(),
    //     );
    // }

    // let found = vfs.paths_matching("narsuite.nif");
    // for (path, file) in found {
    //     dbg!(path, file);
    // }
}
