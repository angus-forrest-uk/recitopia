const WYHASH_SECRET: [u64; 4] = [
    0xa076_1d64_78bd_642f,
    0xe703_7ed1_a0b4_28db,
    0x8ebc_6af0_9c88_c6e3,
    0x5899_65cc_7537_4cc3,
];

pub(crate) fn wyhash(seed: u64, input: &[u8]) -> u64 {
    let initial_state = seed ^ mix(seed ^ WYHASH_SECRET[0], WYHASH_SECRET[1]);
    let mut state = [initial_state; 3];
    let (mut a, mut b) = if input.len() <= 16 {
        small_key(input)
    } else {
        let mut offset = 0;
        if input.len() >= 48 {
            while offset + 48 < input.len() {
                for index in 0..3 {
                    let chunk = offset + 16 * index;
                    state[index] = mix(
                        read_u64(&input[chunk..]) ^ WYHASH_SECRET[index + 1],
                        read_u64(&input[chunk + 8..]) ^ state[index],
                    );
                }
                offset += 48;
            }
            state[0] ^= state[1] ^ state[2];
        }

        let remaining = &input[offset..];
        let mut remaining_offset = 0;
        while remaining_offset + 16 < remaining.len() {
            state[0] = mix(
                read_u64(&remaining[remaining_offset..]) ^ WYHASH_SECRET[1],
                read_u64(&remaining[remaining_offset + 8..]) ^ state[0],
            );
            remaining_offset += 16;
        }
        (
            read_u64(&input[input.len() - 16..]),
            read_u64(&input[input.len() - 8..]),
        )
    };

    a ^= WYHASH_SECRET[1];
    b ^= state[0];
    (a, b) = product_halves(u128::from(a) * u128::from(b));
    mix(
        a ^ WYHASH_SECRET[0] ^ input.len() as u64,
        b ^ WYHASH_SECRET[1],
    )
}

fn small_key(input: &[u8]) -> (u64, u64) {
    if input.len() >= 4 {
        let end = input.len() - 4;
        let quarter = (input.len() >> 3) << 2;
        return (
            (read_u32(input) << 32) | read_u32(&input[quarter..]),
            (read_u32(&input[end..]) << 32) | read_u32(&input[end - quarter..]),
        );
    }
    if input.is_empty() {
        return (0, 0);
    }
    (
        (u64::from(input[0]) << 16)
            | (u64::from(input[input.len() >> 1]) << 8)
            | u64::from(input[input.len() - 1]),
        0,
    )
}

fn read_u32(input: &[u8]) -> u64 {
    u64::from(u32::from_le_bytes(
        input[..4].try_into().expect("four-byte wyhash read"),
    ))
}

fn read_u64(input: &[u8]) -> u64 {
    u64::from_le_bytes(input[..8].try_into().expect("eight-byte wyhash read"))
}

fn mix(a: u64, b: u64) -> u64 {
    let (low, high) = product_halves(u128::from(a) * u128::from(b));
    low ^ high
}

fn product_halves(product: u128) -> (u64, u64) {
    let low = u64::try_from(product & u128::from(u64::MAX)).expect("masked wyhash low half");
    let high = u64::try_from(product >> 64).expect("shifted wyhash high half");
    (low, high)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_zig_016_wyhash_vectors() {
        let vectors = [
            (0, 0x0409_638e_e2bd_e459, ""),
            (1, 0xa841_2d09_1b5f_e0a9, "a"),
            (2, 0x32dd_92e4_b291_5153, "abc"),
            (3, 0x8619_1240_89a3_a16b, "message digest"),
            (4, 0x7a43_afb6_1d7f_5f40, "abcdefghijklmnopqrstuvwxyz"),
            (
                5,
                0xff42_329b_90e5_0d58,
                "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            ),
            (
                6,
                0xc39c_ab13_b115_aad3,
                "12345678901234567890123456789012345678901234567890123456789012345678901234567890",
            ),
        ];
        for (seed, expected, input) in vectors {
            assert_eq!(wyhash(seed, input.as_bytes()), expected);
        }
    }
}
