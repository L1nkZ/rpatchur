RPatchur
========

[![Build Status](https://travis-ci.org/L1nkZ/rpatchur.svg?branch=master)](https://travis-ci.org/L1nkZ/rpatchur)
[![Build status](https://ci.appveyor.com/api/projects/status/uxhueyysdy7f7o9f/branch/master?svg=true)](https://ci.appveyor.com/project/L1nkZ/rpatchur/branch/master)

`rpatchur` is a customizable, cross-platform patcher for Ragnarok Online clients.

Features
--------

* Customizable, web-based UI
* Configurable through an external YAML file
* HTTP/HTTPS support
* GRF file patching (version 0x101, 0x102, 0x103 and 0x200)
* THOR patch format support
* Drop-in replacement for the Thor patcher
* SSO login support (i.e., can act as a launcher)
* Manual patching
* Can use multiple patch mirrors
* Cross-platform (Windows, Linux, macOS)

Known Limitations
-----------------

* Can only build GRF files in version 0x200
* Cannot automatically update the patcher executable
* No support for `RGZ` and `GPF` patch formats
* Cannot patch GRF files containing multiple entries pointing to the same
offset

Screenshot
----------

![screen](https://i.imgur.com/mE51Iif.png)

Documentation
-------------

You can find the project's documentation [here](https://l1nkz.github.io/rpatchur/).

Examples
--------

You can find example files for the UI and the configuration file in the
`examples` directory.

Building
--------

The `rpatchur` directory contains the actual patcher code (UI, archive merging, etc.).

The `mkpatch` directory contains a THOR patch archive generation utility.

The `gruf` directory contains the core library for parsing and building GRF and THOR archives.


To clone the repository and build everything, simply run:
```
$ git clone https://github.com/L1nkZ/rpatchur.git
$ cd rpatchur
$ cargo build --release
```

Note: Rust 1.49 or later is required.

Note: For targetting 32bit Windows when building on a 64bit system, you need to manually add the target with `rustup target add i686-pc-windows-msvc`. You can now run:
```
$ cargo build --target=i686-pc-windows-msvc --release
```

### Cross Compilation

It is recommended to build the project on the platform that you target. However,
for those of you who'd like to compile from Linux and distribute to Windows,
there's a `Dockerfile` [here](docker). This `Dockerfile` builds a Docker image
that can be used to easily cross-compile the project from Linux to Windows.

Note: The executable's icon and description will be missing for cross compiled
builds.

Additional Notes
----------------

The icon used for Windows executables was taken from
[rathena.org](https://rathena.org/board/files/file/3190-s1-lykos-icon-pack/).

License
-------

Copyright (c) 2020-2021 rpatchur developers

`rpatchur` is distributed under the terms of both the MIT License and the Apache License 2.0.

See the [LICENSE-APACHE](LICENSE-APACHE) and [LICENSE-MIT](LICENSE-MIT) files for license details.
