// Note(LinkZ): This code was taken from RustCrypto/des crate

const SHIFTS: [u8; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

// These boxes are not the same ones that appear in the literature. Normally,
// the first and the last bits of the six input bits are used to choose the row
// and the middle four bits are used to choose the column. These sboxes are
// rearranged so that the bottom four bits choose the column and the top two
// bits choose the row. In other words, we can directly index the sbox array
// with the 6 input bits to get the correct value.
#[rustfmt::skip]
const SBOXES: [[u8; 64]; 8] = [
    [
        14,  0,  4, 15, 13,  7,  1,  4,  2, 14, 15,  2, 11, 13,  8,  1,
         3, 10, 10,  6,  6, 12, 12, 11,  5,  9,  9,  5,  0,  3,  7,  8,
         4, 15,  1, 12, 14,  8,  8,  2, 13,  4,  6,  9,  2,  1, 11,  7,
        15,  5, 12, 11,  9,  3,  7, 14,  3, 10, 10,  0,  5,  6,  0, 13,
    ],
    [
        15,  3,  1, 13,  8,  4, 14,  7,  6, 15, 11,  2,  3,  8,  4, 14,
         9, 12,  7,  0,  2,  1, 13, 10, 12,  6,  0,  9,  5, 11, 10,  5,
         0, 13, 14,  8,  7, 10, 11,  1, 10,  3,  4, 15, 13,  4,  1,  2,
         5, 11,  8,  6, 12,  7,  6, 12,  9,  0,  3,  5,  2, 14, 15,  9,
    ],
    [
        10, 13,  0,  7,  9,  0, 14,  9,  6,  3,  3,  4, 15,  6,  5, 10,
         1,  2, 13,  8, 12,  5,  7, 14, 11, 12,  4, 11,  2, 15,  8,  1,
        13,  1,  6, 10,  4, 13,  9,  0,  8,  6, 15,  9,  3,  8,  0,  7,
        11,  4,  1, 15,  2, 14, 12,  3,  5, 11, 10,  5, 14,  2,  7, 12,
    ],
    [
         7, 13, 13,  8, 14, 11,  3,  5,  0,  6,  6, 15,  9,  0, 10,  3,
         1,  4,  2,  7,  8,  2,  5, 12, 11,  1, 12, 10,  4, 14, 15,  9,
        10,  3,  6, 15,  9,  0,  0,  6, 12, 10, 11,  1,  7, 13, 13,  8,
        15,  9,  1,  4,  3,  5, 14, 11,  5, 12,  2,  7,  8,  2,  4, 14,
    ],
    [
         2, 14, 12, 11,  4,  2,  1, 12,  7,  4, 10,  7, 11, 13,  6,  1,
         8,  5,  5,  0,  3, 15, 15, 10, 13,  3,  0,  9, 14,  8,  9,  6,
         4, 11,  2,  8,  1, 12, 11,  7, 10,  1, 13, 14,  7,  2,  8, 13,
        15,  6,  9, 15, 12,  0,  5,  9,  6, 10,  3,  4,  0,  5, 14,  3,
    ],
    [
        12, 10,  1, 15, 10,  4, 15,  2,  9,  7,  2, 12,  6,  9,  8,  5,
         0,  6, 13,  1,  3, 13,  4, 14, 14,  0,  7, 11,  5,  3, 11,  8,
         9,  4, 14,  3, 15,  2,  5, 12,  2,  9,  8,  5, 12, 15,  3, 10,
         7, 11,  0, 14,  4,  1, 10,  7,  1,  6, 13,  0, 11,  8,  6, 13,
    ],
    [
         4, 13, 11,  0,  2, 11, 14,  7, 15,  4,  0,  9,  8,  1, 13, 10,
         3, 14, 12,  3,  9,  5,  7, 12,  5,  2, 10, 15,  6,  8,  1,  6,
         1,  6,  4, 11, 11, 13, 13,  8, 12,  1,  3,  4,  7, 10, 14,  7,
        10,  9, 15,  5,  6,  0,  8, 15,  0, 14,  5,  2,  9,  3,  2, 12,
    ],
    [
        13,  1,  2, 15,  8, 13,  4,  8,  6, 10, 15,  3, 11,  7,  1,  4,
        10, 12,  9,  5,  3,  6, 14, 11,  5,  0,  0, 14, 12,  9,  7,  2,
         7,  2, 11,  1,  4, 14,  1,  7,  9,  4, 12, 10, 14,  8,  2, 13,
         0, 15,  6, 12, 10,  9, 13,  0, 15,  3,  3,  5,  5,  6,  8, 11,
    ],
];

#[derive(Copy, Clone)]
pub struct Des {
    pub keys: [u64; 16],
}

/// Swap bits in `a` using a delta swap
fn delta_swap(a: u64, delta: u64, mask: u64) -> u64 {
    let b = (a ^ (a >> delta)) & mask;
    a ^ b ^ (b << delta)
}

/// Swap bits using the PC-1 table
fn pc1(mut key: u64) -> u64 {
    key = delta_swap(key, 2, 0x3333000033330000);
    key = delta_swap(key, 4, 0x0f0f0f0f00000000);
    key = delta_swap(key, 8, 0x009a000a00a200a8);
    key = delta_swap(key, 16, 0x00006c6c0000cccc);
    key = delta_swap(key, 1, 0x1045500500550550);
    key = delta_swap(key, 32, 0x00000000f0f0f5fa);
    key = delta_swap(key, 8, 0x00550055006a00aa);
    key = delta_swap(key, 2, 0x0000333330000300);
    key & 0xFFFFFFFFFFFFFF00
}

/// Swap bits using the PC-2 table
fn pc2(key: u64) -> u64 {
    let key = key.rotate_left(61);
    let b1 = (key & 0x0021000002000000) >> 7;
    let b2 = (key & 0x0008020010080000) << 1;
    let b3 = key & 0x0002200000000000;
    let b4 = (key & 0x0000000000100020) << 19;
    let b5 = (key.rotate_left(54) & 0x0005312400000011).wrapping_mul(0x0000000094200201)
        & 0xea40100880000000;
    let b6 = (key.rotate_left(7) & 0x0022110000012001).wrapping_mul(0x0001000000610006)
        & 0x1185004400000000;
    let b7 = (key.rotate_left(6) & 0x0000520040200002).wrapping_mul(0x00000080000000c1)
        & 0x0028811000200000;
    let b8 = (key & 0x01000004c0011100).wrapping_mul(0x0000000000004284) & 0x0400082244400000;
    let b9 = (key.rotate_left(60) & 0x0000000000820280).wrapping_mul(0x0000000000089001)
        & 0x0000000110880000;
    let b10 = (key.rotate_left(49) & 0x0000000000024084).wrapping_mul(0x0000000002040005)
        & 0x000000000a030000;
    b1 | b2 | b3 | b4 | b5 | b6 | b7 | b8 | b9 | b10
}

/// Swap bits using the reverse FP table
fn fp(mut message: u64) -> u64 {
    message = delta_swap(message, 24, 0x000000FF000000FF);
    message = delta_swap(message, 24, 0x00000000FF00FF00);
    message = delta_swap(message, 36, 0x000000000F0F0F0F);
    message = delta_swap(message, 18, 0x0000333300003333);
    delta_swap(message, 9, 0x0055005500550055)
}

/// Swap bits using the IP table
fn ip(mut message: u64) -> u64 {
    message = delta_swap(message, 9, 0x0055005500550055);
    message = delta_swap(message, 18, 0x0000333300003333);
    message = delta_swap(message, 36, 0x000000000F0F0F0F);
    message = delta_swap(message, 24, 0x00000000FF00FF00);
    delta_swap(message, 24, 0x000000FF000000FF)
}

/// Swap bits using the E table
fn e(block: u64) -> u64 {
    const BLOCK_LEN: usize = 32;
    const RESULT_LEN: usize = 48;

    let b1 = (block << (BLOCK_LEN - 1)) & 0x8000000000000000;
    let b2 = (block >> 1) & 0x7C00000000000000;
    let b3 = (block >> 3) & 0x03F0000000000000;
    let b4 = (block >> 5) & 0x000FC00000000000;
    let b5 = (block >> 7) & 0x00003F0000000000;
    let b6 = (block >> 9) & 0x000000FC00000000;
    let b7 = (block >> 11) & 0x00000003F0000000;
    let b8 = (block >> 13) & 0x000000000FC00000;
    let b9 = (block >> 15) & 0x00000000003E0000;
    let b10 = (block >> (RESULT_LEN - 1)) & 0x0000000000010000;
    b1 | b2 | b3 | b4 | b5 | b6 | b7 | b8 | b9 | b10
}

/// Swap bits using the P table
fn p(block: u64) -> u64 {
    let block = block.rotate_left(44);
    let b1 = (block & 0x0000000000200000) << 32;
    let b2 = (block & 0x0000000000480000) << 13;
    let b3 = (block & 0x0000088000000000) << 12;
    let b4 = (block & 0x0000002020120000) << 25;
    let b5 = (block & 0x0000000442000000) << 14;
    let b6 = (block & 0x0000000001800000) << 37;
    let b7 = (block & 0x0000000004000000) << 24;
    let b8 = (block & 0x0000020280015000).wrapping_mul(0x0000020080800083) & 0x02000a6400000000;
    let b9 = (block.rotate_left(29) & 0x01001400000000aa).wrapping_mul(0x0000210210008081)
        & 0x0902c01200000000;
    let b10 = (block & 0x0000000910040000).wrapping_mul(0x0000000c04000020) & 0x8410010000000000;
    b1 | b2 | b3 | b4 | b5 | b6 | b7 | b8 | b9 | b10
}

/// Generate the 16 subkeys
pub fn gen_keys(key: u64) -> [u64; 16] {
    let mut keys: [u64; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let key = pc1(key);

    // The most significant bit is bit zero, and there are only 56 bits in
    // the key after applying PC1, so we need to remove the eight least
    // significant bits from the key.
    let key = key >> 8;

    let mut c = key >> 28;
    let mut d = key & 0x0FFF_FFFF;
    for i in 0..16 {
        c = rotate(c, SHIFTS[i]);
        d = rotate(d, SHIFTS[i]);

        // We need the `<< 8` because the most significant bit is bit zero,
        // so we need to shift our 56 bit value 8 bits to the left.
        keys[i] = pc2(((c << 28) | d) << 8);
    }

    keys
}

/// Performs a left rotate on a 28 bit number
fn rotate(mut val: u64, shift: u8) -> u64 {
    let top_bits = val >> (28 - shift);
    val <<= shift;

    (val | top_bits) & 0x0FFF_FFFF
}

fn round(input: u64, key: u64) -> u64 {
    let l = input & (0xFFFF_FFFF << 32);
    let r = input << 32;

    r | ((f(r, key) ^ l) >> 32)
}

fn f(input: u64, key: u64) -> u64 {
    let mut val = e(input as u64);
    val ^= key;
    val = apply_sboxes(val);
    p(val)
}

/// Applies all eight sboxes to the input
fn apply_sboxes(input: u64) -> u64 {
    let mut output: u64 = 0;
    for (i, sbox) in SBOXES.iter().enumerate() {
        let val = (input >> (58 - (i * 6))) & 0x3F;
        output |= (sbox[val as usize] as u64) << (60 - (i * 4));
    }
    output
}

impl Des {
    #[allow(dead_code)]
    pub fn encrypt_block_1_round(&self, mut data: u64) -> u64 {
        data = ip(data);
        data = round(data, *self.keys.first().unwrap());
        fp((data << 32) | (data >> 32))
    }

    pub fn decrypt_block_1_round(&self, mut data: u64) -> u64 {
        data = ip(data);
        data = round(data, *self.keys.last().unwrap());
        fp((data << 32) | (data >> 32))
    }
}
