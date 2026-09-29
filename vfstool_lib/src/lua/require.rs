// SPDX-License-Identifier: MIT OR Apache-2.0
//! `require` over a VFS: an [`l3i::require::RequireNavigator`] whose module space is a shared
//! [`Vfs`] handle, with `.luaurc` aliases, host aliases, and `OpenMW`'s script template cache:
//! one compiled template per module per VM, cloned for every `require`.
//!
//! A chunk name is a VFS key: a script loaded as `@scripts/main.luau` requires `./util` as
//! `scripts/util.luau`, `scripts/util.lua` or `scripts/util/init.luau`, and `../lib/x` as
//! `lib/x`. A module at `dir/init.luau` is the module `dir`, so its `./` is `dir`'s parent, as
//! in Luau's own file navigator. `@alias` is looked up in the host's aliases first
//! ([`VfsRequireNavigator::alias`]), then in the `.luaurc` files up the requirer's directories
//! (JSON, read through the VFS); an alias value that is not `./` or `../` relative is a key
//! from the VFS root.
//!
//! ```no_run
//! use std::rc::Rc;
//! use l3i::Runtime;
//! use l3i::extension::RuntimePlan;
//! use l3i::sandbox::SandboxOptions;
//! use vfstool_lib::VFS;
//! use vfstool_lib::lua::{Vfs, VfsExtension, VfsRequireNavigator};
//!
//! let plan = RuntimePlan::builder()
//!     .extension(dream_path::lua::PathExtension)
//!     .extension(VfsExtension)
//!     .finalize()?;
//! let runtime = Runtime::from_plan(&plan)?;
//! let sandbox = Rc::new(runtime.sandbox(|line| println!("{line}"), SandboxOptions::default())?);
//! let vfs = Vfs::new(VFS::from_directories(["Data Files"], None));
//! runtime.install_require(
//!     VfsRequireNavigator::new(vfs.clone(), sandbox).alias("dream", "scripts/dream"),
//! )?;
//! # Ok::<(), l3i::Error>(())
//! ```

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

use l3i::{
    Error, Result,
    require::{ConfigStatus, Load, Navigate, RequireNavigator, impl_scope::Requirer},
    sandbox::{Sandbox, Template},
    stack::Scope,
    value::{Table, Value},
};

use super::{Vfs, io_error, with_key};
use crate::{NormalizedPath, VfsFile};

const SUFFIXES: [&str; 2] = [".luau", ".lua"];
const INIT_SUFFIXES: [&str; 2] = ["/init.luau", "/init.lua"];

// ---------------------------------------------------------------------------------------------
// The template cache
// ---------------------------------------------------------------------------------------------

/// `OpenMW`'s `ScriptTemplateCache`, per VM: one compiled [`Template`] per module file key, kept
/// for the VM's life. The first `require` of a module compiles it once (and, under l3i's `jit`,
/// generates its native code once); every later `require` of that key, from any script
/// instance in the runtime, clones the template's prototype into the requiring environment.
///
/// Nothing here watches the files: a module rewritten on disk is served from its template until
/// the host calls [`TemplateCache::invalidate`] or [`TemplateCache::clear`] (`OpenMW`'s
/// `dropScriptCache`, for a load order reload), after which it reloads lazily. A template
/// belongs to the VM that compiled it, so the cache is shared between navigators of one runtime
/// only, through an `Rc`.
#[derive(Default)]
pub struct TemplateCache {
    templates: RefCell<HashMap<String, Template>>,
    compiles: Cell<usize>,
}

impl std::fmt::Debug for TemplateCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TemplateCache")
            .field("len", &self.len())
            .field("compiles", &self.compiles())
            .finish()
    }
}

impl TemplateCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        TemplateCache::default()
    }

    /// The number of templates held, one per module file key.
    #[must_use]
    pub fn len(&self) -> usize {
        self.templates.borrow().len()
    }

    /// Whether no template is held.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// How many templates the cache has compiled since it was made.
    #[must_use]
    pub fn compiles(&self) -> usize {
        self.compiles.get()
    }

    /// Drops the template of the module file `key` (any spelling), so its next `require`
    /// compiles it again; whether there was one.
    pub fn invalidate(&self, key: &[u8]) -> bool {
        let key = String::from_utf8_lossy(&dream_path::normalize_path(key)).into_owned();
        self.templates.borrow_mut().remove(&key).is_some()
    }

    /// Drops every template.
    pub fn clear(&self) {
        self.templates.borrow_mut().clear();
    }

    /// How native compilation of the module file `key`'s template went: `None` when the key
    /// has no template yet or the runtime has no native code generator. Read once at load, as
    /// the cache itself does; instances share the template's native code.
    #[cfg(feature = "jit")]
    #[must_use]
    pub fn native_code(&self, key: &[u8]) -> Option<l3i::native_code::NativeCodeResult> {
        let key = String::from_utf8_lossy(&dream_path::normalize_path(key)).into_owned();
        self.templates
            .borrow()
            .get(&key)
            .and_then(l3i::sandbox::Template::native_code)
    }

    fn has(&self, key: &str) -> bool {
        self.templates.borrow().contains_key(key)
    }

    fn record(&self, key: String, template: Template) {
        self.compiles.set(self.compiles.get() + 1);
        self.templates.borrow_mut().insert(key, template);
    }
}

// ---------------------------------------------------------------------------------------------
// The navigator
// ---------------------------------------------------------------------------------------------

/// Luau's `require` over a VFS. Install it with `runtime.install_require(navigator)`; the
/// module docs describe the resolution rules.
pub struct VfsRequireNavigator {
    vfs: Vfs,
    sandbox: Rc<Sandbox>,
    cache: Rc<TemplateCache>,
    position: RefCell<Vec<String>>,
    aliases: BTreeMap<String, Vec<String>>,
}

impl std::fmt::Debug for VfsRequireNavigator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VfsRequireNavigator")
            .field("position", &self.position.borrow())
            .field("aliases", &self.aliases)
            .field("cache", &self.cache)
            .finish_non_exhaustive()
    }
}

/// `path` as `/`-separated components, empty ones and `.` dropped.
fn components(path: &str) -> Vec<String> {
    path.split(['/', '\\'])
        .filter(|part| !part.is_empty() && *part != ".")
        .map(str::to_owned)
        .collect()
}

impl VfsRequireNavigator {
    /// A navigator over `vfs`, compiling modules as templates of `sandbox` (with the sandbox's
    /// compile options) into a fresh [`TemplateCache`]. It needs no handle to the runtime: a
    /// template loads from any thread of the VM, and the runtime owns the navigator once it is
    /// installed.
    #[must_use]
    pub fn new(vfs: Vfs, sandbox: Rc<Sandbox>) -> Self {
        Self::with_cache(vfs, sandbox, Rc::new(TemplateCache::new()))
    }

    /// [`Self::new`] over `cache`, which the host keeps to `invalidate` or `clear` it.
    #[must_use]
    pub fn with_cache(vfs: Vfs, sandbox: Rc<Sandbox>, cache: Rc<TemplateCache>) -> Self {
        VfsRequireNavigator {
            vfs,
            sandbox,
            cache,
            position: RefCell::new(Vec::new()),
            aliases: BTreeMap::new(),
        }
    }

    /// Resolves `@name` to the VFS key `key` before any `.luaurc` is consulted:
    /// `alias("dream", "scripts/dream")` makes `require("@dream/ui")` the module
    /// `scripts/dream/ui`. Alias names compare without regard to ASCII case.
    #[must_use]
    pub fn alias(mut self, name: &str, key: &str) -> Self {
        self.aliases
            .insert(name.to_ascii_lowercase(), components(key));
        self
    }

    /// The template cache.
    #[must_use]
    pub fn cache(&self) -> &Rc<TemplateCache> {
        &self.cache
    }

    /// The VFS the modules come from.
    #[must_use]
    pub fn vfs(&self) -> &Vfs {
        &self.vfs
    }

    /// The position as a key, as typed.
    fn key(&self) -> String {
        self.position.borrow().join("/")
    }

    fn contains(&self, key: &str) -> bool {
        self.vfs
            .with(|vfs| with_key(key.as_bytes(), |key| vfs.contains_normalized(key)))
            .unwrap_or(false)
    }

    fn is_directory(&self, key: &str) -> bool {
        self.vfs
            .with(|vfs| vfs.has_directory(&NormalizedPath::new(key)))
            .unwrap_or(false)
    }

    fn file(&self, key: &str) -> Option<VfsFile> {
        self.vfs
            .with(|vfs| with_key(key.as_bytes(), |key| vfs.get_file_normalized(key).cloned()))
            .ok()
            .flatten()
    }

    /// The file the position names as a module: `<pos>.luau`, `<pos>.lua`, or under a
    /// directory `<pos>/init.luau`, `<pos>/init.lua`. `Err(())` when more than one exists.
    fn module_file(&self) -> std::result::Result<Option<String>, ()> {
        let key = self.key();
        if key.is_empty() {
            return Ok(None);
        }
        let mut found = Vec::new();
        for suffix in SUFFIXES {
            let candidate = format!("{key}{suffix}");
            if self.contains(&candidate) {
                found.push(candidate);
            }
        }
        if self.is_directory(&key) {
            for suffix in INIT_SUFFIXES {
                let candidate = format!("{key}{suffix}");
                if self.contains(&candidate) {
                    found.push(candidate);
                }
            }
        }
        if found.len() > 1 {
            return Err(());
        }
        Ok(found.pop().map(|key| normalized(&key)))
    }

    /// The `.luaurc` key at the position, when the VFS has one.
    fn config_key(&self) -> Option<String> {
        let key = self.key();
        let candidate = if key.is_empty() {
            ".luaurc".to_owned()
        } else {
            format!("{key}/.luaurc")
        };
        self.contains(&candidate).then_some(candidate)
    }

    /// The module's source, read through the VFS.
    fn source(&self, key: &str) -> Result<String> {
        let file = self
            .file(key)
            .ok_or_else(|| Error::runtime(format!("dream.vfs: require: no module at '{key}'")))?;
        let bytes = file.read_to_vec().map_err(io_error)?;
        String::from_utf8(bytes)
            .map_err(|_| Error::runtime(format!("dream.vfs: require: module '{key}' is not UTF-8")))
    }
}

/// `key` in its normalized spelling.
fn normalized(key: &str) -> String {
    String::from_utf8_lossy(&dream_path::normalize_path(key.as_bytes())).into_owned()
}

/// The environment of the function that called `require`: the requiring script instance's
/// sandbox environment, or the globals. `None` when nothing called it (a host invocation).
fn requirer_env(scope: &impl Scope) -> Result<Option<Table>> {
    scope.with_frame(|frame| {
        let state = frame.state();
        // SAFETY: `lua_Debug` is plain data for which all zeros is a valid value. Level 0 is the
        // `require` C function running this navigator, level 1 the function that called it;
        // `lua_getinfo` with "f" pushes that function, `lua_getfenv` its environment, and the
        // frame pops both when it closes.
        unsafe {
            let mut ar: l3i::ffi::lua_Debug = std::mem::zeroed();
            if l3i::ffi::lua_getinfo(state, 1, c"f".as_ptr(), &raw mut ar) == 0 {
                return Ok(None);
            }
            l3i::ffi::lua_getfenv(state, -1);
        }
        Table::from_value(Value::store(frame.top_value())?).map(Some)
    })
}

impl RequireNavigator for VfsRequireNavigator {
    fn reset(&self, requirer_chunkname: &str) -> Navigate {
        // Only a `@key` chunk name is a place in the VFS. A host label (`=exec`) stands for a
        // module at the root, whose `./` is the root itself: one empty component, which the
        // `to_parent` every relative path begins with pops.
        let Some(mut key) = requirer_chunkname.strip_prefix('@') else {
            *self.position.borrow_mut() = vec![String::new()];
            return Navigate::Success;
        };
        for suffix in SUFFIXES {
            if let Some(stripped) = key.strip_suffix(suffix) {
                key = stripped;
                break;
            }
        }
        // `dir/init` is the module `dir`.
        key = key
            .strip_suffix("/init")
            .or_else(|| (key == "init").then_some(""))
            .unwrap_or(key);
        *self.position.borrow_mut() = components(key);
        Navigate::Success
    }

    fn jump_to_alias(&self, path: &str) -> Navigate {
        *self.position.borrow_mut() = components(path);
        Navigate::Success
    }

    fn to_alias_override(&self, alias: &str) -> Option<Navigate> {
        let target = self.aliases.get(&alias.to_ascii_lowercase())?;
        self.position.borrow_mut().clone_from(target);
        Some(Navigate::Success)
    }

    fn to_parent(&self) -> Navigate {
        if self.position.borrow_mut().pop().is_some() {
            Navigate::Success
        } else {
            Navigate::NotFound
        }
    }

    fn to_child(&self, name: &str) -> Navigate {
        self.position.borrow_mut().push(name.to_owned());
        match self.module_file() {
            Ok(_) => Navigate::Success,
            Err(()) => Navigate::Ambiguous,
        }
    }

    fn is_module_present(&self) -> bool {
        matches!(self.module_file(), Ok(Some(_)))
    }

    fn chunkname(&self) -> Option<String> {
        self.module_file()
            .ok()
            .flatten()
            .map(|key| format!("@{key}"))
    }

    fn loadname(&self) -> Option<String> {
        self.module_file().ok().flatten()
    }

    fn cache_key(&self) -> Option<String> {
        self.module_file().ok().flatten()
    }

    fn config_status(&self) -> ConfigStatus {
        if self.config_key().is_some() {
            ConfigStatus::Json
        } else {
            ConfigStatus::Absent
        }
    }

    fn config(&self) -> Option<String> {
        self.source(&self.config_key()?).ok()
    }

    fn load(
        &self,
        scope: &Requirer<'_>,
        _path: &str,
        chunkname: &str,
        loadname: &str,
    ) -> Result<Load> {
        // The template, compiled on the first require of this file and kept. No borrow of the
        // cache is held while the module runs, so a module's own requires reach it too.
        if !self.cache.has(loadname) {
            let source = self.source(loadname)?;
            let template = self.sandbox.load_template_in(scope, chunkname, &source)?;
            self.cache.record(loadname.to_owned(), template);
        }
        let env = requirer_env(scope)?;
        let instance = {
            let templates = self.cache.templates.borrow();
            let template = templates.get(loadname).ok_or_else(|| {
                Error::logic(format!(
                    "dream.vfs: require: the template of '{loadname}' vanished"
                ))
            })?;
            self.sandbox.instantiate_in(scope, template, env.as_ref())?
        };
        let state = scope.state();
        let top = scope.stack().top();
        instance.value().push_to_scope(scope)?;
        // The stack holds the closure now; the pin is released before it runs, so no Rust
        // destructor runs the Lua API while an error unwinds.
        drop(instance);
        drop(env);
        // SAFETY: the closure is on top of the requiring thread, inside the native call Luau
        // made for `require`; `lua_call` runs it and leaves its results above `top` for Luau
        // to collect, and an error it raises unwinds into the requiring script.
        unsafe {
            l3i::ffi::lua_call(state, 0, l3i::ffi::LUA_MULTRET);
            Ok(Load::Results(l3i::ffi::lua_gettop(state) - top))
        }
    }
}
