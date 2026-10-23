//! SBOM（软件物料清单）：输出 CycloneDX 1.4 JSON，含生成产物的 SHA-256 校验和。
//! 无三方依赖（自带 SHA-256 实现），用于供应链审计与产物完整性追踪。

use crate::bridge_ir::BridgeIr;
use std::fmt::Write;

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// SHA-256（FIPS 180-4）
pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    // 填充：0x80 ... 长度(bit, big-endian)
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 { msg.push(0); }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    let mut w = [0u32; 64];
    for chunk in msg.chunks(64) {
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4], chunk[i * 4 + 1], chunk[i * 4 + 2], chunk[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh.wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g; g = f; f = e; e = d.wrapping_add(t1);
            d = c; c = b; b = a; a = t1.wrapping_add(t2);
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for i in 0..8 { out[i * 4..i * 4 + 4].copy_from_slice(&h[i].to_be_bytes()); }
    out
}

pub fn hex(bytes: &[u8]) -> String {
    let mut s = String::new();
    for b in bytes { let _ = write!(s, "{:02x}", b); }
    s
}

fn jstr(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// 生成 CycloneDX 1.4 SBOM。
/// `files` 为 (显示名, 内容) 列表，逐项计算 SHA-256。
pub fn generate(tool_version: &str, ir: &BridgeIr, files: &[(String, String)]) -> String {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // ISO-8601 近似（UTC，秒精度）
    let secs = ts % 86400;
    let days = ts / 86400;
    let (y, m, d) = days_to_ymd(days);
    let hh = secs / 3600;
    let mm = (secs % 3600) / 60;
    let ss = secs % 60;
    let timestamp = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, m, d, hh, mm, ss);

    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"bomFormat\": \"CycloneDX\",\n");
    s.push_str("  \"specVersion\": \"1.4\",\n");
    s.push_str("  \"serialNumber\": \"urn:uuid:");
    s.push_str(&hex(&sha256(
        format!("{}:{}:{}", ir.module.name, ir.module.version, ir.fingerprint()).as_bytes(),
    )[..16]));
    s.push_str("\",\n");
    s.push_str("  \"version\": 1,\n");
    s.push_str("  \"metadata\": {\n");
    s.push_str(&format!("    \"timestamp\": {},\n", jstr(&timestamp)));
    s.push_str("    \"tools\": [ { \"vendor\": \"xso\", \"name\": \"xso-idlc\", \"version\": ");
    s.push_str(&jstr(tool_version));
    s.push_str(" } ],\n");
    s.push_str("    \"component\": { \"type\": \"application\", \"bom-ref\": ");
    s.push_str(&jstr(&format!("{}-v{}", ir.module.name, ir.module.version)));
    s.push_str(", \"name\": ");
    s.push_str(&jstr(&ir.module.name));
    s.push_str(", \"version\": ");
    s.push_str(&jstr(&format!("{}", ir.module.version)));
    s.push_str(" }\n");
    s.push_str("  },\n");

    // 属性：ABI 指纹 / IR 版本（便于审计与兼容追踪）
    s.push_str("  \"properties\": [\n");
    s.push_str(&format!("    {{ \"name\": \"xso.abi.fingerprint\", \"value\": \"0x{:08x}\" }},\n",
        ir.fingerprint()));
    s.push_str(&format!("    {{ \"name\": \"xso.ir.version\", \"value\": \"{}\" }},\n", ir.ir_version));
    s.push_str(&format!("    {{ \"name\": \"xso.fingerprint.algo\", \"value\": \"{}\" }}\n",
        crate::bridge_ir::FINGERPRINT_ALGO_VERSION));
    s.push_str("  ],\n");

    // 组件：每个生成产物（含 SHA-256）
    s.push_str("  \"components\": [\n");
    for (i, (name, content)) in files.iter().enumerate() {
        let digest = hex(&sha256(content.as_bytes()));
        s.push_str("    { \"type\": \"file\", \"bom-ref\": ");
        s.push_str(&jstr(name));
        s.push_str(", \"name\": ");
        s.push_str(&jstr(name));
        s.push_str(", \"hashes\": [ { \"alg\": \"SHA-256\", \"content\": ");
        s.push_str(&jstr(&digest));
        s.push_str(" } ] }");
        s.push_str(if i + 1 == files.len() { "\n" } else { ",\n" });
    }
    s.push_str("  ]\n");
    s.push_str("}\n");
    s
}

/// 天数 -> (年, 月, 日)，基于 1970-01-01 的简化 civil-from-days
fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let z = days + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
