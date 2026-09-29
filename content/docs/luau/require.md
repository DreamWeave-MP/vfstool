+++
title = "require over the VFS"
description = "VfsRequireNavigator: Luau's require resolving ./ and ../ paths, .luaurc aliases and host aliases as VFS keys, with OpenMW's script template cache so a module is compiled once per VM and cloned for every instance that requires it."
weight = 65

[extra]
kind = "api"
+++

Rust, in `vfstool_lib::lua`, behind the `lua` feature. A host that wants scripts to `require`
each other through the VFS installs `VfsRequireNavigator`, an `l3i::require::RequireNavigator`
over a shared [`Vfs`](@/docs/luau/extension.md#vfs) handle:

```rust
use std::rc::Rc;
use l3i::Runtime;
use l3i::extension::RuntimePlan;
use l3i::sandbox::SandboxOptions;
use vfstool_lib::VFS;
use vfstool_lib::lua::{Vfs, VfsExtension, VfsRequireNavigator};

fn main() -> l3i::Result<()> {
    let plan = RuntimePlan::builder()
        .extension(dream_path::lua::PathExtension)
        .extension(VfsExtension)
        .finalize()?;
    let runtime = Rc::new(Runtime::from_plan(&plan)?);
    let sandbox = Rc::new(runtime.sandbox(|line| println!("{line}"), SandboxOptions::default())?);

    let vfs = Vfs::new(VFS::from_directories(["Data Files", "mods/Scripts"], None));
    runtime.install_require(
        VfsRequireNavigator::new(vfs.clone(), sandbox, &runtime).alias("dream", "scripts/dream"),
    )?;

    // The entry point runs as the chunk `@scripts/main.luau`, so its `./` is `scripts/`.
    let source = vfs.with(|vfs| vfs.get_file("scripts/main.luau").map(|f| f.read_to_vec()))?
        .expect("the install has a main script")
        .map_err(|e| l3i::Error::runtime(e.to_string()))?;
    let stack = runtime.stack();
    stack.with_frame(|frame| {
        let chunk = runtime.load(frame, "@scripts/main.luau", std::str::from_utf8(&source).unwrap(), &runtime.compile_options())?;
        chunk.as_function()?.invoke::<(), ()>(frame, ())
    })
}
```

The navigator takes the `Sandbox` because modules are compiled as its templates, with its
compile options, and a weak reference to the runtime, which owns the navigator once it is
installed; the host keeps the runtime in an `Rc` and the sandbox in another.

`install_require` replaces the `require` a plan installs, which only knows the plan's own
modules; those keep resolving (`require("@dream/vfs")` still works), because Luau looks a
registered module up before it navigates. The `VfsExtension` itself has no part in this: it never
installs a navigator, and a runtime without one requires only the plan's modules.

## Resolution

A chunk name is a VFS key. A module loaded as `@scripts/lib/greet.luau` is the module
`scripts/lib/greet`; `@scripts/pkg/init.luau` is the module `scripts/pkg`, as in Luau's own file
navigator. From there:

| Written | Resolves to |
|---|---|
| `require("./math")` | `math` next to the requirer: `scripts/lib/math` |
| `require("../util")` | `util` in the requirer's parent: `scripts/util` |
| `require("./x")` from `scripts/pkg/init.luau` | `scripts/x`, a sibling of `pkg`, not `scripts/pkg/x` |
| `require("@dream/ui")` | The host alias `dream`, then `ui` under it: `scripts/dream/ui` |
| `require("@lib/math")` | The alias `lib` of the nearest `.luaurc` up the requirer's directories |
| `require("math")` | An error: a path starts with `./`, `../` or `@` |

A module path `p` is present when the VFS has `p.luau`, `p.lua`, or, when `p` is a directory,
`p/init.luau` or `p/init.lua`. Two of those at once is an error naming the ambiguity, as in Luau's
CLI. Keys compare in their normalized spelling, so `require("./Math")` finds `scripts/lib/math.luau`.

A chunk whose name is not a key, such as the `=exec` of `Runtime::exec`, requires from the VFS
root: `require("./scripts/util")` there is `scripts/util`.

`.luaurc` files are read through the VFS as JSON, with Luau's own parser: the navigator walks up
from the requirer's directory and reports the first one it finds. An alias value written `./lib`
or `../lib` is relative to that file's directory; any other value (`scripts/lib`) is a key from
the VFS root. Host aliases (`alias(name, key)`) win over `.luaurc` aliases; names compare without
regard to ASCII case, as Luau lowercases them.

The module runs on the requiring thread with the chunk name `@<file key>` (`@scripts/lib/math.luau`),
so errors and the debug library name the real file. Luau caches the result under the same key,
so a module runs once per runtime whatever path reached it.

## The template cache

{{ api_signature(value="struct TemplateCache") }}

OpenMW's `ScriptTemplateCache`, ported: one compiled template per module file per VM, for the
VM's life. The first `require` of a module compiles it once, with `Sandbox::load_template_in`,
into a prototype on the Lua heap (and, when l3i is built with `jit` and the runtime has a
generator, its native code is generated once, there). Every later `require` of that file, from
any script instance in the runtime, is one `Sandbox::instantiate_in`: a `lua_clonefunction`
sharing the prototype, its environment set to the requiring function's own (a sandbox
instance's environment, or the globals), then run on the requiring thread. Nothing is compiled
twice, and instances share bytecode, constants and native code.

The cache is per runtime, because a template belongs to the VM that compiled it. The navigator
holds it in an `Rc`; `with_cache` lets the host keep the other reference. Nothing watches the
files: like OpenMW's `dropScriptCache`, invalidation is explicit, for a host that reloads its
load order, and the dropped templates reload lazily.

{{ api_signature(value="fn new() -> TemplateCache") }}

{{ api_signature(value="fn len(&self) -> usize") }}

{{ api_signature(value="fn is_empty(&self) -> bool") }}

The number of templates, one per module file key.

{{ api_signature(value="fn compiles(&self) -> usize") }}

How many templates the cache has compiled, for tests and measurements.

{{ api_signature(value="fn invalidate(&self, key: &[u8]) -> bool") }}

Drops the template of the module file `key`, in any spelling; whether there was one.

{{ api_signature(value="fn clear(&self)") }}

Drops every template.

Luau's `require` keeps its own cache too: the *result* of a module, per VM, under the file key.
A second `require` of the same file anywhere in the runtime returns that result without
reaching the navigator, so two sandbox instances share one module table. A host that wants each
instance to run its own copy of a module, as OpenMW's per-script `require` does, clears Luau's
cache between instances (`Runtime::clear_require_cache`, or `clear_require_cache_entry(key)`);
the template cache then makes the reload one clone, not a compile. Clearing the template cache
alone changes nothing that Luau has already cached: clear both to see a rewritten module.

## VfsRequireNavigator

{{ api_signature(value="struct VfsRequireNavigator") }}

{{ api_signature(value="fn new(vfs: Vfs, sandbox: Rc<Sandbox>, runtime: &Rc<Runtime>) -> VfsRequireNavigator") }}

A navigator over `vfs` for `runtime`, compiling modules as templates of `sandbox`, with a cache
of its own.

{{ api_signature(value="fn with_cache(vfs: Vfs, sandbox: Rc<Sandbox>, runtime: &Rc<Runtime>, cache: Rc<TemplateCache>) -> VfsRequireNavigator") }}

The same over `cache`, which the host keeps to `invalidate` or `clear` it.

{{ api_signature(value="fn alias(self, name: &str, key: &str) -> VfsRequireNavigator") }}

Makes `@name/...` resolve under the VFS key `key`, before any `.luaurc` is consulted.

{{ api_signature(value="fn cache(&self) -> &Rc<TemplateCache>") }}

{{ api_signature(value="fn vfs(&self) -> &Vfs") }}

The cache and the VFS handle the navigator was made with. The `Vfs` is shared with whatever the
host pushed to scripts, so a `vfs:writeFile` or `vfs:pushDirectory` a script makes is visible to
the next `require`.

## Errors

A `require` that fails raises in the requiring script with Luau's own prefix, `error requiring
module "./x": `, followed by the cause:

| Cause | Text |
|---|---|
| No file for the path | `no module present at resolved path` |
| `x.luau` and `x.lua`, or a file and a directory module, both present | `could not resolve child component "x" (ambiguous)` |
| A path without `./`, `../` or `@` | `require path must start with a valid prefix: ./, ../, or @` |
| An alias no host alias and no `.luaurc` defines | `unknown alias "nope"` |
| A module that is not UTF-8 | `dream.vfs: require: module 'scripts/bad.luau' is not UTF-8` |
| A module the VFS lost between resolution and load | `dream.vfs: require: no module at 'scripts/x.luau'` |
| A `require` after the runtime that owns the navigator was dropped | `dream.vfs: require: the runtime that owns this navigator is gone` (a logic error, not a script error) |

A syntax error in the module, and an error the module raises while it runs, propagate as they are.

## Cost

`cargo bench -p vfstool_lib --all-features --bench luau_boundary` has a `luau_require` group:
100 sandbox instances each requiring the same 200-line module, with the template cache
(`require_cached`: one compile and 100 clones) and with it cleared before every require
(`require_uncached`: 100 compiles), as time per require. [I/O](@/docs/luau/io.md#cost) lists
the numbers.
