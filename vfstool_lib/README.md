# dw_vfs_lib

dw_vfs_lib is a reimplementation of OpenMW's virtual file system, or VFS. It provides tools for working with directory structures, archive files, and file metadata, making it ideal for applications that need to handle complex file hierarchies - including complex mod loadouts handled by mod managers. dw_vfs_lib does not inherently depend on OpenMW or any particular game or technology - it can be repurposed easily for almost any use case.

---

## Features

- **Virtual File System (VFS)**:
  - Manage files and directories in a virtualized structure.
  - Normalize paths for consistent access.
  - Query files by name, prefix, or other criteria.

- **Parallel Processing**:
  - Leverages the `rayon` crate for efficient parallel operations on large file sets.

- **Serialization (Optional)**:
  - Serialize the VFS structure to JSON, YAML, or TOML formats using `serde`.

- **Archive Support**:
  - Integrates with the `ba2` crate to handle Bethesda archive formats (e.g., BSA, BA2).

---

## Installation

Add dw_vfs_lib to your Cargo.toml:

```toml
[dependencies]
dw_vfs_lib = "0.1.0"
```

To enable optional serialization features:

```toml
[dependencies]
dw_vfs_lib = { version = "0.1.0", features = ["serde"] }
```

---

## Usage

### Basic Example

```rust
use dw_vfs_lib::VFS;
use std::path::PathBuf;

fn main() {
    // Directories to scan
    let search_dirs = vec![
        PathBuf::from("path/to/dir1"),
        PathBuf::from("path/to/dir2"),
        PathBuf::from("path/to/dir3"),
    ];

    // List of Bethesda archive files to load
    let archive_list = Some(vec!["archive1.bsa", "archive2.bsa"]);

    // Construct the VFS
    let vfs = VFS::from_directories(search_dirs, archive_list);

    // Example: Iterate over all files in the VFS
    for (path, file) in vfs.iter() {
        println!("File: {:?}, Path: {:?}", file, path);
    }
}
```

---

### Serialization (Optional)

Enable the `serialize` feature to serialize the VFS structure to your preferred text format:

```rust
use dw_vfs_lib::{VFS, SerializeType};

fn main() {
    let search_dirs = vec![
        PathBuf::from("path/to/dir1"),
        PathBuf::from("path/to/dir2"),
        PathBuf::from("path/to/dir3"),
    ];

    let vfs = VFS::from_directories(search_dirs, None);

    // Serialize the VFS to JSON
    let tree = vfs.tree(false);
    let json = vfs.serialize_from_tree(&tree, SerializeType::Json).unwrap();
    println!("Serialized VFS: {}", json);
}
```

---

## Feature Flags

- `default`: No optional features enabled.
- `serialize`: Enables serialization to JSON, YAML, and TOML.

---

## License

This project is licensed under the MIT License. See the LICENSE file for details.

---
