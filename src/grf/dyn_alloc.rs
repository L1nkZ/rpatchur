use std::collections::BTreeMap;
use std::convert::TryFrom;
use std::io;

use crate::grf::reader::{GrfArchive, GrfFileEntry, GRF_HEADER_SIZE};

#[derive(Debug)]
pub struct AvailableChunk {
    pub size: Option<usize>, // Note(LinkZ): None means infinite size (i.e. EOF)
}

pub struct AvailableChunkList {
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
        let space_between_entries =
            match usize::try_from(right_entry.offset - expected_entry_offset) {
                Err(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "File too big or malformed",
                    ))
                }
                Ok(v) => v,
            };
        available_chunks.insert(
            expected_entry_offset,
            AvailableChunk {
                size: Some(space_between_entries),
            },
        );
    }
    let last_entry = entries.last().unwrap();
    let last_entry_offset = last_entry.offset + last_entry.size_compressed_aligned as u64;
    available_chunks.insert(last_entry_offset, AvailableChunk { size: None });
    Ok(AvailableChunkList {
        chunks: available_chunks,
    })
}

impl AvailableChunkList {
    pub fn new() -> AvailableChunkList {
        let mut map = BTreeMap::new();
        let offset = GRF_HEADER_SIZE as u64;
        map.insert(offset, AvailableChunk { size: None });
        AvailableChunkList { chunks: map }
    }

    /// Acquire a chunk of memory
    pub fn alloc_chunk(&mut self, size: usize) -> Option<u64> {
        let chunk_offset = self.find_suitable_chunk(size)?;
        // Update chunk list
        let chunk = self.chunks.remove(&chunk_offset)?;
        match chunk.size {
            None => {
                let new_offset = chunk_offset + size as u64;
                self.chunks
                    .insert(new_offset, AvailableChunk { size: None });
            }
            Some(chunk_size) => {
                if chunk_size > size {
                    let new_offset = chunk_offset + size as u64;
                    self.chunks.insert(
                        new_offset,
                        AvailableChunk {
                            size: Some(chunk_size - size),
                        },
                    );
                }
            }
        }
        Some(chunk_offset)
    }

    fn find_suitable_chunk(&self, size: usize) -> Option<u64> {
        for (offset, chunk) in &self.chunks {
            match chunk.size {
                None => {
                    return Some(*offset);
                }
                Some(chunk_size) => {
                    if chunk_size >= size {
                        return Some(*offset);
                    }
                }
            }
        }
        None
    }

    /// Resizes an already "allocated" chunk of memory
    /// This realloc method assumes all free chunks are merged (i.e. there can
    /// only be used chunks between 2 free chunks)
    pub fn realloc_chunk(&mut self, offset: u64, size: usize, new_size: usize) -> Option<u64> {
        let end_offset = offset + size as u64;
        if let Some(next_chunk) = self.chunks.get(&end_offset) {
            // Next chunk is available
            let new_offset = offset + new_size as u64;
            match next_chunk.size {
                None => {
                    self.chunks.remove(&end_offset)?;
                    self.chunks
                        .insert(new_offset, AvailableChunk { size: None });
                    return Some(offset);
                }
                Some(next_chunk_size) => {
                    if size + next_chunk_size >= new_size {
                        // Sufficient space for in-place grow
                        self.chunks.remove(&end_offset)?;
                        self.chunks.insert(
                            new_offset,
                            AvailableChunk {
                                size: Some(size + next_chunk_size - new_size),
                            },
                        );
                        return Some(offset);
                    }
                }
            }
        }

        // Next chunk is used or free but too small, must move
        self.free_chunk(offset, size);
        self.alloc_chunk(new_size)
    }

    /// Releases a chunk of memory
    /// This method trusts the input given by the caller.
    /// At the moment, passing bad parameters to this method can mess up the list.
    pub fn free_chunk(&mut self, offset: u64, size: usize) {
        // TODO(LinkZ): Merge to the left when possible
        let chunk_end_offset = offset + size as u64;
        if self.chunks.contains_key(&chunk_end_offset) {
            // Merge to the right
            let mut chunk = self.chunks.remove(&chunk_end_offset).unwrap();
            chunk.size = match chunk.size {
                None => None,
                Some(v) => Some(v + size),
            };
            self.chunks.insert(offset, chunk);
        } else {
            self.chunks
                .insert(offset, AvailableChunk { size: Some(size) });
        }
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
        assert_eq!(Some(START_OFFSET), res);
        // Alloc a second chunk which should be located right after the previous one
        let res = chunk_list.alloc_chunk(size2);
        assert_eq!(Some(START_OFFSET + size1 as u64), res);

        // Free the first chunk
        chunk_list.free_chunk(START_OFFSET, size1);
        // Allocated chunk should fit into the previously freed chunk
        let res = chunk_list.alloc_chunk(size1);
        assert_eq!(Some(START_OFFSET), res);
        // Alloc another chunk which should be located after the first two chunks
        let res = chunk_list.alloc_chunk(size3);
        assert_eq!(Some(START_OFFSET + size1 as u64 + size2 as u64), res);
    }

    #[test]
    fn test_chunk_list_realloc() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let _ = chunk_list.alloc_chunk(chunk_size);
        let _ = chunk_list.alloc_chunk(chunk_size);

        // Reallocate the first block with a smaller size, should not move
        let res = chunk_list.realloc_chunk(START_OFFSET, chunk_size, chunk_size - 1);
        assert_eq!(Some(START_OFFSET), res);

        // Reallocate the first block with a bigger size, should move
        let res = chunk_list.realloc_chunk(START_OFFSET, chunk_size, chunk_size + 1);
        assert_eq!(Some(START_OFFSET + 2 * chunk_size as u64), res);
        let res = chunk_list.alloc_chunk(chunk_size);
        assert_eq!(Some(START_OFFSET), res);
    }
}
