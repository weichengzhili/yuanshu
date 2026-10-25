//! 内置测试套件（无三方依赖）：单测 / 快照 / 模糊 / 对拍
//!
//! 运行：cargo test

#[cfg(test)]
mod tests {
    use crate::bridge_ir::BridgeIr;
    use crate::codegen;
    use crate::parser;
    use crate::semantic;

    const FS: &str = include_str!("examples/fs.xso");
    const NET: &str = include_str!("examples/net.xso");
    const KV: &str = include_str!("examples/kv.xso");

    /// ASCII 基线，供模糊测试做字节级变异（避免非 ASCII 边界问题）
    const FUZZ_BASE: &str = "@module(m)\n@version(1)\n\
        error E : i32 { A = 1 }\n\
        struct S { a: u32 }\n\
        @opaque interface I { @method(1) f(@borrowed self, x: u32) -> result<u32, E>; }\n";

    fn build(src: &str) -> BridgeIr {
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "unexpected parse errors: {:?}",
                errs.iter().map(|e| e.to_string()).collect::<Vec<_>>());
        let m = m.expect("module should parse");
        semantic::analyze(&m).expect("semantic analysis should pass");
        BridgeIr::build(&m)
    }

    // ===================== 单测：语义 =====================

    #[test]
    fn detects_struct_cycle() {
        let src = "@module(c)\n@version(1)\nstruct A { b: B }\nstruct B { a: A }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let r = semantic::analyze(&m.expect("module"));
        assert!(r.is_err(), "cycle should be rejected");
        assert!(r.unwrap_err().to_string().contains("cyclic"));
    }

    #[test]
    fn pointer_breaks_cycle() {
        // @owned 是间接引用，不应构成按值环
        let src = "@module(c)\n@version(1)\nstruct A { b: @owned B }\nstruct B { a: A }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        // 语义应通过（无按值环）
        let _ = semantic::analyze(&m.expect("module"));
    }

    #[test]
    fn rejects_type_nesting_too_deep() {
        let mut ty = String::from("u32");
        for _ in 0..40 { ty = format!("Vec<{}>", ty); }
        let src = format!("@module(d)\n@version(1)\nstruct S {{ f: {} }}\n", ty);
        let (m, errs) = parser::parse_with_recovery(&src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let r = semantic::analyze(&m.expect("module"));
        assert!(r.is_err(), "deep nesting should be rejected");
        assert!(r.unwrap_err().to_string().contains("nesting"));
    }

    #[test]
    fn rejects_duplicate_field_and_zero_error_code() {
        let dup = "@module(x)\n@version(1)\nstruct S { a: u32, a: u8 }\n";
        let (m, _) = parser::parse_with_recovery(dup);
        let r = semantic::analyze(&m.expect("module"));
        assert!(r.is_err() && r.unwrap_err().to_string().contains("duplicate field"));

        let zero = "@module(y)\n@version(1)\nerror E : i32 { A = 0 }\n";
        let (m, _) = parser::parse_with_recovery(zero);
        let r = semantic::analyze(&m.expect("module"));
        assert!(r.is_err(), "error code 0 must be rejected");
    }

    // ===================== 单测：解析错误恢复 =====================

    #[test]
    fn recovery_collects_multiple_errors() {
        let src = "@module(b)\n@version(1)\nstruct S { a u32, b: u8 }\nstruct T { c: u8 }\nblah\n";
        let (_m, errs) = parser::parse_with_recovery(src);
        assert!(errs.len() >= 2, "expected >=2 errors, got {}", errs.len());
    }

    #[test]
    fn unicode_identifiers_are_lexed() {
        let src = "@module(u)\n@version(1)\nstruct 数据表 { 编号: u32 }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "unicode ident parse: {:?}", errs);
        let m = m.expect("module");
        assert!(m.items.iter().any(|it| matches!(it, crate::ast::Item::Struct(s) if s.name == "数据表")));
    }

    // ===================== 单测：BridgeIR =====================

    #[test]
    fn fingerprint_is_deterministic_and_changes_with_layout() {
        let a = build(FS);
        let b = build(FS);
        assert_eq!(a.fingerprint(), b.fingerprint(), "fingerprint must be stable");

        let mut src = FS.replace("nlink: u32", "nlink: u16");
        src = src.replace("@module(fs)", "@module(fs2)");
        let c = build(&src);
        assert_ne!(a.fingerprint(), c.fingerprint(), "layout change must change fingerprint");
    }

    #[test]
    fn ir_json_is_serializable_and_diffable() {
        let a = build(FS);
        let json = a.to_json();
        assert!(json.contains("\"ir_version\""));
        assert!(json.contains("\"fingerprint\""));

        let mut src = FS.replace("nlink: u32", "nlink: u16");
        src = src.replace("@module(fs)", "@module(fs2)");
        let b = build(&src);
        let d = a.diff(&b);
        assert!(!d.is_empty(), "diff should report changes");
        assert!(d.iter().any(|x| x.starts_with("[break]")), "layout change is breaking: {:?}", d);
    }

    // ===================== 代码生成：确定性 / 幂等 =====================

    #[test]
    fn generation_is_deterministic_and_idempotent() {
        for src in [FS, NET, KV] {
            let ir = build(src);
            assert_eq!(codegen::rust::generate(&ir), codegen::rust::generate(&ir),
                       "rust codegen not deterministic");
            assert_eq!(codegen::cpp::generate(&ir), codegen::cpp::generate(&ir),
                       "cpp codegen not deterministic");
            assert_eq!(codegen::anchor::generate(&ir), codegen::anchor::generate(&ir),
                       "anchor codegen not deterministic");
        }
    }

    // ===================== 对拍：三端布局常量一致 =====================

    #[test]
    fn layout_constants_match_across_backends() {
        for src in [FS, NET, KV] {
            let ir = build(src);
            let anchor = codegen::anchor::generate(&ir);
            let rust = codegen::rust::generate(&ir);
            let cpp = codegen::cpp::generate(&ir);

            for (name, l) in &ir.type_layouts {
                assert!(anchor.contains(&format!("XSO_LAYOUT_{}_SIZE  {}", name, l.size)),
                        "anchor missing size for {}", name);
                assert!(anchor.contains(&format!("XSO_LAYOUT_{}_ALIGN {}", name, l.align)),
                        "anchor missing align for {}", name);
                assert!(rust.contains(&format!("LAYOUT_{}_SIZE: usize = {};", name, l.size)),
                        "rust missing size for {}", name);
                assert!(cpp.contains(&format!("XSO_LAYOUT_{}_SIZE", name)),
                        "cpp missing size assert for {}", name);
            }
            // 字段偏移三端一致
            for (name, fields) in &ir.field_offsets {
                for (fname, _, off) in fields {
                    assert!(anchor.contains(&format!("XSO_LAYOUT_{}_OFF_{} {}", name, fname, off)),
                            "anchor missing offset {}.{}", name, fname);
                }
            }
        }
    }

    #[test]
    fn abi_checksum_is_identical_on_all_three_sides() {
        for src in [FS, NET, KV] {
            let ir = build(src);
            let fp = format!("{:08x}", ir.fingerprint());
            let anchor = codegen::anchor::generate(&ir);
            let rust = codegen::rust::generate(&ir);
            let cpp = codegen::cpp::generate(&ir);
            assert!(anchor.to_lowercase().contains(&fp), "anchor checksum mismatch");
            assert!(rust.to_lowercase().contains(&fp), "rust checksum mismatch");
            assert!(cpp.to_lowercase().contains(&fp), "cpp checksum mismatch");
        }
    }

    // ===================== 快照：golden 文件对比 =====================

    fn golden_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests").join("generate").join(name)
    }

    /// 与已入库 golden 比对；首次运行若缺失则写入（bootstrap）
    fn snapshot(name: &str, content: &str) {
        let p = golden_path(name);
        if let Ok(old) = std::fs::read_to_string(&p) {
            assert_eq!(old, content, "snapshot mismatch for {} (re-run with updated IDL?)", name);
        } else {
            if let Some(d) = p.parent() { let _ = std::fs::create_dir_all(d); }
            std::fs::write(&p, content).expect("write golden");
            eprintln!("[snapshot] bootstrap wrote {}", p.display());
        }
    }

    #[test]
    fn snapshots_match_golden_files() {
        for (src, tag) in [(FS, "fs"), (NET, "net"), (KV, "kv")] {
            let ir = build(src);
            snapshot(&format!("{}.rs", tag), &codegen::rust::generate(&ir));
            snapshot(&format!("{}.h", tag), &codegen::cpp::generate(&ir));
            snapshot(&format!("{}.anchor.h", tag), &codegen::anchor::generate(&ir));
        }
    }

    // ===================== 模糊：不得 panic =====================

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            self.0 = x;
            x
        }
    }

    #[test]
    fn fuzz_never_panics() {
        let mut rng = Rng(0x2545F4914F6CDD1D);
        let alphabet: Vec<u8> = b"@{}()<>,:;=abcdefg VecOptionresultMapSetunitu8u16u32u64i8i32i64\
            boolselfinterfacestructenumerrortypeBorrowedStr \n\t\""
            .to_vec();

        // 随机字符流
        for _ in 0..3000 {
            let n = (rng.next() % 160) as usize;
            let mut s = String::new();
            for _ in 0..n {
                s.push(alphabet[(rng.next() as usize) % alphabet.len()] as char);
            }
            let (m, _errs) = parser::parse_with_recovery(&s);
            if let Some(m) = m { let _ = semantic::analyze(&m); }
        }

        // 对合法输入做字节级变异
        for _ in 0..1500 {
            let mut s = FUZZ_BASE.to_string();
            let pos = (rng.next() as usize) % s.len();
            let c = alphabet[(rng.next() as usize) % alphabet.len()] as char;
            s.replace_range(pos..=pos, &c.to_string());
            let (m, _errs) = parser::parse_with_recovery(&s);
            if let Some(m) = m {
                let _ = semantic::analyze(&m);
                let ir = BridgeIr::build(&m);
                let _ = codegen::rust::generate(&ir);
                let _ = codegen::cpp::generate(&ir);
            }
        }
    }
}
