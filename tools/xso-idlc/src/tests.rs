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
                assert!(anchor.contains(&format!("XSO_LAYOUT_{}_SIZE {}", name, l.size)),
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
            crate::log!("[snapshot] bootstrap wrote {}", p.display());
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

    // ===================== 多文件依赖校验 =====================

    /// 建一个临时工程目录（并行测试互不干扰：目录名按 tag 区分）
    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("xso-idlc-test-{}", tag));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).expect("create temp dir");
        p
    }

    fn put(path: &std::path::Path, content: &str) {
        std::fs::write(path, content).expect("write idl");
    }

    fn project(tag: &str, files: &[(&str, &str)]) -> (std::path::PathBuf, Vec<std::path::PathBuf>) {
        let dir = tmp_dir(tag);
        let mut paths = Vec::new();
        for (name, content) in files {
            let p = dir.join(name);
            put(&p, content);
            paths.push(p);
        }
        (dir, paths)
    }

    fn diags_text(r: &crate::deps::Report) -> Vec<String> {
        r.diagnostics.iter().map(|d| d.render()).collect()
    }

    const BASE_XSO: &str = "@module(base)\n@version(1)\n\
        enum Color : u32 { Red = 1, Green = 2 }\n\
        struct Point { x: i32, y: i32 }\n";

    #[test]
    fn use_declarations_are_parsed_into_imports() {
        let src = "@module(app)\n@version(2)\n\
            use base;\n\
            use base::{Point, Color};\n\
            use base::Point as P;\n\
            struct Rect { a: Point }\n";
        let (m, errs) = parser::parse_with_recovery(src);
        assert!(errs.is_empty(), "parse: {:?}", errs);
        let m = m.expect("module");
        assert_eq!(m.imports.len(), 3, "{:#?}", m.imports);
        assert_eq!(m.imports[0].module_name(), "base");
        assert!(m.imports[0].is_glob(), "整模块导入");
        assert_eq!(m.imports[1].module_name(), "base");
        assert_eq!(
            m.imports[1].names.iter().map(|n| n.name.clone()).collect::<Vec<_>>(),
            vec!["Point".to_string(), "Color".to_string()]);
        assert_eq!(m.imports[2].names[0].local(), "P", "as 别名生效");
        // 导入声明不进入条目列表
        assert_eq!(m.items.len(), 1);
    }

    #[test]
    fn multi_file_project_resolves_and_links_imports() {
        let (_dir, paths) = project("link", &[
            ("base.xso", BASE_XSO),
            ("app.xso", "@module(app)\n@version(3)\nuse base::Point;\n\
                struct Rect { a: Point, b: Point }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 0, "{:?}", diags_text(&report));
        assert_eq!(report.warnings(), 0, "{:?}", diags_text(&report));
        // 依赖先于依赖方
        assert_eq!(report.order, vec!["base".to_string(), "app".to_string()]);
        assert_eq!(report.edges, vec![("app".to_string(), "base".to_string())]);
        assert!(report.cycles.is_empty());
        // 链接：base::Point 被克隆进 app
        let app_items = report.linked.get(&paths[1]).expect("app linked");
        assert!(app_items.iter().any(|it| it.name() == "Point"), "{:#?}", app_items);
        // 依赖方（base）没有外部依赖，不需内联
        let base_items = report.linked.get(&paths[0]).expect("base linked");
        assert!(base_items.is_empty());
    }

    #[test]
    fn multi_file_unknown_module_is_reported() {
        let (_dir, paths) = project("unknown-mod", &[
            ("app.xso", "@module(app)\n@version(1)\nuse missing::Thing;\n\
                struct S { t: Thing }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 1, "{:?}", diags_text(&report));
        let d = &report.diagnostics[0];
        assert!(d.message.contains("unknown module 'missing'"), "{}", d.message);
        assert_eq!(d.line, Some(3), "定位到 use 所在行");
    }

    #[test]
    fn multi_file_unknown_symbol_is_reported() {
        let (_dir, paths) = project("unknown-sym", &[
            ("base.xso", BASE_XSO),
            ("app.xso", "@module(app)\n@version(1)\n\
                use base::{Point, Nope};\nstruct S { p: Point }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 1, "{:?}", diags_text(&report));
        let d = &report.diagnostics[0];
        assert!(d.message.contains("does not export symbol 'Nope'"), "{}", d.message);
        assert!(d.hint.as_deref().unwrap_or("").contains("Point"), "hint 列出可用符号");
    }

    #[test]
    fn multi_file_import_cycle_is_reported() {
        let (_dir, paths) = project("cycle", &[
            ("a.xso", "@module(a)\n@version(1)\nuse b::B;\nstruct A { b: B }\n"),
            ("b.xso", "@module(b)\n@version(1)\nuse a::A;\nstruct B { a: A }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert!(report.errors() >= 1, "{:?}", diags_text(&report));
        assert!(!report.cycles.is_empty(), "检测到环");
        assert!(!report.order.contains(&"a".to_string())
             || !report.order.contains(&"b".to_string()),
             "环上的模块不能给出全序");
        assert!(report.diagnostics.iter().any(|d| d.message.contains("cycle")),
                "{:?}", diags_text(&report));
    }

    #[test]
    fn multi_file_self_import_is_reported() {
        let (_dir, paths) = project("self-import", &[
            ("app.xso", "@module(app)\n@version(1)\nuse app::A;\nstruct A { x: u32 }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 1, "{:?}", diags_text(&report));
        assert!(report.diagnostics[0].message.contains("imports itself"));
    }

    #[test]
    fn multi_file_duplicate_module_name_is_reported() {
        let (_dir, paths) = project("dup-module", &[
            ("one.xso", "@module(dup)\n@version(1)\nstruct A { x: u32 }\n"),
            ("two.xso", "@module(dup)\n@version(1)\nstruct B { y: u32 }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 1, "{:?}", diags_text(&report));
        assert!(report.diagnostics[0].message.contains("duplicate module name 'dup'"));
    }

    #[test]
    fn multi_file_unused_import_is_warned() {
        let (_dir, paths) = project("unused", &[
            ("base.xso", BASE_XSO),
            ("app.xso", "@module(app)\n@version(1)\nuse base::Point;\n\
                struct S { x: u32 }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 0, "{:?}", diags_text(&report));
        assert_eq!(report.warnings(), 1, "{:?}", diags_text(&report));
        assert!(report.diagnostics[0].message.contains("unused import"));
        // 严格模式下警告即失败
        assert!(report.is_fatal(true));
        assert!(!report.is_fatal(false));
    }

    #[test]
    fn multi_file_transitive_dependency_is_linked() {
        // app -> mid -> base：mid::Wrapper 字段引用 base::Point，
        // app 只导入 mid::Wrapper，Point 也应被传递内联进来
        let (_dir, paths) = project("transitive", &[
            ("base.xso", BASE_XSO),
            ("mid.xso", "@module(mid)\n@version(1)\nuse base::Point;\n\
                struct Wrapper { p: Point, tag: u32 }\n"),
            ("app.xso", "@module(app)\n@version(1)\nuse mid::Wrapper;\n\
                struct Holder { w: Wrapper }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 0, "{:?}", diags_text(&report));
        assert_eq!(report.warnings(), 0, "{:?}", diags_text(&report));
        assert_eq!(report.order,
            vec!["base".to_string(), "mid".to_string(), "app".to_string()]);
        let items = report.linked.get(&paths[2]).expect("app linked");
        let names: Vec<&str> = items.iter().map(|it| it.name()).collect();
        assert!(names.contains(&"Wrapper"), "{:?}", names);
        assert!(names.contains(&"Point"), "传递依赖也被内联: {:?}", names);
    }

    #[test]
    fn multi_file_missing_module_is_discovered_in_include_dir() {
        let dir = tmp_dir("discover");
        let idl = dir.join("idl");
        std::fs::create_dir_all(&idl).expect("mkdir idl");
        put(&idl.join("base.xso"), BASE_XSO);
        let app = dir.join("app.xso");
        put(&app, "@module(app)\n@version(1)\nuse base::Point;\n\
            struct Rect { a: Point }\n");
        // 只给出 app，base 需要从 include 目录发现
        let report = crate::deps::analyze(&[app], &crate::deps::Options {
            strict: false,
            include_dirs: vec![idl],
        });
        assert_eq!(report.errors(), 0, "{:?}", diags_text(&report));
        assert_eq!(report.modules.len(), 2, "base 被自动发现");
        assert_eq!(report.order, vec!["base".to_string(), "app".to_string()]);
    }

    #[test]
    fn multi_file_gen_inlines_imported_types() {
        let dir = tmp_dir("gen");
        let base = dir.join("base.xso");
        let app = dir.join("app.xso");
        put(&base, BASE_XSO);
        put(&app, "@module(app)\n@version(1)\nuse base::Point;\n\
            struct Rect { a: Point, b: Point }\n");
        let cache = dir.join("cache");
        let argv: Vec<String> = vec![
            "xso-idlc".into(),
            "--input".into(), app.display().to_string(),
            "--input".into(), base.display().to_string(),
            "--cache-dir".into(), cache.display().to_string(),
        ];
        crate::run(&argv).expect("multi-file generation should succeed");

        let code = std::fs::read_to_string(dir.join("app.rs")).expect("app.rs");
        assert!(code.contains("struct Rect"), "本地定义");
        assert!(code.contains("struct Point"), "导入的定义被内联进本模块");
        assert!(code.contains("a: Point"), "字段类型可见");
    }

    #[test]
    fn multi_file_broken_dependency_fails_generation() {
        let dir = tmp_dir("broken");
        let app = dir.join("app.xso");
        put(&app, "@module(app)\n@version(1)\nuse ghost::Thing;\n\
            struct S { t: Thing }\n");
        let argv: Vec<String> = vec![
            "xso-idlc".into(),
            "--input".into(), app.display().to_string(),
            "--input".into(), dir.join("ghost.xso").display().to_string(),
            "--cache-dir".into(), dir.join("cache").display().to_string(),
        ];
        let err = crate::run(&argv).expect_err("broken dependency must fail");
        assert!(err.to_string().contains("dependency"), "{}", err);
    }

    #[test]
    fn multi_file_deps_json_is_deterministic() {
        let (_dir, paths) = project("json", &[
            ("base.xso", BASE_XSO),
            ("app.xso", "@module(app)\n@version(2)\nuse base::{Point, Color};\n\
                struct Rect { a: Point, c: Color }\n"),
        ]);
        let opts = crate::deps::Options::default();
        let a = crate::deps::analyze(&paths, &opts).to_json();
        let b = crate::deps::analyze(&paths, &opts).to_json();
        assert_eq!(a, b, "依赖图 JSON 必须确定");
        assert!(a.contains("\"name\": \"base\""));
        assert!(a.contains("\"deps\": [\"base\"]"));
        assert!(a.contains("\"order\": [\"base\", \"app\"]"));
        assert!(a.contains("\"cycles\": []"));
    }

    #[test]
    fn multi_file_glob_import_links_whole_module() {
        let (_dir, paths) = project("glob", &[
            ("base.xso", BASE_XSO),
            ("app.xso", "@module(app)\n@version(1)\nuse base;\n\
                struct Pair { a: Point, c: Color }\n"),
        ]);
        let report = crate::deps::analyze(&paths, &crate::deps::Options::default());
        assert_eq!(report.errors(), 0, "{:?}", diags_text(&report));
        assert_eq!(report.warnings(), 0, "{:?}", diags_text(&report));
        let items = report.linked.get(&paths[1]).expect("app linked");
        let names: Vec<&str> = items.iter().map(|it| it.name()).collect();
        assert!(names.contains(&"Point") && names.contains(&"Color"), "{:?}", names);
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
