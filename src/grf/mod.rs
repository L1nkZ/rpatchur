extern crate nom;

use nom::number::complete::{le_i32, le_u32};
use nom::*;

const HEADER_MAGIC: &str = "Master of Magic\0";

#[derive(Debug, PartialEq, Eq)]
pub struct GrfHeader {
    pub key: String,
    pub file_table_offset: u32,
    pub seed: i32,
    pub files_count: i32,
    pub version_major: u32,
    pub version_minor: u32,
}

named!(parse_grf_header<&[u8], GrfHeader>,
    do_parse!(
        tag!(HEADER_MAGIC)
            >> key: take_str!(14)
            >> file_table_offset: le_u32
            >> seed: le_i32
            >> v_files_count: le_i32
            >> version: le_u32
            >> (GrfHeader {
                key: key.to_string(),
                file_table_offset: file_table_offset,
                seed: seed,
                files_count: v_files_count - seed - 7,
                version_major: (version >> 8) & 0xFF,
                version_minor: version & 0xFF
            }
    )
));

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::prelude::*;
    use std::path::{Path, PathBuf};

    fn open_grf(grf_path: &Path) -> GrfHeader {
        let mut buf: Vec<u8> = vec![];
        let mut file = File::open(grf_path).unwrap();
        let _bytes_read = file.read_to_end(&mut buf).unwrap();
        let (_, grf_header) = match parse_grf_header(buf.as_slice()) {
            IResult::Ok(v) => v,
            _ => panic!("Failed to parse archive."),
        };
        grf_header
    }

    #[test]
    fn test_parse_grf_header() {
        let mut grf_dir_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        grf_dir_path.push("resources/tests/grf");
        {
            let grf_path = grf_dir_path.join("200-empty.grf");
            let grf_header = open_grf(&grf_path);
            assert_eq!(grf_header.files_count, 0);
            assert_eq!(grf_header.version_major, 2);
            assert_eq!(grf_header.version_minor, 0);
        }

        {
            let grf_path = grf_dir_path.join("200-small.grf");
            let grf_header = open_grf(&grf_path);
            assert_eq!(grf_header.files_count, 8);
            assert_eq!(grf_header.version_major, 2);
            assert_eq!(grf_header.version_minor, 0);
        }

        {
            let grf_path = grf_dir_path.join("103-empty.grf");
            let grf_header = open_grf(&grf_path);
            assert_eq!(grf_header.files_count, 0);
            assert_eq!(grf_header.version_major, 1);
            assert_eq!(grf_header.version_minor, 3);
        }

        {
            let grf_path = grf_dir_path.join("103-small.grf");
            let grf_header = open_grf(&grf_path);
            assert_eq!(grf_header.files_count, 8);
            assert_eq!(grf_header.version_major, 1);
            assert_eq!(grf_header.version_minor, 3);
        }

        {
            let grf_path = grf_dir_path.join("102-empty.grf");
            let grf_header = open_grf(&grf_path);
            assert_eq!(grf_header.files_count, 0);
            assert_eq!(grf_header.version_major, 1);
            assert_eq!(grf_header.version_minor, 2);
        }

        {
            let grf_path = grf_dir_path.join("102-small.grf");
            let grf_header = open_grf(&grf_path);
            assert_eq!(grf_header.files_count, 8);
            assert_eq!(grf_header.version_major, 1);
            assert_eq!(grf_header.version_minor, 2);
        }
    }
}
