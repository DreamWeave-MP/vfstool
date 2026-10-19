+++
title = "DreamWeave VFS"
description = "A virtual filesystem over the host's, with archive and overlay providers, as a Jess library."
date = 2026-10-19

[taxonomies]
tags = ["Jess", "Luau"]

[extra]
sections = ["overview", "releases", "credits"]
+++

A virtual filesystem over the host's, with archive and overlay providers, as a Jess library.

<!-- more -->

A [Jess](https://github.com/DreamWeave-MP/jess) package, `org.dreamweave.vfs`. Add it to a `jess.toml` and
`jess workspace sync` pins it from this site:

```toml
[dependencies]
"org.dreamweave.vfs" = "0.1"
```

Every release below is the package's own archive, with its content digest beside the archive's
SHA-256; `jess.lock` pins the first and checks both.
