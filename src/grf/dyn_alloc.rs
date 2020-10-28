use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::io;

use crate::grf::reader::{GrfArchive, GrfFileEntry, GRF_HEADER_SIZE};

#[derive(Debug)]
pub struct AvailableChunk {
    pub size: usize,
}

pub struct AvailableChunkList {
    end_offset: u64,
    chunks: BTreeMap<u64, AvailableChunk>, // Indexed and ordered by offset
}

pub fn list_available_chunks(archive: &mut GrfArchive) -> io::Result<AvailableChunkList> {
    if archive.file_count() == 0 {
        return Ok(AvailableChunkList::new());
    }

    let mut entries: Vec<&GrfFileEntry> = archive.get_entries().collect();
    entries.sort_by(|a, b| a.offset.cmp(&b.offset));
    let mut available_chunks = BTreeMap::new();
    for i in 0..entries.len() - 1 {
        let left_entry = entries[i];
        let right_entry = entries[i + 1];
        let expected_entry_offset = left_entry.offset + left_entry.size_compressed_aligned as u64;
        let space_between_entries = usize::try_from(right_entry.offset - expected_entry_offset)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "File too big or malformed"))?;
        available_chunks.insert(
            expected_entry_offset,
            AvailableChunk {
                size: space_between_entries,
            },
        );
    }
    let last_entry = entries.last().unwrap();
    let last_entry_offset = last_entry.offset + last_entry.size_compressed_aligned as u64;
    Ok(AvailableChunkList {
        end_offset: last_entry_offset,
        chunks: available_chunks,
    })
}

impl AvailableChunkList {
    pub fn new() -> AvailableChunkList {
        let map = BTreeMap::new();
        let offset = GRF_HEADER_SIZE as u64;
        AvailableChunkList {
            end_offset: offset,
            chunks: map,
        }
    }

    /// Acquire a chunk of memory
    pub fn alloc_chunk(&mut self, size: usize) -> u64 {
        let chunk_offset = self.find_suitable_chunk(size);
        // Update chunk list
        if chunk_offset == self.end_offset {
            let new_offset = chunk_offset + size as u64;
            self.end_offset = new_offset;
        } else {
            let chunk = self.chunks.remove(&chunk_offset).unwrap();
            if chunk.size > size {
                let new_offset = chunk_offset + size as u64;
                self.chunks.insert(
                    new_offset,
                    AvailableChunk {
                        size: chunk.size - size,
                    },
                );
            }
        }
        chunk_offset
    }

    fn find_suitable_chunk(&self, size: usize) -> u64 {
        let opt_item = self
            .chunks
            .iter()
            .find(|(_, chunk)| if chunk.size >= size { true } else { false });
        match opt_item {
            None => self.end_offset,
            Some((offset, _)) => *offset,
        }
    }

    /// Resizes an already "allocated" chunk of memory
    /// This realloc method assumes all free chunks are merged (i.e. there can
    /// only be used chunks between 2 free chunks)
    pub fn realloc_chunk(&mut self, offset: u64, size: usize, new_size: usize) -> u64 {
        let end_offset = offset + size as u64;
        let new_end_offset = offset + new_size as u64;
        if let Some(next_chunk) = self.chunks.get(&end_offset) {
            // Next chunk is available
            let next_chunk_size = next_chunk.size;
            if size + next_chunk_size >= new_size {
                // Sufficient space for in-place grow
                let _ = self.chunks.remove(&end_offset).unwrap();
                self.chunks.insert(
                    new_end_offset,
                    AvailableChunk {
                        size: size + next_chunk_size - new_size,
                    },
                );
                return offset;
            }
        }
        if end_offset == self.end_offset {
            self.end_offset = new_end_offset;
            return offset;
        }

        // Next chunk is used or free but too small, must move
        self.free_chunk(offset, size);
        self.alloc_chunk(new_size)
    }

    /// Releases a chunk of memory
    /// This method trusts the input given by the caller.
    /// At the moment, passing bad parameters to this method can mess up the list.
    pub fn free_chunk(&mut self, offset: u64, size: usize) {
        let chunk_end_offset = offset + size as u64;
        let mut new_chunk_offset = offset;
        let mut new_chunk_size = size;

        // Check left merge
        let chunk_left_opt = self.chunks.range(..offset).last();
        if let Some((offset_left_ref, chunk_left)) = chunk_left_opt {
            let offset_left = *offset_left_ref;
            let end_offset_left = offset_left + chunk_left.size as u64;
            if end_offset_left == offset {
                // Merge to the left
                let chunk = self.chunks.remove(&offset_left).unwrap();
                new_chunk_offset = offset_left;
                new_chunk_size += chunk.size;
            }
        }
        // Check right merge
        if chunk_end_offset == self.end_offset {
            // "Merge" to the right
            self.end_offset = new_chunk_offset;
        } else if self.chunks.contains_key(&chunk_end_offset) {
            // Merge to the right with another chunk
            let chunk = self.chunks.remove(&chunk_end_offset).unwrap();
            new_chunk_size += chunk.size;
        }
        self.chunks.insert(
            new_chunk_offset,
            AvailableChunk {
                size: new_chunk_size,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const START_OFFSET: u64 = GRF_HEADER_SIZE as u64;

    #[test]
    fn test_chunk_list_basic() {
        let size1: usize = 90;
        let size2: usize = 23;
        let size3: usize = 50;
        let mut chunk_list = AvailableChunkList::new();
        // Alloc a first block
        let res = chunk_list.alloc_chunk(size1);
        assert_eq!(START_OFFSET, res);
        // Alloc a second chunk which should be located right after the previous one
        let res = chunk_list.alloc_chunk(size2);
        assert_eq!(START_OFFSET + size1 as u64, res);

        // Free the first chunk
        chunk_list.free_chunk(START_OFFSET, size1);
        // Allocated chunk should fit into the previously freed chunk
        let res = chunk_list.alloc_chunk(size1);
        assert_eq!(START_OFFSET, res);
        // Alloc another chunk which should be located after the first two chunks
        let res = chunk_list.alloc_chunk(size3);
        assert_eq!(START_OFFSET + size1 as u64 + size2 as u64, res);
    }

    #[test]
    fn test_chunk_list_realloc() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let _ = chunk_list.alloc_chunk(chunk_size);
        let _ = chunk_list.alloc_chunk(chunk_size);

        // Reallocate the first block with a smaller size, should not move
        let res = chunk_list.realloc_chunk(START_OFFSET, chunk_size, chunk_size - 1);
        assert_eq!(START_OFFSET, res);

        // Reallocate the first block with a bigger size, should move
        let res = chunk_list.realloc_chunk(START_OFFSET, chunk_size, chunk_size + 1);
        assert_eq!(START_OFFSET + 2 * chunk_size as u64, res);
        let res = chunk_list.alloc_chunk(chunk_size);
        assert_eq!(START_OFFSET, res);
    }

    #[test]
    fn test_chunk_list_right_merge() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let offset1 = chunk_list.alloc_chunk(chunk_size);
        let offset2 = chunk_list.alloc_chunk(chunk_size);
        let offset3 = chunk_list.alloc_chunk(chunk_size);

        // Free the second chunk
        chunk_list.free_chunk(offset2, chunk_size);
        // Free the first chunk
        chunk_list.free_chunk(offset1, chunk_size);
        let offset4 = chunk_list.alloc_chunk(2 * chunk_size);
        assert_eq!(offset4, offset1);

        // Free the third chunk
        chunk_list.free_chunk(offset3, chunk_size);
        // Free the fourth chunk
        chunk_list.free_chunk(offset4, 2 * chunk_size);
        let offset5 = chunk_list.alloc_chunk(4 * chunk_size);
        assert_eq!(offset5, offset1);
    }

    #[test]
    fn test_chunk_list_left_merge() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let offset1 = chunk_list.alloc_chunk(chunk_size);
        let offset2 = chunk_list.alloc_chunk(chunk_size);
        let offset3 = chunk_list.alloc_chunk(chunk_size);

        // Free the first chunk
        chunk_list.free_chunk(offset1, chunk_size);
        // Free the second chunk
        chunk_list.free_chunk(offset2, chunk_size);

        let offset4 = chunk_list.alloc_chunk(2 * chunk_size);
        assert_eq!(offset4, offset1);

        // Free the fourth chunk
        chunk_list.free_chunk(offset4, 2 * chunk_size);
        // Free the third chunk
        chunk_list.free_chunk(offset3, chunk_size);
        let offset5 = chunk_list.alloc_chunk(4 * chunk_size);
        assert_eq!(offset5, offset1);
    }
}
