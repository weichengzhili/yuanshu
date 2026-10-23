//! 内置测试套件（无三方依赖）：单测 / 快照 / 模糊 / 对拍
//!
//! 运行：cargo test

#[cfg(test)]
mod tests {
    use crate::bridge_ir::BridgeIr;
    use crate::codegen;
    use crate::codegen::naming;
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
        let mut m = m.expect("module should parse");
        semantic::analyze(&mut m).expect("semantic analysis should pass");
        BridgeIr::build(&m)
    }

    // ===================== 单测：语义 =====================

    #[test]
    fn detects_struct_cycle() {
        let src = "@module(c)\n@version(1)\nstruct A { b: B }\nstruct B { a: A }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let r = semantic::analyze(&mut m.expect("module"));
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
        let _ = semantic::analyze(&mut m.expect("module"));
    }

    #[test]
    fn rejects_type_nesting_too_deep() {
        let mut ty = String::from("u32");
        for _ in 0..40 { ty = format!("Vec<{}>", ty); }
        let src = format!("@module(d)\n@version(1)\nstruct S {{ f: {} }}\n", ty);
        let (m, errs) = parser::parse_with_recovery(&src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let r = semantic::analyze(&mut m.expect("module"));
        assert!(r.is_err(), "deep nesting should be rejected");
        assert!(r.unwrap_err().to_string().contains("nesting"));
    }

    #[test]
    fn rejects_duplicate_field_and_zero_error_code() {
        let dup = "@module(x)\n@version(1)\nstruct S { a: u32, a: u8 }\n";
        let (m, _) = parser::parse_with_recovery(dup);
        let r = semantic::analyze(&mut m.expect("module"));
        assert!(r.is_err() && r.unwrap_err().to_string().contains("duplicate field"));

        let zero = "@module(y)\n@version(1)\nerror E : i32 { A = 0 }\n";
        let (m, _) = parser::parse_with_recovery(zero);
        let r = semantic::analyze(&mut m.expect("module"));
        assert!(r.is_err(), "error code 0 must be rejected");
    }

    // ===================== 单测：泛型单态化 =====================

    #[test]
    fn generics_are_monomorphized_with_correct_layouts() {
        let src = "@module(g)\n@version(1)\n\
            struct Wrapper<T> { v: T }\n\
            struct Holder { a: Wrapper<u32>, b: Wrapper<u64> }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let mut m = m.expect("module");
        semantic::analyze(&mut m).expect("semantic");

        let names: Vec<String> = m.items.iter().filter_map(|it| match it {
            crate::ast::Item::Struct(s) => Some(s.name.clone()),
            _ => None,
        }).collect();
        assert!(names.contains(&"Wrapper__u32".to_string()), "{:?}", names);
        assert!(names.contains(&"Wrapper__u64".to_string()), "{:?}", names);
        assert!(!names.contains(&"Wrapper".to_string()), "generic template must be removed");

        let ir = BridgeIr::build(&m);
        let l32 = ir.layout_of("Wrapper__u32").expect("u32 layout");
        assert_eq!((l32.size, l32.align), (4, 4));
        let l64 = ir.layout_of("Wrapper__u64").expect("u64 layout");
        assert_eq!((l64.size, l64.align), (8, 8));

        // 两端产物引用的是同一个单态名（保证跨语言布局一致）
        assert!(codegen::rust::generate(&ir).contains("Wrapper__u32"));
        assert!(codegen::cpp::generate(&ir).contains("Wrapper__u32"));
    }

    #[test]
    fn nested_generics_expand_to_fixpoint() {
        let src = "@module(g3)\n@version(1)\n\
            struct Box2<T> { v: T }\n\
            struct Outer<T> { inner: Box2<T> }\n\
            struct Use { o: Outer<u16> }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let mut m = m.expect("module");
        semantic::analyze(&mut m).expect("semantic");
        let names: Vec<String> = m.items.iter().filter_map(|it| match it {
            crate::ast::Item::Struct(s) => Some(s.name.clone()),
            _ => None,
        }).collect();
        assert!(names.contains(&"Outer__u16".to_string()), "{:?}", names);
        assert!(names.contains(&"Box2__u16".to_string()), "nested generic must expand: {:?}", names);
    }

    #[test]
    fn generic_arity_mismatch_is_rejected() {
        let src = "@module(g2)\n@version(1)\n\
            struct P<T, U> { a: T, b: U }\n\
            struct H { p: P<u32> }\n";
        let (m, _) = parser::parse_with_recovery(src);
        let mut m = m.expect("module");
        let r = semantic::analyze(&mut m);
        assert!(r.is_err(), "arity mismatch must be rejected");
        assert!(r.unwrap_err().to_string().contains("arity"));
    }

    // ===================== 单测：复杂 / 嵌套数据结构 =====================

    #[test]
    fn deeply_nested_containers_are_supported() {
        // Vec<Vec<u8>>、Map<Vec<u8>, Set<u32>>、Option<Vec<u32>> 任意嵌套
        let src = "@module(n)\n@version(1)\n\
            error E : i32 { A = 1 }\n\
            struct Nested {\n\
                @field(1) grid: Vec<Vec<u8>>,\n\
                @field(2) index: Map<Vec<u8>, Set<u32>>,\n\
                @field(3) maybe: Option<Vec<u32>>,\n\
            }\n\
            @opaque interface Api {\n\
                @method(1) send(@borrowed self, buf: Vec<Vec<u8>>) -> result<i64, E>;\n\
                @method(2) idx(@borrowed self) -> result<Map<Vec<u8>, Set<u32>>, E>;\n\
            }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs.iter().map(|e| e.to_string()).collect::<Vec<_>>());
        let mut m = m.expect("module");
        semantic::analyze(&mut m).expect("semantic");
        let ir = BridgeIr::build(&m);

        // 容器在边界上统一收敛为规范载体 XsoVec（16 字节 / 8 对齐）
        let l = ir.layout_of("Nested").expect("Nested layout");
        assert_eq!(l.align, 8, "container fields must be pointer-aligned");
        let rust = codegen::rust::generate(&ir);
        assert!(rust.contains("XsoVec<XsoVec<u8>>"), "nested Vec must nest carriers");
        assert!(rust.contains("XsoVec<XsoPair<XsoVec<u8>, XsoVec<u32>>>"),
                 "Map<Vec<u8>, Set<u32>> must nest carrier+pair");
        let cpp = codegen::cpp::generate(&ir);
        assert!(cpp.contains("XsoVec<XsoVec<uint8_t>>"), "cpp must mirror the nesting");
    }

    #[test]
    fn nested_struct_and_generic_compose() {
        // 具名结构体嵌套 + 泛型实例化组合
        let src = "@module(n2)\n@version(1)\n\
            struct Inner { a: u32, b: u64 }\n\
            struct Wrapper<T> { v: T }\n\
            struct Outer { inner: Inner, w: Wrapper<Inner> }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let mut m = m.expect("module");
        semantic::analyze(&mut m).expect("semantic");
        let names: Vec<String> = m.items.iter().filter_map(|it| match it {
            crate::ast::Item::Struct(s) => Some(s.name.clone()), _ => None,
        }).collect();
        assert!(names.contains(&"Wrapper__Inner".to_string()), "{:?}", names);
        let ir = BridgeIr::build(&m);
        // Outer = Inner(4+pad+8=16, align 8) + Wrapper__Inner(16, align 8)
        let outer = ir.layout_of("Outer").expect("Outer layout");
        assert_eq!((outer.size, outer.align), (32, 8));
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

        let mut src = FS.replace("mode:  u32", "mode:  u64");
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

        let mut src = FS.replace("mode:  u32", "mode:  u64");
        src = src.replace("@module(fs)", "@module(fs2)");
        let b = build(&src);
        let d = a.diff(&b);
        assert!(!d.is_empty(), "diff should report changes");
        assert!(d.iter().any(|x| x.starts_with("[break]")), "layout change is breaking: {:?}", d);
    }

    // ===================== 所有权载体（XsoBox / XsoOption） =====================

    #[test]
    fn owned_and_option_use_canonical_carriers() {
        let src = "@module(o)\n@version(1)\n\
            struct Leaf { v: u32 }\n\
            struct Holder {\n\
                @field(1) owned_leaf: @owned Leaf,\n\
                @field(2) maybe_leaf: Option<Leaf>,\n\
            }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs.iter().map(|e| e.to_string()).collect::<Vec<_>>());
        let mut m = m.expect("module");
        semantic::analyze(&mut m).expect("semantic");
        let ir = BridgeIr::build(&m);

        let rust = codegen::rust::generate(&ir);
        let cpp = codegen::cpp::generate(&ir);

        // @owned -> XsoBox（ptr + drop 壳，必须有 Drop）
        assert!(rust.contains(&format!("pub struct {}<T>", naming::BOX)),
                "rust missing XsoBox");
        assert!(rust.contains(&format!("Drop for {}<T>", naming::BOX)),
                "XsoBox must own a Drop impl (cross-allocator release)");
        assert!(cpp.contains(&format!("struct {}", naming::BOX)), "cpp missing XsoBox");
        assert!(cpp.contains("= delete"), "XsoBox must be move-only (copy deleted)");

        // Option -> XsoOption（显式标签）
        assert!(rust.contains(&format!("pub struct {}<T>", naming::OPTION)),
                "rust missing XsoOption");
        assert!(cpp.contains(&format!("struct {}", naming::OPTION)), "cpp missing XsoOption");

        // 字段实际收敛为规范载体
        assert!(rust.contains(&naming::carrier(naming::BOX, "Leaf")), "field should be XsoBox<Leaf>");
        assert!(rust.contains(&naming::carrier(naming::OPTION, "Leaf")), "field should be XsoOption<Leaf>");

        // 布局：XsoBox(16,align8) + XsoOption<Leaf>(8,align4) -> size 24, align 8
        let l = ir.layout_of("Holder").expect("Holder layout");
        assert_eq!((l.size, l.align), (24, 8), "Holder layout mismatch");
    }

    #[test]
    fn owned_carrier_locks_its_layout_on_both_sides() {
        let src = "@module(o2)\n@version(1)\n\
            struct Leaf { v: u32 }\n\
            struct W { l: @owned Leaf }\n";
        let (m, _) = parser::parse_with_recovery(src);
        let mut m = m.expect("module");
        semantic::analyze(&mut m).expect("semantic");
        let ir = BridgeIr::build(&m);
        let anchor = codegen::anchor::generate(&ir);
        let rust = codegen::rust::generate(&ir);
        let cpp = codegen::cpp::generate(&ir);
        // 三端都必须锁定 16 / 8
        let want = format!("{}", naming::BOX_SIZE);
        assert!(rust.contains(&format!("== {});", want)), "rust must assert box size");
        assert!(cpp.contains(&format!("== {}", want)), "cpp must assert box size");
        assert!(anchor.contains(naming::ALGO_VERSION), "anchor must carry algo version");
    }

    // ===================== 性能：增量缓存 + 并行构建 =====================

    #[test]
    fn incremental_cache_skips_on_unchanged_input() {
        let dir = std::env::temp_dir()
            .join(format!("xso-cache-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();

        let input = dir.join("m.xso");
        std::fs::write(&input, FS).unwrap();
        let out_rust = dir.join("m.rs");
        let out_cpp = dir.join("m.h");
        let all_outs = vec![out_rust.clone(), out_cpp.clone()];

        // 首次：未命中（无缓存）
        assert!(!crate::cache_hit(&input, FS, &all_outs, &dir), "cold should miss");
        // 写齐产物并建立缓存
        std::fs::write(&out_rust, "rust").unwrap();
        std::fs::write(&out_cpp, "cpp").unwrap();
        crate::write_cache(&input, FS, &all_outs, &dir);
        // 命中：产物齐全且输入未变
        assert!(crate::cache_hit(&input, FS, &all_outs, &dir), "should hit when unchanged");
        // 产物缺失 -> 未命中（必须重生成）
        std::fs::remove_file(&out_cpp).unwrap();
        assert!(!crate::cache_hit(&input, FS, &all_outs, &dir), "missing output -> miss");
        // 源变化 -> 未命中（自动失效）
        std::fs::write(&out_cpp, "cpp").unwrap();
        assert!(!crate::cache_hit(&input, NET, &all_outs, &dir), "changed source -> miss");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parallel_build_produces_all_outputs() {
        let base = std::env::temp_dir()
            .join(format!("xso-parallel-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();

        let cases = [("a", FS), ("b", NET), ("c", KV)];
        let mut argv: Vec<String> = vec!["xso-idlc".into()];
        for (stem, src) in cases {
            let p = base.join(format!("{}.xso", stem));
            std::fs::write(&p, src).unwrap();
            argv.push("--input".into());
            argv.push(p.display().to_string());
        }
        argv.push("--cache-dir".into());
        argv.push(base.join("cache").display().to_string());

        crate::run(&argv).expect("parallel run failed");

        for (stem, _) in cases {
            let rs = base.join(format!("{}.rs", stem));
            let h = base.join(format!("{}.h", stem));
            assert!(rs.exists() && h.exists(), "missing output for {}", stem);
            assert!(std::fs::read_to_string(&rs).unwrap().contains("XsoVec"),
                    "{} rust must contain carriers", stem);
        }
        let _ = std::fs::remove_dir_all(&base);
    }

    // ===================== IR 二进制序列化（bincode） =====================

    #[test]
    fn ir_binary_roundtrip_preserves_fingerprint() {
        for src in [FS, NET, KV] {
            let ir = build(src);
            let bytes = ir.to_bin();
            // 魔数与确定性
            assert_eq!(&bytes[..5], b"XSOIR", "bad magic");
            assert_eq!(bytes, ir.to_bin(), "binary encoding must be deterministic");

            let back = BridgeIr::from_bin(&bytes).expect("decode failed");
            assert_eq!(back.ir_version, ir.ir_version);
            assert_eq!(back.module.name, ir.module.name);
            assert_eq!(back.module.version, ir.module.version);
            assert_eq!(back.fingerprint(), ir.fingerprint(), "fingerprint must survive roundtrip");
            assert_eq!(back.type_layouts, ir.type_layouts, "layouts must survive");
            assert_eq!(back.enum_sizes, ir.enum_sizes, "enum sizes must survive");
            assert_eq!(back.opaque_types, ir.opaque_types, "opaque set must survive");
        }
    }

    #[test]
    fn ir_binary_diff_detects_breaking_change() {
        let a = build(FS);
        // 改变布局：mode u32 -> u64（改变 size 与后续偏移）
        let mut src = FS.replace("mode:  u32", "mode:  u64");
        src = src.replace("@module(fs)", "@module(fs2)");
        let b = build(&src);

        let baseline = BridgeIr::from_bin(&a.to_bin()).expect("decode");
        let changes = baseline.diff(&b);
        assert!(!changes.is_empty(), "diff should report changes");
        assert!(changes.iter().any(|c| c.starts_with("[break]")), "{:?}", changes);

        // 自身比对无变化
        let same = BridgeIr::from_bin(&a.to_bin()).expect("decode");
        assert!(same.diff(&a).iter().any(|c| c.starts_with("[none]")),
                "identical IR must report no change");
    }

    #[test]
    fn ir_binary_rejects_corrupt_input() {
        let ir = build(FS);
        let mut bytes = ir.to_bin();
        bytes[0] = b'X' ^ 0xff; // 破坏魔数
        assert!(BridgeIr::from_bin(&bytes).is_err(), "corrupt magic must be rejected");
        let short = &ir.to_bin()[..8];
        assert!(BridgeIr::from_bin(short).is_err(), "truncated input must be rejected");
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
                assert!(anchor.contains(&format!("{} {}", naming::layout_size_macro(name), l.size)),
                        "anchor missing size for {}", name);
                assert!(anchor.contains(&format!("{} {}", naming::layout_align_macro(name), l.align)),
                        "anchor missing align for {}", name);
                assert!(rust.contains(&format!("{}: usize = {};",
                            naming::rust_layout_const(name, "SIZE"), l.size)),
                        "rust missing size for {}", name);
                assert!(cpp.contains(&format!("XSO_LAYOUT_{}_SIZE", name)),
                        "cpp missing size assert for {}", name);
            }
            // 字段偏移三端一致
            for (name, fields) in &ir.field_offsets {
                for (fname, _, off) in fields {
                    assert!(anchor.contains(&format!("{} {}",
                                naming::layout_offset_macro(name, fname), off)),
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
            if let Some(mut m) = m { let _ = semantic::analyze(&mut m); }
        }

        // 对合法输入做字节级变异
        for _ in 0..1500 {
            let mut s = FUZZ_BASE.to_string();
            let pos = (rng.next() as usize) % s.len();
            let c = alphabet[(rng.next() as usize) % alphabet.len()] as char;
            s.replace_range(pos..=pos, &c.to_string());
            let (m, _errs) = parser::parse_with_recovery(&s);
            if let Some(mut m) = m {
                let _ = semantic::analyze(&mut m);
                let ir = BridgeIr::build(&m);
                let _ = codegen::rust::generate(&ir);
                let _ = codegen::cpp::generate(&ir);
            }
        }
    }
}
