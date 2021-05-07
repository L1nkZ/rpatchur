# Changelog

## [Unreleased]

## [0.3.0] - 2021-05-07
### Added
- Add a new `manual_patch` binding for allowing users to apply manual patches
- Add a `window.title` field in the configuration that sets the window's title 

### Changed
- Serialize cache file as JSON
- Startup errors are displayed with native message boxes
- Use structopt instead of clap for CLI parsing
- Switch to tokio's single-threaded runtime
- Multiple patch servers can be specified in the configuration file.
  A new `web.patch_servers` list field replace the `web.plist_url` and `web.patch_url`
  fields in the configuration. A new optional `web.preferred_patch_server` has also
  been added.

### Fixed
- Prevent multiple instances of the patcher to update the game at the same time

## [0.2.3] - 2021-04-15
### Changed
- Migrate to `tokio` v1.5
- Update dependencies

### Fixed
- Fix deadlock that could happen when canceling downloads early at launch

## [0.2.2] - 2021-03-14
### Changed
- Allow launching game clients and setup executables that do not have a `.exe`
  extensions in their file name.

### Fixed
- Fix wrong command-line arguments' order preventing SSO login to work correctly
  (thanks to @waken22 and @vstumpf).

## [0.2.1] - 2021-03-10
### Added
- Implement THOR archive generation in `gruf`.
- Add a new `mkpatch` utility. This is a command-line utility that can be used
  to generate THOR patch archives in a cross-platform manner.

### Changed
- Compile binaries with `panic = 'abort'` in release.
- Update `web-view` from v0.6.3 to v0.7.3.

## [0.2.0] - 2020-12-20
### Added
- Add two new optional `exit_on_success` configuration fields that allow users
  to make the patcher exit when starting the game client or the setup software.
- Add an optional `--working-directory` command line argument to set the
  patcher's working directory.
- Add a new `login` binding that allows Javascript code to start the game
  client with credentials (to act a launcher).
- Add a new `basic_launcher` example that implements a simple launcher
  interface.
- Build additional Linux binary assets with musl.

### Changed
- Replace local copies of JS and CSS assets with links to CDNs in the
  `boostrap` UI example.
- Rename `argument` configuration fields to `arguments` and make them list of
  strings.

### Fixed
- Fix an issue with how command-line arguments are handled when creating
  processes on Linux.

## [0.1.0] - 2020-11-18
Initial release