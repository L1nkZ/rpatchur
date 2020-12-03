use std::collections::{BTreeMap, BTreeSet};
use std::convert::TryFrom;

use crate::error::{GrufError, Result};
use crate::grf::reader::{GrfArchive, GrfFileEntry, GRF_HEADER_SIZE};

#[derive(Debug)]
pub struct AvailableChunk {
    pub size: usize,
}

#[derive(Debug)]
pub struct AvailableChunkList {
    end_offset: u64,
    sizes: BTreeSet<(usize, u64)>, // Indexed and ordered by size
    chunks: BTreeMap<u64, AvailableChunk>, // Indexed and ordered by offset
}

pub fn list_available_chunks(archive: &mut GrfArchive) -> Result<AvailableChunkList> {
    if archive.file_count() == 0 {
        return Ok(AvailableChunkList::new());
    }

    let mut entries: Vec<&GrfFileEntry> = archive.get_entries().collect();
    entries.sort_unstable_by(|a, b| a.offset.cmp(&b.offset));
    let mut chunks_sizes = BTreeSet::new();
    let mut available_chunks = BTreeMap::new();
    for i in 0..entries.len() - 1 {
        let left_entry = entries[i];
        let right_entry = entries[i + 1];
        let expected_entry_offset = left_entry.offset + left_entry.size_compressed_aligned as u64;
        let space_between_entries = right_entry
            .offset
            .checked_sub(expected_entry_offset)
            .ok_or_else(|| GrufError::parsing_error("Archive is malformed"))?;
        let space_between_entries = usize::try_from(space_between_entries)?;
        chunks_sizes.insert((space_between_entries, expected_entry_offset));
        available_chunks.insert(
            expected_entry_offset,
            AvailableChunk {
                size: space_between_entries,
            },
        );
    }
    let last_entry = entries
        .last()
        .ok_or_else(|| GrufError::parsing_error("Cannot get last entry"))?;
    let end_offset = last_entry.offset + last_entry.size_compressed_aligned as u64;
    Ok(AvailableChunkList {
        end_offset,
        sizes: chunks_sizes,
        chunks: available_chunks,
    })
}

impl AvailableChunkList {
    pub fn new() -> AvailableChunkList {
        let end_offset = GRF_HEADER_SIZE as u64;
        let sizes = BTreeSet::new();
        let chunks = BTreeMap::new();
        AvailableChunkList {
            end_offset,
            sizes,
            chunks,
        }
    }

    /// Acquire a chunk of memory
    pub fn alloc_chunk(&mut self, size: usize) -> Result<u64> {
        let chunk_offset = self.find_suitable_chunk(size);
        // Update chunk list
        if chunk_offset == self.end_offset {
            let new_offset = chunk_offset + size as u64;
            self.end_offset = new_offset;
        } else {
            let chunk = self
                .remove_chunk_internal(chunk_offset)
                .ok_or(GrufError::DynAllocError)?;
            if chunk.size > size {
                let new_offset = chunk_offset + size as u64;
                self.insert_chunk_internal(new_offset, chunk.size - size);
            }
        }
        Ok(chunk_offset)
    }

    fn find_suitable_chunk(&self, size: usize) -> u64 {
        // Find first chunk with a sufficient size
        let opt_item = self.sizes.range((size, 0)..).next();
        match opt_item {
            None => self.end_offset,
            Some((_, offset)) => *offset,
        }
    }

    /// Resizes an already "allocated" chunk of memory
    /// This realloc method assumes all free chunks are merged (i.e. there can
    /// only be used chunks between 2 free chunks)
    pub fn realloc_chunk(&mut self, offset: u64, size: usize, new_size: usize) -> Result<u64> {
        let end_offset = offset + size as u64;
        let new_end_offset = offset + new_size as u64;
        if end_offset == self.end_offset {
            self.end_offset = new_end_offset;
            return Ok(offset);
        }

        if let Some(next_chunk) = self.chunks.get(&end_offset) {
            // Next chunk is available
            let next_chunk_size = next_chunk.size;
            if size + next_chunk_size >= new_size {
                // Sufficient space for in-place grow
                let _ = self
                    .remove_chunk_internal(end_offset)
                    .ok_or(GrufError::DynAllocError)?;
                self.insert_chunk_internal(new_end_offset, size + next_chunk_size - new_size);
                return Ok(offset);
            }
        }

        // Next chunk is used or free but too small, must move
        self.free_chunk(offset, size)?;
        self.alloc_chunk(new_size)
    }

    /// Releases a chunk of memory
    /// This method trusts the input given by the caller.
    /// At the moment, passing bad parameters to this method can mess up the list.
    pub fn free_chunk(&mut self, offset: u64, size: usize) -> Result<()> {
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
                let chunk = self
                    .remove_chunk_internal(offset_left)
                    .ok_or(GrufError::DynAllocError)?;
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
            let chunk = self
                .remove_chunk_internal(chunk_end_offset)
                .ok_or(GrufError::DynAllocError)?;
            new_chunk_size += chunk.size;
        }
        self.insert_chunk_internal(new_chunk_offset, new_chunk_size);
        Ok(())
    }

    fn insert_chunk_internal(&mut self, offset: u64, size: usize) {
        self.sizes.insert((size, offset));
        self.chunks.insert(offset, AvailableChunk { size });
    }

    fn remove_chunk_internal(&mut self, offset: u64) -> Option<AvailableChunk> {
        let chunk = self.chunks.remove(&offset)?;
        let tuple = (chunk.size, offset);
        let removed = self.sizes.remove(&tuple);
        assert!(removed);
        Some(chunk)
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
        let res = chunk_list.alloc_chunk(size1).unwrap();
        assert_eq!(START_OFFSET, res);
        // Alloc a second chunk which should be located right after the previous one
        let res = chunk_list.alloc_chunk(size2).unwrap();
        assert_eq!(START_OFFSET + size1 as u64, res);

        // Free the first chunk
        chunk_list.free_chunk(START_OFFSET, size1).unwrap();
        // Allocated chunk should fit into the previously freed chunk
        let res = chunk_list.alloc_chunk(size1).unwrap();
        assert_eq!(START_OFFSET, res);
        // Alloc another chunk which should be located after the first two chunks
        let res = chunk_list.alloc_chunk(size3).unwrap();
        assert_eq!(START_OFFSET + size1 as u64 + size2 as u64, res);
    }

    #[test]
    fn test_chunk_list_realloc() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let _ = chunk_list.alloc_chunk(chunk_size).unwrap();
        let _ = chunk_list.alloc_chunk(chunk_size).unwrap();

        // Reallocate the first block with a smaller size, should not move
        let res = chunk_list
            .realloc_chunk(START_OFFSET, chunk_size, chunk_size - 1)
            .unwrap();
        assert_eq!(START_OFFSET, res);

        // Reallocate the first block with a bigger size, should move
        let res = chunk_list
            .realloc_chunk(START_OFFSET, chunk_size, chunk_size + 1)
            .unwrap();
        assert_eq!(START_OFFSET + 2 * chunk_size as u64, res);
        let res = chunk_list.alloc_chunk(chunk_size).unwrap();
        assert_eq!(START_OFFSET, res);
    }

    #[test]
    fn test_chunk_list_realloc_overlap() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let offset1 = chunk_list.alloc_chunk(0).unwrap();
        let offset2 = chunk_list.alloc_chunk(0).unwrap();
        let offset3 = chunk_list.alloc_chunk(chunk_size).unwrap();
        assert_eq!(offset1, offset2);
        assert_eq!(offset1, offset3);

        chunk_list.free_chunk(offset1, 0).unwrap();
        let res = chunk_list.realloc_chunk(offset2, 0, chunk_size).unwrap();
        assert_ne!(res, offset2);

        let res = chunk_list.realloc_chunk(offset3, chunk_size, 0).unwrap();
        assert_eq!(res, offset3);

        let res = chunk_list.alloc_chunk(chunk_size).unwrap();
        assert_eq!(res, offset3);
    }

    #[test]
    fn test_chunk_list_right_merge() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let offset1 = chunk_list.alloc_chunk(chunk_size).unwrap();
        let offset2 = chunk_list.alloc_chunk(chunk_size).unwrap();
        let offset3 = chunk_list.alloc_chunk(chunk_size).unwrap();

        // Free the second chunk
        chunk_list.free_chunk(offset2, chunk_size).unwrap();
        // Free the first chunk
        chunk_list.free_chunk(offset1, chunk_size).unwrap();
        let offset4 = chunk_list.alloc_chunk(2 * chunk_size).unwrap();
        assert_eq!(offset4, offset1);

        // Free the third chunk
        chunk_list.free_chunk(offset3, chunk_size).unwrap();
        // Free the fourth chunk
        chunk_list.free_chunk(offset4, 2 * chunk_size).unwrap();
        let offset5 = chunk_list.alloc_chunk(4 * chunk_size).unwrap();
        assert_eq!(offset5, offset1);
    }

    #[test]
    fn test_chunk_list_left_merge() {
        let chunk_size: usize = 64;
        let mut chunk_list = AvailableChunkList::new();
        let offset1 = chunk_list.alloc_chunk(chunk_size).unwrap();
        let offset2 = chunk_list.alloc_chunk(chunk_size).unwrap();
        let offset3 = chunk_list.alloc_chunk(chunk_size).unwrap();

        // Free the first chunk
        chunk_list.free_chunk(offset1, chunk_size).unwrap();
        // Free the second chunk
        chunk_list.free_chunk(offset2, chunk_size).unwrap();

        let offset4 = chunk_list.alloc_chunk(2 * chunk_size).unwrap();
        assert_eq!(offset4, offset1);

        // Free the fourth chunk
        chunk_list.free_chunk(offset4, 2 * chunk_size).unwrap();
        // Free the third chunk
        chunk_list.free_chunk(offset3, chunk_size).unwrap();
        let offset5 = chunk_list.alloc_chunk(4 * chunk_size).unwrap();
        assert_eq!(offset5, offset1);
    }
}
