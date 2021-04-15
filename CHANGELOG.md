# Changelog

## [Unreleased]

## [0.2.3] - 2021-04-15
### Changed
- Migrate to tokio 1.5
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