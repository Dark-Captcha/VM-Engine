//! PLV3 RC4-variant stream cipher + Fisher-Yates KSA.
//!
//! Ported from datadome_new/plv3/cipher.py. Uses extracted S-boxes only —
//! no hardcoded data, no external files. Give it S-boxes from the IR
//! extractor and it produces valid PLV3 tokens.
//!
//! Pipeline: random seed → Fisher-Yates KSA on sbox0 copy → RC4-variant cipher → base64url.

// ============================================================================
// Phase table for Fisher-Yates KSA xorshift
// ============================================================================

/// Xorshift phase: (operation, shift_amount).
/// operation 0 = left shift XOR, operation 1 = right shift XOR.
type XorshiftPhase = (u8, u32);

/// 15 phases mapping index ranges to xorshift triplets.
/// Extracted from the KSA function — rarely changes across rotations.
const PHASE_TABLE: &[(u8, u8, [XorshiftPhase; 3])] = &[
    (0, 30, [(1, 17), (1, 13), (0, 3)]),
    (31, 44, [(1, 6), (1, 11), (0, 1)]),
    (45, 61, [(0, 9), (0, 2), (1, 21)]),
    (62, 72, [(0, 3), (1, 27), (0, 11)]),
    (73, 84, [(0, 25), (1, 9), (0, 10)]),
    (85, 118, [(1, 9), (0, 7), (1, 2)]),
    (119, 127, [(1, 1), (0, 5), (1, 19)]),
    (128, 143, [(0, 9), (0, 7), (1, 1)]),
    (144, 157, [(1, 27), (0, 27), (1, 1)]),
    (158, 179, [(0, 21), (0, 7), (1, 17)]),
    (180, 204, [(1, 13), (1, 17), (0, 3)]),
    (205, 214, [(0, 3), (0, 22), (1, 5)]),
    (215, 221, [(0, 1), (0, 10), (1, 3)]),
    (222, 248, [(1, 5), (1, 21), (0, 3)]),
    (249, 254, [(0, 11), (0, 6), (1, 1)]),
];

/// Build lookup: index → xorshift triplet.
fn build_shift_table() -> Vec<[XorshiftPhase; 3]> {
    let mut table = vec![[(0u8, 0u32); 3]; 255];
    for &(start, end, shifts) in PHASE_TABLE {
        for index in start..=end {
            table[index as usize] = shifts;
        }
    }
    table
}

// ============================================================================
// Bit helpers (JS 32-bit integer semantics)
// ============================================================================

/// Truncate to signed 32-bit (JS `| 0`).
fn i32_truncate(value: u64) -> i32 {
    (value & 0xFFFFFFFF) as u32 as i32
}

/// Truncate to unsigned 32-bit.
fn u32_truncate(value: u64) -> u32 {
    (value & 0xFFFFFFFF) as u32
}

// ============================================================================
// Xorshift PRNG
// ============================================================================

fn xorshift(state: i32, shifts: &[XorshiftPhase; 3]) -> i32 {
    let mut x = u32_truncate(state as u64);
    for &(operation, amount) in shifts {
        if operation == 0 {
            // Left shift XOR
            x = u32_truncate((x ^ (x << amount)) as u64);
        } else {
            // Right shift XOR
            x = u32_truncate((x ^ (x >> amount)) as u64);
        }
    }
    i32_truncate(x as u64)
}

// ============================================================================
// Fisher-Yates KSA
// ============================================================================

/// Run Fisher-Yates KSA on a 256-element state array.
/// Returns the final seed value.
pub fn fisher_yates_ksa(state: &mut [u8; 256], seed: i32) -> i32 {
    let shift_table = build_shift_table();
    let mut current_seed = seed;

    for (position_a, shifts) in shift_table.iter().enumerate().take(255) {
        current_seed = xorshift(current_seed, shifts);
        let position_b = (u32_truncate(current_seed as u64) % (position_a as u32 + 1)) as usize;
        if position_a != position_b {
            state.swap(position_a, position_b);
        }
    }
    current_seed
}

// ============================================================================
// RC4-variant cipher
// ============================================================================

/// S-box collection needed by the cipher.
/// Indices match the decompiler output from cipher.py.
pub struct CipherSboxes {
    pub sbox4: [u8; 256],
    pub sbox5: [u8; 256],
    pub sbox6: [u8; 256],
    pub sbox7: [u8; 256],
    pub sbox9: [u8; 256],
    pub sbox10: [u8; 256],
    pub sbox11: [u8; 256],
    pub sbox15: [u8; 256],
    pub sbox16: [u8; 256],
    pub sbox22: [u8; 256],
    pub sbox24: [u8; 256],
}

/// Encrypt plaintext using the PLV3 RC4-variant stream cipher.
///
/// `ksa_state` is the sbox0 copy after Fisher-Yates KSA.
pub fn encrypt(
    plaintext: &[u8],
    ksa_state: &mut [u8; 256],
    sboxes: &CipherSboxes,
) -> Vec<u8> {
    let mut i: usize = 49;
    let mut j: usize = 147;
    let mut output = vec![0u8; plaintext.len()];

    for (n, &plaintext_byte) in plaintext.iter().enumerate() {
        // Advance i
        i = sboxes.sbox11[i] as usize;

        // Compute k1
        let idx1 = (ksa_state[sboxes.sbox5[i] as usize] ^ 160) as usize;
        let k1 = sboxes.sbox9[sboxes.sbox10[idx1] as usize] as usize;

        // Advance j
        j = (j + sboxes.sbox16[k1] as usize) & 255;

        // Compute k2
        let idx2 = (ksa_state[sboxes.sbox22[j] as usize] ^ 160) as usize;
        let k2 = sboxes.sbox9[sboxes.sbox6[sboxes.sbox15[idx2] as usize] as usize] as usize;

        // Swap ksa_state entries
        let swap_a = sboxes.sbox5[i] as usize;
        let swap_b = sboxes.sbox5[sboxes.sbox24[j] as usize] as usize;
        ksa_state.swap(swap_a, swap_b);

        // Compute keystream byte
        let combined = (k1 + k2) & 255;
        let ks_idx = (ksa_state[sboxes.sbox5[sboxes.sbox4[combined] as usize] as usize] ^ 160) as usize;
        let keystream_byte = sboxes.sbox9[sboxes.sbox10[ks_idx] as usize];

        // XOR plaintext with keystream and positional sbox7
        output[n] = plaintext_byte ^ keystream_byte ^ sboxes.sbox7[n & 255];
    }

    output
}

// ============================================================================
// Base64url encoding
// ============================================================================

/// Standard base64 then replace +→-, /→_, strip padding.
pub fn base64url_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut output = String::new();
    let mut index = 0;
    while index < data.len() {
        let b0 = data[index];
        let b1 = data.get(index + 1).copied().unwrap_or(0);
        let b2 = data.get(index + 2).copied().unwrap_or(0);
        let triple = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        output.push(TABLE[((triple >> 18) & 63) as usize] as char);
        output.push(TABLE[((triple >> 12) & 63) as usize] as char);
        if index + 1 < data.len() { output.push(TABLE[((triple >> 6) & 63) as usize] as char); } else { output.push('='); }
        if index + 2 < data.len() { output.push(TABLE[(triple & 63) as usize] as char); } else { output.push('='); }
        index += 3;
    }
    output.trim_end_matches('=').replace('+', "-").replace('/', "_")
}

// ============================================================================
// PLV3 token generation
// ============================================================================

/// Generate a PLV3 token from extracted S-boxes and known keys.
///
/// This is the complete pipeline: KSA → JSON → cipher → base64url.
pub fn generate_plv3_token(
    sbox0: &[u8; 256],
    cipher_sboxes: &CipherSboxes,
    keys: &Plv3Keys,
    pathname: &str,
) -> String {
    // Random KSA seed
    let ksa_seed = i32_truncate(rand_u32() as u64);

    // Copy sbox0 and run KSA
    let mut ksa_state = *sbox0;
    fisher_yates_ksa(&mut ksa_state, ksa_seed);

    // Build plaintext
    let timestamp_ms = current_timestamp_ms();
    let elapsed = 500 + (rand_u32() % 1501) as i64;
    let perf_now = 30.0 + (rand_u32() as f64 / u32::MAX as f64) * 170.0;
    let math_random = rand_u32() as f64 / u32::MAX as f64;

    let plaintext = format!(
        r#"{{"{}":{},"{}":{:?},"{}":{},"{}":{},"{}":{:.6},"{}":{},"{}":{},"{}":{}}}"#,
        keys.timestamp, timestamp_ms,
        keys.pathname, pathname,
        keys.client_width, 15,
        keys.elapsed, elapsed,
        keys.perf_now, perf_now,
        keys.is_secure, true,
        keys.webdriver, false,
        keys.random, math_random,
    );

    let ciphertext = encrypt(plaintext.as_bytes(), &mut ksa_state, cipher_sboxes);
    base64url_encode(&ciphertext)
}

/// The 8 obfuscated field key names.
pub struct Plv3Keys {
    pub timestamp: String,
    pub pathname: String,
    pub client_width: String,
    pub elapsed: String,
    pub perf_now: String,
    pub is_secure: String,
    pub webdriver: String,
    pub random: String,
}

// ============================================================================
// Helpers
// ============================================================================

fn current_timestamp_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn rand_u32() -> u32 {
    // Simple entropy from timestamp + address
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64;
    let mixed = t.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    (mixed >> 16) as u32
}

/// Convert extracted S-box Vec<u8> to [u8; 256].
pub fn to_sbox_array(bytes: &[u8]) -> [u8; 256] {
    let mut array = [0u8; 256];
    let length = bytes.len().min(256);
    array[..length].copy_from_slice(&bytes[..length]);
    array
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xorshift_is_deterministic() {
        let result1 = xorshift(12345, &[(1, 17), (1, 13), (0, 3)]);
        let result2 = xorshift(12345, &[(1, 17), (1, 13), (0, 3)]);
        assert_eq!(result1, result2);
        assert_ne!(result1, 12345); // should change the value
    }

    #[test]
    fn fisher_yates_ksa_shuffles() {
        let mut state: [u8; 256] = std::array::from_fn(|i| i as u8);
        let original = state;
        fisher_yates_ksa(&mut state, 42);
        assert_ne!(state, original, "KSA should shuffle the state");

        // Same seed → same shuffle
        let mut state2: [u8; 256] = std::array::from_fn(|i| i as u8);
        fisher_yates_ksa(&mut state2, 42);
        assert_eq!(state, state2, "same seed should produce same result");
    }

    #[test]
    fn fisher_yates_ksa_different_seeds() {
        let mut state1: [u8; 256] = std::array::from_fn(|i| i as u8);
        let mut state2: [u8; 256] = std::array::from_fn(|i| i as u8);
        fisher_yates_ksa(&mut state1, 100);
        fisher_yates_ksa(&mut state2, 200);
        assert_ne!(state1, state2, "different seeds should produce different results");
    }

    #[test]
    fn base64url_no_padding_no_plus_no_slash() {
        let token = base64url_encode(b"Hello, World!");
        assert!(!token.contains('='), "should have no padding: {token}");
        assert!(!token.contains('+'), "should have no +: {token}");
        assert!(!token.contains('/'), "should have no /: {token}");
    }

    #[test]
    fn encrypt_is_deterministic() {
        // Use identity S-boxes for testing
        let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
        let sboxes = CipherSboxes {
            sbox4: identity, sbox5: identity, sbox6: identity,
            sbox7: [0u8; 256], // zero sbox7 to isolate keystream XOR
            sbox9: identity, sbox10: identity, sbox11: identity,
            sbox15: identity, sbox16: identity, sbox22: identity,
            sbox24: identity,
        };

        let mut ksa1 = identity;
        let mut ksa2 = identity;
        let plaintext = b"test data";

        let ct1 = encrypt(plaintext, &mut ksa1, &sboxes);
        let ct2 = encrypt(plaintext, &mut ksa2, &sboxes);
        assert_eq!(ct1, ct2, "same inputs → same output");
        assert_ne!(&ct1[..], plaintext.as_slice(), "should be encrypted");
    }

    #[test]
    fn token_length_reasonable() {
        let identity: [u8; 256] = std::array::from_fn(|i| i as u8);
        let sboxes = CipherSboxes {
            sbox4: identity, sbox5: identity, sbox6: identity,
            sbox7: identity, sbox9: identity, sbox10: identity,
            sbox11: identity, sbox15: identity, sbox16: identity,
            sbox22: identity, sbox24: identity,
        };
        let keys = Plv3Keys {
            timestamp: "ts".into(), pathname: "pn".into(),
            client_width: "cw".into(), elapsed: "el".into(),
            perf_now: "pf".into(), is_secure: "sc".into(),
            webdriver: "wd".into(), random: "rn".into(),
        };

        let token = generate_plv3_token(&identity, &sboxes, &keys, "/interstitial/");
        assert!(token.len() > 100 && token.len() < 400,
            "token length should be ~200, got {}", token.len());
        // All chars should be base64url safe
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "invalid chars in token: {token}");
    }
}
