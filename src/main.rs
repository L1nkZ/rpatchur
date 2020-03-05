extern crate serde;
extern crate serde_json;
extern crate tempfile;
extern crate url;
extern crate web_view;

mod thor;

use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::SeekFrom;
use std::process::Command;
use std::thread;

use serde::Deserialize;
use thor::*;
use url::Url;
use web_view::*;

const PATCH_LIST_FILE_NAME: &str = "plist.txt";

#[derive(Deserialize, Clone)]
struct PatcherConfiguration {
    window: WindowConfiguration,
    play: PlayConfiguration,
    setup: SetupConfiguration,
    web: WebConfiguration,
    client: ClientConfiguration,
}

#[derive(Deserialize, Clone)]
struct WindowConfiguration {
    width: i32,
    height: i32,
    resizable: bool,
}

#[derive(Deserialize, Clone)]
struct PlayConfiguration {
    path: String,
    argument: String,
}

#[derive(Deserialize, Clone)]
struct SetupConfiguration {
    path: String,
    argument: String,
}

#[derive(Deserialize, Clone)]
struct WebConfiguration {
    index_url: String,
    patch_url: String,
}

#[derive(Deserialize, Clone)]
struct ClientConfiguration {
    default_grf_name: String,
}

#[derive(Debug)]
struct PatchInfo {
    index: i32,
    file_name: String,
}

#[derive(Debug)]
struct PendingPatch {
    info: PatchInfo,
    local_file: File,
}

fn main() {
    // Read the JSON contents of the file as an instance of `PatcherConfiguration`.
    let config = match parse_configuration("./rpatchur.json") {
        Some(v) => v,
        None => return,
    };
    let patching_thread = spawn_patching_thread(config.clone());
    web_view::builder()
        .title("RPatchur")
        .content(Content::Url(config.web.index_url.clone()))
        .size(config.window.width, config.window.height)
        .resizable(config.window.resizable)
        .user_data(config)
        .invoke_handler(|webview, arg| {
            match arg {
                "play" => handle_play(webview),
                "setup" => handle_setup(webview),
                "exit" => handle_exit(webview),
                "cancel_update" => handle_cancel_update(webview),
                "reset_cache" => handle_reset_cache(webview),
                _ => (),
            }
            Ok(())
        })
        .run()
        .unwrap();
    patching_thread.join().unwrap();
}

/// Opens the configured client with arguments, if needed
fn handle_play(webview: &mut WebView<PatcherConfiguration>) {
    let client_path: &String = &webview.user_data().play.path;
    let client_argument: &String = &webview.user_data().play.argument;
    match Command::new(client_path).arg(client_argument).spawn() {
        Ok(child) => println!("Client started: pid={}", child.id()),
        Err(e) => {
            println!("Failed to start client: {}", e);
        }
    }
}

/// Opens the configured 'Setup' software
fn handle_setup(webview: &mut WebView<PatcherConfiguration>) {
    let setup_exe: &String = &webview.user_data().setup.path;
    match Command::new(setup_exe).spawn() {
        Ok(child) => println!("Setup software started: pid={}", child.id()),
        Err(e) => {
            println!("Failed to start setup software: {}", e);
        }
    }
}

/// Exits the patcher cleanly
fn handle_exit(webview: &mut WebView<PatcherConfiguration>) {
    webview.terminate();
}

/// Cancels the update process
fn handle_cancel_update(_webview: &mut WebView<PatcherConfiguration>) {
    println!("FIXME: cancel_update");
}

/// Resets the cache used to keep track of already applied patches
fn handle_reset_cache(_webview: &mut WebView<PatcherConfiguration>) {
    println!("FIXME: reset_cache");
}

fn spawn_patching_thread(config: PatcherConfiguration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        println!("Patching thread started.");
        let patch_url = Url::parse(config.web.patch_url.as_str()).unwrap();
        let patch_list_url = patch_url.join(PATCH_LIST_FILE_NAME).unwrap();
        let resp = reqwest::blocking::get(patch_list_url).unwrap();
        if !resp.status().is_success() {
            println!(
                "Patch list '{}' not found on the remote server, aborting.",
                PATCH_LIST_FILE_NAME
            );
            return;
        }
        let patch_index_content = match resp.text() {
            Ok(v) => v,
            Err(_) => return,
        };
        let patch_list = patch_list_from_string(patch_index_content.as_str());
        println!("Successfully fetched patch list: {:?}", patch_list);
        // Try fetching patch files
        print!("Downloading patches... ");
        let mut pending_patch_queue: Vec<PendingPatch> = vec![];
        for patch in patch_list {
            let patch_file_url = match patch_url.join(patch.file_name.as_str()) {
                Ok(v) => v,
                Err(_) => {
                    println!(
                        "Invalid file name given in '{}': '{}', aborting.",
                        PATCH_LIST_FILE_NAME, patch.file_name
                    );
                    return;
                }
            };
            let mut tmp_file = tempfile::tempfile().unwrap();
            let mut resp = reqwest::blocking::get(patch_file_url).unwrap();
            if !resp.status().is_success() {
                println!(
                    "Patch file '{}' not found on the remote server, aborting.",
                    patch.file_name
                );
                return;
            }
            let _bytes_copied = match resp.copy_to(&mut tmp_file) {
                Ok(v) => v,
                Err(_) => {
                    println!("Failed to download file '{}', aborting.", patch.file_name);
                    return;
                }
            };
            // File's been downloaded, seek to start and add it to the queue
            let _offset = tmp_file.seek(SeekFrom::Start(0));
            pending_patch_queue.push(PendingPatch {
                info: patch,
                local_file: tmp_file,
            });
        }
        println!("Done");
        // Proceed with actual patching
        println!("Applying patches...");
        for pending_patch in pending_patch_queue {
            println!("Processing {}", pending_patch.info.file_name);
            let archive = match ThorArchive::new(pending_patch.local_file) {
                Ok(v) => v,
                Err(_) => {
                    println!("Cannot read '{}', aborting.", pending_patch.info.file_name);
                    break;
                }
            };
            let patch_target_grf_name = archive.get_target_grf_name();
            if patch_target_grf_name.len() == 0 {
                println!("Target GRF: {:?}", config.client.default_grf_name);
            } else {
                println!("Target GRF: {:?}", patch_target_grf_name);
            }
            println!("Entries:");
            for entry in archive.get_entries() {
                println!("{:?}", entry);
            }
        }
        println!("Patching finished!");
    })
}

fn parse_configuration(config_file_path: &str) -> Option<PatcherConfiguration> {
    let config_file = match File::open(config_file_path) {
        Ok(t) => t,
        _ => {
            println!("Cannot open configuration file.");
            return None;
        }
    };
    let config_reader = BufReader::new(config_file);
    let config: PatcherConfiguration = match serde_json::from_reader(config_reader) {
        Ok(t) => t,
        _ => {
            println!("Invalid JSON configuration.");
            return None;
        }
    };
    Some(config)
}

fn patch_list_from_string(content: &str) -> Vec<PatchInfo> {
    println!("Parsing patch index...");
    let vec_lines: Vec<&str> = content.lines().collect();
    let vec_patch_info = vec_lines
        .into_iter()
        .filter_map(|elem| patch_info_from_string(&elem))
        .collect();
    vec_patch_info
}

/// Parses a line to extract patch index and patch file name.
/// Returns a PatchInfo struct in case of success.
/// Returns None in case of failure
fn patch_info_from_string(line: &str) -> Option<PatchInfo> {
    let words: Vec<_> = line.trim().split_whitespace().collect();
    let index_str = match words.get(0) {
        Some(v) => v,
        None => {
            println!("Ignored invalid line '{}'", line);
            return None;
        }
    };
    let index = match str::parse(index_str) {
        Ok(v) => v,
        Err(_) => {
            println!("Ignored invalid line '{}'", line);
            return None;
        }
    };
    let file_name = match words.get(1) {
        Some(v) => v,
        None => {
            println!("Ignored invalid line '{}'", line);
            return None;
        }
    };
    Some(PatchInfo {
        index: index,
        file_name: file_name.to_string(),
    })
}
