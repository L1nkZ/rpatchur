mod des;

use std::convert::TryInto;
use std::result::Result;

pub fn decrypt_file_name(file_name: &[u8]) -> Result<Vec<u8>, &str> {
    let mut mut_vec = file_name.to_vec();
    swap_nibbles(mut_vec.as_mut_slice());
    grf_decrypt_shuffled(0, 1, mut_vec.as_mut_slice());
    remove_zero_padding(&mut mut_vec);
    Ok(mut_vec)
}

fn swap_nibbles(buffer: &mut [u8]) {
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

fn grf_decrypt_shuffled(key: u64, cycle: usize, buffer: &mut [u8]) {
    const DES_BLOCK_SIZE: usize = 8;
    let des_cipher = des::Des {
        keys: des::gen_keys(key),
    };
    let updated_cycle = update_cycle(cycle);
    let buffer_size_in_blocks = buffer.len() / DES_BLOCK_SIZE;
    // Process blocks
    for i in 0..buffer_size_in_blocks {
        let current_block_range = i * DES_BLOCK_SIZE..(i + 1) * DES_BLOCK_SIZE;
        let block_as_u64 = read_be_u64(&buffer[current_block_range.clone()]);
        if i < 0x14 || (i % updated_cycle) == 0 {
            let decrypted_block = des_cipher.decrypt_block_1_round(block_as_u64);
            buffer[current_block_range].copy_from_slice(&u64::to_be_bytes(decrypted_block));
        } else {
            // TODO(LinkZ): Shuffle bytes
        }
    }
}

fn update_cycle(cycle: usize) -> usize {
    if cycle < 3 {
        return 1;
    }
    if cycle < 5 {
        return cycle + 1;
    }
    if cycle < 7 {
        return cycle + 9;
    }
    cycle + 0xF
}

fn read_be_u64(input: &[u8]) -> u64 {
    let (int_bytes, _rest) = input.split_at(std::mem::size_of::<u64>());
    u64::from_be_bytes(int_bytes.try_into().unwrap())
}
