use clap::{Parser, Subcommand, ValueEnum};
use std::{
    fs::{self, hard_link, metadata},
    io::{self, Result, Write},
    path::PathBuf,
};
use vfstool_lib::{SerializeType, normalize_path, vfs::VFS};

#[cfg(unix)]
use std::os::unix::fs::symlink as soft_link;

#[cfg(windows)]
use std::os::windows::fs::symlink_file as soft_link;

mod print {
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const BLUE: &str = "\x1b[34m";
    pub const RESET: &str = "\x1b[0m";

    pub const fn err_prefix() -> &'static str {
        concat!("\x1b[31m", "[ ERROR ]", "\x1b[0m", ": ")
    }

    pub const fn success_prefix() -> &'static str {
        concat!("\x1b[32m", "[ SUCCESS ]", "\x1b[0m", ": ")
    }

    pub fn red<S: std::fmt::Display>(input: S) -> String {
        format!("{RED}{input}{RESET}")
    }

    pub fn blue<S: std::fmt::Display>(input: S) -> String {
        format!("{BLUE}{input}{RESET}")
    }

    pub fn green<S: std::fmt::Display>(input: S) -> String {
        format!("{GREEN}{input}{RESET}")
    }
}

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
        #[arg(short, long)]
        allow_copying: bool,

        /// If enabled, allows extracting files out of BSA/BA2 archives during collapsing
        #[arg(short, long)]
        extract_archives: bool,

        /// Use symbolic instead of hardlinks, to allow cross-device links
        #[arg(short, long)]
        symbolic: bool,
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
        /// VFS Path to query. Supports regular expressions!
        path: PathBuf,

        /// Output format when serializing as text.
        #[arg(short, long, value_enum, default_value = "yaml")]
        format: OutputFormat,

        /// Path to save the resulting search tree to.
        ///
        /// If omitted, the result is printed directly to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
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

fn validate_config_dir(dir: &PathBuf) -> io::Result<PathBuf> {
    let dir_metadata = metadata(&dir);
    let default_location = openmw_config::default_config_path();

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
                Some(dir) => return Ok(dir),
            }
        }
    };

    if let Some(fail_message) = config_arg_fail {
        eprintln!("{}", fail_message);
    };

    Err(std::io::Error::new(
        io::ErrorKind::NotFound,
        "Unable to resolve configuration directory!",
    ))
}

fn filter_data_paths(to_keep: &PathBuf, paths: &mut Vec<PathBuf>) {
    let normalized_input = normalize_path(&to_keep);
    paths.retain(|path| normalize_path(&path).eq(&normalized_input))
}

fn output_to_serialize_type(format: OutputFormat) -> SerializeType {
    match format {
        OutputFormat::Json => SerializeType::Json,
        OutputFormat::Yaml => SerializeType::Yaml,
        OutputFormat::Toml => SerializeType::Toml,
    }
}

fn construct_vfs(config_path: PathBuf) -> VFS {
    let config = match openmw_config::OpenMWConfiguration::new(Some(config_path)) {
        Err(config_err) => {
            eprintln!("Failed to load configuration file: {config_err}");
            std::process::exit(255);
        }
        Ok(config) => config,
    };

    let data_paths = config.data_directories();

    let archives = config
        .fallback_archives_iter()
        .map(|archive| archive.value().as_str())
        .collect();

    VFS::from_directories(data_paths, Some(archives))
}

fn write_serialized_vfs(
    path: Option<PathBuf>,
    format: OutputFormat,
    files: &vfstool_lib::DisplayTree,
) -> io::Result<()> {
    let serialized = VFS::serialize_from_tree(files, output_to_serialize_type(format))?;

    match path {
        None => println!("{serialized}"),
        Some(path) => {
            let parent = path
                .parent()
                .expect("Failed to extract parent directory from output param!");
            fs::create_dir_all(parent)?;
            let mut file = fs::File::create(&path)?;
            write!(file, "{serialized}")?;
        }
    }

    Ok(())
}

fn main() -> Result<()> {
    let args = Cli::parse();
    let config_dir = args.config.unwrap_or(openmw_config::default_config_path());

    let resolved_config_dir = validate_config_dir(&config_dir)?;

    let vfs: VFS = construct_vfs(resolved_config_dir.clone());

    match args.command {
        Commands::Collapse {
            collapse_into,
            allow_copying,
            extract_archives,
            symbolic,
        } => {
            if metadata(&collapse_into).is_err() {
                fs::create_dir_all(&collapse_into)?;
            };

            vfs.iter().for_each(|(relative_path, file)| {
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
                        let extension = extension.to_ascii_lowercase();
                        let file_name = file.file_name().unwrap_or_default().to_ascii_lowercase();

                        if (extension == "bsa" || extension == "ba2") && extract_archives && file_name != "archiveinvalidationinvalidated!.bsa" {
                            println!("Skipping archive {}", file.file_name().unwrap().to_string_lossy());
                            return;
                        }
                    }

                    let link_fn = if symbolic {
                        soft_link
                    } else {
                        hard_link
                    };

                    if let Err(error) = link_fn(file.path(), &merged_path) {
                        eprintln!(
                            "Symlink attempt for {} failed due to error: {}",
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
                    if !extract_archives {
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
                                        "{}Failed extracting loose file from the vfs: {}",
                                        print::err_prefix(),
                                        print::red(error.to_string()),
                                    );
                                } else {
                                    println!(
                                        "{}Successfully extracted {} to {}",
                                        print::success_prefix(),
                                        print::green(file.path().display()),
                                        print::blue(target_dir.display())
                                    );
                                };
                            } else {
                                match file.open() {
                                    Ok(mut data) => {
                                        let mut buf: Vec<u8> = Vec::new();
                                        if let Ok(_) = data.read_to_end(&mut buf) {
                                            if let Err(error) = fs::write(&target_path, buf) {
                                                eprintln!(
                                                    "{}Extracting archived file {} to {} failed due to {}!",
                                                    print::err_prefix(),
                                                    print::green(source_file.display()),
                                                    print::blue(target_path.display()),
                                                    print::red(error.to_string()),
                                                );
                                            } else {
                                                println!(
                                                    "{}Successfully extracted {} to {}",
                                                    print::success_prefix(),
                                                    print::green(file.path().display()),
                                                    print::blue(target_dir.display()),
                                                );
                                            };
                                        };
                                    }
                                    Err(error) => {
                                        eprintln!(
                                            "{}Failed to open archived file: {}",
                                            print::err_prefix(),
                                            print::green(error.to_string())
                                        )
                                    }
                                }
                            }
                        }
                        None => eprintln!(
                            "{}Source file {} does not have a file name! Cannot extract it!",
                            print::err_prefix(),
                            print::green(source_file.display()),
                        ),
                    };
                } else {
                    eprintln!(
                        "{}Provided argument {} is not a directory! Cannot extract here!",
                        print::err_prefix(),
                        print::green(target_dir.display()),
                    );
                }
            }
            None => eprintln!(
                "{}Couldn't locate {} in the vfs!",
                print::err_prefix(),
                print::green(source_file.display()),
            ),
        },
        Commands::Find {
            path,
            format,
            output,
        } => {
            // Lossy compare could produce false positives, but only if there are non-unicode
            // characters at the same position in both the path and string being matched and the
            // rest of the string is the same
            let path_string = path.to_string_lossy();
            let path_regex: regex::Regex = match regex::RegexBuilder::new(&path_string)
                .case_insensitive(true)
                .build()
            {
                Ok(regex) => regex,
                Err(error) => {
                    eprintln!("{error}");
                    std::process::exit(256);
                }
            };

            let tree = vfs.tree_filtered(args.use_relative, |file| {
                path_regex.is_match(&file.path().to_string_lossy())
            });

            write_serialized_vfs(output, format, &tree)?;
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
                        "{}Successfully found VFS File {} at path {}",
                        print::success_prefix(),
                        print::blue(&path.display()),
                        print::green(&path_display),
                    )
                }
            } else {
                eprintln!(
                    "{}Failed to locate {} in the provided VFS.",
                    print::err_prefix(),
                    print::blue(path.display()),
                )
            }
        }
        Commands::Remaining {
            filter_path,
            replacements_only,
            format,
            output,
        } => {
            let config = match openmw_config::OpenMWConfiguration::new(Some(resolved_config_dir)) {
                Err(config_err) => {
                    eprintln!("Failed to load openmw.cfg for comparison: {config_err}");
                    std::process::exit(256);
                }
                Ok(config) => config,
            };

            let mut paths = config
                .data_directories_iter()
                .map(|dir| dir.parsed().to_owned())
                .collect();

            filter_data_paths(&filter_path, &mut paths);

            let filtered_vfs = VFS::from_directories(&paths, None);
            let filter_normalized = normalize_path(&filter_path);

            let files_remaining = vfs.tree_filtered(args.use_relative, |file| {
                let path = file.path();
                // Check if there's a file whose ending matches this path, but not this exact path
                if replacements_only {
                    filtered_vfs.has_normalized_not_exact(path)
                } else {
                    normalize_path(path).starts_with(&filter_normalized)
                }
            });

            write_serialized_vfs(output, format, &files_remaining)?;
        }
    }

    Ok(())
}
