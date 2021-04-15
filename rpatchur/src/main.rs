#![windows_subsystem = "windows"]

mod patcher;
mod process;
mod ui;

use log::LevelFilter;
use std::env;
use std::path::PathBuf;

use anyhow::Result;
use clap::{App, Arg};
use patcher::{patcher_thread_routine, retrieve_patcher_configuration};
use simple_logger::SimpleLogger;
use tokio::runtime;
use ui::{UiController, WebViewUserData};

const PKG_NAME: &str = env!("CARGO_PKG_NAME");
const PKG_VERSION: &str = env!("CARGO_PKG_VERSION");
const PKG_AUTHORS: &str = env!("CARGO_PKG_AUTHORS");
const PKG_DESCRIPTION: &str = env!("CARGO_PKG_DESCRIPTION");
const WINDOW_TITLE: &str = "RPatchur";

fn main() {
    SimpleLogger::new()
        .with_level(LevelFilter::Off)
        .with_module_level(PKG_NAME, LevelFilter::Info)
        .init()
        .expect("Failed to initalize the logger");

    // Parse CLI arguments
    let matches = App::new(PKG_NAME)
        .version(PKG_VERSION)
        .author(PKG_AUTHORS)
        .about(PKG_DESCRIPTION)
        .arg(
            Arg::with_name("working-directory")
                .short("w")
                .long("working-directory")
                .value_name("GAME_DIRECTORY")
                .help("Sets a custom working directory")
                .takes_value(true),
        )
        .get_matches();
    if let Some(working_directory) = matches.value_of("working-directory") {
        env::set_current_dir(PathBuf::from(working_directory))
            .expect("Specified working directory is invalid or inaccessible");
    };

    let tokio_rt = build_tokio_runtime().expect("Failed to build a tokio runtime");
    let config = match retrieve_patcher_configuration(None) {
        Err(e) => {
            let err_msg = "Failed to retrieve the patcher's configuration";
            log::error!("{}", err_msg);
            ui::msg_box(WINDOW_TITLE, format!("<b>Error:</b> {}: {:#}.", err_msg, e));
            return;
        }
        Ok(v) => v,
    };
    // Create a channel to allow the webview's thread to communicate with the patching thread
    let (tx, rx) = flume::bounded(8);
    let webview = ui::build_webview(WINDOW_TITLE, WebViewUserData::new(config.clone(), tx))
        .expect("Failed to build a web view");
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
}

/// Builds a tokio runtime with a threaded scheduler and a reactor
fn build_tokio_runtime() -> Result<runtime::Runtime> {
    let rt = runtime::Builder::new_multi_thread().enable_all().build()?;
    Ok(rt)
}
