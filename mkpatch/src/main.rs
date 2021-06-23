mod patch_definition;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::{env, process};

use anyhow::{anyhow, Context, Result};
use gruf::thor::ThorArchiveBuilder;
use log::LevelFilter;
use patch_definition::{parse_patch_definition, PatchDefinition};
use simple_logger::SimpleLogger;
use structopt::StructOpt;
use walkdir::WalkDir;

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const PKG_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

#[derive(Debug, StructOpt)]
#[structopt(name = PKG_NAME, about = PKG_DESCRIPTION, author = PKG_AUTHORS)]
struct Opt {
    #[structopt(short, long, help = "Enable verbose logging")]
    verbose: bool,
    #[structopt(parse(from_os_str), help = "Path to a patch definition file")]
    patch_definition_file: PathBuf,
    #[structopt(
        parse(from_os_str),
        short,
        long,
        help = "Path to the directory that contains patch data (default: current working directory)"
    )]
    patch_data_directory: Option<PathBuf>,
    #[structopt(
        parse(from_os_str),
        short,
        long,
        help = "Path to the output archive (default: <patch_definition_file_name>.thor)"
    )]
    output_file: Option<PathBuf>,
}

fn run(cli_args: Opt) -> Result<()> {
    let patch_data_directory = cli_args
        .patch_data_directory
        .unwrap_or_else(|| PathBuf::from("."));
    let output_file_path = cli_args.output_file.unwrap_or(PathBuf::from(
        cli_args
            .patch_definition_file
            .with_extension("thor")
            .file_name()
            .ok_or_else(|| anyhow!("Invalid patch definition file name"))?,
    ));

    // Parse the YAML definition file
    log::info!(
        "Processing '{}'",
        cli_args.patch_definition_file.to_string_lossy()
    );
    let patch_definition = parse_patch_definition(&cli_args.patch_definition_file)
        .context("Failed to parse the patch definition")?;

    // Display patch info
    log::info!("GRF merging: {}", patch_definition.use_grf_merging);
    log::info!("Checksums included: {}", patch_definition.include_checksums);
    if let Some(target_grf_name) = &patch_definition.target_grf_name {
        log::info!("Target GRF: '{}'", target_grf_name);
    } else {
        log::info!("Target: Game directory");
    }

    // Generate THOR archive
    generate_patch_from_definition(patch_definition, patch_data_directory, &output_file_path)
        .context("Failed to generate patch from definition")?;
    log::info!(
        "Patch generated at '{}'",
        output_file_path.to_string_lossy()
    );
    Ok(())
}

fn generate_patch_from_definition<P1, P2>(
    patch_definition: PatchDefinition,
    patch_data_directory: P1,
    output_path: P2,
) -> Result<()>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    let output_file = File::create(output_path)?;
    let mut archive_builder = ThorArchiveBuilder::new(
        output_file,
        patch_definition.use_grf_merging,
        patch_definition.target_grf_name,
        patch_definition.include_checksums,
    )?;
    for entry in patch_definition.entries {
        let win32_relative_path = win32_path(&entry.relative_path);
        let target_win32_relative_path = entry.in_grf_path.unwrap_or(win32_relative_path.clone());

        if entry.is_removed {
            log::trace!("'{}' will be REMOVED", &win32_relative_path);
            archive_builder.append_file_removal(win32_relative_path);
            continue;
        }

        let native_path = patch_data_directory
            .as_ref()
            .join(posix_path(entry.relative_path));
        if native_path.is_file() {
            // Path points to a single file
            log::trace!("'{}' will be UPDATED", &target_win32_relative_path);
            let file = File::open(native_path)?;
            archive_builder.append_file_update(target_win32_relative_path, file)?;
        } else if native_path.is_dir() {
            // Path points to a directory
            append_directory_update(
                &mut archive_builder,
                patch_data_directory.as_ref(),
                native_path,
            )?;
        } else {
            return Err(anyhow!(
                "Path '{}' is invalid or does not exist",
                native_path.to_string_lossy()
            ));
        }
    }
    Ok(())
}

fn append_directory_update<P1, P2>(
    archive_builder: &mut ThorArchiveBuilder<File>,
    patch_data_directory: P1,
    directory_path: P2,
) -> Result<()>
where
    P1: AsRef<Path>,
    P2: AsRef<Path>,
{
    let walker = WalkDir::new(directory_path).follow_links(false).into_iter();
    for entry in walker {
        let entry = entry?;
        if entry.file_type().is_file() {
            let rel_path = entry.path().strip_prefix(patch_data_directory.as_ref())?;
            let rel_path_str = rel_path
                .to_str()
                .ok_or_else(|| anyhow!("Invalid file path encountered"))?;
            let win32_relative_path = win32_path(rel_path_str);
            log::trace!("'{}' will be UPDATED", &win32_relative_path);
            let file = File::open(entry.path())?;
            archive_builder.append_file_update(win32_relative_path, file)?;
        }
    }
    Ok(())
}

fn main() {
    const SUCCESS_EXIT_CODE: i32 = 0;
    const FAILURE_EXIT_CODE: i32 = 1;

    // Parse CLI arguments
    let cli_args = Opt::from_args();
    // Initialize the logger
    init_logger(cli_args.verbose).expect("Failed to initalize the logger");

    // Run the actual program
    let result = run(cli_args);
    match result {
        Ok(()) => {
            process::exit(SUCCESS_EXIT_CODE);
        }
        Err(err) => {
            log::error!("{:#}", err);
            process::exit(FAILURE_EXIT_CODE);
        }
    }
}

fn init_logger(verbose: bool) -> Result<()> {
    let level_filter = if verbose {
        LevelFilter::Trace
    } else {
        LevelFilter::Info
    };

    SimpleLogger::new()
        .with_level(LevelFilter::Off)
        .with_module_level(PKG_NAME, level_filter)
        .init()?;
    Ok(())
}

// Utility functions to make sure paths are serialized/accessed correctly between posix and Windows platforms
fn posix_path<S: AsRef<str>>(path: S) -> String {
    path.as_ref().replace("\\", "/")
}

fn win32_path<S: AsRef<str>>(path: S) -> String {
    path.as_ref().replace("/", "\\")
}
