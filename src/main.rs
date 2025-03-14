use clap::{Parser, Subcommand, ValueEnum};
use dw_vfs_lib::{SerializeType, VfsFile, normalize_path, vfs::VFS};
use rayon::prelude::*;
use std::{
    env,
    fs::{self, hard_link, metadata},
    io::{self, Result, Write},
    path::PathBuf,
};

pub const RED: &str = "\x1b[31m";
pub const GREEN: &str = "\x1b[32m";
pub const BLUE: &str = "\x1b[34m";
pub const WHITE: &str = "\x1b[37m";
pub const RESET: &str = "\x1b[0m"; // Reset to default terminal color

#[derive(Parser)]
#[command(
    name = "vfstool",
    about = "vfstool allows users to reconstruct and interact with OpenMW's virtual file system in any way they might see fit, using this application to locate files, serialize their VFS to most major text formats, extract files out of the vfs, and even collapse their VFS to a single directory for space savings."
)]
struct Cli {
    /// Path to openmw.cfg.
    ///
    /// Note this is the directory containing it, not the path to the file itself.
    ///
    /// Example: C:\Documents\My Games\openmw
    ///
    /// This argument assumes the config used is called `openmw.cfg`
    /// (case-insensitive).
    ///
    /// If you need to use an openmw.cfg which is named something else,
    ///
    /// set the `OPENMW_CONFIG` variable to the absolute path of your desired config file instead.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Whether or not to use relative paths in output
    #[arg(short = 'r', long)]
    use_relative: bool,

    #[command(subcommand)]
    command: Commands,
}

/// Subcommands for `vfstool`
#[derive(Subcommand)]
enum Commands {
    /// Given a target directory, create a set of hardlinks for the entire virtual
    /// filesystem inside of it. Skyrim support ;)
    Collapse {
        /// Target folder to collapse the VFS into
        collapse_into: PathBuf,

        /// If this is used, any case where hard linking failed or won't work (files in BSA
        /// archives), falls back to normal copying operations
        ///
        /// Without this argument, files inside of BSA archives are ignored completely during
        /// collapsing.
        #[arg(short, long)]
        allow_copying: bool,
    },
    /// Extract a given file from the VFS into a given directory
    Extract {
        /// Full relative path to a VFS file, eg meshes/xbase_anim.nif
        source_file: PathBuf,

        /// Directory to extract the file to
        target_dir: PathBuf,
    },
    /// Given some VFS path, like `meshes/xbase_anim.nif`, return its absolute path (if found)
    FindFile {
        /// Full (relative) VFS Path to query.
        /// Returns the absolute path, of the file referenced by this VFS path. EG:
        ///
        /// vfstool find-file meshes/xbase_anim.nif
        ///
        /// C:\Games\Morrowind\Data Files\Meshes\XBase_Anim.nif
        path: PathBuf,

        /// Simple output, no coloration or formatting. Useful for pipes
        #[arg(short, long)]
        simple: bool,
    },
    /// Given some query term, locate all matches in the vfs.
    Find {
        /// VFS Path to query. What exactly the input should be depends on the `--filter` argument.
        #[arg(short, long)]
        path: PathBuf,

        /// Output format when serializing as text.
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,

        /// Path to save the resulting search tree to.
        ///
        /// If omitted, the result is printed directly to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Type of filter to use when searching.
        ///
        /// For an `exact` filter, to find `meshes/xbase_anim.nif`, you must use
        /// `meshes/xbase_anim.nif`
        ///
        /// For a `name` filter, you can simply use `xbase_anim.nif`, etc.
        ///
        /// Only the `exact` filter is guaranteed to return a single result, if it does.
        ///
        /// `folder` filters will match any parent directories of the file, eg `meshes/` would
        /// locate all files under the `meshes/` directory and not deeper subdirectories.
        ///
        /// `prefix` filters will match any prefix on the *normalized* path of the file.
        ///
        /// `extension` filters will match only the file extension.
        ///
        /// `name` and `name-exact` filters will match either on strings which the
        /// filename contains or the exact file name.
        ///
        /// `stem` and `stem-exact` work in the same manner, but the file extension is not
        /// included in matching.
        #[arg(short, long, default_value = "name")]
        r#type: FindType,
    },
    /// Given an absolute path, return a filtered version of the VFS containing either things
    /// replacing it, or files from this directory which are not being replaced
    Remaining {
        filter_path: PathBuf,

        /// If used, show only files replacing contents of this path, instead of ones still in it
        #[arg(short, long)]
        replacements_only: bool,

        /// Output format when serializing as text.
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,

        /// Path to save the resulting search tree to.
        ///
        /// If omitted, the result is printed directly to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
}

/// Supported output formats
#[derive(Debug, ValueEnum, Clone)]
enum OutputFormat {
    Json,
    Yaml,
    Toml,
}

/// Type of search to do when finding a file
#[derive(Debug, PartialEq, ValueEnum, Clone)]
enum FindType {
    Contains,
    Extension,
    Folder,
    Prefix,
    Stem,
    StemExact,
    Name,
    NameExact,
}

fn validate_config_dir(dir: &PathBuf) -> io::Result<()> {
    let dir_metadata = metadata(&dir);
    let default_location = openmw_cfg::config_path();

    let config_arg_fail = match dir_metadata.is_ok() && dir_metadata.unwrap().is_dir() {
        false => Some(format!(
            "[ WARNING ]: The requested openmw.cfg dir {} is not a directory! Using the system default location of {} instead.",
            dir.display(),
            &default_location.display()
        )),
        true => {
            match fs::read_dir(&dir)?
                .filter_map(|entry| entry.ok())
                .find(|entry| entry.file_name().eq_ignore_ascii_case("openmw.cfg"))
                .map(|entry| entry.path())
            {
                None => Some(format!(
                    "[ WARNING ]: An openmw.cfg could not be located in {}! Using the system default location of {} instead.",
                    dir.display(),
                    &default_location.display()
                )),
                Some(dir) => {
                    // This is a single threaded application!
                    unsafe { env::set_var("OPENMW_CONFIG", &dir) };
                    None
                }
            }
        }
    };

    if let Some(fail_message) = config_arg_fail {
        eprintln!("{}", fail_message);
    };

    Ok(())
}

fn get_config() -> openmw_cfg::Ini {
    openmw_cfg::get_config().expect(&format!(
        "{RED}[ CRITICAL ERROR ]{RESET}: Failed to read openmw_cfg!"
    ))
}

fn get_data_paths(config: &openmw_cfg::Ini) -> Vec<PathBuf> {
    openmw_cfg::get_data_dirs(&config)
        .expect(&format!(
            "{RED}[CRITICAL ERROR ]{RESET}: Failed to get data directories from Openmw.cfg!"
        ))
        .iter()
        .map(PathBuf::from)
        .collect()
}

fn filter_data_paths(to_keep: &PathBuf, paths: &mut Vec<PathBuf>) {
    paths.retain(|path| normalize_path(&path).eq(&normalize_path(&to_keep)))
}

fn construct_vfs() -> VFS {
    let config = get_config();

    let data_paths = get_data_paths(&config);

    // Collect archives from openmw.cfg, in order
    let archives = config
        .general_section()
        .iter()
        .filter_map(|(k, v)| match k == "fallback-archive" {
            false => None,
            true => Some(v),
        })
        .collect();

    VFS::from_directories(data_paths, Some(archives))
}

fn main() -> Result<()> {
    let args = Cli::parse();

    if let Some(config_path) = args.config {
        validate_config_dir(&config_path)?;
    }

    let vfs = construct_vfs();

    match args.command {
        Commands::Collapse {
            collapse_into,
            allow_copying,
        } => {
            if metadata(&collapse_into).is_err() {
                fs::create_dir_all(&collapse_into)?;
            };

            vfs.par_iter().for_each(|(relative_path, file)| {
                let merged_path = collapse_into.join(relative_path);
                let merged_dir = merged_path.parent().unwrap();

                if metadata(&merged_dir).is_err() {
                    fs::create_dir_all(&merged_dir).unwrap();
                };

                if file.is_loose() {
                    assert!(file.path().exists());

                    if metadata(&merged_path).is_ok() {
                        fs::remove_file(&merged_path).unwrap();
                    }

                    // Since we extract files *out of* BSA archives
                    // Don't bother including them in the collapsed directory
                    if let Some(extension) = file.path().extension() {
                        if extension == "bsa" && allow_copying {
                            println!("Skipping archive {}", file.file_name().unwrap());
                            return;
                        }
                    }

                    if let Err(error) = hard_link(file.path(), &merged_path) {
                        eprintln!(
                            "Hardlink attempt for {} failed due to error: {}",
                            file.path().display(),
                            error.to_string()
                        );

                        if allow_copying {
                            if let Err(error) = fs::copy(file.path(), &merged_path) {
                                eprintln!(
                                    "Fallback file copying was enabled, but copying {} to {} failed due to {}!",
                                    file.path().display(),
                                    merged_path.display(),
                                    error.to_string()
                                );
                            }
                        }
                    } else {
                        let new_metadata = metadata(&merged_path).unwrap();
                        let old_metadata = metadata(file.path()).unwrap();
                        assert_eq!(new_metadata.len(), old_metadata.len());
                        println!("Successfully wrote {} to {}", file.path().display(), merged_path.display());
                    };
                } else {
                    if !allow_copying {
                        println!(
                            "Skipping {}, which is loaded from a BSA file at: {}",
                            relative_path.display(),
                            file.parent_archive_path().unwrap()
                        )
                    } else {
                        match file.open() {
                            Ok(mut data) => {
                                let mut buf: Vec<u8> = Vec::new();
                                if let Ok(_) = data.read_to_end(&mut buf) {
                                    if let Err(error) = fs::write(&merged_path, buf) {
                                        eprintln!(
                                            "Extracting archived file {} to {} failed due to {}!",
                                            relative_path.display(),
                                            merged_path.display(),
                                            error.to_string()
                                        );
                                    };
                                };
                            }
                            Err(error) => {
                                eprintln!("Failed to open archived file: {}", error.to_string())
                            }
                        };
                    }
                }
            });
        }
        Commands::Extract {
            source_file,
            target_dir,
        } => match vfs.get_file(&source_file) {
            Some(file) => {
                let mut dir_meta = metadata(&target_dir);

                if dir_meta.is_err() {
                    fs::create_dir_all(&target_dir)?;
                    dir_meta = metadata(&target_dir);
                }

                let dir_meta = dir_meta?;

                if dir_meta.is_dir() {
                    match source_file.file_name() {
                        Some(name) => {
                            let target_path = target_dir.join(name);

                            if file.is_loose() {
                                if let Err(error) = fs::copy(file.path(), &target_path) {
                                    eprintln!(
                                        "{RED}[ ERROR ]{RESET}: Failed extracting loose file from the vfs: {GREEN}{}{RESET}",
                                        error.to_string()
                                    );
                                } else {
                                    println!(
                                        "{GREEN}[ SUCCESS ]{RESET}: Successfully extracted {GREEN}{}{RESET} to {BLUE}{}{RESET}",
                                        file.path().display(),
                                        target_dir.display()
                                    );
                                };
                            } else {
                                match file.open() {
                                    Ok(mut data) => {
                                        let mut buf: Vec<u8> = Vec::new();
                                        if let Ok(_) = data.read_to_end(&mut buf) {
                                            if let Err(error) = fs::write(&target_path, buf) {
                                                eprintln!(
                                                    "{RED}[ ERROR ]{RESET}: Extracting archived file {GREEN}{}{RESET} to {BLUE}{}{RESET} failed due to {RED}{}{RESET}!",
                                                    source_file.display(),
                                                    target_path.display(),
                                                    error.to_string()
                                                );
                                            } else {
                                                println!(
                                                    "{GREEN}[ SUCCESS ]{RESET}: Successfully extracted {GREEN}{}{RESET} to {BLUE}{}{RESET}",
                                                    file.path().display(),
                                                    target_dir.display()
                                                );
                                            };
                                        };
                                    }
                                    Err(error) => {
                                        eprintln!(
                                            "{RED}[ ERROR ]{RESET}: Failed to open archived file: {GREEN}{}{RESET}",
                                            error.to_string()
                                        )
                                    }
                                }
                            }
                        }
                        None => eprintln!(
                            "{RED}[ ERROR ]{RESET}: Source file {GREEN}{}{RESET} does not have a file name! Cannot extract it!",
                            source_file.display()
                        ),
                    };
                } else {
                    eprintln!(
                        "{RED}[ ERROR ]{RESET}: Provided argument {GREEN}{}{RESET} is not a directory! Cannot extract here!",
                        target_dir.display()
                    );
                }
            }
            None => eprintln!(
                "{RED}[ ERROR ]{RESET}: Couldn't locate {GREEN}{}{RESET} in the vfs!",
                source_file.display()
            ),
        },
        Commands::Find {
            path,
            format,
            output,
            r#type,
        } => {
            let path_string = path.to_string_lossy().to_string();
            let filter_closure = |vfs_file: &VfsFile| match r#type {
                FindType::Extension => vfs_file.path().extension().unwrap_or_default() == &path,
                FindType::NameExact => vfs_file
                    .file_name()
                    .unwrap_or_default()
                    .eq_ignore_ascii_case(&path_string),
                FindType::Name => vfs_file
                    .file_name()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&path_string),
                FindType::Folder => normalize_path(vfs_file.path().parent().unwrap())
                    .to_string_lossy()
                    .contains(&path_string),
                FindType::Prefix => normalize_path(vfs_file.path())
                    .to_string_lossy()
                    .starts_with(&path_string),
                FindType::Stem => vfs_file
                    .file_stem()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(&path_string),
                FindType::StemExact => vfs_file
                    .file_stem()
                    .unwrap_or_default()
                    .eq_ignore_ascii_case(&path_string),
                FindType::Contains => vfs_file
                    .path()
                    .to_string_lossy()
                    .to_string()
                    .replace("\\", "/")
                    .contains(&path_string),
            };

            let tree = vfs.tree_filtered(args.use_relative, filter_closure);

            let serialized = VFS::serialize_from_tree(
                &tree,
                match format {
                    OutputFormat::Json => SerializeType::Json,
                    OutputFormat::Yaml => SerializeType::Yaml,
                    OutputFormat::Toml => SerializeType::Toml,
                },
            )?;

            match output {
                None => println!("{serialized}"),
                Some(path) => {
                    let parent = path
                        .parent()
                        .expect("Failed to extract parent directory from output param!");
                    std::fs::create_dir_all(parent)?;
                    let mut file = std::fs::File::create(&path)?;
                    write!(file, "{serialized}")?;
                }
            }
        }
        Commands::FindFile { path, simple } => {
            let file = vfs.get_file(&path);
            if let Some(found_file) = file {
                let path_display = match found_file.is_archive() {
                    true => PathBuf::from(found_file.parent_archive_path().unwrap())
                        .join(&path)
                        .to_string_lossy()
                        .to_string(),
                    false => found_file.path().to_string_lossy().to_string(),
                };

                if simple {
                    println!("{}", path_display);
                } else {
                    println!(
                        "{GREEN}[ SUCCESS ]{RESET}: Successfully found VFS File {BLUE}{}{RESET} at path {GREEN}{}{RESET}",
                        &path.display(),
                        &path_display,
                    )
                }
            } else {
                eprintln!(
                    "{RED}[ ERROR ]{RESET}: Failed to locate {BLUE}{}{RESET} in the provided VFS.",
                    path.display()
                )
            }
        }
        Commands::Remaining {
            filter_path,
            replacements_only,
            format,
            output,
        } => {
            let mut paths = get_data_paths(&get_config());
            filter_data_paths(&filter_path, &mut paths);

            let filtered_vfs = VFS::from_directories(&paths, None);
            let files_remaining = vfs.tree_filtered(args.use_relative, |file| {

                // Check if there's a file whose ending matches this path, but not this exact path
                if replacements_only {
                    let result = filtered_vfs.has_normalized_file(file.path())
                        && !filtered_vfs.has_file(file.path());
                    result
                } else {
                    normalize_path(&file.path()).starts_with(&normalize_path(&filter_path))
                }
            });

            let serialized = VFS::serialize_from_tree(
                &files_remaining,
                match format {
                    OutputFormat::Json => SerializeType::Json,
                    OutputFormat::Yaml => SerializeType::Yaml,
                    OutputFormat::Toml => SerializeType::Toml,
                },
            )?;

            match output {
                None => println!("{serialized}"),
                Some(path) => {
                    let parent = path
                        .parent()
                        .expect("Failed to extract parent directory from output param!");
                    std::fs::create_dir_all(parent)?;
                    let mut file = std::fs::File::create(&path)?;
                    write!(file, "{serialized}")?;
                }
            }
        }
    }

    Ok(())
}
