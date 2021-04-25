#![windows_subsystem = "windows"]

mod patcher;
mod process;
mod ui;

use log::LevelFilter;
use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use patcher::{patcher_thread_routine, retrieve_patcher_configuration};
use simple_logger::SimpleLogger;
use structopt::StructOpt;
use tinyfiledialogs as tfd;
use tokio::runtime;
use ui::{UiController, WebViewUserData};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const PKG_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const PKG_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
const WINDOW_TITLE: &str = "RPatchur";

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

    let tokio_rt = build_tokio_runtime().with_context(|| "Failed to build a tokio runtime")?;
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
    let (tx, rx) = flume::bounded(8);
    let webview = ui::build_webview(WINDOW_TITLE, WebViewUserData::new(config.clone(), tx))
        .with_context(|| "Failed to build a web view")?;
    let patching_task = tokio_rt.spawn(patcher_thread_routine(
        UiController::new(&webview),
        config,
        rx,
    ));
    webview.run().unwrap();
    // Join the patching task from our synchronous function
    tokio_rt.block_on(async {
        if let Err(e) = patching_task.await {
            log::error!("Failed to join patching thread: {}", e);
        }
    });

    Ok(())
}

/// Builds a tokio runtime with a threaded scheduler and a reactor
fn build_tokio_runtime() -> Result<runtime::Runtime> {
    let rt = runtime::Builder::new_multi_thread().enable_all().build()?;
    Ok(rt)
}
