use log::error;
use std::fs::File;
use std::io::BufReader;

use serde::Deserialize;

#[derive(Deserialize, Clone)]
pub struct PatcherConfiguration {
    pub window: WindowConfiguration,
    pub play: PlayConfiguration,
    pub setup: SetupConfiguration,
    pub web: WebConfiguration,
    pub client: ClientConfiguration,
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
    pub index_url: String,
    pub plist_url: String,
    pub patch_url: String,
}

#[derive(Deserialize, Clone)]
pub struct ClientConfiguration {
    pub default_grf_name: String,
}

pub fn parse_configuration(config_file_path: &str) -> Option<PatcherConfiguration> {
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
