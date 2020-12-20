# Changelog

## [Unreleased]

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