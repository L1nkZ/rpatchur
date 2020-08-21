use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use log::error;
use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct PatcherConfiguration {
    pub window: WindowConfiguration,
    pub play: PlayConfiguration,
    pub setup: SetupConfiguration,
    pub web: WebConfiguration,
    pub client: ClientConfiguration,
    pub patching: PatchingConfiguration,
}

#[derive(Deserialize, Clone)]
pub struct WindowConfiguration {
    pub width: i32,
    pub height: i32,
    pub resizable: bool,
}

#[derive(Deserialize, Clone)]
pub struct PlayConfiguration {
    pub path: String,
    pub argument: String,
}

#[derive(Deserialize, Clone)]
pub struct SetupConfiguration {
    pub path: String,
    pub argument: String,
}

#[derive(Deserialize, Clone)]
pub struct WebConfiguration {
    pub index_url: String, // URL of the index file implementing the UI
    pub plist_url: String, // URL of the plist.txt file
    pub patch_url: String, // URL of the directory containing .thor files
}

#[derive(Deserialize, Clone)]
pub struct ClientConfiguration {
    pub default_grf_name: String, // GRF file to patch by default
}

#[derive(Deserialize, Clone)]
pub struct PatchingConfiguration {
    pub in_place: bool, // In-place GRF patching
                        // pub check_integrity: bool,
}

pub fn parse_configuration<P: AsRef<Path>>(config_file_path: P) -> Option<PatcherConfiguration> {
    let config_file = match File::open(config_file_path) {
        Ok(t) => t,
        _ => {
            error!("Cannot open configuration file.");
            return None;
        }
    };
    let config_reader = BufReader::new(config_file);
    let config: PatcherConfiguration = match serde_json::from_reader(config_reader) {
        Ok(t) => t,
        _ => {
            error!("Invalid JSON configuration.");
            return None;
        }
    };
    Some(config)
}
