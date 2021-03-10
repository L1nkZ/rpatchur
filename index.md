## Introduction

RPatchur is a customizable, cross-platform patcher for Ragnarok Online clients.

## Features

* Customizable, web-based UI
* Cross-platform (Windows, Linux, macOS)
* Configurable through an external YAML file
* HTTP/HTTPS support
* GRF file patching (version 0x101, 0x102, 0x103 and 0x200)
* THOR patch format support
* Drop-in replacement for the Thor patcher
* SSO login support (i.e., can act as a launcher)

## How to Build

Rust version 1.42.0 or later is required to build the project.

```
$ git clone https://github.com/L1nkZ/rpatchur.git
$ cd rpatchur
$ cargo build --release
```

### Cross Compilation

It is recommended to build the project on the platform that you target. However,
a `Dockerfile` is available
[here](https://github.com/L1nkZ/rpatchur/blob/master/docker/Dockerfile)
for those of you who'd like to compile from Linux and distribute to Windows.
This `Dockerfile` builds a Docker image that can be used to easily cross-compile
the project from Linux to Windows.

Note: The executable's icon and description will be missing for cross compiled
builds.


### Musl

A `Dockerfile` is also available for those who'd like to build with musl to
reduce the list of dependencies needed to deploy `rpatchur` on Linux. You can
find this `Dockerfile`
[here](https://github.com/L1nkZ/rpatchur/blob/master/docker/Dockerfile-musl).


## How to Use

Using `rpatchur` is pretty simple, you just need to copy the patcher into
your game client's directory and create a configuration file with the same name
as the patcher. For example, if you name your patcher `mypatcher.exe`, you must
name the configuration file `mypatcher.yml`.

You will also need to have an HTTP server that serves your patches and a web
page to use as the patcher's UI.

### Command-line options

```
$ ./rpatchur --help
rpatchur 0.2.1
LinkZ <wanthost@gmail.com>
A customizable patcher for Ragnarok Online

USAGE:
    rpatchur [OPTIONS]

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information

OPTIONS:
    -w, --working-directory <GAME_DIRECTORY>    Sets a custom working directory
```

### Configuration File

`rpatchur` uses a YAML configuration file to store configurable parameters.
You can find an example of a configuration file 
[here](https://github.com/L1nkZ/rpatchur/blob/master/examples/rpatchur.yml).

#### Fields

Here's a description of each field used in the configuration file.

* `window`
  * `width` *(int):* Width of the main window (in pixels).
  * `height` *(int):* Height of the main window (in pixels).
  * `resizable` *(bool):* Make the main window resizable.
* `play`: Configure the *Play* button's behavior.
  * `path` *(string):* Relative path to the game executable.
  * `arguments` *(list[string]):* Command-line arguments to pass to the
    executable.
  * `exit_on_success` *(bool, optional):* Patcher exits when the game client is
    successfully started. Defaults to `true`.
* `setup`: Configure the *Setup* button's behavior.
  * `path` *(string):* Relative path to the setup executable.
  * `arguments` *(list[string]):* Command-line arguments to pass to the
    executable.
  * `exit_on_success` *(bool, optional):* Patcher exits when the setup software is
    successfully started. Defaults to `false`.
* `web`
  * `index_url` *(string):* URL of the web page to use as the UI.
  * `plist_url` *(string):* URL of the *plist.txt* file containing the list of
  patches to apply.
  * `patch_url` *(string):* URL of the directory containing the patches to
  apply.
* `client`
  * `default_grf_name` *(string):* Name of the GRF to patch when a THOR patch
  indicates the *default* GRF.
* `patching`
  * `in_place` *(bool):* When set to `true`, GRFs are patched without creating
  new files. Setting this parameter to `false` makes patching slower but it
  reduces the risk of file corruption, in case of failure.
  * `check_integrity` *(bool):* When set to `true`, integrity checks are enforced
  on downloaded THOR patches before applying them.
  * `create_grf` *(bool):* When set to `true`, GRF files are created if they do
  not exist prior to patching.

### User Interface

`rpatchur` uses a web view to implement its UI, this makes it completely
customizable and also easily updatable. An important thing to note however,
is that `rpatchur` uses the system's web renderer (i.e. Internet Explorer on
Windows). Nowadays, most Windows systems have Internet Explorer 11 installed,
so you have to make your web application compatible with this browser, at least.

You can find an example of a bootstrap-based patcher UI (compatible with
Internet Explorer >= 10)
[here](https://github.com/L1nkZ/rpatchur/blob/master/examples/bootstrap/).

You can find an example of a bootstrap-based launcher UI (compatible with
Internet Explorer >= 10)
[here](https://github.com/L1nkZ/rpatchur/blob/master/examples/basic_launcher/).

#### JavaScript Bindings

The web view interacts with the patcher through two-way JavaScript bindings.
There are a few JavaScript functions that can be called during execution.

**Functions without arguments**

* `play`: Executes the configured game executable.
* `setup`: Executes the configured setup executable.
* `exit`: Closes the patcher.
* `start_update`: Starts the update process (to download and apply patches).
* `cancel_update`: Cancels the update process if started.
* `reset_cache`: Resets the patcher's cache (to force a re-patch).

These functions do not take any argument and have to be invoked through a
particular `external.invoke` function. For example, to invoke the `setup`
function, you should call `external.invoke('setup')` from your JavaScript code.
These functions do not return anything.

**Functions with arguments**

* `login`: Executes the configured game executable in SSO mode, with the
  provided credentials.

This function takes two arguments and can be invoked with a call to
`external.invoke` as well. This function doesn't return anything.
For example you can call it like so:
```javascript
external.invoke(JSON.stringify({
    function: 'login',
    parameters: {
        'login': login, 'password': password
    }
}));
```

**Callbacks**

The patcher also invokes some callbacks during execution:

* `patchingStatusReady()`: Indicates that the patcher is finished and that the
game client is ready to be launched.
* `patchingStatusError(errorMsg)`: Indicates that an error occured during the
patching process. A `string` error message is given as an argument.
* `patchingStatusDownloading(nbDownloaded, nbTotal, bytesPerSec)`: Indicates that the
patcher is currently downloading patches. `nbDownloaded` is an `int` that
represents the number of patches that have been downloaded. `nbTotal` is an
`int` that represents the total number of patches that will be downloaded.
`bytesPerSec` is an `int` that indicates the current download speed in bytes
per second.
* `patchingStatusInstalling(nbDownloaded, nbTotal)`: Indicates that the
patcher is currently applying patches. `nbDownloaded` is an `int` that
represents the number of patches that have been applied. `nbTotal` is an
`int` that represents the total number of patches that will be applied.

You can define these callbacks to receive useful information to display to the
user.

## mkpatch

mkpatch is a cross-platform utility for generating THOR patch archives with a
command-line interface.

### Command-line options

```
$ ./mkpatch --help
mkpatch 0.1.0
LinkZ <wanthost@gmail.com>
Patch generation utility for THOR patchers

USAGE:
    mkpatch [FLAGS] [OPTIONS] <patch-definition-file>

FLAGS:
    -h, --help       Prints help information
    -V, --version    Prints version information
    -v, --verbose    Enable verbose logging

OPTIONS:
    -o, --output-file <output-file>
            Path to the output archive (default: <patch_definition_file_name>.thor)

    -p, --patch-data-directory <patch-data-directory>
            Path to the directory that contains patch data (default: current working directory)


ARGS:
    <patch-definition-file>    Path to a patch definition file
```

### Usage

Example:

```
$ ./mkpatch examples/patch.yml -p ~/myclient_files/
2021-03-10 16:50:45,236 INFO  [mkpatch] Processing 'patch.yml'
2021-03-10 16:50:45,237 INFO  [mkpatch] GRF merging: true
2021-03-10 16:50:45,237 INFO  [mkpatch] Checksums included: true
2021-03-10 16:50:45,237 INFO  [mkpatch] Target GRF: 'data.grf'
2021-03-10 16:50:45,268 INFO  [mkpatch] Patch generated at 'patch.thor'
```

The executable returns `0` in case of success and a non-zero value in case of
failure.

The `--patch-data-directory` argument must point to a directory that contains
the files declared in the patch definition file (e.g.,
`data/texture/.../image.bmp`). For example, if you specify
`--patch-data-directory /tmp/mydata`, `/tmp/mydata/data/texture/.../image.bmp`
will be added to the archive as `data\texture\...\image.bmp`.
