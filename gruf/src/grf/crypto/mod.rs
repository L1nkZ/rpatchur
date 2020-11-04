use std::cmp;
use std::convert::TryInto;
use std::result::Result;

mod des;

const DES_BLOCK_SIZE: usize = 8; // Block size in bytes

pub fn decrypt_file_name(file_name: &[u8]) -> Result<Vec<u8>, &str> {
    let mut mut_vec = file_name.to_vec();
    swap_nibbles(&mut mut_vec);
    grf_decrypt_shuffled(0, 1, mut_vec.as_mut_slice());
    remove_zero_padding(&mut mut_vec);
    Ok(mut_vec)
}

pub fn decrypt_file_content(data: &mut Vec<u8>, cycle: usize) {
    if cycle == 0 {
        grf_decrypt_first_blocks(0, data.as_mut_slice())
    } else {
        grf_decrypt_shuffled(0, cycle, data.as_mut_slice());
    }
}

fn swap_nibbles(buffer: &mut Vec<u8>) {
    for b in buffer {
        *b = (*b << 4) | (*b >> 4);
    }
}

fn remove_zero_padding(vec: &mut Vec<u8>) {
    for i in (0..vec.len()).rev() {
        if vec[i] != 0 {
            break;
        }
        vec.swap_remove(i);
    }
}

fn grf_decrypt_first_blocks(key: u64, buffer: &mut [u8]) {
    let des_cipher = des::Des {
        keys: des::gen_keys(key),
    };
    let buffer_size_in_blocks = buffer.len() / DES_BLOCK_SIZE;
    for i in 0..cmp::min(buffer_size_in_blocks, 20) {
        let cur_block_range = i * DES_BLOCK_SIZE..(i + 1) * DES_BLOCK_SIZE;
        // Apply 1 round of DES to the block
        let block_as_u64 = read_be_u64(&buffer[cur_block_range.clone()]);
        let decrypted_block = des_cipher.decrypt_block_1_round(block_as_u64);
        buffer[cur_block_range].copy_from_slice(&u64::to_be_bytes(decrypted_block));
    }
}

fn grf_decrypt_shuffled(key: u64, cycle: usize, buffer: &mut [u8]) {
    let des_cipher = des::Des {
        keys: des::gen_keys(key),
    };
    let updated_cycle = update_cycle(cycle);
    let buffer_size_in_blocks = buffer.len() / DES_BLOCK_SIZE;
    // Process blocks
    let mut j = 0;
    for i in 0..buffer_size_in_blocks {
        let cur_block_range = i * DES_BLOCK_SIZE..(i + 1) * DES_BLOCK_SIZE;
        if i < 20 || (i % updated_cycle) == 0 {
            // Apply 1 round of DES to the block
            let block_as_u64 = read_be_u64(&buffer[cur_block_range.clone()]);
            let decrypted_block = des_cipher.decrypt_block_1_round(block_as_u64);
            buffer[cur_block_range].copy_from_slice(&u64::to_be_bytes(decrypted_block));
        } else {
            if j == 7 {
                j = 0;
                // Shuffle bytes in the block
                let cur_block_copy: [u8; DES_BLOCK_SIZE] =
                    buffer[cur_block_range.clone()].try_into().unwrap();
                let cur_block_view = &mut buffer[cur_block_range];
                // 3450162 (initial layout) to 0123456 (final layout)
                cur_block_view[..2].copy_from_slice(&cur_block_copy[3..5]);
                cur_block_view[2] = cur_block_copy[6];
                cur_block_view[3..6].copy_from_slice(&cur_block_copy[..3]);
                cur_block_view[6] = cur_block_copy[5];
                // Mutate the 7th byte
                cur_block_view[7] = permute_byte(cur_block_copy[7]);
            }
            j += 1;
        }
    }
}

fn update_cycle(cycle: usize) -> usize {
    if cycle < 3 {
        return 3;
    }
    if cycle < 5 {
        return cycle + 1;
    }
    if cycle < 7 {
        return cycle + 9;
    }
    cycle + 15
}

fn read_be_u64(input: &[u8]) -> u64 {
    let (int_bytes, _rest) = input.split_at(std::mem::size_of::<u64>());
    u64::from_be_bytes(int_bytes.try_into().unwrap())
}

fn permute_byte(b: u8) -> u8 {
    match b {
        0x00 => 0x2B,
        0x01 => 0x68,
        0x2B => 0x00,
        0x48 => 0x77,
        0x60 => 0xFF,
        0x68 => 0x01,
        0x6C => 0x80,
        0x77 => 0x48,
        0x80 => 0x6C,
        0xB9 => 0xC0,
        0xC0 => 0xB9,
        0xEB => 0xFE,
        0xFE => 0xEB,
        0xFF => 0x60,
        _ => b,
    }
}
