    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::BorderType;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn code(k: KeyCode) -> KeyEvent {
        KeyEvent::new(k, KeyModifiers::NONE)
    }

    #[test]
    fn the_solarized_light_preset_paints_a_light_base() {
        let t = cian_lua::Theme { preset: Some("solarized-light".into()), ..Default::default() };
        let (c, errors) = resolve_theme(&t);
        assert!(errors.is_empty(), "{:?}", errors);
        assert_eq!(c.base_bg, Some(Color::Rgb(0xfd, 0xf6, 0xe3)), "base3 background");
        assert_eq!(c.accent, Color::Rgb(0x26, 0x8b, 0xd2), "solarized blue accent");
        assert_eq!(c.file.directory, Color::Rgb(0x26, 0x8b, 0xd2));
    }

    #[test]
    fn the_default_theme_keeps_the_dark_look() {
        let (c, errors) = resolve_theme(&cian_lua::Theme::default());
        assert!(errors.is_empty());
        assert_eq!(c.base_bg, None, "no painted background — the terminal shows through");
        assert_eq!(c.accent, Color::Cyan);
    }

    #[test]
    fn per_key_overrides_apply_on_top_of_a_preset() {
        let t = cian_lua::Theme {
            preset: Some("solarized-light".into()),
            accent: Some("#ff0000".into()),
            ..Default::default()
        };
        let (c, errors) = resolve_theme(&t);
        assert!(errors.is_empty());
        assert_eq!(c.accent, Color::Rgb(255, 0, 0), "override wins");
        assert_eq!(c.base_bg, Some(Color::Rgb(0xfd, 0xf6, 0xe3)), "rest stays solarized");
    }

    #[test]
    fn an_unknown_preset_reports_and_falls_back_to_dark() {
        let t = cian_lua::Theme { preset: Some("nope".into()), ..Default::default() };
        let (c, errors) = resolve_theme(&t);
        assert!(errors.iter().any(|e| e.contains("unknown preset")), "{:?}", errors);
        assert_eq!(c.base_bg, None);
    }

    /// An app rooted at a temp dir containing `names`.
    fn app_with(names: &[&str]) -> (tempfile::TempDir, App) {
        app_with_keymaps(names, Vec::new())
    }

    /// Like `app_with`, but with the `lang` option set.
    fn app_with_lang(names: &[&str], lang: &str) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(dir.path().join(n), b"").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.options.lang = Some(lang.to_string());
        let app = App::new(p.clone(), p, config).unwrap();
        (dir, app)
    }

    /// Like `app_with`, but with `cian.set_keymap` overrides applied.
    fn app_with_keymaps(names: &[&str], keymaps: Vec<(char, String)>) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(dir.path().join(n), b"").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.keymaps = keymaps;
        let app = App::new(p.clone(), p, config).unwrap();
        (dir, app)
    }

    #[test]
    fn shortcuts_save_as_lua_and_legacy_formats_still_migrate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shortcuts.lua");
        let store = ShortcutStore {
            entries: vec![
                Shortcut::leaf("home".into(), "~/".into()),
                Shortcut::leaf("docs".into(), "https://example.com".into()),
            ],
            path: path.clone(),
        };
        store.save().unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("return {"), "written as Lua:\n{text}");
        assert!(text.contains("name = \"home\""), "written as Lua:\n{text}");
        // Round-trips through the Lua reader the loader uses.
        let back = cian_lua::shortcuts::parse(&text).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back[0].name, "home");

        // A pre-existing YAML/TOML file must still parse, so migration keeps
        // entries for anyone on the old formats.
        let yaml = "shortcuts:\n  - name: srv\n    target: /srv\n";
        let from_yaml: ShortcutsFile = serde_yml::from_str(yaml).unwrap();
        assert_eq!(from_yaml.shortcuts[0].target.as_deref(), Some("/srv"));
        let toml_src = "[[shortcuts]]\nname = \"srv\"\ntarget = \"/srv\"\n";
        let from_toml: ShortcutsFile = toml::from_str(toml_src).unwrap();
        assert_eq!(from_toml.shortcuts[0].target.as_deref(), Some("/srv"));
    }

    #[test]
    fn ai_context_facts_are_folded_into_prompts() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // No facts configured → no context block.
        assert!(app.ai_context_block().is_empty());

        // Global facts from cian.ai_context appear as bullet points.
        app.config.ai_context = vec!["The panes browse RHEL 8.".into(), "Prefer POSIX sh.".into()];
        let block = app.ai_context_block();
        assert!(block.contains("Context about the user's environment"));
        assert!(block.contains("- The panes browse RHEL 8."));
        assert!(block.contains("- Prefer POSIX sh."));
    }

    #[test]
    fn resolve_bg_accepts_preset_names_and_specs() {
        // Preset by name (crmaine matches "crmaine (^_-)"), plus hex / r,g,b.
        assert_eq!(resolve_bg("navy"), Some(Color::Rgb(10, 40, 140)));
        assert_eq!(resolve_bg("crmaine"), Some(Color::Rgb(140, 15, 85)));
        assert_eq!(resolve_bg("#402018"), Some(Color::Rgb(0x40, 0x20, 0x18)));
        assert_eq!(resolve_bg("40,24,24"), Some(Color::Rgb(40, 24, 24)));
        assert_eq!(resolve_bg("default"), None);
        assert_eq!(resolve_bg("nonsense"), None);
    }

    #[test]
    fn broadcast_needs_more_than_one_pane() {
        // With no split panes, synchronize can't turn on (it would be pointless
        // and dangerous), and the toggle is a no-op.
        let mut app = {
            let (_d, a) = app_with(&["a.txt"]);
            a
        };
        assert!(!app.shell.set_broadcast(true), "no panes → stays off");
        assert!(!app.shell.is_broadcasting());
        assert!(!app.shell.toggle_broadcast(), "toggle is a no-op with <2 panes");
    }

    #[test]
    fn the_macro_launcher_opens_and_starts_a_run() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Start from a known-empty set (the dev machine may have a real macro.lua).
        app.macros.clear();
        app.macro_error = None;
        // No macros defined → `@` explains rather than opening an empty menu.
        app.handle_key(key('@')).unwrap();
        assert!(matches!(app.popup, Popup::None), "no empty menu");
        assert!(app.message.as_deref().unwrap_or("").contains("macro"));

        // Inject a couple of macros (as if loaded from macro.lua).
        app.macros = cian_lua::macros::parse(
            r#"return {
                { name = "First",  panes = { { cmd = "echo one" } } },
                { name = "Second", panes = { { cmd = "echo two" }, { dir = "down", cmd = "echo three" } } },
            }"#,
        )
        .unwrap();

        // `@` now opens the launcher listing both names.
        app.handle_key(key('@')).unwrap();
        match &app.popup {
            Popup::Macros { names, cursor } => {
                assert_eq!(names, &["First".to_string(), "Second".to_string()]);
                assert_eq!(*cursor, 0);
            }
            _ => panic!("launcher did not open"),
        }

        // Move to the second and run it: the run starts and focus moves to the shell.
        app.handle_key(key('j')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::None), "launcher closed on run");
        assert!(app.macro_run.is_some(), "a macro run is in progress");
        assert_eq!(app.focused, FocusedPane::Shell, "shell focused for the macro");
    }

    #[test]
    fn edit_queues_the_file_for_the_external_editor() {
        let (_d, mut app) = app_with(&["note.txt"]);
        // Put the cursor on the file (index 0 may be the synthetic `..`).
        {
            let p = app.active_pane_mut().unwrap();
            p.cursor = p.entries.iter().position(|e| e.name == "note.txt").unwrap();
        }
        // `:edit` on a file queues it (the main loop runs the editor).
        app.edit_selected_file();
        match &app.pending_edit {
            Some(e) => {
                assert!(e.path.ends_with("note.txt"));
                assert!(!e.reopen_viewer, ":edit does not re-open the viewer");
            }
            None => panic!("edit was not queued"),
        }

        // From the F3 viewer, `E` queues it and asks to re-open the viewer after.
        app.pending_edit = None;
        app.handle_key(code(KeyCode::F(3))).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('E'), KeyModifiers::SHIFT)).unwrap();
        let e = app.pending_edit.as_ref().expect("viewer edit queued");
        assert!(e.reopen_viewer, "viewer edit re-opens the viewer");
        assert!(matches!(app.popup, Popup::None), "viewer stepped aside");
    }

    /// Spin the op-job worker to completion (bulk copy/zip/extract run threaded).
    fn drain_op_job(app: &mut App) {
        for _ in 0..400 {
            if app.op_job.is_none() {
                return;
            }
            app.poll_op_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("op job did not finish");
    }

    #[test]
    fn unzip_extracts_into_a_named_subfolder() {
        let (d, mut app) = app_with(&[]);
        // Build a real zip in the pane's directory.
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut prog = |_: &cian_core::progress::Progress| {};
        let mut ctl = cian_core::progress::Ctl { cancel: &cancel, on_progress: &mut prog };
        std::fs::write(d.path().join("payload.txt"), b"inside the zip").unwrap();
        let archive = d.path().join("bundle.zip");
        cian_core::archive::create_zip(&[d.path().join("payload.txt")], &archive, None, &mut ctl);

        app.reload_both();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "bundle.zip").unwrap();
        app.extract_selected();
        drain_op_job(&mut app);

        // Extracted into ./bundle/ next to the archive.
        let extracted = d.path().join("bundle").join("payload.txt");
        assert!(extracted.is_file(), "payload extracted: {:?}", extracted);
        assert_eq!(std::fs::read_to_string(extracted).unwrap(), "inside the zip");
    }

    #[test]
    fn encrypted_zip_lists_on_f3_and_extracts_after_a_password() {
        let (d, mut app) = app_with(&[]);
        // Build an AES zip in the pane's directory.
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut prog = |_: &cian_core::progress::Progress| {};
        let mut ctl = cian_core::progress::Ctl { cancel: &cancel, on_progress: &mut prog };
        std::fs::write(d.path().join("secret.txt"), b"top secret").unwrap();
        let archive = d.path().join("locked.zip");
        cian_core::archive::create_zip(&[d.path().join("secret.txt")], &archive, Some("hunter2"), &mut ctl);

        app.reload_both();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "locked.zip").unwrap();

        // F3 lists the members (no more garbled hex dump).
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert!(matches!(app.popup, Popup::Archive { .. }), "F3 shows the archive listing, got {:?}", app.popup);
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // Extract asks for the password first.
        app.extract_selected();
        assert!(
            matches!(&app.popup, Popup::TextInput { kind: InputKind::ExtractPassword { .. }, .. }),
            "encrypted extract prompts for a password"
        );
        // The wrong password extracts nothing; the right one yields the file.
        if let Popup::TextInput { buffer, .. } = &mut app.popup {
            buffer.push_str("hunter2");
        }
        app.finish_text_input().unwrap();
        drain_op_job(&mut app);
        let got = d.path().join("locked").join("secret.txt");
        assert!(got.is_file(), "extracted with the password: {:?}", got);
        assert_eq!(std::fs::read_to_string(got).unwrap(), "top secret");
    }

    #[test]
    fn compress_menu_builds_a_zip() {
        let (d, mut app) = app_with(&["a.rs", "b.rs"]);
        // Mark both files, then run the Compress ▸ .zip flow.
        {
            let p = app.active_pane_mut().unwrap();
            for i in 0..p.entries.len() {
                if !p.entries[i].is_parent {
                    p.toggle_mark_at(i);
                }
            }
        }
        app.prompt_compress(CompressKind::Zip);
        // Type the archive name and submit.
        if let Popup::TextInput { buffer, .. } = &mut app.popup {
            buffer.clear();
            buffer.push_str("out");
        } else {
            panic!("no name prompt");
        }
        app.finish_text_input().unwrap();
        drain_op_job(&mut app);
        assert!(d.path().join("out.zip").is_file(), "out.zip created");
    }

    #[test]
    fn compress_menu_password_zip_chains_to_a_password_prompt() {
        let (d, mut app) = app_with(&["a.rs"]);
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "a.rs").unwrap();
        // Encrypted-zip flow: name prompt → password prompt → build.
        app.prompt_compress(CompressKind::ZipEnc);
        if let Popup::TextInput { buffer, .. } = &mut app.popup {
            buffer.clear();
            buffer.push_str("safe");
        } else {
            panic!("no name prompt");
        }
        app.finish_text_input().unwrap();
        assert!(
            matches!(&app.popup, Popup::TextInput { kind: InputKind::ZipPassword { .. }, .. }),
            "the name prompt chains into a password prompt"
        );
        if let Popup::TextInput { buffer, .. } = &mut app.popup {
            buffer.push_str("hunter2");
        }
        app.finish_text_input().unwrap();
        drain_op_job(&mut app);
        let out = d.path().join("safe.zip");
        assert!(out.is_file(), "safe.zip created");
        assert!(cian_core::archive::zip_needs_password(&out), "it is encrypted");
    }

    #[test]
    fn f3_on_an_image_opens_the_half_block_preview() {
        let (d, mut app) = app_with(&[]);
        // A small PNG in the pane's directory.
        let mut img = image::RgbImage::new(20, 12);
        for px in img.pixels_mut() {
            *px = image::Rgb([30, 160, 90]);
        }
        img.save(d.path().join("pic.png")).unwrap();
        app.reload_both();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "pic.png").unwrap();

        // F3 opens the image preview, not the hex/text viewer.
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert!(matches!(app.popup, Popup::ImageView { .. }), "image preview opened, got {:?}", app.popup);
        // Rendering decodes and caches a thumbnail sized to the box.
        let _ = render(&mut app, 80, 24);
        match &app.popup {
            Popup::ImageView { shown: Some((_, _, t)), error: None, .. } => {
                assert!(t.cols > 0 && t.rows > 0, "decoded to cells");
                assert_eq!((t.src_w, t.src_h), (20, 12));
            }
            other => panic!("no cached thumbnail: {:?}", other),
        }
        // Esc closes.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn count_reports_files_and_steps() {
        let (d, mut app) = app_with(&[]);
        std::fs::write(d.path().join("a.rs"), "fn main() {}\n\n// note\nlet x = 1;\n").unwrap();
        std::fs::write(d.path().join("b.rs"), "let y = 2;\n").unwrap();
        std::fs::write(d.path().join("skip.txt"), "not counted\n").unwrap();
        app.count_opts = cian_core::count::Options {
            extensions: vec!["rs".into()],
            ..Default::default()
        };
        // Reload, then mark the two .rs files: `:count` counts the marked
        // entries (or, unmarked, the one under the cursor) — not the whole dir.
        app.reload_both();
        {
            let p = app.active_pane_mut().unwrap();
            for i in 0..p.entries.len() {
                if p.entries[i].name.ends_with(".rs") {
                    p.toggle_mark_at(i);
                }
            }
        }
        app.start_count();
        assert!(app.count_job.is_some(), "count started on a worker");

        // Wait for the worker, then let poll install the report.
        for _ in 0..200 {
            if app.poll_count() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        match &app.popup {
            Popup::Notice { lines } => {
                let text = lines.join("\n");
                assert!(text.contains("2"), "two rs files: {text}");
                assert!(text.to_lowercase().contains("step"), "shows a step line: {text}");
                assert!(!text.contains("not counted"), "txt excluded");
            }
            _ => panic!("no count notice: {:?}", app.popup),
        }
    }

    #[test]
    fn count_targets_the_cursor_not_the_whole_directory() {
        let (d, mut app) = app_with(&[]);
        // A subdirectory with one file, plus a sibling file that must NOT count.
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/inner.rs"), "let a = 1;\nlet b = 2;\n").unwrap();
        std::fs::write(d.path().join("outside.rs"), "let c = 3;\n").unwrap();
        app.count_opts = cian_core::count::Options { extensions: vec!["rs".into()], ..Default::default() };
        app.reload_both();
        // Cursor on the `sub` folder (nothing marked) → count walks just it.
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "sub").unwrap();
        app.start_count();
        for _ in 0..200 {
            if app.poll_count() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        if let Popup::Notice { lines } = &app.popup {
            let text = lines.join("\n");
            // 1 file, 2 code lines from sub/inner.rs; outside.rs excluded.
            assert!(text.contains("2") && !text.contains('3'), "counted only the cursor's dir: {text}");
        } else {
            panic!("no count notice");
        }
    }

    #[test]
    fn a_macro_can_be_started_by_name() {
        // Backs the `--macro-name` startup option.
        let (_d, mut app) = app_with(&["a.txt"]);
        app.macros = cian_lua::macros::parse(
            r#"return { { name = "Deploy", panes = { { cmd = "echo go" } } } }"#,
        )
        .unwrap();
        assert!(!app.start_macro_by_name("Nope"), "unknown name is rejected");
        assert!(app.macro_run.is_none());
        assert!(app.start_macro_by_name("Deploy"), "known name starts");
        assert!(app.macro_run.is_some());
    }

    #[test]
    fn ai_chat_round_trips_a_mock_reply() {
        let have_py = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        assert!(app.ai.is_some(), "AI configured");
        app.ai_ready = Some(true); // the probe is async; treat mock as ready

        app.open_ai_chat();
        assert!(matches!(app.popup, Popup::AiChat { .. }), "chat opened (mock is available)");
        if let Popup::AiChat { input, .. } = &mut app.popup {
            *input = "hello".into();
        }
        app.send_ai_message();
        // Wait for the worker's reply.
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        match &app.popup {
            Popup::AiChat { log, .. } => {
                assert!(log.iter().any(|m| m.user && m.text == "hello"), "user turn recorded");
                assert!(
                    log.iter().any(|m| !m.user && m.text.contains("[mock] hello")),
                    "assistant echoed via the mock helper: {:?}",
                    log
                );
            }
            other => panic!("expected the chat, got {:?}", other),
        }
    }

    #[test]
    fn ai_chat_copy_uses_selection_then_last_reply() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::AiChat {
            input: String::new(),
            log: vec![
                ChatMsg { user: true, text: "hi".into() },
                ChatMsg { user: false, text: "the answer\nline two".into() },
            ],
            scroll: 0,
            pending: false,
            sel: Some((0, 1)),
        };
        // A selection copies those flat lines (as the draw would have populated).
        app.ai_lines = vec!["one".into(), "two".into(), "three".into()];
        app.copy_ai_text();
        assert_eq!(app.message.as_deref(), Some("copied"));
        assert!(matches!(app.popup, Popup::AiChat { sel: None, .. }), "selection cleared");

        // With no selection, it copies the last assistant reply.
        app.copy_ai_text();
        assert_eq!(app.message.as_deref(), Some("copied"));
    }

    #[test]
    fn clean_ai_command_strips_fences_and_prose() {
        assert_eq!(clean_ai_command("ls -la"), "ls -la");
        assert_eq!(clean_ai_command("```sh\nls -la\n```"), "ls -la");
        assert_eq!(clean_ai_command("`git status`"), "git status");
        assert_eq!(clean_ai_command("\n\n  find . -name '*.log'  \n"), "find . -name '*.log'");
    }

    /// The F3 viewer shows a git change bar for lines that differ from HEAD.
    #[test]
    fn the_viewer_shows_a_git_change_bar() {
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok {
            eprintln!("no git; skipping");
            return;
        }
        for kv in [["user.email", "t@e.com"], ["user.name", "T"], ["core.autocrlf", "false"]] {
            let _ = std::process::Command::new("git").arg("-C").arg(&dir).args(["config", kv[0], kv[1]]).status();
        }
        let f = dir.join("code.txt");
        std::fs::write(&f, "keep\nold\nkeep2\n").unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["add", "."]).status().unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["commit", "-qm", "init"]).status().unwrap();
        std::fs::write(&f, "keep\nNEW\nkeep2\n").unwrap();

        let mut app = App::new(dir.clone(), dir.clone(), cian_lua::Config::default()).unwrap();
        app.open_viewer_at(&f, "code.txt", 0);
        // The map was computed for the modified file.
        let Popup::Viewer { git_lines, .. } = &app.popup else { panic!("no viewer") };
        assert_eq!(git_lines.get(&1), Some(&cian_core::git::LineChange::Modified), "line 2 modified");
        // And the change bar renders on screen.
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains('▏'), "change bar shown:\n{screen}");
    }

    /// The status line shows the repo's branch when the pane is in one.
    #[test]
    fn the_status_line_shows_the_git_branch() {
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q", "-b", "trunk"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok {
            eprintln!("no git (or too old for -b); skipping");
            return;
        }
        std::fs::write(dir.join("a.txt"), "x").unwrap();
        let mut app = App::new(dir.clone(), dir, cian_lua::Config::default()).unwrap();
        let screen = render(&mut app, 120, 30).join("\n");
        assert!(screen.contains("trunk"), "branch shown in the status line:\n{screen}");
    }

    /// Stage / unstage / discard through the app on a real throwaway repo.
    #[test]
    fn git_stage_unstage_and_discard_operate_on_the_selection() {
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let git_ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !git_ok {
            eprintln!("no git; skipping");
            return;
        }
        for kv in [["user.email", "t@example.com"], ["user.name", "Test"], ["core.autocrlf", "false"]] {
            let _ = std::process::Command::new("git").arg("-C").arg(&dir).args(["config", kv[0], kv[1]]).status();
        }
        // Commit an initial file so we have a tracked file to modify/discard.
        std::fs::write(dir.join("tracked.txt"), "one\n").unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["add", "."]).status().unwrap();
        std::process::Command::new("git").arg("-C").arg(&dir).args(["commit", "-qm", "init"]).status().unwrap();
        std::fs::write(dir.join("tracked.txt"), "one\ntwo\n").unwrap();

        let mut app = App::new(dir.clone(), dir.clone(), cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40); // computes git status
        // Cursor onto tracked.txt (index 0 is `..`).
        let idx = app.active_pane().unwrap().entries.iter()
            .position(|e| e.name == "tracked.txt").unwrap();
        app.active_pane_mut().unwrap().cursor = idx;

        // Stage: the worktree change becomes staged.
        app.git_stage();
        let st = cian_core::git::status(&dir).unwrap();
        assert_eq!(st.mark_for(&dir.join("tracked.txt")), Some(cian_core::git::GitMark::Staged));

        // Unstage: back to a plain worktree modification.
        app.git_stage(); // (re-stage to ensure state)
        app.git_unstage();
        let st = cian_core::git::status(&dir).unwrap();
        assert_eq!(st.mark_for(&dir.join("tracked.txt")), Some(cian_core::git::GitMark::Modified));

        // Discard: confirm dialog, then the change is gone.
        let _ = render(&mut app, 100, 40);
        app.active_pane_mut().unwrap().cursor = idx;
        app.git_discard_prompt();
        assert!(matches!(app.popup, Popup::ConfirmDiscard { .. }), "discard confirms first");
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join("tracked.txt")).unwrap(), "one\n",
            "worktree change reverted");
    }

    #[test]
    fn parse_junk_reply_validates_names_and_strips_prose() {
        let names = vec![
            ("target".to_string(), PathBuf::from("/p/target")),
            ("main.rs".to_string(), PathBuf::from("/p/main.rs")),
            (".DS_Store".to_string(), PathBuf::from("/p/.DS_Store")),
        ];
        // Fenced, with prose around it, and a hallucinated name that must be dropped.
        let raw = "Here is the junk:\n```json\n[\
            {\"name\":\"target\",\"reason\":\"build output\"},\
            {\"name\":\".DS_Store\",\"reason\":\"macOS cruft\"},\
            {\"name\":\"nonexistent\",\"reason\":\"made up\"}\
            ]\n```\n";
        let items = parse_junk_reply(raw, &names);
        let got: Vec<&str> = items.iter().map(|i| i.path.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(got, vec!["target", ".DS_Store"], "only shown names survive");
        assert!(items.iter().all(|i| i.selected), "candidates start checked");
        assert_eq!(items[0].reason, "build output");
        // Never flags source — it just isn't in the reply, and couldn't be added.
        assert!(!got.contains(&"main.rs"));
    }

    #[test]
    fn parse_junk_reply_empty_or_garbage_is_no_items() {
        let names = vec![("x".to_string(), PathBuf::from("/p/x"))];
        assert!(parse_junk_reply("[]", &names).is_empty());
        assert!(parse_junk_reply("I could not find any junk.", &names).is_empty());
    }

    /// The whole duplicate flow: scan a dir with two identical files, wait for
    /// the worker, and check the review pre-selects the redundant copy.
    #[test]
    fn dupe_scan_finds_copies_and_preselects_all_but_one() {
        let (d, mut app) = app_with(&["one.txt", "two.txt", "unique.txt"]);
        std::fs::write(d.path().join("one.txt"), b"same bytes here").unwrap();
        std::fs::write(d.path().join("two.txt"), b"same bytes here").unwrap();
        std::fs::write(d.path().join("unique.txt"), b"different").unwrap();
        app.reload_active();

        app.start_dupes();
        assert!(app.dupes_job.is_some(), "scan running on a worker");
        let start = Instant::now();
        while app.dupes_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_dupes_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        let Popup::DupeReview { items, .. } = &app.popup else {
            panic!("expected the dupe review, got {:?}", app.popup)
        };
        // Two identical files → one group of two; exactly one is pre-checked.
        assert_eq!(items.len(), 2, "the duplicate pair (unique.txt omitted)");
        assert_eq!(items.iter().filter(|i| i.selected).count(), 1, "keep one, check the other");
        assert_eq!(items.iter().filter(|i| i.keeper).count(), 1);

        // Approving hands the checked copy to the delete confirmation.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        match &app.popup {
            Popup::ConfirmDelete { targets } => assert_eq!(targets.len(), 1),
            other => panic!("expected delete confirm, got {:?}", other),
        }
    }

    #[test]
    fn junk_review_approval_routes_checked_paths_to_delete_confirm() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::JunkReview {
            items: vec![
                JunkItem { path: PathBuf::from("/p/target"), reason: "build".into(), selected: true },
                JunkItem { path: PathBuf::from("/p/keep"), reason: "".into(), selected: false },
                JunkItem { path: PathBuf::from("/p/cache"), reason: "cache".into(), selected: true },
            ],
            cursor: 0,
            scroll: 0,
        };
        // Enter approves: only the checked ones go to the delete confirmation.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        match &app.popup {
            Popup::ConfirmDelete { targets } => {
                assert_eq!(targets, &vec![PathBuf::from("/p/target"), PathBuf::from("/p/cache")]);
            }
            other => panic!("expected the delete confirm, got {:?}", other),
        }
    }

    #[test]
    fn junk_review_space_toggles_and_a_selects_all() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::JunkReview {
            items: vec![
                JunkItem { path: PathBuf::from("/p/1"), reason: String::new(), selected: true },
                JunkItem { path: PathBuf::from("/p/2"), reason: String::new(), selected: true },
            ],
            cursor: 0,
            scroll: 0,
        };
        // Space unchecks the first.
        app.handle_key(code(KeyCode::Char(' '))).unwrap();
        // `a` toggles all: since not all are on, it turns all on.
        app.handle_key(code(KeyCode::Char('a'))).unwrap();
        if let Popup::JunkReview { items, .. } = &app.popup {
            assert!(items.iter().all(|i| i.selected), "a turned everything on");
        } else {
            panic!("popup changed");
        }
    }

    #[test]
    fn parse_sem_search_reply_matches_orders_and_folds_reasons() {
        let hit = |rel: &str| cian_core::search::Hit {
            path: PathBuf::from("/root").join(rel),
            rel: PathBuf::from(rel),
            is_dir: false,
            line: None,
        };
        let catalog = vec![hit("src/db.rs"), hit("README.md"), hit("src/ui.rs")];
        // Ranked: ui first, then db; a made-up path is dropped.
        let raw = "```json\n[\
            {\"path\":\"src/ui.rs\",\"reason\":\"UI code\"},\
            {\"path\":\"src/db.rs\",\"reason\":\"database layer\"},\
            {\"path\":\"nope.rs\",\"reason\":\"invented\"}\
            ]\n```";
        let out = parse_sem_search_reply(raw, &catalog);
        let rels: Vec<String> = out.iter().map(|h| h.rel.display().to_string()).collect();
        assert_eq!(rels, vec!["src/ui.rs", "src/db.rs"], "kept order, dropped the invented path");
        // The reason is folded into the line so the list shows it and Enter previews.
        assert_eq!(out[0].line.as_ref().map(|(n, t)| (*n, t.as_str())), Some((1, "UI code")));
    }

    #[test]
    fn ai_search_builds_a_catalog_and_fires_a_request() {
        let have_py = std::process::Command::new("python3")
            .arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("src")).unwrap();
        std::fs::write(d.path().join("src/db.rs"), b"x").unwrap();
        std::fs::write(d.path().join("README.md"), b"x").unwrap();
        let p = d.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(), auth_mode: "mock".into(), ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();

        app.start_ai_search("the database code");
        assert!(app.ai_job.is_some(), "a request was fired over the catalog");
        // The mock echoes (not JSON), so the pipeline reports no matches.
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.message.as_deref().unwrap_or("").contains("no relevant"),
            "mock reply parses to no matches: {:?}", app.message);
    }

    #[test]
    fn clean_filename_rejects_paths_and_specials() {
        assert_eq!(clean_filename(" report_v2.txt "), Some("report_v2.txt".to_string()));
        assert_eq!(clean_filename("a/b.txt"), None);
        assert_eq!(clean_filename("a\\b.txt"), None);
        assert_eq!(clean_filename(".."), None);
        assert_eq!(clean_filename("."), None);
        assert_eq!(clean_filename(""), None);
        assert_eq!(clean_filename("C:evil"), None);
    }

    #[test]
    fn parse_rename_reply_validates_and_dedupes() {
        let names = vec![
            ("IMG_1.jpg".to_string(), PathBuf::from("/p/IMG_1.jpg")),
            ("IMG_2.jpg".to_string(), PathBuf::from("/p/IMG_2.jpg")),
            ("keep.txt".to_string(), PathBuf::from("/p/keep.txt")),
        ];
        let raw = "[\
            {\"name\":\"IMG_1.jpg\",\"new_name\":\"photo_01.jpg\"},\
            {\"name\":\"IMG_2.jpg\",\"new_name\":\"../escape.jpg\"},\
            {\"name\":\"keep.txt\",\"new_name\":\"keep.txt\"},\
            {\"name\":\"ghost\",\"new_name\":\"x.jpg\"}\
            ]";
        let items = parse_rename_reply(raw, &names);
        // Only IMG_1 survives: IMG_2's target escapes, keep is a no-op, ghost unknown.
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].old, "IMG_1.jpg");
        assert_eq!(items[0].new, "photo_01.jpg");
    }

    /// The whole rename flow: build the review popup and approve — the checked
    /// file is renamed in place, the unchecked left alone.
    #[test]
    fn rename_plan_renames_checked_files() {
        let (d, mut app) = app_with(&["IMG_1.jpg", "keep.txt"]);
        app.popup = Popup::RenameReview {
            items: vec![
                RenameItem { path: d.path().join("IMG_1.jpg"), old: "IMG_1.jpg".into(),
                    new: "photo_01.jpg".into(), selected: true },
                RenameItem { path: d.path().join("keep.txt"), old: "keep.txt".into(),
                    new: "notes.txt".into(), selected: false },
            ],
            cursor: 0,
            scroll: 0,
        };
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(d.path().join("photo_01.jpg").is_file(), "renamed");
        assert!(!d.path().join("IMG_1.jpg").exists(), "old name gone");
        assert!(d.path().join("keep.txt").is_file(), "unchecked untouched");
        assert!(!d.path().join("notes.txt").exists());
    }

    #[test]
    fn truncate_text_for_ai_caps_and_handles_one_long_line() {
        let short = "a\nb\nc\n";
        assert_eq!(truncate_text_for_ai(short, 1000), short, "short text is unchanged");
        // A single line longer than the cap is cut on a char boundary.
        let long = "x".repeat(5000);
        let out = truncate_text_for_ai(&long, 100);
        assert!(out.len() < long.len() && out.contains("truncated"));
        // Multibyte: cutting must not split a char.
        let multi = "あ".repeat(2000);
        let out = truncate_text_for_ai(&multi, 100);
        assert!(out.starts_with("あ") && out.contains("truncated"));
    }

    /// Pressing `S` in the viewer sends the file's text and opens the chat with
    /// the reply (mock: an echo of the body).
    #[test]
    fn viewer_summarize_opens_the_chat_with_a_reply() {
        let have_py = std::process::Command::new("python3")
            .arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("readme.txt"), "hello world\nsecond line\n").unwrap();
        let p = d.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(), auth_mode: "mock".into(), ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.ai_ready = Some(true); // the probe is async; treat mock as ready
        app.active_pane_mut().unwrap().cursor = 1; // readme.txt (index 0 is `..`)
        let _ = render(&mut app, 100, 40);
        app.look_inside(); // open the F3 viewer
        assert!(matches!(app.popup, Popup::Viewer { .. }), "viewer open");
        let _ = render(&mut app, 100, 40);

        app.handle_key(code(KeyCode::Char('S'))).unwrap();
        assert!(matches!(app.popup, Popup::AiChat { .. }), "summarise opened the chat");
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        let Popup::AiChat { log, .. } = &app.popup else { panic!("chat closed") };
        assert!(log.iter().any(|m| !m.user && m.text.contains("hello world")),
            "the mock echoed the file text back as the summary: {log:?}");
    }

    #[test]
    fn clean_dest_folder_rejects_escapes() {
        assert_eq!(clean_dest_folder("images"), Some("images".to_string()));
        assert_eq!(clean_dest_folder(" docs/2023 "), Some("docs/2023".to_string()));
        assert_eq!(clean_dest_folder("a\\b"), Some("a/b".to_string()));
        // Anything that could escape the current directory is refused.
        assert_eq!(clean_dest_folder("../evil"), None);
        assert_eq!(clean_dest_folder("/abs"), None);
        assert_eq!(clean_dest_folder("C:/x"), None);
        assert_eq!(clean_dest_folder("a/../b"), None);
        assert_eq!(clean_dest_folder(""), None);
    }

    #[test]
    fn parse_structure_reply_validates_names_and_folders() {
        let names = vec![
            ("cat.jpg".to_string(), PathBuf::from("/p/cat.jpg")),
            ("notes.md".to_string(), PathBuf::from("/p/notes.md")),
        ];
        let raw = "```json\n[\
            {\"name\":\"cat.jpg\",\"folder\":\"images\",\"reason\":\"an image\"},\
            {\"name\":\"notes.md\",\"folder\":\"../escape\",\"reason\":\"bad folder\"},\
            {\"name\":\"ghost.txt\",\"folder\":\"docs\",\"reason\":\"not shown\"}\
            ]\n```";
        let items = parse_structure_reply(raw, &names);
        assert_eq!(items.len(), 1, "only the valid, real-name move survives");
        assert_eq!(items[0].name, "cat.jpg");
        assert_eq!(items[0].dest, "images");
        assert!(items[0].selected);
    }

    /// The whole structure flow: build a review popup by hand and approve it —
    /// the checked file is moved into a freshly created sub-folder.
    #[test]
    fn structure_plan_moves_checked_files_into_new_folders() {
        let (d, mut app) = app_with(&["cat.jpg", "keep.txt"]);
        let dir = app.active_pane().unwrap().cwd.clone();
        app.popup = Popup::StructureReview {
            items: vec![
                MoveItem { path: d.path().join("cat.jpg"), name: "cat.jpg".into(),
                    dest: "images".into(), reason: "image".into(), selected: true },
                MoveItem { path: d.path().join("keep.txt"), name: "keep.txt".into(),
                    dest: "docs".into(), reason: String::new(), selected: false },
            ],
            cursor: 0,
            scroll: 0,
            dir,
        };
        app.handle_key(code(KeyCode::Enter)).unwrap(); // run the checked moves
        drain_op(&mut app);
        assert!(d.path().join("images/cat.jpg").is_file(), "moved into the new folder");
        assert!(!d.path().join("cat.jpg").exists(), "gone from the root");
        // The unchecked one is left where it was, and its folder not created.
        assert!(d.path().join("keep.txt").is_file(), "unchecked stays put");
        assert!(!d.path().join("docs").exists(), "no folder for an unchecked move");
    }

    #[test]
    fn clean_ai_commit_message_strips_a_wrapping_fence() {
        assert_eq!(clean_ai_commit_message("feat: add x\n\n- why"), "feat: add x\n\n- why");
        assert_eq!(clean_ai_commit_message("```\nfix: bug\n```"), "fix: bug");
        assert_eq!(clean_ai_commit_message("\n\n  chore: tidy  \n\n"), "chore: tidy");
    }

    #[test]
    fn truncate_diff_for_ai_caps_on_a_line_boundary() {
        let big = "line one\nline two\nline three\n".repeat(100);
        let out = truncate_diff_for_ai(&big, 40);
        assert!(out.len() < big.len());
        assert!(out.contains("truncated"), "marks the cut: {out:?}");
        // Only whole lines are kept before the marker.
        let before_marker = out.split("\n\n[").next().unwrap();
        assert!(before_marker.split('\n').all(|l| l.is_empty() || big.contains(l)));
    }

    /// The whole commit-message flow with a throwaway repo: draft (mock), edit,
    /// and commit — then the message is in the log and the stage is clean.
    #[test]
    fn ai_commit_message_flow_drafts_edits_and_commits() {
        let have_py = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let git_ok = std::process::Command::new("git")
            .arg("-C").arg(&dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !git_ok {
            eprintln!("no git; skipping");
            return;
        }
        for kv in [["user.email", "t@example.com"], ["user.name", "Test"]] {
            let _ = std::process::Command::new("git").arg("-C").arg(&dir).args(["config", kv[0], kv[1]]).status();
        }
        std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
        cian_core::git::stage(&dir, &[dir.join("a.txt")]).unwrap();

        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(dir.clone(), dir.clone(), config).unwrap();
        app.ai_ready = Some(true); // the probe is async; treat mock as ready

        app.start_ai_commit_message();
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(matches!(app.popup, Popup::CommitMessage { .. }), "draft popup, got {:?}", app.popup);

        // Replace the drafted text with our own: e → edit, clear, type.
        app.handle_key(key('e')).unwrap();
        if let Popup::CommitMessage { buffer, .. } = &mut app.popup {
            buffer.clear();
        }
        for c in "add a.txt".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Esc)).unwrap(); // leave edit mode
        app.handle_key(code(KeyCode::Enter)).unwrap(); // commit

        assert!(matches!(app.popup, Popup::None), "committed, popup closed: {:?}", app.popup);
        assert_eq!(cian_core::git::staged_diff(&dir).as_deref(), Some(""), "stage is clean");
        let log = std::process::Command::new("git").arg("-C").arg(&dir).args(["log", "-1", "--pretty=%s"]).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&log.stdout).trim(), "add a.txt");
    }

    #[test]
    fn ai_shell_command_flow_yields_a_confirm_popup() {
        let have_py = std::process::Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !have_py {
            eprintln!("no python3; skipping");
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = cian_lua::Config::default();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();

        app.start_ai_shell_cmd("compress the logs");
        // Wait for the worker; the mock echoes the request as the "command".
        let start = Instant::now();
        while app.ai_job.is_some() && start.elapsed() < Duration::from_secs(10) {
            app.poll_ai_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        match &app.popup {
            Popup::AiShellConfirm { command } => {
                assert!(command.contains("compress the logs"), "got {command:?}");
            }
            other => panic!("expected the command-confirm popup, got {:?}", other),
        }
    }

    #[test]
    fn the_context_menu_drills_into_submenus_and_back() {
        // With SSH hosts, the file menu offers a "Transfer ▸" group.
        let (_d, mut app) = app_with_ssh();
        app.open_context_menu(5, 5);
        let has_group = matches!(&app.popup, Popup::ContextMenu { items, .. } if items.contains(&MenuItem::SendMenu));
        assert!(has_group, "file menu has a Transfer group");

        // Drill in: the submenu shows the SFTP actions and a Back item.
        app.run_menu_item(MenuItem::SendMenu).unwrap();
        match &app.popup {
            Popup::ContextMenu { items, .. } => {
                assert!(items.contains(&MenuItem::ScpUpload));
                assert!(items.contains(&MenuItem::Back));
            }
            other => panic!("expected the submenu, got {:?}", other),
        }
        assert_eq!(app.menu_stack.len(), 1, "parent stashed");

        // Back returns to the parent menu, not to nothing.
        app.run_menu_item(MenuItem::Back).unwrap();
        let back_at_parent = matches!(&app.popup, Popup::ContextMenu { items, .. } if items.contains(&MenuItem::SendMenu));
        assert!(back_at_parent, "Back climbed to the parent");
        assert!(app.menu_stack.is_empty());
    }

    #[test]
    fn ai_chat_is_silent_without_config() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert!(app.ai.is_none());
        app.open_ai_chat();
        assert!(matches!(app.popup, Popup::None), "no chat without cian.ai config");
        assert!(app.message.as_deref().unwrap_or("").contains("not configured"));
    }

    #[test]
    fn glob_match_handles_stars_and_question_marks() {
        assert!(glob_match("*.rs", "main.rs"));
        assert!(glob_match("*.rs", ".rs"));
        assert!(!glob_match("*.rs", "main.rst"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("test_*", "test_foo"));
        assert!(!glob_match("test_*", "footest"));
        assert!(glob_match("a*b*c", "axxbyyc"));
    }

    #[test]
    fn mark_command_marks_matching_entries() {
        let (_d, mut app) = app_with(&["a.rs", "b.rs", "c.txt", "readme.md"]);
        app.command_buffer = "mark *.rs".into();
        app.run_command();
        assert_eq!(app.active_pane().unwrap().mark_count(), 2, "two .rs marked");
        // Unmark one class, then all.
        app.command_buffer = "unmark *.rs".into();
        app.run_command();
        assert_eq!(app.active_pane().unwrap().mark_count(), 0);
    }

    #[test]
    fn a_permission_error_explains_admin_rights() {
        let (_d, mut app) = app_with(&["a.rs"]);
        let mut report = OpReport { permission_denied: true, ..Default::default() };
        report.note_error("C:/Program Files/x: Access is denied (os error 5)");
        app.show_op_report(&report);
        let Popup::Notice { lines } = &app.popup else { panic!("expected a notice") };
        assert!(
            lines.iter().any(|l| l.contains("administrator rights")),
            "the notice names the cause: {lines:?}"
        );
    }

    #[test]
    fn a_user_keymap_rebinds_and_disables_keys() {
        let (_d, mut app) = app_with_keymaps(
            &["a.rs", "b.rs"],
            vec![
                ('x', "delete".into()), // bind a new key to an action
                ('d', "none".into()),   // and turn the default off
            ],
        );
        // `x` now opens the delete confirm…
        app.handle_key(key('x')).unwrap();
        assert!(matches!(app.popup, Popup::ConfirmDelete { .. }), "x deletes");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        // …while the disabled `d` does nothing.
        app.handle_key(key('d')).unwrap();
        assert!(matches!(app.popup, Popup::None), "d is unbound");
    }

    #[test]
    fn every_action_named_in_the_example_config_resolves() {
        // Guards against the docs drifting from the code: each
        // `set_keymap("k", "action")` in examples/init.lua must name a real
        // action, so a user copying a line always gets a working binding.
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/init.lua");
        let text = std::fs::read_to_string(path).expect("read examples/init.lua");
        let mut checked = 0;
        for line in text.lines() {
            // Only the real binding lines (`cian.set_keymap("k", "action")`),
            // not the "key"/"action" placeholder in the section header or the
            // prose examples that have text before the call.
            let trimmed = line.trim_start_matches(['-', ' ']);
            if !trimmed.starts_with("cian.set_keymap(") {
                continue;
            }
            let Some(rest) = trimmed.split_once("set_keymap(").map(|(_, r)| r) else { continue };
            // The action is the second quoted string on the line.
            let quoted: Vec<&str> = rest.split('"').collect();
            if quoted.len() >= 4 {
                let action = quoted[3];
                assert!(
                    action_from_name(action).is_some(),
                    "examples/init.lua names unknown action {:?}",
                    action
                );
                checked += 1;
            }
        }
        assert!(checked > 20, "expected to have checked the documented bindings, got {checked}");
    }

    #[test]
    fn reload_reapplies_the_keymap_live() {
        let (_d, mut app) = app_with(&["a.rs"]);
        // No user binding yet: `x` is not delete.
        assert!(!app.keymap.contains_key(&'x'));
        // Point CIAN_CONFIG_DIR at a temp config that binds x -> delete, then
        // reload — the running app should pick it up without a restart.
        let cfgdir = tempfile::tempdir().unwrap();
        std::fs::write(
            cfgdir.path().join("init.lua"),
            "cian.set_keymap(\"x\", \"delete\")\n",
        )
        .unwrap();
        std::env::set_var("CIAN_CONFIG_DIR", cfgdir.path());
        app.command_buffer = "reload".into();
        app.run_command();
        std::env::remove_var("CIAN_CONFIG_DIR");
        assert_eq!(app.keymap.get(&'x'), Some(&Action::Delete), "reload bound x live");
    }

    #[test]
    fn a_newly_named_action_is_bindable() {
        // `sort` had no bindable name before; confirm it now resolves and works.
        assert_eq!(action_from_name("sort"), Some(Action::Sort));
        let (_d, mut app) = app_with_keymaps(&["a.rs"], vec![('S', "sort".into())]);
        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::SortPicker { .. }), "S opens the sort picker");
    }

    /// Render and hand back the raw buffer, for checking colors.
    fn render_buf(app: &mut App, w: u16, h: u16) -> ratatui::buffer::Buffer {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Render `app` onto a `w`x`h` test terminal and return the text of each row.
    fn render(app: &mut App, w: u16, h: u16) -> Vec<String> {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        (0..buf.area.height)
            .map(|y| {
                (0..buf.area.width)
                    .map(|x| buf[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// Click the centre of the first popup zone matching `want`, after a render
    /// has registered the zones. Returns false if no such zone exists.
    fn click_zone(app: &mut App, want: ZoneKind) -> bool {
        let hit = app.popup_zones.iter().find(|z| z.kind == want).map(|z| z.rect);
        match hit {
            Some(r) => {
                app.handle_mouse(mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    r.x + r.width / 2,
                    r.y,
                ));
                true
            }
            None => false,
        }
    }

    #[test]
    fn the_wheel_scrolls_the_file_pane_under_the_pointer() {
        let names: Vec<String> = (0..40).map(|i| format!("f{:02}.txt", i)).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let (_d, mut app) = app_with(&refs);
        let _ = render(&mut app, 100, 40);
        let start = app.active_pane().unwrap().cursor;
        let left = app.layout_rects.left;
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, left.x + 3, left.y + 3));
        let after = app.active_pane().unwrap().cursor;
        assert!(after > start, "wheel down moved the cursor down: {start} -> {after}");
        app.handle_mouse(mouse(MouseEventKind::ScrollUp, left.x + 3, left.y + 3));
        assert!(app.active_pane().unwrap().cursor < after, "wheel up moved it back up");
    }

    #[test]
    fn dragging_inside_a_pane_rubber_band_selects() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt", "d.txt"]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        // Row 1 is the `..` row; the files start on row 2. Press on the first
        // file, drag down two more, release inside the pane.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, left.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), left.x + 3, left.y + 4));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 3, left.y + 4));
        // The dragged-over range is now marked (3 files), not a copy to elsewhere.
        assert_eq!(app.active_pane().unwrap().mark_count(), 3, "range is marked");
        assert!(app.file_drag.is_none(), "drag released");
    }

    #[test]
    fn clicking_a_sort_picker_row_applies_it() {
        let (_d, mut app) = app_with(&["a.rs", "b.rs"]);
        app.start_sort_picker();
        assert!(matches!(app.popup, Popup::SortPicker { .. }));
        // Render so the row hit-zones are registered, then click the 3rd entry.
        let _ = render(&mut app, 100, 40);
        assert!(click_zone(&mut app, ZoneKind::SelectRow(2)), "row zone present");
        // A pick closes the picker and applies that key.
        assert!(matches!(app.popup, Popup::None), "picker closed after a click");
        assert_eq!(app.active_pane().unwrap().sort.key, SortKey::ALL[2]);
    }

    #[test]
    fn clicking_a_confirm_dialog_button_answers_it() {
        let (_d, mut app) = app_with(&["a.rs"]);
        app.start_quit_confirm();
        assert!(matches!(app.popup, Popup::ConfirmQuit));
        let _ = render(&mut app, 100, 40);
        // The "No" button cancels without quitting.
        assert!(click_zone(&mut app, ZoneKind::Esc), "No button present");
        assert!(matches!(app.popup, Popup::None));
        assert!(!app.should_quit);

        app.start_quit_confirm();
        let _ = render(&mut app, 100, 40);
        assert!(click_zone(&mut app, ZoneKind::Enter), "Yes button present");
        assert!(app.should_quit, "clicking Yes quits");
    }

    #[test]
    fn the_mouse_wheel_scrolls_the_manual() {
        let (_d, mut app) = app_with(&["a.rs"]);
        app.open_manual();
        let _ = render(&mut app, 100, 40);
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, 50, 20));
        app.handle_mouse(mouse(MouseEventKind::ScrollDown, 50, 20));
        let Popup::Manual { scroll, .. } = &app.popup else { panic!("expected manual") };
        assert_eq!(*scroll, 2, "two wheel notches scrolled two lines");
    }

    #[test]
    fn slash_filters_the_listing_incrementally() {
        let (_d, mut app) = app_with(&["alpha.rs", "beta.rs", "gamma.txt"]);
        // Counts include the synthetic `..` row, so a 3-file dir lists 4.
        assert_eq!(app.active_pane().unwrap().entries.len(), 4);

        app.handle_key(key('/')).unwrap();
        assert_eq!(app.mode, Mode::Filter);

        app.handle_key(key('r')).unwrap();
        app.handle_key(key('s')).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);

        // Backspace widens the match: "r" still excludes gamma.txt.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);

        // Emptying the buffer restores the full listing.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert_eq!(app.filter_buffer, "");
        assert_eq!(app.active_pane().unwrap().entries.len(), 4);
    }

    #[test]
    fn enter_keeps_the_filter_and_esc_clears_it() {
        let (_d, mut app) = app_with(&["a.txt", "b.md"]);

        app.handle_key(key('/')).unwrap();
        app.handle_key(key('m')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        // `..` plus the one match survives the filter.
        assert_eq!(app.active_pane().unwrap().entries.len(), 2, "filter should survive Enter");

        // Esc in normal mode drops the narrowing.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);
    }

    #[test]
    fn esc_while_filtering_restores_the_full_list() {
        let (_d, mut app) = app_with(&["a.txt", "b.md"]);
        app.handle_key(key('/')).unwrap();
        app.handle_key(key('m')).unwrap();
        assert_eq!(app.active_pane().unwrap().entries.len(), 2, "`..` plus the match");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.mode, Mode::Normal);
        assert_eq!(app.active_pane().unwrap().entries.len(), 3);
    }

    #[test]
    fn question_mark_opens_the_manual() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(key('?')).unwrap();
        assert!(matches!(app.popup, Popup::Manual { .. }));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn ctrl_dot_opens_the_manual() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Char('.'), KeyModifiers::CONTROL)).unwrap();
        assert!(matches!(app.popup, Popup::Manual { .. }));
    }

    /// Regression: the manual is ~50 lines, far taller than a normal terminal.
    /// Every line must be reachable by scrolling rather than silently clipped.
    #[test]
    fn manual_scrolls_to_reveal_its_last_section() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        app.handle_key(key('?')).unwrap();

        let top = render(&mut app, 100, 24).join("\n");
        assert!(top.contains("key manual"), "manual header should be visible");
        assert!(
            !top.contains("zoom active split pane"),
            "the last section cannot already fit on a 24-row terminal"
        );

        // G jumps to the bottom; the final section must now be on screen.
        app.handle_key(key('G')).unwrap();
        let bottom = render(&mut app, 100, 24).join("\n");
        assert!(
            bottom.contains("zoom active split pane"),
            "scrolling to the end must reveal the last section; got:\n{}",
            bottom
        );
    }

    #[test]
    fn manual_scroll_is_clamped_at_both_ends() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(key('?')).unwrap();

        // Scrolling up at the top is a no-op, not an underflow panic.
        for _ in 0..5 {
            app.handle_key(key('k')).unwrap();
        }
        let Popup::Manual { scroll, .. } = &app.popup else { panic!("expected manual") };
        assert_eq!(*scroll, 0);

        // Paging past the end settles on the last page after a render.
        for _ in 0..50 {
            app.handle_key(key('d')).unwrap();
        }
        let _ = render(&mut app, 100, 24);
        let Popup::Manual { scroll, lines } = &app.popup else { panic!("expected manual") };
        assert!(*scroll < lines.len(), "scroll must stay inside the document");
    }

    /// The manual reflects `init.lua` overrides rather than a hardcoded list.
    #[test]
    fn manual_lists_user_bound_keys() {
        let mut keymap = HashMap::new();
        keymap.insert('x', Action::Delete);
        let text = manual_lines(&keymap, Lang::En).join("\n");
        assert!(text.contains("d, x"), "user-bound key missing from manual:\n{}", text);
    }

    #[test]
    fn the_status_and_hints_default_to_english_and_switch_to_japanese() {
        // Default is English.
        let (_d, mut app) = app_with(&["a.txt"]);
        let en = render(&mut app, 110, 40).join("\n");
        assert!(en.contains("items") && en.contains("help"), "English chrome:\n{en}");

        // lang=ja renders the chrome in Japanese. A wide (CJK) glyph occupies
        // two cells, so the row reconstruction inserts a space after each; strip
        // spaces before matching the words.
        let flat = |app: &mut App| render(app, 110, 40).join("\n").replace(' ', "");
        let (_d2, mut ja) = app_with_lang(&["a.txt"], "ja");
        let screen = flat(&mut ja);
        assert!(screen.contains("件"), "status counts in Japanese:\n{screen}");
        assert!(screen.contains("ヘルプ"), "help hint in Japanese");
        ja.open_context_menu(5, 5);
        let menu = flat(&mut ja);
        assert!(menu.contains("コピー"), "menu in Japanese:\n{menu}");
    }

    #[test]
    fn the_manual_defaults_to_english_and_switches_to_japanese() {
        let keymap = HashMap::new();
        let en = manual_lines(&keymap, Lang::En).join("\n");
        assert!(en.contains("key manual"), "English header");
        assert!(en.contains("delete (to trash)"), "English description present");
        let ja = manual_lines(&keymap, Lang::Ja).join("\n");
        assert!(ja.contains("キー一覧"), "Japanese header:\n{ja}");
        assert!(ja.contains("削除（ゴミ箱へ）"), "Japanese description present");

        // The `lang` option drives which one an App shows.
        let (_d, app_en) = app_with(&["a.rs"]);
        assert_eq!(app_en.lang, Lang::En, "default is English");
        let (_d2, app_ja) = app_with_lang(&["a.rs"], "ja");
        assert_eq!(app_ja.lang, Lang::Ja, "lang=ja switches to Japanese");
    }

    #[test]
    fn the_menu_language_toggle_flips_the_interface() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert_eq!(app.lang, Lang::En, "starts English");
        app.run_menu_item(MenuItem::Lang).unwrap();
        assert_eq!(app.lang, Lang::Ja, "toggled to Japanese");
        // The label reflects the language it switches *to*.
        assert_eq!(MenuItem::Lang.label(Lang::Ja), "Switch to English");
        assert_eq!(MenuItem::Lang.label(Lang::En), "日本語に切替");
        app.run_menu_item(MenuItem::Lang).unwrap();
        assert_eq!(app.lang, Lang::En, "toggled back to English");
    }

    fn mouse(kind: MouseEventKind, col: u16, row: u16) -> MouseEvent {
        MouseEvent { kind, column: col, row, modifiers: KeyModifiers::NONE }
    }

    /// Grab a divider, drag it, release. Returns the app for further asserts.
    fn drag_divider(app: &mut App, target: DividerTarget, to: (u16, u16)) {
        let d = app
            .dividers
            .iter()
            .copied()
            .find(|d| d.target == target)
            .unwrap_or_else(|| panic!("no divider for {:?} in {:?}", target, app.dividers));
        // Grab the middle of the seam, not its very corner — the corner shares
        // a cell with a tab label, which now wins the click.
        let grab = (d.zone.x + d.zone.width / 2, d.zone.y + d.zone.height / 2);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), grab.0, grab.1));
        assert!(app.drag.is_some(), "grabbing the seam should start a drag");
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), to.0, to.1));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), to.0, to.1));
        assert!(app.drag.is_none(), "releasing should end the drag");
    }

    #[test]
    fn dragging_the_vertical_seam_resizes_the_file_panes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        assert_eq!(app.panes_pct, 50);

        // Drag the left/right seam to roughly a quarter of the width.
        drag_divider(&mut app, DividerTarget::Panes, (25, 10));
        assert!(
            (20..=30).contains(&app.panes_pct),
            "expected ~25%, got {}",
            app.panes_pct
        );

        // The rendered rects must follow.
        let _ = render(&mut app, 100, 40);
        assert!(
            app.layout_rects.left.width < app.layout_rects.right.width,
            "left pane should now be the narrow one"
        );
    }

    #[test]
    fn dragging_the_horizontal_seam_resizes_the_shell_panel() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        assert_eq!(app.main_pct, 60);

        drag_divider(&mut app, DividerTarget::Main, (50, 10));
        assert!(app.main_pct < 60, "shell should have grown, got {}", app.main_pct);

        let before = app.layout_rects.shell.height;
        let _ = render(&mut app, 100, 40);
        assert!(app.layout_rects.shell.height > before / 2, "shell rect should follow the drag");
    }

    #[test]
    fn a_split_cannot_be_dragged_past_its_minimum() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);

        // Drag far past the left edge; the pane must keep a usable width.
        drag_divider(&mut app, DividerTarget::Panes, (0, 10));
        assert_eq!(app.panes_pct, MIN_SPLIT_PCT);

        drag_divider(&mut app, DividerTarget::Panes, (999, 10));
        assert_eq!(app.panes_pct, 100 - MIN_SPLIT_PCT);
    }

    #[test]
    fn grabbing_a_seam_does_not_change_focus() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let d = app.dividers.iter().copied().find(|d| d.target == DividerTarget::Main).unwrap();
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), d.zone.x, d.zone.y));
        assert_eq!(app.focused, FocusedPane::Left, "grabbing a border must not steal focus");
    }

    #[test]
    fn clicking_inside_a_pane_still_moves_focus() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let r = app.layout_rects.right;
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), r.x + 5, r.y + 3));
        assert_eq!(app.focused, FocusedPane::Right);
        assert!(app.drag.is_none());
    }

    /// An app with two *different* directories, one per pane.
    fn app_two_dirs(
        left: &[&str],
        right: &[&str],
    ) -> (tempfile::TempDir, tempfile::TempDir, App) {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        for n in left {
            std::fs::write(l.path().join(n), b"x").unwrap();
        }
        for n in right {
            std::fs::write(r.path().join(n), b"y").unwrap();
        }
        let app = App::new(
            l.path().to_path_buf(),
            r.path().to_path_buf(),
            cian_lua::Config::default(),
        )
        .unwrap();
        (l, r, app)
    }

    /// `o` pulls the other pane's directory into the active one; `O` pushes the
    /// active pane's directory onto the other. Focus never moves.
    #[test]
    fn o_and_shift_o_sync_the_two_panes_directories() {
        let (l, r, mut app) = app_two_dirs(&["a.txt"], &["b.txt"]);
        let (ldir, rdir) = (l.path().to_path_buf(), r.path().to_path_buf());
        assert_ne!(app.left.active_ref().cwd, app.right.active_ref().cwd);

        // On the right pane, `o` makes the right pane show the left's directory.
        app.focus(FocusedPane::Right);
        app.handle_key(key('o')).unwrap();
        assert!(app.right.active_ref().cwd.ends_with(ldir.file_name().unwrap()),
            "right pulled the left's dir");
        assert_eq!(app.focused, FocusedPane::Right, "focus stays put");

        // Reset the right pane, then `O` pushes the right's dir onto the left.
        app.right.active_mut().jump_to(rdir.clone()).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::SHIFT)).unwrap();
        assert!(app.left.active_ref().cwd.ends_with(rdir.file_name().unwrap()),
            "left received the right's dir");
        assert_eq!(app.focused, FocusedPane::Right, "focus still on the right");
    }

    #[test]
    fn copy_then_paste_duplicates_into_the_other_directory() {
        let (_l, r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Copy).unwrap();
        assert!(app.file_clip.is_some());

        app.focus(FocusedPane::Right);
        app.run_menu_item(MenuItem::Paste).unwrap();

        assert!(r.path().join("doc.txt").exists(), "file should have been pasted");
        // A copy stays on the clipboard for pasting again elsewhere.
        assert!(app.file_clip.is_some(), "copy should survive its paste");
    }

    #[test]
    fn cut_then_paste_moves_and_empties_the_clipboard() {
        let (l, r, mut app) = app_two_dirs(&["move_me.txt"], &[]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Cut).unwrap();

        app.focus(FocusedPane::Right);
        app.run_menu_item(MenuItem::Paste).unwrap();

        assert!(r.path().join("move_me.txt").exists(), "should exist at destination");
        assert!(!l.path().join("move_me.txt").exists(), "should be gone from source");
        assert!(app.file_clip.is_none(), "a cut is consumed by its paste");
    }

    #[test]
    fn pasting_into_the_source_directory_is_refused() {
        let (l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Copy).unwrap();
        // Paste straight back where it came from.
        app.run_menu_item(MenuItem::Paste).unwrap();

        let n = std::fs::read_dir(l.path()).unwrap().count();
        assert_eq!(n, 1, "must not duplicate into the same directory");
        assert!(app.message.as_deref().unwrap_or("").contains("already"));
    }

    /// Paste is always offered, because it can also take files from the system
    /// clipboard. Hiding it until cian's own register was filled made a file
    /// just copied in Explorer look unpasteable.
    #[test]
    fn paste_is_always_offered() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        let _ = render(&mut app, 100, 40);

        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Paste), "offered with nothing held");
        app.popup = Popup::None;

        app.clip_targets(ClipOp::Copy);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Paste), "and still offered once held");
    }

    /// Plain text on the clipboard must never be treated as a path: the
    /// platform queries return the text coerced into one (copying "hello"
    /// yields `/hello` on macOS), and acting on that would be nonsense.
    #[test]
    fn clipboard_candidates_that_do_not_exist_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real.txt");
        std::fs::write(&real, b"x").unwrap();

        let kept = keep_existing(vec![
            real.clone(),
            PathBuf::from("/just some copied text"),
            dir.path().to_path_buf(),
            PathBuf::from(""),
        ]);
        assert_eq!(kept, vec![real, dir.path().to_path_buf()], "only real entries survive");
        assert!(keep_existing(Vec::new()).is_empty());
    }

    #[test]
    fn right_click_focuses_the_pane_and_opens_the_menu() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let r = app.layout_rects.right;
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), r.x + 5, r.y + 2));
        assert_eq!(app.focused, FocusedPane::Right, "right-click should move focus");
        assert!(matches!(app.popup, Popup::ContextMenu { .. }));
    }

    #[test]
    fn the_shell_menu_omits_file_operations() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.clip_targets(ClipOp::Copy);
        app.focus(FocusedPane::Shell);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Paste));
        assert!(!items.contains(&MenuItem::Delete), "delete makes no sense in a PTY");
        assert!(!items.contains(&MenuItem::Rename));
    }

    /// The manual has to be reachable from the menu everywhere — that is the
    /// whole point of putting it there.
    /// Keys never reach the picker while the shell has focus, so the menu is
    /// the only route to SSH from there. It must lead the shell's menu.
    #[test]
    fn the_shell_menu_leads_with_ssh() {
        let (_d, mut app) = app_with_ssh();
        app.focus(FocusedPane::Shell);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        // SSH is still reachable from the shell menu (the only route to it while
        // the shell has focus), now sitting after Paste / Transfer ▸.
        assert!(items.contains(&MenuItem::Ssh), "got {:?}", items);
    }

    #[test]
    fn the_menu_reaches_the_ssh_picker_from_the_shell() {
        let (_d, mut app) = app_with_ssh();
        app.focus(FocusedPane::Shell);
        app.run_menu_item(MenuItem::Ssh).unwrap();
        assert!(matches!(app.popup, Popup::SshHosts { .. }), "should open the picker");
    }

    /// Both panes offer it, since the picker is useful from either.
    #[test]
    fn the_file_menu_offers_ssh_too() {
        let (_d, mut app) = app_with_ssh();
        app.focus(FocusedPane::Left);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::Ssh), "got {:?}", items);
    }

    #[test]
    fn every_context_menu_offers_the_manual() {
        let (_d, mut app) = app_with(&["a.txt"]);
        for pane in [FocusedPane::Left, FocusedPane::Right, FocusedPane::Shell] {
            app.focus(pane);
            app.open_context_menu(5, 5);
            let Popup::ContextMenu { items, .. } = &app.popup else {
                panic!("no menu for {:?}", pane)
            };
            assert_eq!(
                items.last(),
                Some(&MenuItem::Manual),
                "manual should be the last entry for {:?}",
                pane
            );
            app.popup = Popup::None;
        }
    }

    /// Right-clicking the shell with an empty clipboard used to open nothing
    /// at all; the manual entry means there is always something to show.
    #[test]
    fn the_shell_menu_has_its_own_reduced_set() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        assert!(app.file_clip.is_none());
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        // Strip the optional launchers (present only when snippets/AI/macros are
        // configured in the ambient config dir) so the core set is stable.
        let core: Vec<MenuItem> = items
            .iter()
            .cloned()
            .filter(|i| !matches!(i, MenuItem::Snippets | MenuItem::AiMenu | MenuItem::Macros))
            .collect();
        // No SSH hosts configured here, so Transfer ▸ is omitted; logging and
        // encoding fold into Session ▸.
        assert_eq!(
            core,
            vec![
                MenuItem::Paste,
                MenuItem::Ssh,
                MenuItem::SessionMenu,
                MenuItem::WindowMenu,
                MenuItem::Background,
                MenuItem::Lang,
                MenuItem::Quit,
                MenuItem::Manual
            ]
        );
    }

    #[test]
    fn explain_error_without_a_shell_reports_it() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Force AI on (mock) so we get past the config gate to the shell check.
        app.ai = Some(cian_ai::AiConfig { auth_mode: "mock".into(), ..Default::default() });
        app.focus(FocusedPane::Shell);
        app.explain_shell_error();
        assert!(app.message.as_deref().unwrap_or("").contains("no shell"),
            "reports the absence of a shell: {:?}", app.message);
    }

    #[test]
    fn shell_window_submenu_offers_splits_tabs_and_zoom() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.open_context_menu(3, 3);
        // Drill into Window ▸.
        app.run_menu_item(MenuItem::WindowMenu).unwrap();
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no submenu") };
        assert!(items.contains(&MenuItem::ShellSplitLR));
        assert!(items.contains(&MenuItem::ShellSplitTB));
        assert!(items.contains(&MenuItem::ShellNewTab));
        assert!(items.contains(&MenuItem::ShellZoom));
        // A single (unsplit) tab offers "close tab", not "close split".
        assert!(items.contains(&MenuItem::ShellCloseTab));
        assert!(!items.contains(&MenuItem::ShellCloseSplit));
        assert!(items.contains(&MenuItem::Back));
    }

    #[test]
    fn attributes_lines_show_a_size_for_a_file() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("data.bin");
        std::fs::write(&f, vec![0u8; 2048]).unwrap();
        let (_d2, app) = app_with(&["a.txt"]);
        let lines = app.attributes_lines(&[f], 40);
        // Human-readable size appears on the entry's row.
        assert!(lines.iter().any(|l| l.contains("data.bin") && (l.contains("2.0K") || l.contains("2K") || l.contains("2048"))),
            "size shown: {lines:?}");
    }

    #[test]
    fn scp_upload_walks_picker_then_browses_the_server() {
        let (_d, mut app) = app_with_ssh();
        app.active_pane_mut().unwrap().cursor = 1; // a.txt (index 0 is the `..` row)
        app.start_scp(ScpDir::Upload);
        assert!(matches!(app.popup, Popup::SshHosts { .. }), "opens the host picker");
        assert!(app.scp_dir.is_some());

        // Pick db1 (single user, has a password) → the WinSCP-style remote browser.
        app.command_buffer.clear();
        // Filter to db1 then Enter.
        for c in "db1".chars() {
            app.handle_key(code(KeyCode::Char(c))).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        match &app.popup {
            Popup::RemoteBrowser { purpose: BrowsePurpose::Upload, .. } => {}
            other => panic!("expected the upload browser, got {:?}", other),
        }
        let p = app.scp_pending.as_ref().expect("a pending transfer");
        assert_eq!(p.target.host, "10.0.2.31");
        assert_eq!(p.target.port, 2222);
        assert_eq!(p.target.user, "postgres");
        assert_eq!(p.dir, ScpDir::Upload);
        assert_eq!(p.locals.len(), 1);
    }

    #[test]
    fn manual_ssh_target_parses_user_host_port() {
        let (_d, mut app) = app_with_ssh();
        app.active_pane_mut().unwrap().cursor = 1; // a.txt
        app.start_scp(ScpDir::Upload);
        // F2 from the host picker → type the server by hand.
        app.handle_key(code(KeyCode::F(2))).unwrap();
        assert!(matches!(
            app.popup,
            Popup::TextInput { kind: InputKind::ManualSshTarget { for_scp: true }, .. }
        ));
        for c in "deploy@web9:2201".chars() {
            app.handle_key(code(KeyCode::Char(c))).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        // Advances to the masked password step, carrying the parsed pieces.
        match &app.popup {
            Popup::TextInput { kind: InputKind::ManualSshPass { user, host, port, for_scp }, .. } => {
                assert_eq!(user, "deploy");
                assert_eq!(host, "web9");
                assert_eq!(*port, 2201);
                assert!(for_scp);
            }
            other => panic!("expected the password prompt, got {:?}", other),
        }
    }

    #[test]
    fn scp_upload_without_a_selected_file_is_refused() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("onlydir")).unwrap();
        let mut config = cian_lua::Config::default();
        config.ssh_hosts = vec![cian_lua::SshHost {
            name: "web1".into(),
            host: "10.0.1.11".into(),
            users: vec![cian_lua::SshUser::plain("root")],
            port: None,
            notes: None,
        }];
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.active_pane_mut().unwrap().cursor = 0; // the directory
        app.start_scp(ScpDir::Upload);
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("select a file"));
    }

    #[test]
    fn scp_needs_a_password_for_the_user() {
        let (_d, mut app) = app_with_ssh();
        app.active_pane_mut().unwrap().cursor = 1; // a real file, not the `..` row
        app.start_scp(ScpDir::Upload);
        // web1 / root has no password configured.
        for c in "web1".chars() {
            app.handle_key(code(KeyCode::Char(c))).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap(); // host web1 → user list
        app.handle_key(code(KeyCode::Enter)).unwrap(); // first user (root)
        assert!(app.scp_pending.is_none());
        assert!(app.message.as_deref().unwrap().contains("no password"));
    }

    #[test]
    fn a_host_name_is_pulled_from_a_terminal_title() {
        assert_eq!(host_from_title("taketan@web01: ~/proj"), Some("web01".into()));
        assert_eq!(host_from_title("root@db-server:/var"), Some("db-server".into()));
        // No `@` — nothing to take.
        assert_eq!(host_from_title("just a title"), None);
    }

    #[test]
    fn the_log_prompt_asks_for_a_folder_when_a_shell_exists() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // No shell yet → it declines rather than opening a prompt.
        app.start_log_prompt();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("no shell"));
    }

    #[test]
    fn starting_a_log_in_a_bad_directory_is_refused() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_session_log("/no/such/directory/anywhere");
        assert!(app.message.as_deref().unwrap().contains("not a directory"));
    }

    #[test]
    fn choosing_the_manual_from_the_menu_opens_it() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        let _ = render(&mut app, 100, 40);
        app.open_context_menu(5, 5);

        // Walk to the last entry and activate it with the keyboard.
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        let steps = items.len() - 1;
        for _ in 0..steps {
            app.handle_key(key('j')).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        assert!(matches!(app.popup, Popup::Manual { .. }), "expected the manual");
        let screen = render(&mut app, 100, 40).join("\n");
        assert!(screen.contains("key manual"), "manual should be on screen");
    }

    #[test]
    fn the_color_picker_sets_only_the_chosen_pane() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Right);
        app.run_menu_item(MenuItem::Background).unwrap();
        assert!(matches!(app.popup, Popup::ColorPicker { .. }));

        // Move off "default" and apply.
        app.handle_key(key('j')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();

        assert!(app.pane_bg[1].is_some(), "right pane should be tinted");
        assert!(app.pane_bg[0].is_none(), "left pane must be untouched");
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn a_flash_fades_out_and_then_expires() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert_eq!(app.flash_level(FocusedPane::Left), 0.0);

        app.flash(FocusedPane::Left);
        assert!(app.flash_level(FocusedPane::Left) > 0.9, "should start near full");
        assert_eq!(app.flash_level(FocusedPane::Right), 0.0, "only the named pane lights");
        assert!(app.flash_active());

        // Pretend the flash started long ago.
        app.flash = Some((FocusedPane::Left, Instant::now() - Duration::from_secs(2)));
        assert_eq!(app.flash_level(FocusedPane::Left), 0.0);
        assert!(!app.flash_active());
    }

    #[test]
    fn easing_stays_in_range_and_hits_both_ends() {
        let a = Anim {
            kind: AnimKind::Zoom { from: Rect::new(0, 0, 10, 10), to: Rect::new(0, 0, 20, 20) },
            start: Instant::now(),
            dur: Duration::from_millis(100),
        };
        assert!(a.progress() < 0.2, "should start near zero");
        assert!(!a.done());

        let ended = Anim { start: Instant::now() - Duration::from_secs(1), ..a };
        assert_eq!(ended.progress(), 1.0);
        assert!(ended.done());

        // A zero-length transition is already over.
        let instant = Anim { dur: Duration::ZERO, ..a };
        assert_eq!(instant.progress(), 1.0);
        assert!(instant.done());
    }

    #[test]
    fn lerp_rect_interpolates_between_its_endpoints() {
        let a = Rect::new(0, 0, 10, 10);
        let b = Rect::new(10, 20, 30, 40);
        assert_eq!(lerp_rect(a, b, 0.0), a);
        assert_eq!(lerp_rect(a, b, 1.0), b);
        let mid = lerp_rect(a, b, 0.5);
        assert_eq!((mid.x, mid.y, mid.width, mid.height), (5, 10, 20, 25));
        // Never collapses to nothing, which would make a widget panic.
        let z = lerp_rect(Rect::new(0, 0, 0, 0), Rect::new(0, 0, 0, 0), 0.5);
        assert!(z.width >= 1 && z.height >= 1);
    }

    #[test]
    fn union_rect_ignores_empty_inputs() {
        let a = Rect::new(0, 0, 10, 5);
        let b = Rect::new(10, 0, 10, 5);
        assert_eq!(union_rect(a, b), Rect::new(0, 0, 20, 5));
        assert_eq!(union_rect(a, Rect::new(0, 0, 0, 0)), a);
        assert_eq!(union_rect(Rect::new(0, 0, 0, 0), b), b);
    }

    /// Both directions must actually travel. The un-zoom used to read the
    /// focused pane's rect out of `layout_rects`, which by then described the
    /// *zoomed* layout — so `from` and `to` were both the full window and the
    /// transition, while running, moved nothing.
    #[test]
    fn zoom_animates_in_both_directions() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        assert!(!app.zoomed);
        let pane = app.layout_rects.left;

        app.toggle_zoom();
        assert!(app.zoomed);
        let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim else {
            panic!("expected a zoom")
        };
        assert_eq!(from, pane, "should grow out of the pane it was in");
        assert!(to.width > from.width && to.height > from.height, "{:?} -> {:?}", from, to);
        app.finish_anim();

        // Rendering while zoomed overwrites layout_rects with the zoomed
        // layout — the exact condition that broke the way back.
        let _ = render(&mut app, 100, 40);

        app.toggle_zoom();
        assert!(!app.zoomed);
        let Some(Anim { kind: AnimKind::Zoom { from, to }, .. }) = app.anim else {
            panic!("expected a zoom back")
        };
        assert_ne!(from, to, "the way back must travel, not sit still");
        assert!(to.width < from.width && to.height < from.height, "{:?} -> {:?}", from, to);
        assert_eq!(to, pane, "should shrink into the pane it came from");
    }

    #[test]
    fn zooming_the_shell_returns_to_the_shell_rect() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Shell);
        let shell = app.layout_rects.shell;

        app.toggle_zoom();
        app.finish_anim();
        let _ = render(&mut app, 100, 40);
        app.toggle_zoom();

        let Some(Anim { kind: AnimKind::Zoom { to, .. }, .. }) = app.anim else {
            panic!("expected a zoom back")
        };
        assert_eq!(to, shell, "each surface returns to its own rect");
    }

    #[test]
    fn animation_can_be_switched_off_by_config() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let mut config = cian_lua::Config::default();
        config.options.animation_ms = Some(0);
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, config).unwrap();
        let _ = render(&mut app, 100, 40);

        app.toggle_zoom();
        assert!(app.zoomed, "the zoom itself must still happen");
        assert!(app.anim.is_none(), "but with no transition");
    }

    #[test]
    fn the_ratio_override_only_applies_to_its_own_divider() {
        let ov = AnimOverride {
            ratio: Some((DividerTarget::Panes, 90)),
            freeze_pty: true,
            show_splits: false,
        };
        assert_eq!(ov.ratio_for(DividerTarget::Panes, 50), 90);
        // Other dividers fall through to their stored value.
        assert_eq!(ov.ratio_for(DividerTarget::Main, 60), 60);
        // Stored values are clamped; overrides are not, so a close animation
        // can drive a pane all the way to zero.
        assert_eq!(ov.ratio_for(DividerTarget::Main, 99), 100 - MIN_SPLIT_PCT);
        let zero =
            AnimOverride { ratio: Some((DividerTarget::Main, 0)), freeze_pty: true, show_splits: false };
        assert_eq!(zero.ratio_for(DividerTarget::Main, 50), 0);
    }

    #[test]
    fn a_deferred_close_runs_when_its_transition_lands() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Nothing to close, but the deferral machinery should still fire
        // exactly once and then clear itself.
        app.anim_then = Some(PendingClose::ShellPane);
        app.start_anim(AnimKind::Ratio {
            target: DividerTarget::Main,
            from: 50,
            to: 0,
        });
        assert!(app.anim.is_some());

        app.finish_anim();
        assert!(app.anim.is_none());
        assert!(app.anim_then.is_none(), "deferred work must be consumed");
    }

    #[test]
    fn split_ratio_survives_a_render_round_trip() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.panes_pct = 30;
        let _ = render(&mut app, 100, 40);
        // 30% of a 100-wide window, give or take rounding.
        assert!(
            (28..=32).contains(&app.layout_rects.left.width),
            "got {}",
            app.layout_rects.left.width
        );
    }

    /// Right-clicking a row must select the file actually drawn on that row,
    /// including after the list has scrolled.
    #[test]
    fn right_click_selects_the_row_under_the_pointer_when_scrolled() {
        let names: Vec<String> = (0..60).map(|i| format!("f{:02}.txt", i)).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let (_d, mut app) = app_with(&refs);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);

        let rect = app.layout_rects.left;
        let view_h = rect.height.saturating_sub(2);

        // Every combination of scroll position and clicked row must agree.
        for cursor in [0usize, 5, 20, 45, 59] {
            for off in 0..view_h.min(8) {
                if let Some(p) = app.active_pane_mut() {
                    p.cursor = cursor;
                }
                let before = render(&mut app, 100, 40);
                let row = rect.y + 1 + off;
                let lo = rect.x as usize;
                let hi = (rect.x + rect.width) as usize;
                let drawn: String =
                    before[row as usize].chars().skip(lo).take(hi - lo).collect();
                app.handle_mouse(mouse(
                    MouseEventKind::Down(MouseButton::Right),
                    rect.x + 3,
                    row,
                ));
                let sel = app.active_pane().unwrap().selected().unwrap().name.clone();
                assert!(
                    drawn.contains(&sel),
                    "cursor {} row-offset {}: screen showed {:?}, selected {:?}",
                    cursor,
                    off,
                    drawn.trim(),
                    sel
                );
                app.popup = Popup::None;
            }
        }
    }

    #[test]
    fn right_click_on_a_single_screenful_selects_correctly() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        let rect = app.layout_rects.left;
        // Clicking past the last entry must leave the cursor where it was
        // rather than jumping somewhere arbitrary.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y + 1));
        assert_eq!(app.active_pane().unwrap().cursor, 0);
        app.popup = Popup::None;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y + 2));
        assert_eq!(app.active_pane().unwrap().cursor, 1);
        app.popup = Popup::None;

        // A row inside the pane but past the last entry: stay put.
        let before = app.active_pane().unwrap().cursor;
        let blank = rect.y + rect.height - 3;
        assert!(blank > rect.y + 3, "test needs a pane taller than the listing");
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, blank));
        assert_eq!(app.focused, FocusedPane::Left, "still inside the pane");
        assert_eq!(app.active_pane().unwrap().cursor, before, "empty space must not move it");
        app.popup = Popup::None;

        // The pane's own border row is not a list row either.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y));
        assert_eq!(app.active_pane().unwrap().cursor, before, "the border must not move it");
    }

    /// Degenerate geometry must not panic (u16 underflow in seam maths).
    #[test]
    fn rendering_survives_a_tiny_terminal() {
        let (_d, mut app) = app_with(&["a.txt"]);
        for (w, h) in [(1u16, 1u16), (2, 2), (4, 3), (10, 4), (1, 40), (40, 1)] {
            let _ = render(&mut app, w, h);
        }
        // And with a popup open, which does its own rect maths.
        app.open_manual();
        for (w, h) in [(1u16, 1u16), (3, 3), (12, 5)] {
            let _ = render(&mut app, w, h);
        }
    }

    #[test]
    fn the_shell_menu_offers_a_background_color() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(
            items.contains(&MenuItem::Background),
            "the shell pane should be tintable too, got {:?}",
            items
        );
    }

    #[test]
    fn the_color_picker_tints_only_the_active_split_pane() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.run_menu_item(MenuItem::Background).unwrap();
        app.handle_key(key('j')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // The file panes keep their own (unset) backgrounds.
        assert!(app.pane_bg[0].is_none() && app.pane_bg[1].is_none());
        // With no shell running there is no pane to color, and nothing panics.
        assert!(app.shell.active_pane_bg().is_none());
    }

    /// A pane's color must stop at that pane. This used to be stored per
    /// panel, so coloring one split painted every split and every tab —
    /// including ones meant to keep the terminal's own background.
    #[test]
    fn a_pane_tint_stops_at_that_pane() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let tint = Color::Rgb(17, 45, 87);
        app.pane_bg[0] = Some(tint);

        let buf = render_buf(&mut app, 100, 40);
        let left = app.layout_rects.left;
        let right = app.layout_rects.right;
        assert!(left.height > 2 && right.height > 2, "need a real layout");

        assert_eq!(
            buf[(left.x + 5, left.y + left.height / 2)].bg,
            tint,
            "the colored pane should be tinted"
        );
        assert_ne!(
            buf[(right.x + 5, right.y + right.height / 2)].bg,
            tint,
            "the tint must not reach the other pane"
        );
    }

    /// Two split panes, each with its own background — the case that was
    /// impossible when the color lived on the panel.
    #[test]
    fn split_panes_hold_separate_backgrounds() {
        let dir = tempfile::tempdir().unwrap();
        let sh = cian_pty::default_shell();
        let mk = || cian_pty::PtySession::new(dir.path(), &sh, 24, 80).unwrap();

        let mut tab = ShellTab::new(mk());
        let first = tab.active;
        tab.split(SplitDir::LeftRight, mk());
        let second = tab.active;
        assert_ne!(first, second, "split should make a second leaf");

        let set = |t: &mut ShellTab, leaf: usize, c: Color| {
            if let Some(Node::Leaf { bg, .. }) = t.nodes.get_mut(leaf).and_then(|n| n.as_mut()) {
                *bg = Some(c);
            }
        };
        let get = |t: &ShellTab, leaf: usize| match t.nodes.get(leaf).and_then(|n| n.as_ref()) {
            Some(Node::Leaf { bg, .. }) => *bg,
            _ => None,
        };

        set(&mut tab, first, Color::Rgb(17, 45, 87));
        assert_eq!(get(&tab, first), Some(Color::Rgb(17, 45, 87)));
        assert_eq!(get(&tab, second), None, "the sibling must stay on the default");

        set(&mut tab, second, Color::Rgb(87, 29, 17));
        assert_eq!(get(&tab, first), Some(Color::Rgb(17, 45, 87)), "unchanged by its sibling");
        assert_eq!(get(&tab, second), Some(Color::Rgb(87, 29, 17)));
    }

    /// Clicking a split must act on the pane under the pointer. Without this,
    /// right-clicking the left half of a split colored the right half —
    /// whichever happened to be active.
    #[test]
    fn clicking_a_split_selects_the_pane_under_the_pointer() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);

        // Two leaves side by side, standing in for a real split.
        let shell = app.layout_rects.shell;
        let half = shell.width / 2;
        let l0 = Rect::new(shell.x, shell.y, half, shell.height);
        let l1 = Rect::new(shell.x + half, shell.y, half, shell.height);
        app.shell_leaves = vec![(0, 7, l0, l0), (0, 9, l1, l1)];
        app.shell.tabs.push(ShellTab { nodes: Vec::new(), root: 0, active: 9 });

        app.select_shell_leaf_at(shell.x + 2, shell.y + 2);
        assert_eq!(app.shell.tabs[0].active, 7, "should pick the left pane");

        app.select_shell_leaf_at(shell.x + half + 2, shell.y + 2);
        assert_eq!(app.shell.tabs[0].active, 9, "should pick the right pane");

        // A point outside every pane leaves the selection alone.
        app.select_shell_leaf_at(0, 0);
        assert_eq!(app.shell.tabs[0].active, 9);
    }

    #[test]
    fn the_shell_hints_mention_pane_switching_only_when_split() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        // No panes yet: the key would do nothing, so it is not advertised.
        assert!(!key_hints(&app).iter().any(|(k, _)| *k == "S-F1/F2"));
    }

    #[test]
    fn the_palette_is_distinct_enough_to_tell_panes_apart() {
        // The first entry is "no color"; the rest must be visibly different
        // from one another, which an earlier too-subtle set was not.
        let colors: Vec<(u8, u8, u8)> = PANE_BG_PRESETS
            .iter()
            .filter_map(|(_, c)| match c {
                Some(Color::Rgb(r, g, b)) => Some((*r, *g, *b)),
                _ => None,
            })
            .collect();
        assert_eq!(colors.len(), PANE_BG_PRESETS.len() - 1);
        for (i, a) in colors.iter().enumerate() {
            for b in colors.iter().skip(i + 1) {
                let d = (a.0 as i32 - b.0 as i32).abs()
                    + (a.1 as i32 - b.1 as i32).abs()
                    + (a.2 as i32 - b.2 as i32).abs();
                assert!(d >= 60, "{:?} and {:?} are too close to tell apart", a, b);
            }
            // Dark enough that normal foreground text stays readable.
            let lum = 0.299 * a.0 as f32 + 0.587 * a.1 as f32 + 0.114 * a.2 as f32;
            assert!(lum < 90.0, "{:?} is too light for text on top (lum {})", a, lum);
        }
    }

    /// Cells the shell colored for itself must survive the tint, or ls
    /// colors and vim themes would be flattened.
    #[test]
    fn the_tint_leaves_explicitly_colored_cells_alone() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Give a file pane a background so there are non-Reset cells to guard,
        // then tint the whole screen area and check they are preserved.
        let painted = Color::Rgb(40, 0, 0);
        app.pane_bg[0] = Some(painted);
        let tint = Color::Rgb(0, 0, 40);

        let mut terminal = Terminal::new(TestBackend::new(100, 40)).unwrap();
        terminal
            .draw(|f| {
                draw(f, &mut app);
                tint_default_cells(f, f.area(), tint);
            })
            .unwrap();
        let buf = terminal.backend().buffer().clone();

        let left = app.layout_rects.left;
        let cell = buf[(left.x + 5, left.y + left.height / 2)].bg;
        assert_eq!(cell, painted, "an already-colored cell must not be repainted");

        // And a cell that was Reset did get the tint.
        let right = app.layout_rects.right;
        assert_eq!(buf[(right.x + 5, right.y + right.height / 2)].bg, tint);
    }

    #[test]
    fn comma_opens_the_sort_picker_and_enter_applies_it() {
        let (_d, mut app) = app_with(&["b.rs", "a.rs", "c.md"]);
        app.handle_key(key(',')).unwrap();
        assert!(matches!(app.popup, Popup::SortPicker { .. }));

        // Jump straight to extension with its mnemonic.
        app.handle_key(key('e')).unwrap();
        assert!(matches!(app.popup, Popup::None));
        let p = app.active_pane().unwrap();
        assert_eq!(p.sort.key, SortKey::Extension);
        assert!(!p.sort.reverse);
    }

    /// Picking the key that is already active flips the direction, the way a
    /// column header does.
    #[test]
    fn choosing_the_active_key_again_reverses_it() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        app.apply_sort_key(SortKey::Size);
        assert!(!app.active_pane().unwrap().sort.reverse);
        app.apply_sort_key(SortKey::Size);
        assert!(app.active_pane().unwrap().sort.reverse, "second pick should reverse");
        app.apply_sort_key(SortKey::Name);
        assert!(!app.active_pane().unwrap().sort.reverse, "a different key resets direction");
    }

    #[test]
    fn sorting_is_per_pane() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        app.focus(FocusedPane::Left);
        app.apply_sort_key(SortKey::Size);
        assert_eq!(app.left.active_ref().sort.key, SortKey::Size);
        assert_eq!(app.right.active_ref().sort.key, SortKey::Name, "other pane untouched");
    }

    #[test]
    fn the_status_bar_drops_the_sort_indicator_but_keeps_the_counts() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        let screen = render(&mut app, 100, 40).join("\n");
        // The sort chip was removed; the item/mark counts stay.
        assert!(!screen.contains("name ▲"), "the sort indicator should be gone:\n{}", screen);
        assert!(screen.contains("items"));
        assert!(screen.contains("marks"));

        // Sorting still works even though it is no longer shown here.
        app.apply_sort_key(SortKey::Modified);
        assert_eq!(app.active_pane().unwrap().sort.key, SortKey::Modified);
    }

    #[test]
    fn the_key_hint_bar_is_contextual() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        let normal = render(&mut app, 110, 40).join("\n");
        assert!(normal.contains("sort"), "normal hints missing:\n{}", normal);
        assert!(normal.contains("filter"));

        // Visual mode advertises a different, shorter set.
        app.visual_start();
        let visual = render(&mut app, 110, 40).join("\n");
        assert!(visual.contains("extend"), "visual hints missing:\n{}", visual);
        assert!(!visual.contains("rename"), "normal-mode hints should be gone");
    }

    #[test]
    fn the_key_hint_bar_can_be_switched_off() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let mut config = cian_lua::Config::default();
        config.options.key_hints = Some(false);
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, config).unwrap();

        let screen = render(&mut app, 110, 40).join("\n");
        assert!(!screen.contains("? help"), "hints should be hidden");
        // The row it would have used goes back to the listing.
        assert!(screen.contains("a.txt"));
    }

    /// The bottom rows are claimed one at a time, so a row must only be
    /// consumed by a bar that is actually drawn. Getting that wrong shifts
    /// everything below it down by one and blanks the last line.
    #[test]
    fn the_status_bar_sits_on_the_last_row_in_every_mode() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");

        let normal = render(&mut app, 110, 40);
        assert!(normal[39].contains("items"), "status row: {:?}", normal[39]);
        assert!(normal[38].contains("help"), "hints above it: {:?}", normal[38]);

        // Filter mode adds a prompt row above the hints; the status bar must
        // still be the bottom line.
        app.handle_key(key('/')).unwrap();
        let filtering = render(&mut app, 110, 40);
        assert!(filtering[39].contains("items"), "status row: {:?}", filtering[39]);
        assert!(filtering[37].contains("filter /"), "prompt row: {:?}", filtering[37]);
    }

    /// `? help` is the way out of not knowing any other key, so a narrow
    /// window must drop something else. Adding one hint used to push it off
    /// the end.
    #[test]
    fn the_help_hint_survives_a_narrow_window() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        for w in [40u16, 60, 80, 110, 200] {
            let screen = render(&mut app, w, 40).join("\n");
            assert!(screen.contains("? help"), "lost at width {}:\n{}", w, screen);
        }
    }

    /// A short window drops the hints rather than squeezing the listing out.
    #[test]
    fn a_short_window_drops_the_hints() {
        let (_d, mut app) = app_with_lang(&["a.txt"], "en");
        let tall = render(&mut app, 110, 40).join("\n");
        assert!(tall.contains("? help"));
        let short = render(&mut app, 110, 10).join("\n");
        assert!(!short.contains("? help"), "hints should yield on a short window");
    }

    fn app_with_ssh() -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();
        let mut config = cian_lua::Config::default();
        config.ssh_hosts = vec![
            cian_lua::SshHost {
                name: "web1".into(),
                host: "10.0.1.11".into(),
                users: vec![cian_lua::SshUser::plain("root"), cian_lua::SshUser::plain("deploy")],
                port: None,
                notes: None,
            },
            cian_lua::SshHost {
                name: "db1".into(),
                host: "10.0.2.31".into(),
                users: vec![cian_lua::SshUser {
                    name: "postgres".into(),
                    password: Some("hunter2".into()),
                    password_cmd: None,
                }],
                port: Some(2222),
                notes: None,
            },
        ];
        let p = dir.path().to_path_buf();
        let app = App::new(p.clone(), p, config).unwrap();
        (dir, app)
    }

    #[test]
    fn the_ssh_picker_filters_hosts_as_you_type() {
        let (_d, mut app) = app_with_ssh();
        app.start_ssh();
        assert_eq!(app.ssh_matches("").len(), 2);

        app.handle_key(key('d')).unwrap();
        app.handle_key(key('b')).unwrap();
        let Popup::SshHosts { filter, .. } = &app.popup else { panic!("no picker") };
        assert_eq!(filter, "db");
        assert_eq!(app.ssh_matches("db").len(), 1);

        // Backspace widens it again.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        let Popup::SshHosts { filter, .. } = &app.popup else { panic!("no picker") };
        assert_eq!(filter, "d");
    }

    /// A host with several users needs the second stage; one with a single
    /// user should connect straight away.
    #[test]
    fn a_single_user_host_skips_the_second_stage() {
        let (_d, mut app) = app_with_ssh();
        app.start_ssh();
        app.handle_key(key('d')).unwrap();
        app.handle_key(key('b')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::None), "should have connected already");
        assert!(app.message.as_deref().unwrap_or("").contains("postgres@db1"));
    }

    #[test]
    fn a_multi_user_host_offers_its_users() {
        let (_d, mut app) = app_with_ssh();
        app.start_ssh();
        app.handle_key(key('w')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let Popup::SshUsers { host, .. } = &app.popup else { panic!("expected the user stage") };
        assert_eq!(app.config.ssh_hosts[*host].name, "web1");

        // Esc steps back to the host list rather than closing outright.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::SshHosts { .. }));
    }

    #[test]
    fn connecting_types_the_command_into_the_shell() {
        let (_d, mut app) = app_with_ssh();
        // No shell yet, so the command has to be queued for the spawn.
        assert_eq!(app.shell.count(), 0);
        app.ssh_connect(1, "postgres");
        assert_eq!(app.focused, FocusedPane::Shell, "should hand over to the shell");
        assert_eq!(
            app.pending_shell_input.as_deref(),
            Some("ssh postgres@10.0.2.31 -p 2222\n"),
            "port should be carried through"
        );
    }

    #[test]
    fn nothing_configured_drops_into_manual_entry() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_ssh();
        // With no hosts to pick, go straight to typing a server by hand (#2).
        assert!(
            matches!(
                app.popup,
                Popup::TextInput { kind: InputKind::ManualSshTarget { for_scp: false }, .. }
            ),
            "expected the manual-connection prompt, got {:?}",
            app.popup
        );
    }

    #[test]
    fn a_password_prompt_is_recognised_only_at_the_end_of_the_screen() {
        assert!(looks_like_password_prompt("root@10.0.2.31's password:"));
        assert!(looks_like_password_prompt("Password:"));
        assert!(looks_like_password_prompt("Enter passphrase for key '/x/id_ed25519':"));
        // Trailing blank lines are ignored.
        assert!(looks_like_password_prompt("Password:\n\n  \n"));
    }

    #[test]
    fn things_that_must_not_be_mistaken_for_a_password_prompt() {
        // The word scrolling past in output is not a prompt.
        assert!(!looks_like_password_prompt("password rotation done\n$ "));
        assert!(!looks_like_password_prompt("Failed password for root\n$ "));
        // A host-key question ends in a colon but must be answered by a human.
        assert!(!looks_like_password_prompt(
            "The authenticity of host 'x' can't be established.\n\
             ED25519 key fingerprint is SHA256:abc.\n\
             Are you sure you want to continue connecting (yes/no)?:"
        ));
        assert!(!looks_like_password_prompt(""));
        assert!(!looks_like_password_prompt("$ "));
    }

    #[test]
    fn connecting_as_a_user_with_a_secret_arms_the_prompt_watcher() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(1, "postgres");
        assert!(app.pending_auth.is_some(), "should be waiting for the prompt");
        // The secret must not appear in anything the user or a log can see.
        let msg = app.message.clone().unwrap_or_default();
        assert!(!msg.contains("hunter2"), "secret leaked into the status message: {}", msg);
        assert!(!format!("{:?}", app.pending_auth).contains("hunter2"), "secret leaked via Debug");
    }

    #[test]
    fn a_user_without_a_secret_does_not_arm_it() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(0, "root");
        assert!(app.pending_auth.is_none(), "key-auth logins must not wait to type anything");
    }

    #[test]
    fn the_watcher_gives_up_after_its_window() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(1, "postgres");
        // Pretend the window has passed with no prompt — a keyed host, say.
        app.pending_auth = Some(PendingAuth {
            secret: "hunter2".into(),
            deadline: Instant::now() - Duration::from_secs(1),
        });
        app.pending_shell_input = None;
        assert!(!app.poll_pending_auth());
        assert!(app.pending_auth.is_none(), "should have expired rather than waiting forever");
    }

    /// The command is queued while the PTY spawns; the password must not be
    /// sent before the command it answers has even been delivered.
    #[test]
    fn nothing_is_sent_while_the_command_is_still_queued() {
        let (_d, mut app) = app_with_ssh();
        app.ssh_connect(1, "postgres");
        assert!(app.pending_shell_input.is_some(), "command should be queued");
        assert!(!app.poll_pending_auth());
        assert!(app.pending_auth.is_some(), "still armed, just not fired");
    }

    #[test]
    fn a_secret_can_come_from_a_command_instead_of_the_file() {
        let u = cian_lua::SshUser {
            name: "deploy".into(),
            password: None,
            password_cmd: Some("printf 'from-store'".into()),
        };
        assert!(u.has_secret());
        assert_eq!(u.secret().as_deref(), Some("from-store"));
    }

    #[test]
    fn z_prompts_for_a_path_seeded_with_the_current_directory() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let here = app.active_pane().unwrap().cwd.clone();
        app.handle_key(key('z')).unwrap();
        let Popup::TextInput { buffer, kind, .. } = &app.popup else { panic!("no prompt") };
        assert!(matches!(kind, InputKind::JumpPath));
        assert_eq!(buffer, &here.display().to_string(), "seeded with where you are");
    }

    #[test]
    fn a_typed_directory_is_entered() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        std::fs::write(dir.path().join("sub").join("inner.txt"), b"x").unwrap();
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        let target = dir.path().join("sub");
        app.finish_jump_path(&target.display().to_string()).unwrap();
        // jump_to canonicalises, so compare on the final component.
        assert_eq!(app.active_pane().unwrap().cwd.file_name().unwrap(), "sub");
        // entries[0] is the `..` row; the first real entry follows it.
        assert_eq!(app.active_pane().unwrap().entries[1].name, "inner.txt");
    }

    /// Naming a file should land the cursor on it, so the pane is left
    /// somewhere useful rather than wherever it happened to be.
    #[test]
    fn a_typed_file_moves_the_cursor_to_it() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["a.txt", "b.txt", "c.txt"] {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        let target = dir.path().join("c.txt");
        app.finish_jump_path(&target.display().to_string()).unwrap();
        let pane = app.active_pane().unwrap();
        assert_eq!(pane.selected().unwrap().name, "c.txt");
    }

    #[test]
    fn a_path_that_does_not_exist_says_so_and_stays_put() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let before = app.active_pane().unwrap().cwd.clone();
        app.finish_jump_path("/no/such/place/at/all").unwrap();
        assert_eq!(app.active_pane().unwrap().cwd, before, "must not move");
        assert!(app.message.as_deref().unwrap_or("").contains("no such path"));
    }

    /// Paths get typed after copying them out of a shell or an address bar,
    /// which is where these forms come from.
    #[test]
    fn typed_paths_expand_env_vars_tildes_and_quotes() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("sub");
        std::fs::create_dir(&sub).unwrap();
        std::env::set_var("CIAN_TEST_BASE", dir.path());

        for form in [
            "$CIAN_TEST_BASE/sub",
            "${CIAN_TEST_BASE}/sub",
            "%CIAN_TEST_BASE%/sub",
        ] {
            assert_eq!(expand_path(form), sub, "failed to expand {:?}", form);
        }
        // Surrounding quotes, as pasted from a shell.
        let quoted = format!("\"{}\"", sub.display());
        assert_eq!(expand_path(&quoted), sub);

        // An unset variable is left alone rather than silently becoming empty.
        assert_eq!(expand_path("$CIAN_NOT_SET_ANYWHERE"), PathBuf::from("$CIAN_NOT_SET_ANYWHERE"));
        std::env::remove_var("CIAN_TEST_BASE");
    }

    #[test]
    fn shift_enter_opens_the_context_menu_by_the_cursor() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 2;
        }

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        let Popup::ContextMenu { at, items, .. } = &app.popup else {
            panic!("expected the context menu")
        };
        assert!(items.contains(&MenuItem::Delete), "the file-pane menu");
        let left = app.layout_rects.left;
        assert!(at.0 >= left.x && at.0 < left.x + left.width, "anchored in the pane");
        assert_eq!(at.1, left.y + 1 + 2, "on the cursor's row");
    }

    /// Rounded corners are missing from several stock console fonts, so
    /// Windows font-links only the corners and the frame looks a few pixels
    /// out at each one. Square corners are in every font.
    #[test]
    fn border_corners_fall_back_to_square_where_fonts_lack_the_rounded_ones() {
        // An explicit setting always wins, on every platform.
        assert_eq!(resolve_border_type(Some("plain")), BorderType::Plain);
        assert_eq!(resolve_border_type(Some("square")), BorderType::Plain);
        assert_eq!(resolve_border_type(Some("rounded")), BorderType::Rounded);
        assert_eq!(resolve_border_type(Some("  Rounded  ")), BorderType::Rounded);
        // An unrecognised value falls through to the automatic choice rather
        // than failing; a bad config should not cost you your borders.
        let auto = resolve_border_type(None);
        assert_eq!(resolve_border_type(Some("nonsense")), auto);

        // Unix terminals handle the rounded set.
        #[cfg(not(windows))]
        assert_eq!(auto, BorderType::Rounded);
    }

    #[test]
    fn the_rendered_frame_uses_the_chosen_corner_glyphs() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let screen = render(&mut app, 100, 40).join("\n");
        let (round, square) = (
            screen.contains('\u{256d}'),
            screen.contains('\u{250c}'),
        );
        assert!(round ^ square, "exactly one corner style should be on screen");
        assert_eq!(round, border_type() == BorderType::Rounded);
    }

    /// Names are often Japanese here, and CJK characters take two cells. Using
    /// the character count to pad pushed everything after a Japanese name two
    /// columns right and off the edge.
    #[test]
    fn width_and_padding_count_cells_not_characters() {
        assert_eq!(width("work"), 4);
        assert_eq!(width("社内Wiki"), 8, "two cells per CJK character");
        assert_eq!("社内Wiki".chars().count(), 6, "which is not the character count");

        assert_eq!(width(&pad_to("社内Wiki", 12)), 12);
        assert_eq!(width(&pad_to("work", 12)), 12);
        // Already at or past the target: left alone rather than truncated.
        assert_eq!(pad_to("work", 2), "work");
    }

    /// Paths identify themselves at the end, URLs at the start. Cutting either
    /// end loses what tells them apart, so the middle goes.
    #[test]
    fn middle_truncation_keeps_both_ends() {
        assert_eq!(truncate_middle("short", 20), "short");
        let long = "/var/log/application/deploy/current/output.log";
        let cut = truncate_middle(long, 20);
        assert!(width(&cut) <= 20, "must fit: {:?} is {}", cut, width(&cut));
        assert!(cut.starts_with("/var"), "keeps the head: {:?}", cut);
        assert!(cut.ends_with(".log"), "keeps the tail: {:?}", cut);
        assert!(cut.contains('…'));

        // Wide characters cost two cells here too.
        let jp = truncate_middle("社内ドキュメント一覧ページ", 10);
        assert!(width(&jp) <= 10, "{:?} is {} cells", jp, width(&jp));

        // Degenerate widths must not panic or overrun.
        for w in 0..6 {
            let out = truncate_middle("/some/path/file.txt", w);
            assert!(width(&out) <= w.max(1), "w={} gave {:?}", w, out);
        }
    }

    #[test]
    fn visual_a_selects_the_whole_listing() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt", "d.txt"]);
        app.handle_key(key('v')).unwrap();
        assert_eq!(app.mode, Mode::Visual);
        app.handle_key(key('a')).unwrap();

        assert_eq!(app.visual_anchor, Some(0), "anchored at the top");
        // 4 files plus the `..` row → last index is 4.
        assert_eq!(app.active_pane().unwrap().cursor, 4, "cursor at the bottom");

        // Enter commits the range to marks; `..` is never marked, so 4 files.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.active_pane().unwrap().mark_count(), 4);
    }

    /// The other route the user asked for: gg, visual, G.
    #[test]
    fn gg_then_visual_then_g_selects_everything() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt", "d.txt"]);
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 2;
        }
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('g')).unwrap();
        assert_eq!(app.active_pane().unwrap().cursor, 0);

        app.handle_key(key('v')).unwrap();
        app.handle_key(key('G')).unwrap();
        // 4 files plus the `..` row → last index is 4.
        assert_eq!(app.active_pane().unwrap().cursor, 4, "G must move in visual mode too");

        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.active_pane().unwrap().mark_count(), 4);
    }

    #[test]
    fn gg_works_inside_visual_mode() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        // Start on the last file (index 3, after the `..` row) so the range up
        // to the top covers all three files.
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 3;
        }
        app.handle_key(key('v')).unwrap();
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('g')).unwrap();
        assert_eq!(app.active_pane().unwrap().cursor, 0);
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.active_pane().unwrap().mark_count(), 3);
    }

    /// Ctrl+<key> used to fall through to the plain-character arm, so every
    /// Ctrl combination typed its bare letter into the field.
    ///
    /// Checked with a binding that does nothing rather than Ctrl+V: that one
    /// really does paste, and asserting on the result would depend on whatever
    /// happened to be on the machine's clipboard.
    #[test]
    fn unbound_ctrl_keys_do_not_type_their_letter_into_a_text_field() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_shortcut_add(Vec::new(), false);
        app.handle_key(key('w')).unwrap();
        for c in ['x', 'a', 'k'] {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)).unwrap();
        }
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no field") };
        assert_eq!(buffer, "w", "a Ctrl combination leaked its letter");
    }

    #[test]
    fn ctrl_u_clears_the_field() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_shortcut_add(Vec::new(), false);
        for c in "typo".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no field") };
        assert!(buffer.is_empty());
    }

    /// A new shortcut is nearly always for the thing under the cursor, so the
    /// target starts filled in rather than blank.
    #[test]
    fn a_new_shortcut_defaults_its_target_to_the_current_entry() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 1;
        }
        let expected = app.active_pane().unwrap().selected().unwrap().path.clone();

        app.start_shortcut_add(Vec::new(), false);
        for c in "mine".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        let Popup::TextInput { buffer, kind, .. } = &app.popup else { panic!("no target step") };
        assert!(matches!(kind, InputKind::ShortcutTarget { .. }));
        assert_eq!(buffer, &expected.display().to_string());
    }

    /// `A` makes a folder in the current level; Enter steps in; `A` again nests;
    /// Esc/← climbs back out. The tree is what gets saved.
    #[test]
    fn shortcuts_menu_creates_and_navigates_nested_folders() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Bookmarks live in a temp dir so the test never touches the real config,
        // and start empty so indices are predictable regardless of the dev's own.
        let sd = tempfile::tempdir().unwrap();
        app.shortcuts.path = sd.path().join("shortcuts.lua");
        app.shortcuts.entries.clear();

        // Open the menu and add a top-level folder "Projects" with `A`.
        app.start_shortcuts();
        app.handle_key(key('A')).unwrap();
        for c in "Projects".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // Back in the menu, the folder is there; step into it with Enter.
        assert!(matches!(&app.popup, Popup::Shortcuts { path, .. } if path.is_empty()));
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let Popup::Shortcuts { path, .. } = &app.popup else { panic!("menu closed") };
        assert_eq!(path, &vec![0], "stepped into the folder");

        // Add a leaf shortcut inside it: name then target.
        app.handle_key(key('a')).unwrap();
        for c in "cian".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap(); // name -> target step
        // Clear the auto-filled target and type our own.
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        for c in "~/workspace/cian".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // The store now holds Projects/cian.
        assert_eq!(app.shortcuts.entries.len(), 1);
        let projects = &app.shortcuts.entries[0];
        assert_eq!(projects.name, "Projects");
        assert!(projects.is_group());
        let kids = projects.children.as_ref().unwrap();
        assert_eq!(kids.len(), 1);
        assert_eq!(kids[0].name, "cian");
        assert_eq!(kids[0].target.as_deref(), Some("~/workspace/cian"));

        // Esc climbs back to the top rather than closing.
        assert!(matches!(&app.popup, Popup::Shortcuts { path, .. } if path == &vec![0]));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(&app.popup, Popup::Shortcuts { path, .. } if path.is_empty()));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None), "Esc at the top closes the menu");
    }

    /// Wait for the search worker to finish, draining as it goes.
    fn drain_find(app: &mut App) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            app.poll_find_job();
            if app.find_job.as_ref().and_then(|j| j.done).is_some() {
                app.poll_find_job();
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("search did not finish");
    }

    fn find_tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("src/deep")).unwrap();
        std::fs::create_dir_all(d.path().join("build")).unwrap();
        std::fs::write(d.path().join("readme.md"), b"").unwrap();
        std::fs::write(d.path().join("src/main.rs"), b"").unwrap();
        std::fs::write(d.path().join("src/deep/main.rs"), b"").unwrap();
        std::fs::write(d.path().join("build/main.o"), b"").unwrap();
        d
    }

    #[test]
    fn shift_f_searches_the_tree_below_the_pane() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT)).unwrap();
        let Popup::TextInput { kind, .. } = &app.popup else { panic!("no prompt") };
        assert!(matches!(kind, InputKind::FindRecursive));

        for c in "main".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        drain_find(&mut app);

        let Popup::FindResults { hits, .. } = &app.popup else { panic!("no results") };
        assert_eq!(hits.len(), 3, "got {:?}", hits.iter().map(|h| &h.rel).collect::<Vec<_>>());
    }

    #[test]
    fn choosing_a_grep_hit_opens_the_viewer_at_that_line() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("code.txt"),
            "first line\nsecond has TARGET here\nthird line\n",
        )
        .unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.start_find("TARGET", cian_core::search::Mode::Content);
        drain_find(&mut app);
        let has_hit = matches!(&app.popup, Popup::FindResults { hits, .. } if !hits.is_empty());
        assert!(has_hit, "grep found the line");

        app.open_find_hit().unwrap();
        // The viewer opened on the matched line (line 2 → 0-based index 1).
        match &app.popup {
            Popup::Viewer { line, view, .. } => {
                assert_eq!(*line, 1, "cursor on the matched line");
                assert!(view.lines[*line].contains("TARGET"));
            }
            other => panic!("expected the viewer, got {:?}", other),
        }

        // Esc from the viewer returns to the grep results, not to nothing.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(
            matches!(app.popup, Popup::FindResults { .. }),
            "Esc returns to the results list, got {:?}",
            app.popup
        );
        // A second Esc closes the results.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    /// Choosing a result should leave the pane somewhere useful: in the file's
    /// directory, with the cursor on it.
    #[test]
    fn choosing_a_result_navigates_to_it() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.start_find("main.rs", cian_core::search::Mode::Name);
        drain_find(&mut app);
        // Pick the deepest hit, whichever position it landed in.
        let idx = match &app.popup {
            Popup::FindResults { hits, .. } => hits
                .iter()
                .position(|h| h.rel.to_string_lossy().contains("deep"))
                .expect("expected a hit under src/deep"),
            _ => panic!("no results"),
        };
        if let Popup::FindResults { cursor, .. } = &mut app.popup {
            *cursor = idx;
        }
        app.open_find_hit().unwrap();

        assert!(matches!(app.popup, Popup::None), "the popup should close");
        let pane = app.active_pane().unwrap();
        assert_eq!(pane.cwd.file_name().unwrap(), "deep");
        assert_eq!(pane.selected().unwrap().name, "main.rs");
        assert!(app.find_job.is_none(), "the worker should be released");
    }

    #[test]
    fn a_search_with_no_matches_says_so_rather_than_hanging() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.start_find("nothing-matches-this", cian_core::search::Mode::Name);
        drain_find(&mut app);
        let Popup::FindResults { hits, .. } = &app.popup else { panic!("no results popup") };
        assert!(hits.is_empty());
        assert_eq!(app.find_job.as_ref().unwrap().done, Some(cian_core::search::Outcome::Complete));
    }

    #[test]
    fn closing_the_results_stops_the_worker() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.start_find("main", cian_core::search::Mode::Name);
        assert!(app.find_job.is_some());
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.find_job.is_none(), "Esc must release the search");
    }

    #[test]
    fn ctrl_f_greps_inside_files_and_reports_the_line() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "one\nTODO: fix\nthree\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "nothing\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL)).unwrap();
        let Popup::TextInput { kind, .. } = &app.popup else { panic!("no prompt") };
        assert!(matches!(kind, InputKind::GrepRecursive));
        for c in "todo".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        drain_find(&mut app);

        let Popup::FindResults { hits, .. } = &app.popup else { panic!("no results") };
        assert_eq!(hits.len(), 1);
        let (n, text) = hits[0].line.clone().expect("a content hit carries its line");
        assert_eq!(n, 2, "1-based line number");
        assert_eq!(text, "TODO: fix");
    }

    #[test]
    fn the_menu_offers_the_new_entries() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        // Top level carries the groups + Shortcuts; the old flat entries now
        // live one level down.
        for want in [MenuItem::InspectMenu, MenuItem::ViewMenu, MenuItem::Shortcuts] {
            assert!(items.contains(&want), "{:?} missing from {:?}", want, items);
        }
        // Attributes / Hash are under Inspect ▸; Show-hidden is under View ▸.
        let inspect = app.submenu_children(MenuItem::InspectMenu).unwrap();
        assert!(inspect.contains(&MenuItem::Attributes) && inspect.contains(&MenuItem::Hash));
        let view = app.submenu_children(MenuItem::ViewMenu).unwrap();
        assert!(view.contains(&MenuItem::HiddenToggle));
    }

    /// `M` opens the context menu on every terminal (Shift+Enter can't be
    /// distinguished from Enter on e.g. macOS Terminal.app).
    #[test]
    fn m_key_opens_the_context_menu() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.focus(FocusedPane::Left);
        app.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::ContextMenu { .. }), "M opened the menu");
        // Also works when the terminal doesn't tag the uppercase char with SHIFT.
        app.popup = Popup::None;
        app.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::NONE)).unwrap();
        assert!(matches!(app.popup, Popup::ContextMenu { .. }), "M works without a SHIFT tag too");
    }

    #[test]
    fn the_menu_shortcuts_entry_opens_the_bookmarks() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.run_menu_item(MenuItem::Shortcuts).unwrap();
        assert!(matches!(app.popup, Popup::Shortcuts { .. }), "opened the shortcuts menu");
    }

    #[test]
    fn the_menu_toggles_dotfiles_for_the_focused_pane_only() {
        let (_d, mut app) = app_with(&["a.txt", ".hidden"]);
        app.focus(FocusedPane::Left);
        // Counts include the `..` row: 2 files + `..` = 3.
        assert_eq!(app.left.active_ref().entries.len(), 3);

        app.run_menu_item(MenuItem::HiddenToggle).unwrap();
        assert_eq!(app.left.active_ref().entries.len(), 2, "dotfile hidden here");
        assert_eq!(app.right.active_ref().entries.len(), 3, "and not in the other pane");
    }

    /// Dragging from one pane to the other should raise the transfer
    /// confirmation, not act silently.
    #[test]
    fn dragging_between_panes_offers_a_transfer() {
        let (_l, r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let (left, right) = (app.layout_rects.left, app.layout_rects.right);

        // Row 1 is `..`; press on the file on row 2.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 2));
        assert!(app.file_drag.is_some(), "pressing on an entry arms a drag");

        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            right.x + 5,
            right.y + 2,
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 2));

        let Popup::ConfirmTransfer { op, targets, dest } = &app.popup else {
            panic!("expected a transfer confirmation, got {:?}", app.popup)
        };
        assert_eq!(*op, PendingOp::Copy, "a plain drag copies");
        assert_eq!(targets.len(), 1);
        assert_eq!(dest.file_name(), r.path().file_name());
        assert!(app.file_drag.is_none(), "the drag is released");
    }

    #[test]
    fn shift_dragging_moves_instead_of_copying() {
        let (_l, _r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let (left, right) = (app.layout_rects.left, app.layout_rects.right);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), right.x + 5, right.y + 2));
        let mut up = mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 2);
        up.modifiers = KeyModifiers::SHIFT;
        app.handle_mouse(up);

        let Popup::ConfirmTransfer { op, .. } = &app.popup else { panic!("no confirmation") };
        assert_eq!(*op, PendingOp::Move);
    }

    /// Regression: a click that the terminal reported with a stray same-row
    /// Drag used to mark that row. Clicking file A then file B then A must
    /// leave the marks untouched — a bare click is not a mark.
    #[test]
    fn clicking_files_never_marks_them() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        // Rows: 1 = `..`, 2 = a.txt, 3 = b.txt, 4 = c.txt.
        for cy in [left.y + 2, left.y + 3, left.y + 2] {
            // A press, a same-row drag (the terminal's jitter), then release.
            app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, cy));
            app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), left.x + 3, cy));
            app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 3, cy));
        }
        assert_eq!(app.active_pane().unwrap().mark_count(), 0, "clicks must not mark");
    }

    /// The `..` row navigates up on a single click, and can never be marked.
    #[test]
    fn the_parent_row_navigates_up_and_is_never_marked() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/inner.txt"), b"x").unwrap();
        let start = d.path().join("sub");
        let mut app = App::new(start.clone(), start, cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        // The first row is `..`; a single click steps up to the parent.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, left.y + 1));
        assert!(!app.left.active_ref().cwd.ends_with("sub"), "left sub via ..");
        // Marking the `..` row (e.g. via Space on it) is a no-op.
        if let Some(p) = app.active_pane_mut() {
            p.cursor = 0; // back onto `..`
            p.toggle_mark_at(0);
        }
        assert_eq!(app.active_pane().unwrap().mark_count(), 0, "`..` is never marked");
    }

    /// Press and release without moving is a click. It must not transfer
    /// anything, or every click would raise a dialog.
    #[test]
    fn a_click_without_movement_is_not_a_drag() {
        let (_l, _r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 1));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 5, left.y + 1));
        assert!(matches!(app.popup, Popup::None), "a click must not start a transfer");
        assert!(app.file_drag.is_none());
    }

    #[test]
    fn dropping_back_on_the_same_pane_does_nothing() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt", "b.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 1));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), left.x + 5, left.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 5, left.y + 2));
        assert!(matches!(app.popup, Popup::None));
    }

    /// The nearest thing to dragging a file into a terminal.
    #[test]
    fn dragging_onto_the_shell_types_the_paths() {
        let (_l, _r, mut app) = app_two_dirs(&["doc.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let (left, shell) = (app.layout_rects.left, app.layout_rects.shell);

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), shell.x + 5, shell.y + 2));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), shell.x + 5, shell.y + 2));

        assert_eq!(app.focused, FocusedPane::Shell);
        let queued = app.pending_shell_input.clone().unwrap_or_default();
        assert!(queued.contains("doc.txt"), "got {:?}", queued);
        assert!(!queued.ends_with('\n'), "paths are typed, not run");
    }

    #[test]
    fn destinations_are_remembered_most_recent_first_and_deduped() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.remember_dest(Path::new("/tmp/one"));
        app.remember_dest(Path::new("/tmp/two"));
        app.remember_dest(Path::new("/tmp/one"));
        assert_eq!(
            app.dest_history,
            vec![PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")],
            "re-using a destination promotes it rather than duplicating it"
        );

        for i in 0..DEST_HISTORY_CAP + 5 {
            app.remember_dest(&PathBuf::from(format!("/tmp/d{}", i)));
        }
        assert_eq!(app.dest_history.len(), DEST_HISTORY_CAP, "the list is capped");
    }

    #[test]
    fn the_destination_picker_leads_with_the_other_pane() {
        let (_l, r, mut app) = app_two_dirs(&["a.txt"], &[]);
        app.remember_dest(Path::new("/tmp/somewhere"));
        app.focus(FocusedPane::Left);
        app.start_dest_picker(PendingOp::Copy);

        assert!(matches!(app.popup, Popup::DestPicker { .. }));
        let choices = app.dest_choices();
        assert_eq!(choices[0].0, "other pane");
        assert_eq!(choices[0].1.file_name(), r.path().file_name());
        assert!(choices.iter().any(|(k, p)| k == "recent" && p == Path::new("/tmp/somewhere")));
    }

    /// Two panes, one file each, both cursors on the first entry.
    fn two_panes_with(
        a: &str,
        b: &str,
    ) -> (tempfile::TempDir, tempfile::TempDir, App) {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("a.txt"), a).unwrap();
        std::fs::write(r.path().join("b.txt"), b).unwrap();
        let app = App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
            .unwrap();
        (l, r, app)
    }

    #[test]
    fn equals_compares_the_two_panes_files() {
        let (_l, _r, mut app) = two_panes_with("one\ntwo\nthree\n", "one\nTWO\nthree\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();

        let Popup::Diff { left, right, result, .. } = &app.popup else {
            panic!("expected the diff, got {:?}", app.popup)
        };
        assert_eq!((left.as_str(), right.as_str()), ("a.txt", "b.txt"));
        assert_eq!(result.changed, 1);
        assert!(!result.identical);
    }

    #[test]
    fn identical_files_report_a_notice_not_an_empty_diff() {
        let (_l, _r, mut app) = two_panes_with("same\nlines\n", "same\nlines\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();
        match &app.popup {
            Popup::Notice { lines } => assert!(lines.iter().any(|l| l.contains("identical"))),
            other => panic!("expected an identical notice, got {:?}", other),
        }
    }

    #[test]
    fn a_diff_can_be_copied_and_saved() {
        let (l, _r, mut app) = two_panes_with("one\ntwo\n", "one\nTWO\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::Diff { .. }));

        // c copies a unified-style text with the changed lines. (On a headless
        // CI box there is no system clipboard, so accept that outcome too.)
        app.handle_key(code(KeyCode::Char('c'))).unwrap();
        let msg = app.message.as_deref().unwrap_or("");
        assert!(msg.contains("diff copied") || msg.contains("clipboard unavailable"), "got {msg:?}");

        // w prompts for a filename; saving writes it into the active pane's dir
        // (the left pane, which is focused by default).
        app.handle_key(code(KeyCode::Char('w'))).unwrap();
        assert!(matches!(&app.popup, Popup::TextInput { kind: InputKind::DiffSaveAs { .. }, .. }));
        // Clear the default and type a name.
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        for c in "out.diff".chars() { app.handle_key(key(c)).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let saved = std::fs::read_to_string(l.path().join("out.diff")).unwrap();
        assert!(saved.contains("- two") && saved.contains("+ TWO"), "saved diff:\n{saved}");
    }

    #[test]
    fn the_diff_can_be_searched() {
        // Put a distinctive word far down so a search has to move the view.
        let mut a: Vec<String> = (0..30).map(|i| format!("line {}", i)).collect();
        let b = a.clone();
        a[25] = "NEEDLE here".into();
        let (_l, _r, mut app) = two_panes_with(&(a.join("\n") + "\n"), &(b.join("\n") + "\n"));
        app.handle_key(code(KeyCode::Char('='))).unwrap();
        // Unfold so every row is present and the index is predictable.
        app.handle_key(code(KeyCode::Char('f'))).unwrap();

        // /NEEDLE<CR> jumps the view to the matching row and remembers the query.
        app.handle_key(code(KeyCode::Char('/'))).unwrap();
        for c in "needle".chars() { app.handle_key(key(c)).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let Popup::Diff { find, scroll, .. } = &app.popup else { panic!("no diff") };
        assert_eq!(find.as_deref(), Some("needle"), "query kept");
        assert_eq!(*scroll, 25, "jumped to the matching row");

        // Esc clears the search but keeps the diff open.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(&app.popup, Popup::Diff { find: None, .. }));
    }

    /// Which pane holds the focus must not decide which file is the "before".
    #[test]
    fn the_left_pane_is_always_the_left_side() {
        let (_l, _r, mut app) = two_panes_with("old\n", "new\n");
        app.focus(FocusedPane::Right);
        app.handle_key(code(KeyCode::Char('='))).unwrap();

        let Popup::Diff { result, left, .. } = &app.popup else { panic!("no diff") };
        assert_eq!(left, "a.txt");
        match &result.rows[0] {
            cian_core::diff::Row::Changed { left, right } => {
                assert_eq!((left.text.as_str(), right.text.as_str()), ("old", "new"));
            }
            other => panic!("expected a change, got {:?}", other),
        }
    }

    #[test]
    fn comparing_a_directory_against_a_file_is_refused() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::create_dir(l.path().join("adir")).unwrap();
        std::fs::write(r.path().join("b.txt"), "x").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();

        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("not one of each"));
    }

    #[test]
    fn comparing_two_directories_lists_the_differences() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::create_dir(l.path().join("proj")).unwrap();
        std::fs::create_dir(r.path().join("proj")).unwrap();
        std::fs::write(l.path().join("proj/same.txt"), b"xy").unwrap();
        std::fs::write(r.path().join("proj/same.txt"), b"xy").unwrap();
        // Equal size AND mtime, so the quick compare treats them as identical.
        let t = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        cian_core::dirdiff::set_mtime(&l.path().join("proj/same.txt"), t).unwrap();
        cian_core::dirdiff::set_mtime(&r.path().join("proj/same.txt"), t).unwrap();
        std::fs::write(l.path().join("proj/only_left.txt"), b"l").unwrap();
        std::fs::write(r.path().join("proj/changed.txt"), b"aaaa").unwrap();
        std::fs::write(l.path().join("proj/changed.txt"), b"a").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        // Cursor on "proj" in each pane (index 0 is the `..` row).
        app.left.active_mut().cursor = 1;
        app.right.active_mut().cursor = 1;

        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(app.diff_job.is_some(), "comparison started on a worker");
        // Drain the worker.
        for _ in 0..200 {
            if app.diff_job.is_none() { break; }
            app.poll_diff_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let Popup::DirCompare { entries, .. } = &app.popup else {
            panic!("expected the comparison, got {:?}", app.popup)
        };
        let paths: Vec<String> =
            entries.iter().map(|e| e.rel.display().to_string().replace('\\', "/")).collect();
        // Paths are relative to the compared folders (proj), not the roots.
        assert!(paths.contains(&"only_left.txt".to_string()), "{:?}", paths);
        assert!(paths.contains(&"changed.txt".to_string()), "{:?}", paths);
        assert!(!paths.contains(&"same.txt".to_string()), "identical file omitted: {:?}", paths);
    }

    #[test]
    fn an_empty_pane_reports_rather_than_opening_an_empty_diff() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(r.path().join("b.txt"), "x").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();

        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap().contains("select a file"));
    }

    #[test]
    fn n_jumps_to_the_next_difference_and_f_unfolds() {
        // Two differences far enough apart that folding hides the gap.
        let mut a: Vec<String> = (0..40).map(|i| format!("line {}", i)).collect();
        let b = a.clone();
        a[5] = "first change".into();
        a[30] = "second change".into();
        let (_l, _r, mut app) =
            two_panes_with(&(a.join("\n") + "\n"), &(b.join("\n") + "\n"));
        app.handle_key(code(KeyCode::Char('='))).unwrap();

        let Popup::Diff { folded, scroll, fold, .. } = &app.popup else { panic!("no diff") };
        assert!(*fold, "opens folded");
        assert_eq!(*scroll, 0);
        let folded_len = folded.len();

        app.handle_key(code(KeyCode::Char('n'))).unwrap();
        let Popup::Diff { folded, scroll, .. } = &app.popup else { panic!("no diff") };
        assert!(folded[*scroll].is_difference(), "n landed on a change");
        let first = *scroll;

        app.handle_key(code(KeyCode::Char('n'))).unwrap();
        let Popup::Diff { folded, scroll, .. } = &app.popup else { panic!("no diff") };
        assert!(*scroll > first && folded[*scroll].is_difference(), "and on to the next");
        let second = *scroll;

        app.handle_key(code(KeyCode::Char('N'))).unwrap();
        let Popup::Diff { scroll, .. } = &app.popup else { panic!("no diff") };
        assert_eq!(*scroll, first, "N goes back");
        assert!(second > first);

        app.handle_key(code(KeyCode::Char('f'))).unwrap();
        let Popup::Diff { fold, result, scroll, .. } = &app.popup else { panic!("no diff") };
        assert!(!*fold);
        assert_eq!(*scroll, 0, "the row lists differ in length; the old offset is meaningless");
        assert!(result.rows.len() > folded_len, "unfolding shows more");
    }

    #[test]
    fn esc_closes_the_diff() {
        let (_l, _r, mut app) = two_panes_with("a\n", "b\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();
        assert!(matches!(app.popup, Popup::Diff { .. }));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn the_diff_renders_without_panicking_at_any_size() {
        let (_l, _r, mut app) = two_panes_with("one\ntwo\n", "one\nTWO\nthree\n");
        app.handle_key(code(KeyCode::Char('='))).unwrap();
        let wide = render(&mut app, 120, 30).join("\n");
        assert!(wide.contains("a.txt ↔ b.txt"), "both names in the title:\n{}", wide);
        assert!(wide.contains("two") && wide.contains("TWO"), "both sides shown:\n{}", wide);
        assert!(wide.contains("three"), "the added line too:\n{}", wide);

        // Narrow enough that the column arithmetic would underflow if it were
        // not saturating.
        for (w, h) in [(80u16, 24u16), (24, 8), (10, 5)] {
            render(&mut app, w, h);
        }
    }

    /// Wait for a background file operation to finish.
    fn drain_op(app: &mut App) {
        for _ in 0..200 {
            if app.op_job.is_none() { break; }
            app.poll_op_job();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Run a `:`-command as if it were typed and Enter pressed.
    fn run_cmd(app: &mut App, line: &str) {
        app.command_buffer = line.to_string();
        app.mode = Mode::Command;
        app.run_command();
    }

    /// A terminal with the kitty keyboard protocol (WezTerm, kitty) reports the
    /// Shift held to type `:`, so the binding must not require Shift to be
    /// absent — otherwise `:` does nothing there and command mode is unreachable.
    #[test]
    fn colon_opens_command_mode_even_with_shift_reported() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Char(':'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(app.mode, Mode::Command, "Shift+: must still enter command mode");
        // And it still works without the modifier (a plain-PTY terminal).
        app.mode = Mode::Normal;
        app.handle_key(code(KeyCode::Char(':'))).unwrap();
        assert_eq!(app.mode, Mode::Command);
    }

    /// The other shifted-punctuation bindings, likewise reachable with the
    /// modifier set.
    #[test]
    fn punctuation_bindings_ignore_the_shift_modifier() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(app.mode, Mode::Filter, "/ opens the filter regardless of shift");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        app.handle_key(KeyEvent::new(KeyCode::Char(','), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::SortPicker { .. }), ", opens the sort picker");
    }

    #[test]
    fn mkdir_makes_a_directory_and_dash_p_makes_the_chain() {
        let (d, mut app) = app_with(&["existing.txt"]);
        run_cmd(&mut app, "mkdir fresh");
        assert!(d.path().join("fresh").is_dir());
        // Plain mkdir into a missing parent fails and says so.
        run_cmd(&mut app, "mkdir a/b/c");
        assert!(!d.path().join("a/b/c").exists());
        assert!(app.message.as_deref().unwrap().to_lowercase().contains("mkdir"));
        // -p builds the whole path.
        run_cmd(&mut app, "mkdir -p a/b/c");
        assert!(d.path().join("a/b/c").is_dir());
        // The new entries show up without an explicit refresh.
        assert!(app.active_pane().unwrap().all_entries.iter().any(|e| e.name == "fresh"));
    }

    #[test]
    fn touch_creates_a_file_that_appears_in_the_listing() {
        let (d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "touch new.log");
        assert!(d.path().join("new.log").is_file());
        assert!(app.active_pane().unwrap().all_entries.iter().any(|e| e.name == "new.log"));
    }

    #[test]
    fn pwd_reports_and_copies_the_directory() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Compare against the pane's canonicalised cwd, which is what pwd prints.
        let cwd = app.active_pane().unwrap().cwd.display().to_string();
        run_cmd(&mut app, "pwd");
        let msg = app.message.clone().unwrap();
        assert!(msg.contains(&cwd), "msg {:?} should contain {:?}", msg, cwd);
        assert!(msg.contains("copied"));
    }

    #[test]
    fn cp_with_no_argument_targets_the_other_pane() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("doc.txt"), b"hi").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        // The pane canonicalises its cwd (differently per platform), so compare
        // against the pane's own path rather than the raw tempdir.
        let right_cwd = app.right.active_ref().cwd.clone();
        run_cmd(&mut app, "cp");
        // Opens the confirm-transfer popup aimed at the right pane.
        match &app.popup {
            Popup::ConfirmTransfer { op, dest, targets } => {
                assert_eq!(*op, PendingOp::Copy);
                assert_eq!(*dest, right_cwd);
                assert_eq!(targets.len(), 1);
            }
            other => panic!("expected a transfer confirm, got {:?}", other),
        }
    }

    #[test]
    fn mv_with_a_path_renames_a_single_file() {
        let (d, mut app) = app_with(&["old.txt", "z.txt"]);
        // Cursor on the first file (index 0 is the `..` row): old.txt.
        app.active_pane_mut().unwrap().cursor = 1;
        let first = app.active_pane().unwrap().selected().unwrap().name.clone();
        run_cmd(&mut app, &format!("mv {}", d.path().join("renamed.txt").display()));
        assert!(d.path().join("renamed.txt").is_file(), "moved to the new name");
        assert!(!d.path().join(&first).exists(), "original is gone");
    }

    #[test]
    fn rm_asks_before_deleting() {
        let (_d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "rm");
        assert!(matches!(app.popup, Popup::ConfirmDelete { .. }), "rm confirms first");
    }

    #[test]
    fn ls_dash_a_toggles_hidden() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let before = app.active_pane().unwrap().show_hidden;
        run_cmd(&mut app, "ls -a");
        assert_ne!(app.active_pane().unwrap().show_hidden, before);
    }

    #[test]
    fn file_and_wc_open_a_notice() {
        let (d, mut app) = app_with(&["notes.txt"]);
        std::fs::write(d.path().join("notes.txt"), "one two three\nsecond line\n").unwrap();
        app.reload_active();
        app.active_pane_mut().unwrap().cursor = 1; // notes.txt (index 0 is `..`)

        run_cmd(&mut app, "file");
        let Popup::Notice { lines } = &app.popup else { panic!("file → notice") };
        assert!(lines.iter().any(|l| l.contains("text")), "{:?}", lines);

        run_cmd(&mut app, "wc");
        let Popup::Notice { lines } = &app.popup else { panic!("wc → notice") };
        // 2 newlines, 5 words.
        assert!(lines.iter().any(|l| l.contains(" 2 ") && l.contains(" 5 ")), "{:?}", lines);
    }

    #[test]
    fn head_and_tail_show_the_right_ends() {
        let (d, mut app) = app_with(&["log.txt"]);
        let text: String = (1..=50).map(|i| format!("line {}\n", i)).collect();
        std::fs::write(d.path().join("log.txt"), text).unwrap();
        app.reload_active();
        app.active_pane_mut().unwrap().cursor = 1; // log.txt (index 0 is `..`)

        run_cmd(&mut app, "head -n 2");
        let Popup::Notice { lines } = &app.popup else { panic!("head → notice") };
        assert!(lines.iter().any(|l| l == "line 1"));
        assert!(!lines.iter().any(|l| l == "line 3"), "only 2 asked for: {:?}", lines);

        run_cmd(&mut app, "tail -n 2");
        let Popup::Notice { lines } = &app.popup else { panic!("tail → notice") };
        assert!(lines.iter().any(|l| l == "line 50"));
        assert!(lines.iter().any(|l| l == "line 49"));
    }

    #[test]
    fn df_reports_free_space() {
        let (_d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "df -h");
        let Popup::Notice { lines } = &app.popup else { panic!("df → notice") };
        assert!(lines.iter().any(|l| l.starts_with("total")));
        assert!(lines.iter().any(|l| l.starts_with("available")));

        run_cmd(&mut app, "df -z");
        assert!(app.message.as_deref().unwrap().contains("unknown flag"), "bad flag reported");
    }

    #[test]
    fn zip_bundles_the_selection() {
        let (d, mut app) = app_with(&["one.txt", "two.txt"]);
        std::fs::write(d.path().join("one.txt"), b"1").unwrap();
        // Mark both so the whole selection is zipped.
        app.reload_active();
        let paths: Vec<PathBuf> =
            app.active_pane().unwrap().all_entries.iter().map(|e| e.path.clone()).collect();
        for p in paths {
            app.active_pane_mut().unwrap().marks.insert(p);
        }
        run_cmd(&mut app, "zip bundle");
        drain_op(&mut app);
        assert!(d.path().join("bundle.zip").is_file(), "zip created");
        let names: Vec<String> = cian_core::archive::list(&d.path().join("bundle.zip"))
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert!(names.contains(&"one.txt".to_string()), "{:?}", names);
    }

    #[test]
    fn zip_dash_e_asks_for_a_password_which_is_masked() {
        let (d, mut app) = app_with(&["secret.txt"]);
        app.active_pane_mut().unwrap().cursor = 1; // secret.txt (index 0 is `..`)
        run_cmd(&mut app, "zip -e locked");
        match &app.popup {
            Popup::TextInput { kind, .. } => {
                assert!(kind.is_secret(), "the password field is a secret");
            }
            other => panic!("expected a password prompt, got {:?}", other),
        }
        // The masked field renders as dots, not the typed text.
        app.handle_key(code(KeyCode::Char('p'))).unwrap();
        app.handle_key(code(KeyCode::Char('w'))).unwrap();
        let shown = render(&mut app, 80, 20).join("\n");
        assert!(shown.contains("••"), "password shown masked:\n{}", shown);
        assert!(!shown.contains(">pw"), "the literal password must not appear");
        let _ = d;
    }

    #[test]
    fn bang_runs_in_the_shell_with_substitutions() {
        let (d, mut app) = app_with(&["target file.txt"]);
        app.active_pane_mut().unwrap().cursor = 1; // the file (index 0 is `..`)
        run_cmd(&mut app, "!echo %f");
        assert_eq!(app.focused, FocusedPane::Shell, "hands over to the shell");
        // No shell spawned in tests, so the command is queued verbatim.
        let queued = app.pending_shell_input.clone().unwrap_or_default();
        assert!(queued.starts_with("echo "), "got {:?}", queued);
        // The filename has a space, so it must be quoted as one argument.
        assert!(queued.contains("target file.txt"), "the file path is substituted: {:?}", queued);
        assert!(queued.contains('\''), "quoted because of the space: {:?}", queued);
        let _ = d;
    }

    #[test]
    fn an_unknown_command_says_so() {
        let (_d, mut app) = app_with(&["a.txt"]);
        run_cmd(&mut app, "frobnicate");
        assert!(app.message.as_deref().unwrap().contains("unknown command"));
    }

    #[test]
    fn paste_lands_in_the_command_line() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.mode = Mode::Command;
        app.command_buffer = "cd ".into();
        // A bracketed-paste event carrying a path, with a stray newline.
        app.insert_into_active_text("/some/path\n");
        assert_eq!(app.command_buffer, "cd /some/path", "newline stripped, text appended");
    }

    #[test]
    fn o_on_a_file_mirrors_the_directory_to_the_other_pane() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("doc.txt"), b"x").unwrap();
        std::fs::create_dir(r.path().join("elsewhere")).unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        app.focus(FocusedPane::Left);
        app.active_pane_mut().unwrap().cursor = 1; // doc.txt (a file; index 0 is `..`)
        app.open_in_other_pane(false).unwrap();
        assert_eq!(
            app.right.active_ref().cwd,
            app.left.active_ref().cwd,
            "the other pane lines up on this directory"
        );
    }

    #[test]
    fn f_keys_manage_file_tabs() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        assert_eq!(app.left.tabs.len(), 1);
        app.handle_key(code(KeyCode::F(9))).unwrap(); // new tab
        assert_eq!(app.left.tabs.len(), 2);
        assert_eq!(app.left.active, 1);
        app.handle_key(code(KeyCode::F(1))).unwrap(); // previous
        assert_eq!(app.left.active, 0);
        app.handle_key(code(KeyCode::F(2))).unwrap(); // next
        assert_eq!(app.left.active, 1);
    }

    #[test]
    fn ctrl_digit_no_longer_jumps_file_tabs() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.left.add_clone().unwrap(); // now 2 tabs, active 1
        // Ctrl+1 used to select tab 0; it must not any more.
        app.handle_key(KeyEvent::new(KeyCode::Char('1'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(app.left.active, 1, "Ctrl+1 is no longer a tab jump");
    }

    #[test]
    fn the_default_home_prefers_config_then_desktop() {
        // A configured home directory wins when it exists.
        let d = tempfile::tempdir().unwrap();
        let mut config = cian_lua::Config::default();
        config.options.home = Some(d.path().display().to_string());
        assert_eq!(default_home(&config), d.path());

        // A configured but missing directory falls through (to Desktop/home/.).
        let mut config = cian_lua::Config::default();
        config.options.home = Some("/definitely/not/here".into());
        let fallback = default_home(&config);
        assert_ne!(fallback, PathBuf::from("/definitely/not/here"));
    }

    #[test]
    fn a_notice_can_be_copied_then_closes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::Notice { lines: vec!["abc123".into()] };
        app.handle_key(code(KeyCode::Char('y'))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(app.message.as_deref(), Some("copied"));
    }

    #[test]
    fn double_clicking_a_directory_enters_it() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/inner.txt"), b"x").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p.clone(), cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40);
        let r = app.layout_rects.left;
        // Row 1 is the `..` row; "sub" (dirs first) is on row 2.
        let (cx, cy) = (r.x + 3, r.y + 2);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), cx, cy));
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));

        // Compare by the final component (the pane canonicalises differently
        // per platform than std::fs::canonicalize).
        assert!(
            app.left.active_ref().cwd.ends_with("sub"),
            "double-click entered the directory: {:?}",
            app.left.active_ref().cwd
        );
    }

    #[test]
    fn a_slow_second_click_is_not_a_double_click() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p.clone(), cian_lua::Config::default()).unwrap();
        let _ = render(&mut app, 100, 40);
        let root = app.left.active_ref().cwd.clone();
        let r = app.layout_rects.left;
        // Row 2 is "sub"; row 1 is the `..` row (which would navigate up).
        let (cx, cy) = (r.x + 3, r.y + 2);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), cx, cy));
        // Age the first click past the double-click window.
        app.last_click = Some((Instant::now() - Duration::from_secs(2), cy));
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        assert_eq!(app.left.active_ref().cwd, root,
            "a slow second click just selects, does not enter");
    }

    #[test]
    fn clicking_a_tab_switches_to_it() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.left.add_clone().unwrap(); // second tab, now active
        // Wide enough that the first tab is not collapsed into a +N marker.
        let _ = render(&mut app, 300, 40);
        assert_eq!(app.left.active, 1);

        let (_, _, r) = app
            .tab_rects
            .iter()
            .copied()
            .find(|(p, i, _)| *p == FocusedPane::Left && *i == 0)
            .expect("a rect for the left pane's first tab");
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), r.x + 1, r.y));
        assert_eq!(app.left.active, 0, "clicking the first tab selected it");
        assert_eq!(app.focused, FocusedPane::Left);
    }

    #[test]
    fn the_context_menu_runs_the_item_that_was_clicked() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        let _ = render(&mut app, 100, 40);
        // Open the menu at a known spot, then render so menu_rect is set.
        app.open_context_menu(10, 10);
        let _ = render(&mut app, 100, 40);
        let m = app.menu_rect;
        // The Quit item is second-to-last; click its row.
        let (quit_idx, _) = {
            let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
            items.iter().enumerate().find(|(_, it)| **it == MenuItem::Quit).expect("quit item")
        };
        let row = m.y + 1 + quit_idx as u16;
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), m.x + 2, row));
        assert!(matches!(app.popup, Popup::ConfirmQuit), "clicking Quit opened the confirm");
    }

    #[test]
    fn clicking_off_the_context_menu_dismisses_it() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 40);
        app.open_context_menu(10, 10);
        let _ = render(&mut app, 100, 40);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), 1, 1));
        assert!(matches!(app.popup, Popup::None));
    }

    // ---- keyboard pane resize ----

    fn ctrl_shift(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT)
    }

    #[test]
    fn ctrl_shift_arrows_resize_the_file_panes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        assert_eq!(app.panes_pct, 50);
        assert_eq!(app.main_pct, 60);

        // Right pushes the left|right divider right → left pane grows.
        app.handle_key(ctrl_shift(KeyCode::Right)).unwrap();
        assert!(app.panes_pct > 50, "left grew: {}", app.panes_pct);
        let wider = app.panes_pct;
        app.handle_key(ctrl_shift(KeyCode::Left)).unwrap();
        assert!(app.panes_pct < wider, "left shrank back");

        // Down grows the file area (files|shell divider moves down).
        app.handle_key(ctrl_shift(KeyCode::Down)).unwrap();
        assert!(app.main_pct > 60, "files grew: {}", app.main_pct);
        app.handle_key(ctrl_shift(KeyCode::Up)).unwrap();
        app.handle_key(ctrl_shift(KeyCode::Up)).unwrap();
        assert!(app.main_pct < 60, "and shrank past the start");
    }

    #[test]
    fn resize_is_clamped_so_a_pane_never_vanishes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        for _ in 0..50 {
            app.handle_key(ctrl_shift(KeyCode::Left)).unwrap();
        }
        assert_eq!(app.panes_pct, MIN_SPLIT_PCT, "cannot shrink below the floor");
        for _ in 0..50 {
            app.handle_key(ctrl_shift(KeyCode::Right)).unwrap();
        }
        assert_eq!(app.panes_pct, 100 - MIN_SPLIT_PCT, "nor grow past the ceiling");
    }

    #[test]
    fn from_the_shell_up_down_resizes_the_shell_area() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        assert_eq!(app.main_pct, 60);
        // With no inner split, Up grows the shell (files|shell divider up).
        app.handle_key(ctrl_shift(KeyCode::Up)).unwrap();
        assert!(app.main_pct < 60, "shell grew: {}", app.main_pct);
    }

    // ---- editing, confirms, search, history refinements ----

    #[test]
    fn the_text_field_edits_at_the_caret_not_only_the_end() {
        let (_d, mut app) = app_with(&["report.txt"]);
        app.active_pane_mut().unwrap().cursor = 1; // report.txt (index 0 is `..`)
        app.handle_key(code(KeyCode::Char('r'))).unwrap(); // rename prompt
        // Seeded with the name, caret at the end.
        {
            let Popup::TextInput { buffer, cursor, .. } = &app.popup else { panic!("no prompt") };
            assert_eq!(buffer, "report.txt");
            assert_eq!(*cursor, "report.txt".chars().count());
        }
        // Move left past ".txt" (4 chars) and insert.
        for _ in 0..4 { app.handle_key(code(KeyCode::Left)).unwrap(); }
        for c in "_v2".chars() { app.handle_key(code(KeyCode::Char(c))).unwrap(); }
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "report_v2.txt", "inserted before the extension");

        // Home, then Delete removes the first char.
        app.handle_key(code(KeyCode::Home)).unwrap();
        app.handle_key(code(KeyCode::Delete)).unwrap();
        let Popup::TextInput { buffer, cursor, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "eport_v2.txt");
        assert_eq!(*cursor, 0);

        // Backspace at the start is a no-op, not a panic.
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "eport_v2.txt");
    }

    #[test]
    fn caret_editing_handles_multibyte_characters() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = text_input("t", "p", "あい".to_string(), InputKind::JumpPath);
        // Caret at end (2 chars). Left once → between あ and い. Insert 'X'.
        app.handle_key(code(KeyCode::Left)).unwrap();
        app.handle_key(code(KeyCode::Char('X'))).unwrap();
        let Popup::TextInput { buffer, .. } = &app.popup else { panic!("no prompt") };
        assert_eq!(buffer, "あXい", "insert respects char boundaries");
    }

    #[test]
    fn enter_is_yes_on_a_transfer_confirm() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("doc.txt"), b"hi").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        run_cmd(&mut app, "cp"); // ConfirmTransfer to the right pane
        app.handle_key(code(KeyCode::Enter)).unwrap();
        drain_op(&mut app);
        assert!(r.path().join("doc.txt").is_file(), "Enter confirmed the copy");
    }

    #[test]
    fn r_on_a_move_confirm_renames_into_the_destination() {
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("old.txt"), b"data").unwrap();
        let mut app =
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), cian_lua::Config::default())
                .unwrap();
        app.active_pane_mut().unwrap().cursor = 1; // old.txt (index 0 is `..`)
        app.handle_key(code(KeyCode::Char('m'))).unwrap(); // move confirm
        app.handle_key(code(KeyCode::Char('r'))).unwrap(); // rename & move
        // Seeded with the source name; clear it and type a new one.
        let Popup::TextInput { kind: InputKind::TransferAs { .. }, .. } = &app.popup else {
            panic!("expected the rename prompt, got {:?}", app.popup)
        };
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        for c in "new.txt".chars() { app.handle_key(code(KeyCode::Char(c))).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        assert!(r.path().join("new.txt").is_file(), "moved under the new name");
        assert!(!l.path().join("old.txt").exists(), "and gone from the source");
    }

    #[test]
    fn search_arrows_step_through_the_matches() {
        let (_d, mut app) = app_with(&["a1.txt", "a2.txt", "zzz.txt"]);
        // Sorted: a1, a2, zzz.
        app.handle_key(code(KeyCode::Char('f'))).unwrap(); // search
        app.handle_key(code(KeyCode::Char('a'))).unwrap(); // matches a1, a2
        app.handle_key(code(KeyCode::Down)).unwrap();
        let first = app.active_pane().unwrap().cursor;
        assert!(app.active_pane().unwrap().entries[first].name.contains('a'));
        app.handle_key(code(KeyCode::Down)).unwrap();
        let second = app.active_pane().unwrap().cursor;
        assert_ne!(first, second, "Down moved to the other match");
        assert!(app.active_pane().unwrap().entries[second].name.contains('a'));
    }

    #[test]
    fn history_a_bookmarks_the_selected_path_as_a_shortcut() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Seed some history and open it.
        app.active_pane_mut().unwrap().history =
            vec![PathBuf::from("/tmp/one"), PathBuf::from("/tmp/two")];
        app.handle_key(code(KeyCode::Char('h'))).unwrap();
        assert!(matches!(app.popup, Popup::History { .. }));
        app.handle_key(code(KeyCode::Down)).unwrap(); // select /tmp/two
        app.handle_key(code(KeyCode::Char('a'))).unwrap(); // add shortcut

        // Now on the name step; type a name and continue.
        let Popup::TextInput { kind: InputKind::ShortcutName { .. }, .. } = &app.popup else {
            panic!("expected the shortcut-name prompt, got {:?}", app.popup)
        };
        for c in "mydir".chars() { app.handle_key(code(KeyCode::Char(c))).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // The target step must be pre-filled with the chosen history path.
        let Popup::TextInput { buffer, kind: InputKind::ShortcutTarget { .. }, .. } = &app.popup
        else {
            panic!("expected the target step, got {:?}", app.popup)
        };
        assert_eq!(buffer, "/tmp/two", "target seeded from the history selection");
    }

    #[test]
    fn the_history_popup_highlights_the_selection() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.active_pane_mut().unwrap().history =
            vec![PathBuf::from("/tmp/alpha"), PathBuf::from("/tmp/beta")];
        app.handle_key(code(KeyCode::Char('h'))).unwrap();
        let shown = render(&mut app, 100, 20).join("\n");
        assert!(shown.contains("▸"), "the selected row has a marker:\n{}", shown);
        assert!(shown.contains("/tmp/alpha") && shown.contains("/tmp/beta"), "{}", shown);
    }

    /// Right-click Paste in the shell must send text to the terminal, not try
    /// to paste files as it does in a file pane.
    #[test]
    fn shell_paste_sends_text_not_files() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.file_clip = None;
        app.run_menu_item(MenuItem::Paste).unwrap();
        // Whatever the clipboard held, this took the shell text path — never
        // the file path, whose messages talk about "files".
        let msg = app.message.clone().unwrap_or_default();
        assert!(!msg.contains("files"), "should not paste files in the shell: {:?}", msg);
    }

    #[test]
    fn f3_views_a_text_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.rs"), "fn main() {}\nsecond\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        let Popup::Viewer { view, title, .. } = &app.popup else {
            panic!("expected the viewer, got {:?}", app.popup)
        };
        assert_eq!(title, "a.rs");
        assert_eq!(view.kind, cian_core::viewer::ViewKind::Text);
        assert_eq!(view.lines, vec!["fn main() {}", "second"]);
    }

    #[test]
    fn a_markdown_file_opens_in_preview_and_toggles_to_source() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("readme.md"), "# Title\n\n- item\n\n```mermaid\ngraph TD; A-->B\n```\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        // A .md file opens straight into rendered preview.
        assert!(matches!(&app.popup, Popup::Viewer { markdown: true, preview: true, .. }), "opened in preview");
        // The render swaps the rendered document into view.lines (and fills the
        // per-char style grid) so the whole viewer works over the preview.
        let _ = render(&mut app, 100, 30);
        if let Popup::Viewer { view, md_styles, source, .. } = &app.popup {
            let flat = view.lines.join("\n");
            assert!(flat.contains("mermaid diagram"), "mermaid label is rendered");
            assert!(flat.contains("A-->B"), "the diagram source is kept");
            assert!(!md_styles.is_empty(), "per-char styles were built");
            assert!(source.iter().any(|l| l == "# Title"), "the raw source is preserved");
        } else {
            panic!("not a viewer");
        }

        // Search works in preview: `/` then a query jumps the cursor to a match.
        app.handle_key(key('/')).unwrap();
        for c in "mermaid".chars() { app.handle_key(key(c)).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        if let Popup::Viewer { view, line, find_query, .. } = &app.popup {
            assert_eq!(find_query.as_deref(), Some("mermaid"), "search is confirmed");
            assert!(view.lines[*line].contains("mermaid"), "cursor landed on a match");
        } else {
            panic!("not a viewer");
        }

        // p toggles to raw source (view.lines becomes the file text again), p back.
        app.handle_key(key('p')).unwrap();
        let _ = render(&mut app, 100, 30);
        if let Popup::Viewer { view, md_styles, preview, .. } = &app.popup {
            assert!(!*preview, "toggled to source");
            assert!(md_styles.is_empty(), "styles dropped in source mode");
            assert!(view.lines.iter().any(|l| l == "# Title"), "shows raw source");
        } else {
            panic!("not a viewer");
        }
        app.handle_key(key('p')).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { preview: true, .. }), "back to preview");
        // Esc peels state: the still-active search clears first (viewer stays),
        // then a second Esc closes.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        match &app.popup {
            Popup::Viewer { find_query, .. } => assert!(find_query.is_none(), "search cleared, not closed"),
            _ => panic!("first Esc should have kept the viewer open"),
        }
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None), "second Esc closes");
    }

    #[test]
    fn undo_reverses_a_rename() {
        let (d, mut app) = app_with(&["old.txt"]);
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "old.txt").unwrap();
        app.start_rename();
        if let Popup::TextInput { buffer, .. } = &mut app.popup {
            buffer.clear();
            buffer.push_str("new.txt");
        } else {
            panic!("no rename prompt");
        }
        app.finish_text_input().unwrap();
        assert!(d.path().join("new.txt").exists() && !d.path().join("old.txt").exists());

        app.undo_last();
        assert!(d.path().join("old.txt").exists(), "rename undone");
        assert!(!d.path().join("new.txt").exists());
        // Nothing left to undo.
        app.undo_last();
        assert!(app.message.as_deref().unwrap_or("").contains("undo"));
    }

    #[test]
    fn undo_reverses_a_move_between_panes() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        std::fs::write(src.path().join("f.txt"), b"data").unwrap();
        let mut app = App::new(
            src.path().to_path_buf(),
            dst.path().to_path_buf(),
            cian_lua::Config::default(),
        )
        .unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "f.txt").unwrap();

        app.start_transfer(PendingOp::Move);
        assert!(matches!(app.popup, Popup::ConfirmTransfer { .. }), "move confirm");
        app.finish_transfer(Conflict::Overwrite).unwrap();
        drain_op_job(&mut app);
        assert!(dst.path().join("f.txt").exists() && !src.path().join("f.txt").exists(), "moved");

        app.undo_last();
        assert!(src.path().join("f.txt").exists(), "move undone");
        assert!(!dst.path().join("f.txt").exists());
    }

    #[test]
    fn menu_lang_overrides_lang_for_menu_and_manual() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().to_path_buf();
        let mut cfg = cian_lua::Config::default();
        cfg.options.lang = Some("en".into());
        cfg.options.menu_lang = Some("ja".into());
        let app = App::new(p.clone(), p, cfg).unwrap();
        assert_eq!(app.lang, Lang::En, "the rest of the UI stays English");
        assert_eq!(app.menu_lang, Lang::Ja, "menu + manual follow menu_lang");

        // Unset menu_lang follows lang.
        let d2 = tempfile::tempdir().unwrap();
        let p2 = d2.path().to_path_buf();
        let mut cfg2 = cian_lua::Config::default();
        cfg2.options.lang = Some("ja".into());
        let app2 = App::new(p2.clone(), p2, cfg2).unwrap();
        assert_eq!(app2.menu_lang, Lang::Ja, "falls back to lang when unset");
    }

    #[test]
    fn where_shows_config_paths() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.show_config_paths();
        match &app.popup {
            Popup::Notice { lines } => {
                assert!(lines.iter().any(|l| l.starts_with("portable mode:")), "reports portable status");
                assert!(lines.iter().any(|l| l.contains("shortcuts.lua")), "lists shortcuts.lua");
                assert!(lines.iter().any(|l| l.contains("user config dir:")), "shows the user config dir");
            }
            _ => panic!("no notice"),
        }
    }

    #[test]
    fn malformed_shortcuts_lua_is_an_error_not_silence() {
        // The parser must reject a bad hand-edit so the app can surface it
        // instead of loading an empty list without a word.
        assert!(cian_lua::shortcuts::parse("return 42").is_err(), "non-table rejected");
        assert!(cian_lua::shortcuts::parse("this is not lua {{{").is_err(), "syntax error rejected");
        assert!(cian_lua::shortcuts::parse("return { { target = \"/x\" } }").is_err(), "entry without name rejected");
        // A well-formed file still parses.
        assert!(cian_lua::shortcuts::parse("return { { name = \"home\", target = \"/home\" } }").is_ok());
    }

    #[test]
    fn menu_label_splits_name_and_hint() {
        use crate::render::menu_label_parts;
        assert_eq!(menu_label_parts("Bulk rename…  (:brename)"), ("Bulk rename…", "(:brename)"));
        assert_eq!(menu_label_parts("Copy"), ("Copy", ""));
        assert_eq!(menu_label_parts("Ⓒ crmaine - Ajent ▸"), ("Ⓒ crmaine - Ajent ▸", ""));
    }

    #[test]
    fn chmod_field_parses_octal() {
        use crate::parse_chmod;
        assert_eq!(parse_chmod("777"), (Some(0o777), None));
        assert_eq!(parse_chmod(" 644 "), (Some(0o644), None));
        assert_eq!(parse_chmod(""), (None, None)); // blank = keep
        assert!(parse_chmod("999").1.is_some(), "8/9 are not octal");
        assert!(parse_chmod("rwx").1.is_some(), "symbolic not accepted");
    }

    #[test]
    fn readable_on_flips_with_background_luminance() {
        use crate::render::readable_on;
        use ratatui::style::Color;
        // Light ground (Solarized Light selection) → dark text.
        let dark = readable_on(Color::Rgb(0xdc, 0xd5, 0xbe));
        assert!(matches!(dark, Color::Rgb(r, _, _) if r < 80), "dark text on light bg");
        // Dark ground (default selection) → light text.
        let light = readable_on(Color::Rgb(60, 60, 90));
        assert!(matches!(light, Color::Rgb(r, _, _) if r > 180), "light text on dark bg");
    }

    #[test]
    fn snippet_launcher_filters_and_confirms() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().to_path_buf();
        let mut cfg = cian_lua::Config::default();
        cfg.snippets = vec![
            cian_lua::Snippet { name: "list".into(), cmd: "ls -la".into(), enter: true, confirm: false },
            cian_lua::Snippet { name: "danger".into(), cmd: "rm -rf x".into(), enter: true, confirm: true },
        ];
        let mut app = App::new(p.clone(), p, cfg).unwrap();

        // Ctrl+Shift+Enter opens it from a file pane...
        let cse = KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL | KeyModifiers::SHIFT);
        app.handle_key(cse).unwrap();
        assert!(matches!(app.popup, Popup::Snippets { .. }), "Ctrl+Shift+Enter opens the launcher");
        app.popup = Popup::None;

        // ...and also while the shell pane is focused (the whole point — a plain
        // key there would go to the terminal instead).
        app.focused = FocusedPane::Shell;
        app.handle_key(cse).unwrap();
        assert!(matches!(app.popup, Popup::Snippets { .. }), "opens from the shell too");
        app.popup = Popup::None;
        app.focused = FocusedPane::Left;

        // Opening lists all; typing filters by name/command.
        app.start_snippets();
        assert!(matches!(app.popup, Popup::Snippets { .. }), "launcher opens");
        assert_eq!(app.snippet_matches("").len(), 2);
        assert_eq!(app.snippet_matches("dang").len(), 1);
        assert_eq!(app.snippet_matches("ls").len(), 1, "matches command text too");

        // A plain snippet is delivered and the picker closes.
        app.send_snippet(0);
        assert!(!matches!(app.popup, Popup::ConfirmSnippet { .. }), "no confirm for a safe snippet");

        // A confirm-flagged snippet routes through the confirmation.
        app.send_snippet(1);
        match &app.popup {
            Popup::ConfirmSnippet { name, cmd, .. } => {
                assert_eq!(name, "danger");
                assert_eq!(cmd, "rm -rf x");
            }
            _ => panic!("destructive snippet must confirm"),
        }
    }

    #[test]
    fn bulk_rename_previews_then_applies() {
        let d = tempfile::tempdir().unwrap();
        for n in ["a.txt", "b.txt"] {
            std::fs::write(d.path().join(n), b"x").unwrap();
        }
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p.clone(), cian_lua::Config::default()).unwrap();
        let targets = vec![p.join("a.txt"), p.join("b.txt")];

        // Template with a padded counter → a review checklist, nothing on disk yet.
        app.build_bulk_rename(&targets, "img_{n2}.{ext}");
        match &app.popup {
            Popup::RenameReview { items, .. } => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].new, "img_01.txt");
                assert_eq!(items[1].new, "img_02.txt");
            }
            _ => panic!("no review popup"),
        }
        assert!(p.join("a.txt").exists(), "not renamed until applied");

        app.apply_rename_plan();
        assert!(p.join("img_01.txt").exists() && p.join("img_02.txt").exists(), "renamed");
        assert!(!p.join("a.txt").exists());

        // A regex substitution over the current names.
        let targets = vec![p.join("img_01.txt"), p.join("img_02.txt")];
        app.build_bulk_rename(&targets, "s/img/photo/");
        match &app.popup {
            Popup::RenameReview { items, .. } => assert_eq!(items[0].new, "photo_01.txt"),
            _ => panic!("no review popup"),
        }

        // A pattern that changes nothing reports rather than opening a review.
        app.popup = Popup::None;
        app.build_bulk_rename(&targets, "{name}.{ext}");
        assert!(matches!(app.popup, Popup::None), "no-op does not open a review");

        // A malformed pattern is reported, not opened.
        app.build_bulk_rename(&targets, "s/[/x/");
        assert!(matches!(app.popup, Popup::None), "bad pattern does not open a review");
    }

    #[test]
    fn dir_compare_copy_across_reconciles_entries() {
        use std::sync::atomic::AtomicBool;
        use std::sync::Arc;
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("only_left.txt"), b"L").unwrap();
        std::fs::write(l.path().join("both.txt"), b"AAA").unwrap();
        std::fs::write(r.path().join("both.txt"), b"BBB").unwrap();

        let mut app = App::new(
            l.path().to_path_buf(),
            r.path().to_path_buf(),
            cian_lua::Config::default(),
        )
        .unwrap();

        // Build the folder comparison synchronously (skip the async job).
        let cancel = Arc::new(AtomicBool::new(false));
        let diff = cian_core::dirdiff::compare(l.path(), r.path(), &cancel, &mut |_| {});
        let find = |app: &App, name: &str| {
            let Popup::DirCompare { entries, .. } = &app.popup else { panic!("not dircompare") };
            entries.iter().position(|e| e.rel.to_string_lossy() == name)
        };
        let set = |app: &mut App, cur: usize| {
            if let Popup::DirCompare { cursor, .. } = &mut app.popup { *cursor = cur; }
        };
        let mk = |app: &mut App, entries: Vec<cian_core::dirdiff::Entry>| {
            app.popup = Popup::DirCompare {
                left: "L".into(), right: "R".into(),
                left_root: l.path().to_path_buf(), right_root: r.path().to_path_buf(),
                entries, cursor: 0, scroll: 0, truncated: false,
            };
        };
        mk(&mut app, diff.entries.clone());

        // only_left.txt → right: destination absent, so it copies immediately
        // and the entry drops out (both sides now match).
        let i = find(&app, "only_left.txt").unwrap();
        set(&mut app, i);
        app.dir_compare_copy(true);
        assert!(r.path().join("only_left.txt").exists(), "created on the right");
        assert!(find(&app, "only_left.txt").is_none(), "entry reconciled");

        // both.txt differs → overwrite needs confirmation.
        let i = find(&app, "both.txt").unwrap();
        set(&mut app, i);
        app.dir_compare_copy(true);
        assert!(matches!(app.popup, Popup::ConfirmDiffCopy { .. }), "overwrite confirms");
        app.confirm_diff_copy();
        assert_eq!(std::fs::read(r.path().join("both.txt")).unwrap(), b"AAA", "overwritten");

        // Cancel path restores the comparison without copying.
        mk(&mut app, cian_core::dirdiff::compare(l.path(), r.path(), &cancel, &mut |_| {}).entries);
        std::fs::write(l.path().join("both.txt"), b"CCC").unwrap();
        std::fs::write(r.path().join("both.txt"), b"DDD").unwrap();
        mk(&mut app, cian_core::dirdiff::compare(l.path(), r.path(), &cancel, &mut |_| {}).entries);
        let i = find(&app, "both.txt").unwrap();
        set(&mut app, i);
        app.dir_compare_copy(true);
        app.cancel_diff_copy();
        assert!(matches!(app.popup, Popup::DirCompare { .. }), "comparison restored");
        assert_eq!(std::fs::read(r.path().join("both.txt")).unwrap(), b"DDD", "not copied on cancel");
    }

    #[test]
    fn git_log_diff_and_blame() {
        use std::process::Command;
        let d = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(d.path()).unwrap();
        let ok = Command::new("git").arg("-C").arg(&dir).args(["init", "-q"]).status()
            .map(|s| s.success()).unwrap_or(false);
        if !ok {
            eprintln!("git not available; skipping");
            return;
        }
        for kv in [["user.email", "t@e.com"], ["user.name", "Alice"], ["core.autocrlf", "false"]] {
            let _ = Command::new("git").arg("-C").arg(&dir).args(["config", kv[0], kv[1]]).status();
        }
        std::fs::write(dir.join("f.rs"), "let a = 1;\nlet b = 2;\n").unwrap();
        Command::new("git").arg("-C").arg(&dir).args(["add", "."]).status().unwrap();
        Command::new("git").arg("-C").arg(&dir).args(["commit", "-qm", "seed"]).status().unwrap();

        let mut app = App::new(dir.clone(), dir.clone(), cian_lua::Config::default()).unwrap();
        // Give the pane's git status a moment (ensure_git runs in the loop; call it).
        app.ensure_git();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "f.rs").unwrap();

        // History → a GitLog popup with the seed commit; Enter shows its diff.
        app.start_git_log();
        match &app.popup {
            Popup::GitLog { commits, .. } => assert_eq!(commits[0].subject, "seed"),
            _ => panic!("no git log popup"),
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "commit diff opens in the viewer");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // Diff vs HEAD after an edit.
        std::fs::write(dir.join("f.rs"), "let a = 1;\nlet B = 2;\n").unwrap();
        let _ = app.active_file_tabs_mut().map(|t| t.active_mut().reload());
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "f.rs").unwrap();
        app.git_diff_file();
        match &app.popup {
            Popup::Viewer { view, .. } => assert!(view.lines.join("\n").contains("+let B = 2;"), "diff shown"),
            _ => panic!("diff did not open"),
        }
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // F3 then B toggles blame.
        app.handle_key(code(KeyCode::F(3))).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('B'), KeyModifiers::SHIFT)).unwrap();
        match &app.popup {
            Popup::Viewer { blame, .. } => assert!(!blame.is_empty(), "blame computed"),
            _ => panic!("not a viewer"),
        }
    }

    #[test]
    fn disk_usage_cache_populates_for_the_active_pane() {
        let d = tempfile::tempdir().unwrap();
        let p = std::fs::canonicalize(d.path()).unwrap();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        assert!(app.disk_for(app.focused).is_none(), "cold before the first refresh");
        app.ensure_git();
        let u = app.disk_for(app.focused).expect("mount is queryable");
        assert!(u.total > 0 && u.free <= u.total);
    }

    #[test]
    fn svn_status_log_and_diff() {
        use std::process::Command;
        // Needs both svnadmin (to make a repo) and svn (to check one out).
        let have = |bin: &str| Command::new(bin).arg("--version").output().map(|o| o.status.success()).unwrap_or(false);
        if !have("svnadmin") || !have("svn") {
            eprintln!("svn not available; skipping");
            return;
        }
        let root = tempfile::tempdir().unwrap();
        let repo = std::fs::canonicalize(root.path()).unwrap().join("repo");
        assert!(Command::new("svnadmin").args(["create"]).arg(&repo).status().unwrap().success());
        let url = format!("file://{}", repo.display());
        let wc_parent = tempfile::tempdir().unwrap();
        let wc = std::fs::canonicalize(wc_parent.path()).unwrap().join("wc");
        assert!(Command::new("svn").args(["checkout", &url]).arg(&wc).status().unwrap().success());

        std::fs::write(wc.join("f.rs"), "let a = 1;\nlet b = 2;\n").unwrap();
        let svn = |args: &[&str]| assert!(Command::new("svn").current_dir(&wc).args(args).status().unwrap().success(), "svn {:?}", args);
        svn(&["add", "f.rs"]);
        svn(&["commit", "-m", "seed"]);

        let mut app = App::new(wc.clone(), wc.clone(), cian_lua::Config::default()).unwrap();
        app.ensure_git();
        // The status bar label comes from RepoStatus.branch → "svn r1".
        assert_eq!(app.vcs_kind(), Some(Vcs::Svn), "detected as svn");
        assert!(app.git_for(app.focused).map(|s| s.branch.starts_with("svn r")).unwrap_or(false), "revision label");

        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "f.rs").unwrap();

        // History → GitLog popup carrying Vcs::Svn; Enter shows the revision diff.
        app.start_git_log();
        match &app.popup {
            Popup::GitLog { commits, vcs, .. } => {
                assert_eq!(*vcs, Vcs::Svn);
                assert_eq!(commits[0].subject, "seed");
            }
            _ => panic!("no svn log popup"),
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "revision diff opens in the viewer");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // Diff vs BASE after an edit.
        std::fs::write(wc.join("f.rs"), "let a = 1;\nlet B = 2;\n").unwrap();
        let _ = app.active_file_tabs_mut().map(|t| t.active_mut().reload());
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "f.rs").unwrap();
        app.git_diff_file();
        match &app.popup {
            Popup::Viewer { view, .. } => assert!(view.lines.join("\n").contains("+let B = 2;"), "diff shown"),
            _ => panic!("diff did not open"),
        }
    }

    #[test]
    fn f3_syntax_highlights_recognised_code() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.rs"), "fn main() {\n    let x = 1; // hi\n}\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "a.rs").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { hl_lang: Some(_), .. }), "rust detected");
        // The render computes and caches the per-char highlight styles.
        let _ = render(&mut app, 100, 30);
        match &app.popup {
            Popup::Viewer { hl, .. } => {
                assert!(!hl.is_empty(), "highlight computed");
                // `fn` (keyword mauve) differs from a plain identifier's colour.
                let kw = hl[0][0];
                let plain = hl[2][0]; // the closing `}` line, char 0
                assert_ne!(kw.fg, plain.fg, "keyword coloured differently from plain");
            }
            _ => panic!("not a viewer"),
        }
        // A .txt file is not highlighted.
        std::fs::write(d.path().join("b.txt"), "plain\n").unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
    }

    #[test]
    fn the_viewer_edits_and_saves_a_text_file() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("note.txt");
        std::fs::write(&file, "hello\nworld\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "note.txt").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // Enter edit mode and type at the start of line 1.
        app.handle_key(key('i')).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { editing: true, .. }), "editing started");
        for c in "AB".chars() {
            app.handle_key(key(c)).unwrap();
        }
        // A newline splits the line.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { dirty: true, .. }), "buffer is dirty");

        // Ctrl+S writes it back.
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { dirty: false, .. }), "saved → clean");
        let on_disk = std::fs::read_to_string(&file).unwrap();
        assert_eq!(on_disk, "AB\nhello\nworld\n", "edit persisted: {on_disk:?}");

        // Esc leaves edit mode; another Esc closes (nothing unsaved now).
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { editing: false, .. }));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn the_viewer_refuses_to_drop_unsaved_edits() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "x\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "a.txt").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        app.handle_key(key('i')).unwrap();
        app.handle_key(key('z')).unwrap(); // dirty
        app.handle_key(code(KeyCode::Esc)).unwrap(); // leave edit mode
        // Esc / q won't discard unsaved work…
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { dirty: true, .. }), "still open, warned");
        // …but Shift+Q does.
        app.handle_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::None), "Shift+Q discards and closes");
    }

    #[test]
    fn viewer_esc_clears_search_before_closing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "a.txt").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // Run a `/` search, then Esc: it clears the search (viewer stays), and a
        // second Esc closes — never dropping the viewer while a search is active.
        app.handle_key(key('/')).unwrap();
        for c in "beta".chars() { app.handle_key(key(c)).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { find_query: Some(_), .. }), "search active");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { find_query: None, .. }), "Esc cleared the search");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None), "Esc then closes");
    }

    #[test]
    fn a_docx_previews_as_searchable_text() {
        use std::io::Write;
        let d = tempfile::tempdir().unwrap();
        // A minimal .docx: a zip with word/document.xml.
        let docx = d.path().join("report.docx");
        {
            let f = std::fs::File::create(&docx).unwrap();
            let mut zw = zip::ZipWriter::new(f);
            let opts: zip::write::FileOptions<'_, ()> =
                zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            zw.start_file("word/document.xml", opts).unwrap();
            zw.write_all(
                br#"<w:document><w:body>
                    <w:p><w:r><w:t>Quarterly results</w:t></w:r></w:p>
                    <w:p><w:r><w:t>Revenue is up</w:t></w:r></w:p>
                </w:body></w:document>"#,
            )
            .unwrap();
            zw.finish().unwrap();
        }
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        // F3 opens the extracted document in the ordinary viewer (not markdown).
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        if let Popup::Viewer { view, markdown, preview, .. } = &app.popup {
            assert!(!*markdown && !*preview, "a document, not a markdown preview");
            let flat = view.lines.join("\n");
            assert!(flat.contains("Word"), "header names the format");
            assert!(flat.contains("Quarterly results"), "body text extracted: {:?}", view.lines);
            assert!(flat.contains("Revenue is up"));
        } else {
            panic!("F3 did not open a viewer");
        }

        // Search works over the extracted text, just like any file.
        app.handle_key(key('/')).unwrap();
        for c in "Revenue".chars() { app.handle_key(key(c)).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        if let Popup::Viewer { view, line, .. } = &app.popup {
            assert!(view.lines[*line].contains("Revenue"), "search jumped to the match");
        } else {
            panic!("not a viewer");
        }
        app.handle_key(code(KeyCode::Esc)).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn the_viewer_line_visual_selects_and_copies_a_range() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30); // size viewer_rect so motion works

        // Move to line 1 (two), start line-visual, extend to line 2 (three).
        app.handle_key(key('j')).unwrap();
        app.handle_key(key('V')).unwrap();
        app.handle_key(key('j')).unwrap();
        assert!(
            matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Line), .. }),
            "line-visual is active"
        );
        app.handle_key(key('y')).unwrap();
        assert_eq!(app.message.as_deref(), Some("copied"));
        // Visual ends after the copy; the viewer stays open.
        assert!(matches!(app.popup, Popup::Viewer { visual: None, .. }));
    }

    #[test]
    fn the_viewer_shift_arrow_selects_characters() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello world\nsecond\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // Shift+Right three times: a character-wise selection begins at col 0
        // and the cursor advances, extending it.
        for _ in 0..3 {
            app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::SHIFT)).unwrap();
        }
        match &app.popup {
            Popup::Viewer { visual: Some(ViewVisual::Char), anchor, col, .. } => {
                assert_eq!(*anchor, (0, 0), "anchored where selection began");
                assert_eq!(*col, 3, "cursor advanced three chars");
            }
            other => panic!("expected a char selection, got {:?}", other),
        }
        // A plain motion keeps the vim-style selection; `y` copies and ends it.
        app.handle_key(key('y')).unwrap();
        assert_eq!(app.message.as_deref(), Some("copied"));
        assert!(matches!(app.popup, Popup::Viewer { visual: None, .. }));
    }

    #[test]
    fn the_viewer_alt_arrow_and_alt_drag_select_a_block() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello world\nsecond line\nthird row!!\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // Alt+Down then Alt+Right builds a rectangle from the cursor.
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)).unwrap();
        match &app.popup {
            Popup::Viewer { visual: Some(ViewVisual::Block), anchor, line, col, .. } => {
                assert_eq!(*anchor, (0, 0));
                assert_eq!((*line, *col), (1, 2), "block cursor advanced down 1, right 2");
            }
            other => panic!("expected a block selection, got {:?}", other),
        }
        app.handle_key(code(KeyCode::Esc)).unwrap(); // drop the selection

        // Alt+drag also makes a block selection.
        let body = app.viewer_rect;
        let x0 = body.x + app.viewer_gutter;
        let mut down = mouse(MouseEventKind::Down(MouseButton::Left), x0 + 1, body.y);
        down.modifiers = KeyModifiers::ALT;
        app.handle_mouse(down);
        let mut drag = mouse(MouseEventKind::Drag(MouseButton::Left), x0 + 4, body.y + 2);
        drag.modifiers = KeyModifiers::ALT;
        app.handle_mouse(drag);
        assert!(matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Block), .. }),
            "alt-drag makes a block selection");
    }

    #[test]
    fn the_viewer_mouse_drag_selects_characters() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "hello world\nsecond line\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        let body = app.viewer_rect;
        let x0 = body.x + app.viewer_gutter;
        // Press on (line 0, char 2), drag to (line 0, char 8): a char selection.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), x0 + 2, body.y));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x0 + 8, body.y));
        match &app.popup {
            Popup::Viewer { visual: Some(ViewVisual::Char), anchor, line, col, .. } => {
                assert_eq!(*anchor, (0, 2), "anchored at the press char");
                assert_eq!((*line, *col), (0, 8), "cursor at the drag char");
            }
            other => panic!("expected a char selection, got {:?}", other),
        }
        // Right-click copies the selection.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), x0 + 8, body.y));
        assert_eq!(app.message.as_deref(), Some("copied"));
    }

    /// Drive the viewer with a sequence of plain-char keys.
    fn vkeys(app: &mut App, s: &str) {
        for c in s.chars() {
            app.handle_key(key(c)).unwrap();
        }
    }

    #[test]
    fn the_viewer_searches_and_jumps_between_matches() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "alpha\nbeta needle\ngamma\nneedle again\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // /needle<CR> jumps to the first match (line 1, col 5).
        app.handle_key(key('/')).unwrap();
        vkeys(&mut app, "needle");
        app.handle_key(code(KeyCode::Enter)).unwrap();
        if let Popup::Viewer { line, col, .. } = &app.popup {
            assert_eq!((*line, *col), (1, 5), "first match");
        } else {
            panic!("viewer");
        }
        // n advances to the next match (line 3, col 0).
        app.handle_key(key('n')).unwrap();
        if let Popup::Viewer { line, col, .. } = &app.popup {
            assert_eq!((*line, *col), (3, 0), "second match");
        } else {
            panic!("viewer");
        }
    }

    #[test]
    fn the_viewer_goto_line_and_bracket_match() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "fn f() {\n    body\n}\nfour\nfive\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // 4G jumps to line 4 (0-based index 3).
        vkeys(&mut app, "4");
        app.handle_key(key('G')).unwrap();
        if let Popup::Viewer { line, .. } = &app.popup {
            assert_eq!(*line, 3, "goto line 4");
        } else {
            panic!("viewer");
        }
        // Back to the top, move onto the `{` (col 7 of "fn f() {"), then % to
        // its matching `}` on line 2.
        app.handle_key(key('g')).unwrap();
        vkeys(&mut app, "lllllll"); // 7 × l → col 7 = '{'
        if let Popup::Viewer { col, .. } = &app.popup {
            assert_eq!(*col, 7, "cursor on the brace");
        }
        vkeys(&mut app, "%");
        if let Popup::Viewer { line, .. } = &app.popup {
            assert_eq!(*line, 2, "matching brace is on line 2");
        } else {
            panic!("viewer");
        }
    }

    #[test]
    fn the_viewer_char_visual_yanks_across_lines() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "abcd\nefgh\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        // From (0,1)=b, char-visual to (1,1)=f → "bcd\nef".
        app.handle_key(key('l')).unwrap();
        app.handle_key(key('v')).unwrap();
        app.handle_key(key('j')).unwrap();
        // cursor col follows the goal (1) on line 1.
        let text = if let Popup::Viewer { view, line, col, visual, anchor, .. } = &app.popup {
            assert_eq!((*line, *col), (1, 1));
            let (s, e) = order_pos(*anchor, (*line, *col));
            assert!(visual.is_some());
            viewer_charwise(&view.lines, s, e)
        } else {
            panic!("viewer")
        };
        assert_eq!(text, "bcd\nef");
    }

    #[test]
    fn e_opens_the_encoding_picker_and_applies_the_choice() {
        let d = tempfile::tempdir().unwrap();
        // "日本語" in Shift_JIS: mojibake as UTF-8 until switched.
        std::fs::write(d.path().join("s.txt"), [0x93u8, 0xfa, 0x96, 0x7b, 0x8c, 0xea, b'\n']).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();

        // `e` opens the picker (a list), not an immediate cycle.
        app.handle_key(key('e')).unwrap();
        assert!(
            matches!(app.popup, Popup::EncodingPicker { target: EncTarget::Viewer(_), .. }),
            "e opens the picker targeting the viewer"
        );
        // Move to Shift_JIS and confirm; the viewer comes back re-decoded.
        let sjis = cian_core::viewer::TextEncoding::ALL
            .iter()
            .position(|e| *e == cian_core::viewer::TextEncoding::ShiftJis)
            .unwrap();
        if let Popup::EncodingPicker { cursor, .. } = &mut app.popup {
            *cursor = sjis;
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let Popup::Viewer { view, .. } = &app.popup else { panic!("viewer restored") };
        assert_eq!(view.encoding, cian_core::viewer::TextEncoding::ShiftJis);
        assert_eq!(view.lines[0], "日本語");
    }

    #[test]
    fn cancelling_the_encoding_picker_restores_the_viewer_unchanged() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("s.txt"), b"plain\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        app.handle_key(key('e')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "Esc returns to the viewer");
    }

    #[test]
    fn shift_enter_reveals_the_viewed_file_in_the_pane() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub").join("deep.txt"), "content\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        // Open the file directly in the viewer, then Shift+Enter to reveal it.
        app.open_viewer_at(&d.path().join("sub").join("deep.txt"), "deep.txt", 0);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::None), "viewer closed");
        let pane = app.active_pane().unwrap();
        assert!(pane.cwd.ends_with("sub"), "pane moved into the file's dir: {:?}", pane.cwd);
        assert_eq!(pane.selected().map(|e| e.name.as_str()), Some("deep.txt"));
    }

    #[test]
    fn ctrl_n_steps_through_grep_hits_in_the_viewer() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "NEEDLE one\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "two NEEDLE\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.start_find("NEEDLE", cian_core::search::Mode::Content);
        drain_find(&mut app);
        // Sort of results is by rel path, so a.txt is first. Open it.
        if let Popup::FindResults { cursor, .. } = &mut app.popup {
            *cursor = 0;
        }
        app.open_find_hit().unwrap();
        let first = match &app.popup {
            Popup::Viewer { title, .. } => title.clone(),
            _ => panic!("viewer"),
        };
        // Ctrl+n → the other file's hit.
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL)).unwrap();
        let second = match &app.popup {
            Popup::Viewer { title, .. } => title.clone(),
            other => panic!("expected viewer, got {:?}", other),
        };
        assert_ne!(first, second, "Ctrl+n moved to the other hit");
        // Esc still returns to the (stepped) results list.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::FindResults { .. }));
    }

    #[test]
    fn f3_on_an_archive_lists_it_instead() {
        let d = tempfile::tempdir().unwrap();
        {
            use std::io::Write as _;
            let f = std::fs::File::create(d.path().join("a.zip")).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let o: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file("inside.txt", o).unwrap();
            w.write_all(b"hi").unwrap();
            w.finish().unwrap();
        }
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        let Popup::Archive { members, .. } = &app.popup else {
            panic!("expected the archive list, got {:?}", app.popup)
        };
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].name, "inside.txt");
    }

    #[test]
    fn extracting_sends_the_members_to_the_other_pane() {
        let src = tempfile::tempdir().unwrap();
        let out = tempfile::tempdir().unwrap();
        {
            use std::io::Write as _;
            let f = std::fs::File::create(src.path().join("a.zip")).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let o: zip::write::FileOptions<()> = zip::write::FileOptions::default();
            w.start_file("inside.txt", o).unwrap();
            w.write_all(b"hi").unwrap();
            w.finish().unwrap();
        }
        let mut app = App::new(
            src.path().to_path_buf(),
            out.path().to_path_buf(),
            cian_lua::Config::default(),
        )
        .unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        app.extract_from_archive(true);
        assert!(app.op_job.is_some(), "extraction runs on the worker");

        let start = Instant::now();
        while app.op_job.is_some() && start.elapsed() < Duration::from_secs(5) {
            app.poll_op_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(std::fs::read_to_string(out.path().join("inside.txt")).unwrap(), "hi");
        // The destination is worth remembering like any other transfer target.
        assert!(app.dest_history.iter().any(|p| p.file_name() == out.path().file_name()));
    }

    #[test]
    fn f3_on_a_directory_says_so_rather_than_opening_a_blank_viewer() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert!(app.message.as_deref().unwrap_or("").contains("directory"));
    }

    #[test]
    fn shell_panel_starts_empty_and_focusing_it_does_not_block() {
        let (_d, mut app) = app_with(&["a.txt"]);
        assert_eq!(app.shell.count(), 0);

        // Focusing the shell must return immediately, leaving the spawn in
        // flight rather than blocking the event loop on fork/exec.
        app.focus(FocusedPane::Shell);
        assert!(app.shell.is_starting(), "spawn should be pending, not resolved inline");

        // The placeholder renders without a session present.
        let out = render(&mut app, 100, 24).join("\n");
        assert!(out.contains("starting shell"), "expected placeholder; got:\n{}", out);
    }
