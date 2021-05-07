#![windows_subsystem = "windows"]

mod patcher;
mod process;
mod ui;

use log::LevelFilter;
use std::env;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use simple_logger::SimpleLogger;
use structopt::StructOpt;
use tinyfiledialogs as tfd;
use tokio::runtime;

use patcher::{
    patcher_thread_routine, retrieve_patcher_configuration, PatcherCommand, PatcherConfiguration,
};
use ui::{UiController, WebViewUserData};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const PKG_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const PKG_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");

#[derive(Debug, StructOpt)]
#[structopt(name = PKG_NAME, version = PKG_VERSION, author = PKG_AUTHORS, about = PKG_DESCRIPTION)]
struct Opt {
    /// Sets a custom working directory
    #[structopt(short, long, parse(from_os_str))]
    working_directory: Option<PathBuf>,
}

fn main() -> Result<()> {
    SimpleLogger::new()
        .with_level(LevelFilter::Off)
        .with_module_level(PKG_NAME, LevelFilter::Info)
        .init()
        .with_context(|| "Failed to initalize the logger")?;

    // Parse CLI arguments
    let cli_args = Opt::from_args();
    if let Some(working_directory) = cli_args.working_directory {
        env::set_current_dir(working_directory)
            .with_context(|| "Specified working directory is invalid or inaccessible")?;
    };

    let config = match retrieve_patcher_configuration(None) {
        Err(e) => {
            let err_msg = "Failed to retrieve the patcher's configuration";
            tfd::message_box_ok(
                "Error",
                format!("Error: {}: {:#}.", err_msg, e).as_str(),
                tfd::MessageBoxIcon::Error,
            );
            return Err(e);
        }
        Ok(v) => v,
    };

    // Create a channel to allow the webview's thread to communicate with the patching thread
    let (tx, rx) = flume::bounded(32);
    let window_title = config.window.title.clone();
    let webview = ui::build_webview(
        window_title.as_str(),
        WebViewUserData::new(config.clone(), tx),
    )
    .with_context(|| "Failed to build a web view")?;

    // Spawn a patching thread
    let patching_thread = new_patching_thread(rx, UiController::new(&webview), config);
    webview
        .run()
        .with_context(|| "Failed to run the web view")?;
    // Join the patching thread
    patching_thread
        .join()
        .map_err(|_| anyhow!("Failed to join patching thread"))?
        .with_context(|| "Patching thread ran into an error")?;

    Ok(())
}

/// Spawns a new thread that runs a single threaded tokio runtime to execute the patcher routine
fn new_patching_thread(
    rx: flume::Receiver<PatcherCommand>,
    ui_ctrl: UiController,
    config: PatcherConfiguration,
) -> std::thread::JoinHandle<Result<()>> {
    std::thread::spawn(move || {
        // Build a tokio runtime that runs a scheduler on the current thread and a reactor
        let tokio_rt = runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .with_context(|| "Failed to build a tokio runtime")?;
        // Block on the patching task from our synchronous function
        tokio_rt.block_on(patcher_thread_routine(ui_ctrl, config, rx));

        Ok(())
    })
}
