    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::widgets::BorderType;

    /// The active theme lives in a process-wide global, so tests that mutate or
    /// assert on it must not run concurrently with each other. They all take this
    /// lock first.
    static THEME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

    /// Close the file the viewer is reading, the way it closes: `:q` — Esc
    /// peels state and stops, as it does in vi.
    fn quit_viewer(app: &mut App) {
        for k in [':', 'q'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
    }

    /// Close it and throw away unsaved edits.
    fn quit_viewer_discarding(app: &mut App) {
        for k in [':', 'q', '!'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
    }

    /// An app rooted at a temp dir containing `names`.
    /// A default config that asks for English, which is what the assertions
    /// in this file read. cian's own default is Japanese — see
    /// `the_interface_is_japanese_unless_asked`.
    fn en_config() -> cian_lua::Config {
        let mut c = cian_lua::Config::default();
        c.options.lang = Some("en".into());
        c
    }

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
        let mut config = en_config();
        config.options.lang = Some(lang.to_string());
        let app = App::new(p.clone(), p, config).unwrap();
        (dir, app)
    }

    /// Like `app_with`, but with `cian.set_keymap` overrides applied.
    fn app_with_keymaps(names: &[&str], keymaps: Vec<(&str, String)>) -> (tempfile::TempDir, App) {
        let dir = tempfile::tempdir().unwrap();
        for n in names {
            std::fs::write(dir.path().join(n), b"").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut config = en_config();
        config.keymaps = keymaps.into_iter().map(|(k, a)| (k.to_string(), a)).collect();
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
                // Layout macros are tagged ▦ in the launcher (⚙ marks scripts).
                assert_eq!(names, &["▦ First".to_string(), "▦ Second".to_string()]);
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
                assert!(
                    matches!(e.kind, crate::edit::EditKind::File { reopen_viewer: false, .. }),
                    ":edit does not re-open the viewer"
                );
            }
            None => panic!("edit was not queued"),
        }

        // From the F3 viewer, `E` queues it and asks to re-open the viewer after.
        app.pending_edit = None;
        app.handle_key(code(KeyCode::F(3))).unwrap();
        for k in [':', 'e', 'd', 'i', 't'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let e = app.pending_edit.as_ref().expect("viewer edit queued");
        assert!(
            matches!(e.kind, crate::edit::EditKind::File { reopen_viewer: true, .. }),
            "viewer edit re-opens the viewer"
        );
        assert!(matches!(app.popup, Popup::None), "viewer stepped aside");
    }

    /// The editor-rename round trip against a real directory: `:bulkrename`
    /// writes the list, an "edit" rewrites it, and applying renames the files —
    /// including an a↔b swap, the case a naive one-pass rename cannot do.
    #[test]
    fn editor_rename_applies_the_edited_list_even_swaps() {
        let (d, mut app) = app_with(&["a.txt", "b.txt", "keep.txt"]);
        app.start_editor_rename();
        let edit = app.pending_edit.take().expect("list queued for the editor");
        let (dir, names) = match &edit.kind {
            crate::edit::EditKind::BulkRename { dir, names } => (dir.clone(), names.clone()),
            _ => panic!("queued as a plain edit"),
        };
        assert_eq!(names, vec!["a.txt", "b.txt", "keep.txt"], "the pane's listing, in order");

        // The "editor session": swap a and b, leave keep alone.
        std::fs::write(&edit.path, "b.txt\na.txt\nkeep.txt\n").unwrap();
        app.finish_editor_rename(&edit.path, &dir, &names);

        assert!(d.path().join("a.txt").exists() && d.path().join("b.txt").exists());
        assert!(d.path().join("keep.txt").exists());
        assert!(!edit.path.exists(), "the temp list is cleaned up");
        // No half-moved temp names left behind.
        let leftovers: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".cian-rename-"))
            .collect();
        assert!(leftovers.is_empty(), "no staging temp files remain");
    }

    /// Marks narrow the list — and a rename onto a file *outside* the list (the
    /// case the in-list duplicate check cannot see) is refused by the on-disk
    /// collision check, cancelling the batch before anything moves.
    #[test]
    fn editor_rename_refuses_to_clobber_a_bystander() {
        let (d, mut app) = app_with(&["a.txt", "keep.txt"]);
        {
            let p = app.active_pane_mut().unwrap();
            let i = p.entries.iter().position(|e| e.name == "a.txt").unwrap();
            p.set_mark_at(i);
        }
        app.start_editor_rename();
        let edit = app.pending_edit.take().unwrap();
        let (dir, names) = match &edit.kind {
            crate::edit::EditKind::BulkRename { dir, names } => (dir.clone(), names.clone()),
            _ => panic!(),
        };
        assert_eq!(names, vec!["a.txt"], "marks narrow the list");

        // The "editor session" renames a.txt onto the unmarked keep.txt.
        std::fs::write(&edit.path, "keep.txt\n").unwrap();
        app.finish_editor_rename(&edit.path, &dir, &names);
        assert!(
            d.path().join("a.txt").exists() && d.path().join("keep.txt").exists(),
            "clobber rejected, both files untouched"
        );
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains("exists") || msg.contains("存在"), "says why: {msg}");
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

    /// The ☁ column appears only where a sync client left placeholders, and
    /// the badge lands on the placeholder rows. A real placeholder needs a
    /// sync client, so the flag is set directly — the detection itself is
    /// covered in `cian_core::cloud`.
    #[test]
    fn the_cloud_column_shows_only_where_placeholders_are() {
        let (_d, mut app) = app_with(&["local.txt", "onedrive.txt"]);
        // An ordinary folder pays nothing for the feature.
        let plain = render(&mut app, 100, 20).join("\n");
        assert!(!plain.contains('☁'), "no cloud column in a plain folder");

        {
            // Set on the visible listing: a reload would re-stat and clear it.
            let pane = app.active_pane_mut().unwrap();
            for e in pane.entries.iter_mut() {
                if e.name == "onedrive.txt" {
                    e.cloud = true;
                }
            }
        }
        assert!(app.active_pane().unwrap().has_cloud());
        let out = render(&mut app, 100, 20);
        let cloud_row = out.iter().find(|l| l.contains("onedrive.txt")).expect("row shown");
        let local_row = out.iter().find(|l| l.contains("local.txt")).expect("row shown");
        assert!(cloud_row.contains('☁'), "placeholder badged: {cloud_row}");
        assert!(!local_row.contains('☁'), "local file not badged: {local_row}");
    }

    /// The preview refuses a placeholder rather than downloading it just
    /// because the cursor came to rest there — unless the toggle says otherwise.
    #[test]
    fn preview_refuses_a_cloud_placeholder() {
        let (_d, mut app) = app_with(&["cloudy.txt"]);
        std::fs::write(_d.path().join("cloudy.txt"), "secret contents here\n").unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            let _ = pane.reload();
            pane.cursor = pane.entries.iter().position(|e| e.name == "cloudy.txt").unwrap();
            for e in pane.entries.iter_mut() {
                e.cloud = true;
            }
        }
        app.preview_on = true;
        let out = render(&mut app, 110, 30).join("\n");
        assert!(
            out.contains("not been downloaded") || out.contains("ダウンロードされていません"),
            "explains why, in full: {out}"
        );
        assert!(out.contains("F3"), "and names the way to see it anyway: {out}");
        assert!(!out.contains("secret contents"), "the file was not read");

        // Opting in makes the preview read it like any other file.
        cian_core::cloud::set_include(true);
        app.preview = None;
        let out = render(&mut app, 110, 30).join("\n");
        cian_core::cloud::set_include(false);
        assert!(out.contains("secret contents"), "opt-in reads it: {out}");
    }

    /// `:` opens replace in the viewer; a plain command replaces everything
    /// at once, and `u` takes the whole thing back as one step.
    #[test]
    fn viewer_replace_all_is_one_undo_step() {
        let (_d, mut app) = viewer_on("alpha bravo\nbravo charlie\nbravo\n");
        app.handle_key(key(':')).unwrap();
        // The prompt opens empty — the word commands share it, and none of
        // them should start by deleting a seeded `s/`.
        assert!(matches!(&app.popup, Popup::Viewer { sub_input: Some(s), .. } if s.is_empty()));
        for c in "s/bravo/BRAVO/g".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["alpha BRAVO", "BRAVO charlie", "BRAVO"]);
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains('3'), "reports the count: {msg}");

        app.handle_key(key('u')).unwrap();
        assert_eq!(
            viewer_lines(&app),
            ["alpha bravo", "bravo charlie", "bravo"],
            "one undo takes back the whole replace"
        );
    }

    /// The `c` flag walks the hits: y replaces, n skips, and the walk reports
    /// both tallies at the end.
    #[test]
    fn viewer_replace_can_confirm_each_one() {
        let (_d, mut app) = viewer_on("x one\nx two\nx three\n");
        app.handle_key(key(':')).unwrap();
        for c in "s/x/Y/c".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { sub_walk: Some(_), .. }), "walk started");

        app.handle_key(key('y')).unwrap(); // line 0: replace
        app.handle_key(key('n')).unwrap(); // line 1: skip
        app.handle_key(key('y')).unwrap(); // line 2: replace → walk ends
        assert!(matches!(app.popup, Popup::Viewer { sub_walk: None, .. }), "walk finished");
        assert_eq!(viewer_lines(&app), ["Y one", "x two", "Y three"]);
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains('2') && msg.contains('1'), "reports 2 replaced, 1 skipped: {msg}");
    }

    /// `q` stops a walk partway, keeping what was already done; `a` takes the
    /// whole remainder in one go.
    #[test]
    fn a_confirm_walk_can_be_stopped_or_finished_wholesale() {
        let (_d, mut app) = viewer_on("x\nx\nx\nx\n");
        let start = |app: &mut App| {
            app.handle_key(key(':')).unwrap();
            for c in "s/x/Y/c".chars() {
                app.handle_key(key(c)).unwrap();
            }
            app.handle_key(code(KeyCode::Enter)).unwrap();
        };
        start(&mut app);
        app.handle_key(key('y')).unwrap();
        app.handle_key(key('q')).unwrap();
        assert_eq!(viewer_lines(&app), ["Y", "x", "x", "x"], "stopped, keeping the first");
        assert!(app.message.clone().unwrap_or_default().contains("stopped")
            || app.message.clone().unwrap_or_default().contains("中断"));

        start(&mut app);
        app.handle_key(key('a')).unwrap();
        assert_eq!(viewer_lines(&app), ["Y", "Y", "Y", "Y"], "`a` took the rest");
    }

    /// A CRLF file keeps its line endings through an edit — the viewer's
    /// lines never hold the ending, so saving used to quietly rewrite every
    /// Windows file as LF. `:crlf` / `:lf` convert on purpose.
    #[test]
    fn line_endings_survive_an_edit_and_convert_on_request() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("win.txt");
        std::fs::write(&f, b"one\r\ntwo\r\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "win.txt").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        match &app.popup {
            Popup::Viewer { view, .. } => {
                assert_eq!(view.eol, cian_core::viewer::Eol::Crlf, "detected as CRLF");
            }
            _ => panic!("not a viewer"),
        }
        let shown = render(&mut app, 100, 30).join("\n");
        assert!(shown.contains("CRLF"), "and says so in the title: {shown}");

        // An edit and a save keep the CRLFs.
        app.handle_key(key('i')).unwrap();
        app.handle_key(key('X')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        let raw = std::fs::read(&f).unwrap();
        assert!(raw.windows(2).any(|w| w == b"\r\n"), "still CRLF after saving");
        assert_eq!(String::from_utf8_lossy(&raw), "Xone\r\ntwo\r\n");

        // `:lf` converts, deliberately.
        app.handle_key(code(KeyCode::Esc)).unwrap(); // leave insert
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("lf".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        let raw = std::fs::read(&f).unwrap();
        assert!(!raw.contains(&b'\r'), "converted to LF on request: {:?}", String::from_utf8_lossy(&raw));
    }

    /// A replace can be limited to a visual selection.
    #[test]
    fn replace_honours_a_selection() {
        let (_d, mut app) = viewer_on("a\na\na\n");
        app.handle_key(key('V')).unwrap();
        app.handle_key(key('j')).unwrap();
        app.handle_key(key(':')).unwrap();
        for c in "s/a/B/g".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["B", "B", "a"], "only the selected lines");
    }


    /// The line transforms act on the whole file, or on a v/V selection, and
    /// each lands as one undo step.
    #[test]
    fn viewer_line_transforms_work_on_file_and_selection() {
        let run = |app: &mut App, cmd: &str| {
            app.handle_key(key(':')).unwrap();
            if let Popup::Viewer { sub_input, .. } = &mut app.popup {
                *sub_input = Some(cmd.into());
            }
            app.handle_key(code(KeyCode::Enter)).unwrap();
        };

        let (_d, mut app) = viewer_on("c\na\nb\na\n");
        run(&mut app, "sort");
        assert_eq!(viewer_lines(&app), ["a", "a", "b", "c"]);
        run(&mut app, "uniq");
        assert_eq!(viewer_lines(&app), ["a", "b", "c"]);
        run(&mut app, "rsort");
        assert_eq!(viewer_lines(&app), ["c", "b", "a"]);
        // Each transform is one undo step, so this walks back to sorted.
        app.handle_key(key('u')).unwrap();
        assert_eq!(viewer_lines(&app), ["a", "b", "c"]);

        // Full-width Latin and half-width kana both come out normal.
        let (_d2, mut app) = viewer_on("ＡＢＣ１２３\nｶﾞｯｺｳ\n");
        run(&mut app, "han");
        assert_eq!(viewer_lines(&app), ["ABC123", "ガッコウ"]);

        // A selection limits it: sort only the middle two lines.
        let (_d3, mut app) = viewer_on("z\nd\nc\na\n");
        if let Popup::Viewer { line, .. } = &mut app.popup {
            *line = 1;
        }
        app.handle_key(key('V')).unwrap();
        app.handle_key(key('j')).unwrap();
        run(&mut app, "sort");
        assert_eq!(viewer_lines(&app), ["z", "c", "d", "a"], "only the selected pair moved");
    }

    /// `:ws` makes the invisible characters visible — the pass where a
    /// trailing space or an ideographic space is the actual bug.
    #[test]
    fn ws_shows_the_invisible_characters() {
        let (_d, mut app) = viewer_on("trailing   \n全角\u{3000}空白\n");
        // Body rows only: the title carries its own `·` for the encoding and
        // line-ending badges. Matched on single characters because the test
        // backend dumps a wide char's second cell as a space, so "空白" comes
        // back as "空 白".
        let body = |app: &mut App| -> String {
            render(app, 100, 30)
                .into_iter()
                .filter(|l| l.contains("trailing") || l.contains('全'))
                .collect::<Vec<_>>()
                .join("\n")
        };
        // On by default now: the invisible characters are the ones that cause
        // the trouble, so they are shown until asked not to be.
        let after = body(&mut app);
        assert!(after.contains('·'), "spaces are marked: {after}");
        assert!(after.contains('□'), "and the ideographic space: {after}");
        assert!(after.contains('↓'), "and the line ending: {after}");

        // `:ws` turns them off.
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("ws".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let off = body(&mut app);
        assert!(!off.contains('·') && !off.contains('□'), "off on request: {off}");

        // …and back on.
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("ws".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let after = body(&mut app);
        assert!(after.contains('·'), "trailing spaces are marked: {after}");
        assert!(after.contains('□'), "ideographic space is marked: {after}");
        // Marking is display only — the buffer is untouched.
        assert_eq!(viewer_lines(&app)[0], "trailing   ");
    }

    /// The outline: on by default when the file type has rules, `]]` and `[[`
    /// step through it, a click jumps, and `:outline` puts the column away.
    #[test]
    fn the_outline_shows_a_files_shape_and_jumps_around_it() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("code.rs"),
            "use std::io;\n\nstruct Config {\n    a: u8,\n}\n\npub fn run() {\n    let x = 1;\n}\n\nfn helper() {}\n",
        )
        .unwrap();
        std::fs::write(d.path().join("plain.txt"), "no structure here\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let open = |app: &mut App, name: &str| {
            app.active_pane_mut().unwrap().cursor =
                app.active_pane().unwrap().entries.iter().position(|e| e.name == name).unwrap();
            app.handle_key(code(KeyCode::Enter)).unwrap();
            if !app.zoomed {
                app.handle_key(code(KeyCode::F(12))).unwrap();
            }
            let _ = render(app, 120, 30);
        };
        let shape = |app: &App| match &app.popup {
            Popup::Viewer { shape, .. } => shape.clone(),
            other => panic!("not a viewer: {other:?}"),
        };
        let at = |app: &App| match &app.popup {
            Popup::Viewer { line, .. } => *line,
            other => panic!("not a viewer: {other:?}"),
        };

        open(&mut app, "code.rs");
        let sh = shape(&app).expect("Rust has outline rules");
        assert!(sh.shown, "shown without being asked for");
        assert_eq!(
            sh.items.iter().map(|i| i.text.as_str()).collect::<Vec<_>>(),
            ["struct Config {", "pub fn run() {", "fn helper() {}"],
        );

        // `]]` steps forward, `[[` back. A single bracket does nothing, so it
        // stays free for something else.
        app.handle_key(key(']')).unwrap();
        assert_eq!(at(&app), 0, "one bracket is not a motion");
        app.handle_key(key(']')).unwrap();
        assert_eq!(at(&app), 2, "struct Config");
        for _ in 0..2 {
            app.handle_key(key(']')).unwrap();
        }
        assert_eq!(at(&app), 6, "pub fn run");
        for _ in 0..2 {
            app.handle_key(key('[')).unwrap();
        }
        assert_eq!(at(&app), 2, "back to the struct");

        // A click in the outline column lands on the entry drawn there.
        let ol = app.outline_rect;
        assert!(ol.width > 0, "the column is drawn at this width");
        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: ol.x,
            row: ol.y + 2,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(at(&app), 10, "the third entry, fn helper");

        // `:outline` puts it away, and the body gets the width back.
        let narrow = app.viewer_rect.width;
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("outline".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let _ = render(&mut app, 120, 30);
        assert!(!shape(&app).unwrap().shown);
        assert!(app.viewer_rect.width > narrow, "the text got the column back");
        assert_eq!(app.outline_rect.width, 0);

        // A file type with no rules says so rather than showing an empty box.
        quit_viewer(&mut app);
        open(&mut app, "plain.txt");
        assert!(shape(&app).is_none());
        for _ in 0..2 {
            app.handle_key(key(']')).unwrap();
        }
        assert!(app.message.as_deref().unwrap_or("").contains("outline"));
    }

    /// Editing and saving must hand the file back the way it came: same tabs,
    /// same byte-order mark. Both were being spent silently — a Makefile came
    /// out indented with spaces, and a UTF-8-BOM file came out without one,
    /// which is precisely what `:nobom` is a deliberate command for.
    #[test]
    fn saving_keeps_the_tabs_and_the_bom_the_file_arrived_with() {
        let d = tempfile::tempdir().unwrap();
        let mk = d.path().join("Makefile");
        let bom = d.path().join("bom.txt");
        std::fs::write(&mk, b"all:\n\techo one\n\techo two\n").unwrap();
        std::fs::write(&bom, [&[0xEF, 0xBB, 0xBF][..], b"alpha\nbeta\n"].concat()).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let open = |app: &mut App, name: &str| {
            app.active_pane_mut().unwrap().cursor =
                app.active_pane().unwrap().entries.iter().position(|e| e.name == name).unwrap();
            app.handle_key(code(KeyCode::F(3))).unwrap();
            let _ = render(app, 100, 30);
        };

        open(&mut app, "Makefile");
        assert_eq!(viewer_lines(&app)[1], "\techo one", "the buffer holds the real tab");
        // A tab is still drawn four columns wide — the fix is about what is
        // written, not about how it looks. With the marks on (the default) the
        // first of those columns says what it is.
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("→   echo one↓"), "the tab and the line ending are marked: {screen}");
        app.show_ws = false;
        let plain = render(&mut app, 100, 30).join("\n");
        assert!(plain.contains("    echo one"), "and plain with the marks off");
        app.show_ws = true;

        // A tab is one buffer character but four drawn columns, so a click has
        // to be walked back through the same expansion: anywhere in the tab is
        // the tab, and the column after it is the first letter.
        let b = app.viewer_rect;
        let g = app.viewer_gutter;
        let click = |app: &mut App, dx: u16| {
            app.handle_mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: b.x + g + dx,
                row: b.y + 1,
                modifiers: KeyModifiers::NONE,
            });
            match &app.popup {
                Popup::Viewer { col, .. } => *col,
                other => panic!("not a viewer: {other:?}"),
            }
        };
        assert_eq!(click(&mut app, 0), 0, "the start of the tab");
        assert_eq!(click(&mut app, 3), 0, "still inside the tab");
        assert_eq!(click(&mut app, 4), 1, "the e of echo");
        assert_eq!(click(&mut app, 6), 3, "three characters in");

        // Make an edit somewhere else entirely, then save.
        if let Popup::Viewer { line, .. } = &mut app.popup {
            *line = 0;
        }
        app.handle_key(key('o')).unwrap(); // opens a line, entering insert
        app.handle_key(code(KeyCode::Esc)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        let out = std::fs::read(&mk).unwrap();
        assert!(app.message.as_deref().unwrap_or("").starts_with("saved"), "{:?}", app.message);
        assert!(
            out.windows(9).any(|w| w == b"\techo one"),
            "the recipe lines still start with a tab: {:?}",
            String::from_utf8_lossy(&out),
        );
        quit_viewer(&mut app);

        open(&mut app, "bom.txt");
        assert_eq!(viewer_lines(&app), ["alpha", "beta"], "the BOM is not part of the text");
        app.handle_key(key('o')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(
            &std::fs::read(&bom).unwrap()[..3],
            &[0xEF, 0xBB, 0xBF],
            "the byte-order mark came back",
        );
    }

    /// F3 with several files marked opens them all: having marked them is how
    /// you say "these ones", and opening the first while forgetting the rest
    /// answers a question nobody asked.
    #[test]
    fn f3_on_marked_files_opens_them_as_tabs() {
        let d = tempfile::tempdir().unwrap();
        for (n, body) in [("a.txt", "AAA\n"), ("b.txt", "BBB\n"), ("c.txt", "CCC\n")] {
            std::fs::write(d.path().join(n), body).unwrap();
        }
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let mark = |app: &mut App, name: &str| {
            let path = app
                .active_pane()
                .unwrap()
                .entries
                .iter()
                .find(|e| e.name == name)
                .unwrap()
                .path
                .clone();
            app.active_pane_mut().unwrap().marks.insert(path);
        };
        let shown = |app: &App| match &app.popup {
            Popup::Viewer { view, .. } => view.lines.join("\n"),
            other => panic!("not a viewer: {other:?}"),
        };

        for n in ["a.txt", "b.txt", "c.txt"] {
            mark(&mut app, n);
        }
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert_eq!(app.viewer_tab_count(), 3, "one tab per marked file");
        assert_eq!(shown(&app), "AAA", "the first is on screen");

        // F2 walks them, and wraps.
        app.handle_key(code(KeyCode::F(2))).unwrap();
        assert_eq!(shown(&app), "BBB");
        app.handle_key(code(KeyCode::F(2))).unwrap();
        assert_eq!(shown(&app), "CCC");
        app.handle_key(code(KeyCode::F(2))).unwrap();
        assert_eq!(shown(&app), "AAA", "wrapped round");
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(shown(&app), "CCC", "and back the other way");

        // Each tab keeps its own place in its own file.
        if let Popup::Viewer { line, .. } = &mut app.popup {
            *line = 0;
        }
        app.handle_key(code(KeyCode::F(2))).unwrap(); // to a.txt
        app.handle_key(key('i')).unwrap();
        app.handle_key(key('X')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(shown(&app), "XAAA", "edited");
        app.handle_key(code(KeyCode::F(2))).unwrap();
        assert_eq!(shown(&app), "BBB", "the other tab is untouched");
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(shown(&app), "XAAA", "and the edit is still there on return");

        // Esc closes this file; the rest stay open. Only the last one closes
        // the viewer. (The edited tab needs its discard key.)
        quit_viewer_discarding(&mut app);
        assert_eq!(app.viewer_tab_count(), 2, "one closed, two left");
        assert!(matches!(app.popup, Popup::Viewer { .. }), "still viewing");
        quit_viewer(&mut app);
        quit_viewer(&mut app);
        assert!(matches!(app.popup, Popup::None), "the last one closes it");
        assert_eq!(app.viewer_tab_count(), 0);
    }

    /// Paste goes where vi puts it: `p` after, `P` before, whole lines when
    /// whole lines were copied.
    #[test]
    fn p_and_shift_p_paste_after_and_before() {
        let (_d, mut app) = viewer_on("one\ntwo\nthree\n");
        let lines = |app: &App| viewer_lines(app);

        // Line-wise: `V` then `y` copies a whole line, `p` puts it below.
        app.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT)).unwrap();
        app.handle_key(key('y')).unwrap();
        // Kept inside cian as well as on the system clipboard: a machine
        // reached over SSH often has neither a clipboard service nor a need
        // for one, and copy-and-paste within a file must work there.
        assert_eq!(app.yank.as_deref(), Some("one\n"), "the yank carries its newline");
        app.handle_key(key('p')).unwrap();
        assert_eq!(lines(&app), ["one", "one", "two", "three"], "below the cursor");
        app.handle_key(key('u')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(lines(&app), ["one", "one", "two", "three"], "above it, same result at line 0");

        // Character-wise: `p` lands after the character under the cursor.
        let (_d2, mut app) = viewer_on("abc\n");
        app.handle_key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::NONE)).unwrap();
        app.handle_key(key('y')).unwrap(); // copies "a"
        app.handle_key(key('p')).unwrap();
        assert_eq!(lines(&app), ["aabc"], "after the cursor");
        app.handle_key(key('u')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('P'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(lines(&app), ["aabc"], "before it, same result at column 0");
    }

    /// The tab strip: every open file named, and the mouse able to reach both
    /// the arrows and the names. Also that a menu opened from the viewer is
    /// drawn *over* it rather than instead of it.
    #[test]
    fn the_tab_strip_is_visible_and_clickable() {
        let d = tempfile::tempdir().unwrap();
        for n in ["alpha.txt", "beta.txt", "gamma.txt"] {
            std::fs::write(d.path().join(n), format!("{n}\n")).unwrap();
        }
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        for n in ["alpha.txt", "beta.txt", "gamma.txt"] {
            let path = app.active_pane().unwrap().entries.iter().find(|e| e.name == n).unwrap().path.clone();
            app.active_pane_mut().unwrap().marks.insert(path);
        }
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let screen = render(&mut app, 160, 30).join("\n");
        for n in ["alpha.txt", "beta.txt", "gamma.txt"] {
            assert!(screen.contains(n), "every open file is named in the strip:\n{screen}");
        }
        assert!(!app.viewer_tab_rects.is_empty(), "and each has somewhere to click");

        let shown = |app: &App| match &app.popup {
            Popup::Viewer { view, .. } => view.lines.join("\n"),
            other => panic!("not a viewer: {other:?}"),
        };
        let click = |app: &mut App, c: u16, r: u16| {
            app.handle_mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: c,
                row: r,
                modifiers: KeyModifiers::NONE,
            });
        };

        // Click the third tab by name.
        let (rect, _) = app.viewer_tab_rects[2];
        click(&mut app, rect.x + 1, rect.y);
        assert_eq!(shown(&app), "gamma.txt", "clicked straight to the third");

        // The arrows step, at their fixed columns.
        let f = app.viewer_frame;
        click(&mut app, f.x + 2, f.y);
        assert_eq!(shown(&app), "beta.txt", "◂ went back one");
        click(&mut app, f.x + 4, f.y);
        assert_eq!(shown(&app), "gamma.txt", "▸ went forward one");

        // A menu opened from the viewer keeps the file on screen behind it.
        let _ = render(&mut app, 160, 30);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        let screen = render(&mut app, 160, 30).join("\n");
        assert!(screen.contains("gamma.txt"), "the file is still there:\n{screen}");
        assert!(
            screen.contains("Theme") || screen.contains("テーマ"),
            "with the menu over it:\n{screen}",
        );
    }

    /// F3 inside a zip opens the member; saving puts it back into the zip
    /// rather than leaving the work in a temp file nobody will look at again.
    #[test]
    fn editing_a_zip_member_writes_it_back_into_the_zip() {
        let d = tempfile::tempdir().unwrap();
        let src = d.path().join("stage");
        std::fs::create_dir_all(src.join("conf")).unwrap();
        std::fs::write(src.join("conf").join("app.ini"), "[main]\nlevel=INFO\n").unwrap();
        let zip = d.path().join("bundle.zip");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        let mut sink = |_: &cian_core::progress::Progress| {};
        let mut ctl = cian_core::progress::Ctl { cancel: &cancel, on_progress: &mut sink };
        let r = cian_core::archive::create_zip(
            &[src.join("conf")],
            &zip,
            None,
            &mut ctl,
        );
        assert!(r.errors.is_empty(), "{:?}", r.errors);

        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.enter_archive(zip.clone(), String::new());
        // Into conf/, then onto the member.
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name.starts_with("conf")).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "app.ini").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the member opened: {:?}", app.popup);

        // Edit it and save.
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("s/INFO/DEBUG/".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("w".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(
            app.message.as_deref().unwrap_or("").contains("bundle.zip"),
            "it says where it went: {:?}",
            app.message,
        );

        // The archive itself now holds the edit.
        let out = d.path().join("out");
        std::fs::create_dir_all(&out).unwrap();
        let mut sink = |_: &cian_core::progress::Progress| {};
        let mut ctl = cian_core::progress::Ctl { cancel: &cancel, on_progress: &mut sink };
        let r = cian_core::archive::extract(
            &zip,
            &["conf/app.ini".to_string()],
            &out,
            None,
            "",
            &mut ctl,
        );
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let back = std::fs::read_to_string(out.join("conf").join("app.ini")).unwrap();
        assert!(back.contains("level=DEBUG"), "the zip has the edit: {back:?}");
        assert!(back.contains("[main]"), "and the rest of the file");
    }

    /// The ruler and the crosshair, and Enter reading rather than launching.
    #[test]
    fn the_viewer_shows_a_column_scale_and_where_the_cursor_is() {
        let (_d, mut app) = viewer_on("abcdefghijklmnopqrstuvwxyz\nsecond line\n");
        app.show_ws = false;
        let screen = render(&mut app, 120, 20);
        // Every tenth column numbered, every fifth marked.
        let scale = screen.iter().find(|r| r.contains("····+····1")).cloned();
        assert!(scale.is_some(), "a column scale over the text:\n{}", screen.join("\n"));

        // …and it says which column the cursor is in, as the corner does.
        //
        // Walked across rather than set to one number: the scale is built from
        // `·`, which is two bytes wide, and cutting it at a *column* number
        // took the program down on the very first press of the right arrow.
        // A single hand-picked column can sit on a byte boundary by luck.
        for want in 2..=12 {
            app.handle_key(code(KeyCode::Right)).unwrap();
            let screen = render(&mut app, 120, 20).join("\n");
            assert!(screen.contains(&format!("1:{want}")), "the corner agrees with the corner");
        }

        // The scale starts where the text starts. Measured in cells, because
        // "roughly above it" is what it looked like and was not.
        let rows = render(&mut app, 120, 20);
        let ruler = rows.iter().find(|r| r.contains("····+")).expect("a ruler");
        let text = rows.iter().find(|r| r.contains("abcdefghij")).expect("the line");
        assert_eq!(
            ruler.find('·').unwrap(),
            text.find('a').unwrap(),
            "the first column of the scale is over the first column of the text",
        );

        // …and the column is counted the way the screen counts it. Two
        // full-width characters take four columns, so the cursor on the third
        // is in column five — which is what the ruler marks and therefore what
        // the corner has to say, or the two disagree on every Japanese line.
        let (_d2, mut app) = viewer_on("あいうえお\n");
        app.show_ws = false;
        for (chars_over, want_col) in [(0, 1), (1, 3), (2, 5), (4, 9)] {
            if let Popup::Viewer { col, .. } = &mut app.popup {
                *col = chars_over;
            }
            let screen = render(&mut app, 120, 20).join("\n");
            assert!(
                screen.contains(&format!("1:{want_col}")),
                "{chars_over} characters in is column {want_col}:\n{screen}",
            );
        }

        // `:ruler` puts both away and gives the row back to the text.
        let rows_with = render(&mut app, 120, 20).iter().filter(|r| r.contains("second line")).count();
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("ruler".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let screen = render(&mut app, 120, 20);
        assert!(!screen.iter().any(|r| r.contains("····+····1")), "the scale is gone");
        assert_eq!(
            screen.iter().filter(|r| r.contains("second line")).count(),
            rows_with,
            "the text is still all there",
        );
    }

    /// Enter reads the file — in the pane, since the editor is `F3` — and
    /// launching it is Ctrl+Enter. Looking at a file is the hundred-times-a-
    /// day action and can be left with Esc; an application opened by accident
    /// has to be found and closed.
    #[test]
    fn enter_reads_the_file_and_ctrl_enter_launches_it() {
        let (_d, mut app) = app_with(&["note.txt"]);
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "note.txt").unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "Enter opened the viewer");
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left), "docked in the pane it came from");
        for k in [':', 'q'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // On a directory Enter still goes in, which is not something a
        // launcher could have meant.
        let d2 = tempfile::tempdir().unwrap();
        std::fs::create_dir(d2.path().join("sub")).unwrap();
        let p = d2.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "sub").unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(app.active_pane().unwrap().cwd.ends_with("sub"), "went in");
    }

    /// "Select all" means the listing in a pane and the file in the viewer —
    /// one idea, and which of the two is simply which is in front of you.
    #[test]
    fn select_all_means_this_directory_or_this_file() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt", "c.txt"]);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)).unwrap();
        let p = app.active_pane().unwrap();
        assert_eq!(p.marks.len(), 3, "everything here");
        assert!(
            !p.marks.iter().any(|m| m.ends_with("..")),
            "but not the parent, which is not a file to operate on",
        );

        // In the viewer it is a line-wise selection of the whole buffer, so
        // `y` copies the file and Esc clears it — the ordinary visual keys.
        let (_d2, mut app) = viewer_on("one\ntwo\nthree\n");
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)).unwrap();
        match &app.popup {
            Popup::Viewer { visual: Some(ViewVisual::Line), anchor, line, .. } => {
                assert_eq!(*anchor, (0, 0));
                assert_eq!(*line, 2, "down to the last line");
            }
            other => panic!("expected a whole-file selection, got {other:?}"),
        }
        app.handle_key(key('y')).unwrap();
        assert_eq!(app.yank.as_deref(), Some("one\ntwo\nthree\n"), "and y takes the lot");

        // Reachable without Ctrl, which this terminal does not deliver.
        let (_d3, mut app) = app_with_keymaps(&["a.txt"], vec![("alt+a", "mark_all".into())]);
        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::ALT)).unwrap();
        assert_eq!(app.active_pane().unwrap().marks.len(), 1);
    }

    /// `=` compares the two halves in place: the marks appear on the real
    /// lines, both files stay editable, and the comparison follows the edit.
    #[test]
    fn a_split_can_be_compared_while_both_halves_stay_editable() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "same\nold\ngone\ntail\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "same\nnew\ntail\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        for n in ["a.txt", "b.txt"] {
            let path = app.active_pane().unwrap().entries.iter().find(|e| e.name == n).unwrap().path.clone();
            app.active_pane_mut().unwrap().marks.insert(path);
        }
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 160, 30);

        // Without a split it says what to do rather than doing nothing.
        app.handle_key(key('=')).unwrap();
        assert!(app.viewer_diff.is_none());
        assert!(app.message.as_deref().unwrap_or("").contains("F8"));

        app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT)).unwrap();
        let _ = render(&mut app, 160, 30);
        app.handle_key(key('=')).unwrap();
        let marks = |app: &App| app.viewer_diff.as_deref().unwrap().mine.clone();
        use cian_core::diff::Mark;
        assert_eq!(
            marks(&app),
            vec![Mark::Same, Mark::Changed, Mark::Only, Mark::Same],
            "one mark per real line — nothing inserted to line the two up",
        );

        // `]c` / `[c` step the differences — vimdiff's own keys. Tab used to
        // do the forward half and belongs to the window now.
        let at = |app: &App| match &app.popup {
            Popup::Viewer { line, .. } => *line,
            other => panic!("not a viewer: {other:?}"),
        };
        app.handle_key(key(']')).unwrap();
        app.handle_key(key('c')).unwrap();
        assert_eq!(at(&app), 1, "the changed line");
        app.handle_key(key(']')).unwrap();
        app.handle_key(key('c')).unwrap();
        assert_eq!(at(&app), 2, "the one only this side has");
        app.handle_key(key('[')).unwrap();
        app.handle_key(key('c')).unwrap();
        assert_eq!(at(&app), 1, "and back");

        // Editing one half is allowed, and the comparison follows it: making
        // the changed line match makes the difference go away.
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("s/old/new/".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["same", "new", "gone", "tail"], "edited in place");
        let _ = render(&mut app, 160, 30);
        assert_eq!(marks(&app)[1], Mark::Same, "the edit closed the difference");

        // `=` again stops.
        app.handle_key(key('=')).unwrap();
        assert!(app.viewer_diff.is_none());

        // A key that refuses has to refuse every time, not only the first —
        // the reply is about the keystroke, not about the words changing.
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::SHIFT)).unwrap();
        for _ in 0..3 {
            app.handle_key(key('=')).unwrap();
            assert!(app.message_fresh, "said so again");
            assert!(app.message.as_deref().unwrap_or("").contains("F8"));
            app.handle_key(code(KeyCode::Down)).unwrap();
            assert!(!app.message_fresh, "and stood down for the next key");
        }
    }

    /// `?` in the viewer answers "what can I do here", not "what can cian do".
    #[test]
    fn question_mark_lists_only_the_editor_panels_keys() {
        let (_d, mut app) = viewer_on("hello\n");
        app.handle_key(key('?')).unwrap();
        let Popup::Report { lines, .. } = &app.popup else { panic!("no help: {:?}", app.popup) };
        let text = lines.join("\n");
        assert!(
            text.contains("text editor panel") || text.contains("テキストエディタパネル"),
            "it names the panel, not the key that used to open it:\n{text}",
        );
        assert!(!text.contains("(F3)"), "and does not put F3 in its name:\n{text}");
        // Things the viewer cannot do are not in it.
        for absent in ["Rename", "SSH", "trash"] {
            assert!(!text.contains(absent), "{absent:?} does not belong here:\n{text}");
        }
        // The keys it *does* have are, grouped by what you are doing.
        for present in ["Move", "Edit", "gg", "zz", "*", ">>", ":wq"] {
            assert!(text.contains(present), "{present:?} is missing:\n{text}");
        }
        // It scrolls — it is far taller than a dialog.
        app.handle_key(key('j')).unwrap();
        let Popup::Report { scroll, .. } = &app.popup else { panic!("gone") };
        assert_eq!(*scroll, 1);
        // …and it goes back to the file.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "back to the file");
    }

    /// The mouse reaches both halves of a split and both tab arrows. All of
    /// this is geometry, and the geometry used to be measured against a
    /// viewer that filled the screen even when it had half of it.
    #[test]
    fn the_mouse_reaches_the_other_half_and_the_tab_arrows() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "AAA\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "BBB\n").unwrap();
        std::fs::write(d.path().join("c.txt"), "CCC\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        for n in ["a.txt", "b.txt", "c.txt"] {
            let path = app.active_pane().unwrap().entries.iter().find(|e| e.name == n).unwrap().path.clone();
            app.active_pane_mut().unwrap().marks.insert(path);
        }
        // Marked files open together; F12 gives the panel the window, which
        // is the geometry a split is measured against here.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        app.handle_key(code(KeyCode::F(12))).unwrap();
        let shown = |app: &App| match &app.popup {
            Popup::Viewer { view, .. } => view.lines.join("\n"),
            other => panic!("not a viewer: {other:?}"),
        };
        let click = |app: &mut App, c: u16, r: u16| {
            app.handle_mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
                column: c,
                row: r,
                modifiers: KeyModifiers::NONE,
            });
        };

        // The arrows step through the open files.
        let _ = render(&mut app, 160, 30);
        let f = app.viewer_frame;
        click(&mut app, f.x + 2, f.y);
        assert_eq!(shown(&app), "CCC", "◂ wrapped back to the last");
        click(&mut app, f.x + 4, f.y);
        assert_eq!(shown(&app), "AAA", "▸ came round again");

        // Split, then click the half the keyboard is not on.
        app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT)).unwrap();
        let _ = render(&mut app, 160, 30);
        let theirs = app.viewer_half_rects[1];
        assert!(theirs.width > 0, "the other half was measured");
        // Well inside it: its own left edge is the seam with the first half,
        // and a click on a seam is a resize.
        click(&mut app, theirs.x + 8, theirs.y + 3);
        assert_eq!(
            shown(&app),
            "BBB",
            "the keyboard crossed to the half that was clicked (halves {:?}, frame {:?}, dock {:?}, zoomed {})",
            app.viewer_half_rects,
            app.viewer_frame,
            app.viewer_dock,
            app.zoomed,
        );
        let theirs = app.viewer_half_rects[1];
        click(&mut app, theirs.x + 5, theirs.y + 3);
        assert_eq!(shown(&app), "AAA", "and back again");
    }

    /// A split must not draw anything but the viewer. It used to draw every
    /// popup as though it were one — so the menu, and worse the quit
    /// confirmation, were on screen and invisible, quietly taking the Enter
    /// that followed.
    #[test]
    fn a_split_does_not_swallow_the_dialogs_that_open_over_it() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "AAA\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "BBB\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        for n in ["a.txt", "b.txt"] {
            let path = app.active_pane().unwrap().entries.iter().find(|e| e.name == n).unwrap().path.clone();
            app.active_pane_mut().unwrap().marks.insert(path);
        }
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 160, 30);
        app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT)).unwrap();

        // Shift+Enter opens the menu, and the menu is drawn.
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::ContextMenu { .. }), "the menu opened");
        let screen = render(&mut app, 160, 30).join("\n");
        assert!(
            screen.contains("Theme") || screen.contains("テーマ"),
            "the menu is actually on screen:\n{screen}",
        );
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "and the file came back");

        // Closing every file leaves nothing of the split behind, so the next
        // dialog to open is visible.
        quit_viewer(&mut app);
        quit_viewer(&mut app);
        assert!(matches!(app.popup, Popup::None), "viewer gone: {:?}", app.popup);
        assert!(app.viewer_split.is_none(), "and so is the split");
        app.handle_key(key('q')).unwrap();
        let screen = render(&mut app, 160, 30).join("\n");
        assert!(!matches!(app.popup, Popup::None), "the quit confirmation opened");
        assert!(
            screen.contains("uit") || screen.contains("終了"),
            "and is on screen:\n{screen}",
        );
    }

    /// Two files side by side, on the keys the shell panel already uses.
    #[test]
    fn the_viewer_splits_and_puts_itself_back_together() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "AAA\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "BBB\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        for n in ["a.txt", "b.txt"] {
            let path = app.active_pane().unwrap().entries.iter().find(|e| e.name == n).unwrap().path.clone();
            app.active_pane_mut().unwrap().marks.insert(path);
        }
        // Open them, then give the panel the window: this is about how a
        // split is laid out across it.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        app.handle_key(code(KeyCode::F(12))).unwrap();
        let shown = |app: &App| match &app.popup {
            Popup::Viewer { view, .. } => view.lines.join("\n"),
            other => panic!("not a viewer: {other:?}"),
        };
        let other = |app: &App| match app.viewer_split.as_deref() {
            Some(Popup::Viewer { view, .. }) => view.lines.join("\n"),
            _ => panic!("not split"),
        };

        // Shift+F8 puts the next open file beside this one.
        app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT)).unwrap();
        assert!(app.viewer_split.is_some(), "split");
        assert_eq!(shown(&app), "AAA");
        assert_eq!(other(&app), "BBB");
        // …and the strip no longer holds it, since it is on screen.
        assert!(app.viewer_tabs.is_empty(), "both halves are on screen");

        // Shift+L crosses over, Shift+H comes back.
        app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(shown(&app), "BBB", "the keyboard is on the other half");
        app.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(shown(&app), "AAA");

        // Both halves are drawn side by side — and crossing over moves the
        // focus, not the files: each stays on the side it was put.
        let side_of = |app: &mut App, needle: &str| -> usize {
            let rows = render(app, 160, 30);
            let row = rows.iter().find(|r| r.contains(needle)).expect("on screen");
            usize::from(row.find(needle).expect("column") >= 80)
        };
        assert_eq!(side_of(&mut app, "AAA"), 0, "AAA is on the left");
        assert_eq!(side_of(&mut app, "BBB"), 1, "BBB is on the right");
        app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(shown(&app), "BBB", "the keyboard crossed over");
        assert_eq!(side_of(&mut app, "AAA"), 0, "…and AAA did not move");
        assert_eq!(side_of(&mut app, "BBB"), 1, "…nor did BBB");
        app.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT)).unwrap();

        // Shift+F10 keeps the one being read and returns the other to the strip.
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::SHIFT)).unwrap();
        assert!(app.viewer_split.is_none(), "one file again");
        assert_eq!(shown(&app), "AAA", "the half in focus stayed");
        assert_eq!(app.viewer_tab_count(), 2, "the other went back to the tabs");
        app.handle_key(code(KeyCode::F(2))).unwrap();
        assert_eq!(shown(&app), "BBB", "and is still reachable");
    }

    /// `:q` closes the file it was typed into, not the viewer. In a split it
    /// used to take the other half down with it — two files read together,
    /// one `:q`, and both were gone.
    #[test]
    fn q_in_a_split_closes_only_the_half_it_was_typed_into() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "AAA\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "BBB\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        for n in ["a.txt", "b.txt"] {
            let path =
                app.active_pane().unwrap().entries.iter().find(|e| e.name == n).unwrap().path.clone();
            app.active_pane_mut().unwrap().marks.insert(path);
        }
        app.handle_key(code(KeyCode::F(3))).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::F(8), KeyModifiers::SHIFT)).unwrap();
        let shown = |app: &App| match &app.popup {
            Popup::Viewer { view, .. } => view.lines.join("\n"),
            other => panic!("not a viewer: {other:?}"),
        };
        assert_eq!(shown(&app), "AAA");

        // `:q` on the half in focus leaves the other one being read.
        for k in [':', 'q'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(app.viewer_split.is_none(), "the split is over");
        assert_eq!(shown(&app), "BBB", "the other half is what's left");

        // The second `:q` has nothing else open, so the viewer closes.
        for k in [':', 'q'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::None), "the last file closes the viewer");
    }

    /// With a Japanese IME on, punctuation arrives full-width: `：` for the
    /// colon key, `／` for slash. Where a keystroke is a command that is still
    /// the key being pressed, so it opens what the key opens — but text must
    /// arrive exactly as typed, because a name may hold those characters on
    /// purpose (and on Windows, must).
    #[test]
    fn ime_punctuation_works_as_a_command_but_never_inside_text() {
        use crate::util::{fold_ime_key, fold_ime_word};
        assert_eq!(fold_ime_key('：'), Some(':'));
        assert_eq!(fold_ime_key('／'), Some('/'));
        assert_eq!(fold_ime_key('？'), Some('?'));
        assert_eq!(fold_ime_key('ｑ'), Some('q'));
        assert_eq!(fold_ime_key('・'), Some('/'), "the kana layout's slash key");
        assert_eq!(fold_ime_key('あ'), None, "kana is not a key press");
        assert_eq!(fold_ime_word("ｒａｇ"), "rag");

        // In a pane, `：` opens the command line.
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(key('：')).unwrap();
        assert_eq!(app.mode, Mode::Command, "the colon key opened the command line");
        // …and what is typed into it is left alone: this is text.
        app.handle_key(key('：')).unwrap();
        assert_eq!(app.command_buffer, "：", "typed text keeps its full-width form");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // A verb typed with the IME on still runs, since verbs are ASCII.
        app.command_buffer = "ｍａｎ".into();
        app.run_command();
        assert!(matches!(app.popup, Popup::Manual { .. }), "ｍａｎ ran :man");
        app.popup = Popup::None;

        // A rename keeps every character exactly as typed — folding a
        // full-width colon into a real one would be a different file name,
        // and an illegal one on Windows.
        app.start_rename();
        for c in "メモ：一覧".chars() {
            app.handle_key(key(c)).unwrap();
        }
        match &app.popup {
            Popup::TextInput { buffer, .. } => assert!(
                buffer.ends_with("メモ：一覧"),
                "the name is what was typed: {buffer:?}"
            ),
            other => panic!("expected the rename prompt, got {other:?}"),
        }
    }

    /// In the viewer the same rule applies: `／` searches, but the text of the
    /// search itself is left as typed — a Japanese file is searched for
    /// Japanese.
    #[test]
    fn ime_punctuation_opens_the_viewer_search_but_not_its_text() {
        let (_d, mut app) = viewer_on("メモ：一覧\nplain\n");
        app.handle_key(key('／')).unwrap();
        assert!(
            matches!(&app.popup, Popup::Viewer { find_input: Some(_), .. }),
            "the slash key opened the search"
        );
        for c in "：一覧".chars() {
            app.handle_key(key(c)).unwrap();
        }
        match &app.popup {
            Popup::Viewer { find_input: Some(q), .. } => {
                assert_eq!(q, "：一覧", "the query is what was typed")
            }
            other => panic!("expected a search in progress, got {other:?}"),
        }
    }

    /// A terminal paste (Cmd/Ctrl+V) arrives as one event carrying the whole
    /// text. In the viewer it used to arrive nowhere at all — the paste path
    /// knew about every one-line field and not about the file — so the only
    /// way to get text in was to have the terminal type it, a frame per
    /// character. It lands as one edit, undone in one step.
    #[test]
    fn a_terminal_paste_lands_in_the_viewer_in_one_edit() {
        let (_d, mut app) = viewer_on("first\nsecond\n");
        let before = viewer_lines(&app).join("\n");

        // Reading: it goes in where `p` would put it, newlines and all.
        app.insert_into_active_text("alpha\nbeta\n");
        let after = viewer_lines(&app);
        assert!(after.iter().any(|l| l.contains("alpha")), "the text is in: {after:?}");
        assert!(after.iter().any(|l| l.contains("beta")));
        assert!(after.len() > 2, "both lines landed: {after:?}");

        // One edit: one `u` puts the file back.
        app.handle_key(key('u')).unwrap();
        assert_eq!(viewer_lines(&app).join("\n"), before, "the paste undoes in one step");

        // Editing: it lands at the caret rather than on the next line.
        app.handle_key(key('i')).unwrap();
        app.insert_into_active_text("XY");
        let l = viewer_lines(&app);
        assert!(l[0].starts_with("XY"), "at the cursor: {l:?}");
    }

    /// A paste while a prompt is open over the file belongs to the prompt.
    /// It used to go into the file: typing `/` and pasting the search term
    /// left the search box empty and the term spliced into the text.
    #[test]
    fn a_paste_goes_to_the_prompt_that_is_open_over_the_file() {
        let (_d, mut app) = viewer_on("alpha\nbeta\n");
        app.handle_key(key('/')).unwrap();
        app.insert_into_active_text("bet");
        match &app.popup {
            Popup::Viewer { find_input, view, .. } => {
                assert_eq!(find_input.as_deref(), Some("bet"), "into the search box");
                assert_eq!(view.lines, vec!["alpha", "beta"], "and not into the file");
            }
            other => panic!("expected the viewer, got {other:?}"),
        }

        // The `:` line likewise.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        app.handle_key(key(':')).unwrap();
        app.insert_into_active_text("w");
        match &app.popup {
            Popup::Viewer { sub_input, view, .. } => {
                assert_eq!(sub_input.as_deref(), Some("w"));
                assert_eq!(view.lines, vec!["alpha", "beta"]);
            }
            other => panic!("expected the viewer, got {other:?}"),
        }
    }

    /// Text cannot be pasted into a binary file. What is on screen is a hex
    /// rendering of the bytes, not the bytes — a pasted line would be saved
    /// as whatever that rendering parses back to.
    #[test]
    fn text_is_refused_for_a_binary_file() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("bin.dat"), [0u8, 1, 2, 3, 255, 254]).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let i = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "bin.dat")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = i;
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let before = match &app.popup {
            Popup::Viewer { view, .. } => view.lines.clone(),
            other => panic!("expected the viewer, got {other:?}"),
        };
        app.insert_into_active_text("hello\n");
        match &app.popup {
            Popup::Viewer { view, dirty, .. } => {
                assert_eq!(view.lines, before, "the hex dump is untouched");
                assert!(!*dirty, "and the file is not marked as edited");
            }
            other => panic!("expected the viewer, got {other:?}"),
        }
        assert!(app.message.is_some(), "it says why");
    }

    /// Typing `48G` used to happen in the dark: the count built up invisibly,
    /// so there was no way to tell what had been pressed. It now shows on the
    /// prompt row, where `:` and `/` show theirs, and Esc abandons it.
    #[test]
    fn a_half_typed_command_is_visible_and_cancellable() {
        let (_d, mut app) = viewer_on(&(1..=80).map(|i| format!("line {i}\n")).collect::<String>());
        app.handle_key(key('4')).unwrap();
        app.handle_key(key('8')).unwrap();
        match &app.popup {
            Popup::Viewer { count, .. } => assert_eq!(*count, Some(48)),
            other => panic!("expected the viewer, got {other:?}"),
        }
        let rows = render(&mut app, 100, 30);
        assert!(rows.iter().any(|r| r.contains("48_")), "what is typed is on screen:\n{rows:?}");

        // Esc abandons it rather than closing the file.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        match &app.popup {
            Popup::Viewer { count, .. } => assert_eq!(*count, None, "the count is gone"),
            other => panic!("Esc closed the viewer instead: {other:?}"),
        }

        // And it still jumps.
        for k in ['4', '8', 'G'] {
            app.handle_key(key(k)).unwrap();
        }
        match &app.popup {
            Popup::Viewer { line, .. } => assert_eq!(*line, 47, "48G is line 48"),
            other => panic!("expected the viewer, got {other:?}"),
        }
    }

    /// A count repeats the motion it precedes, as it does in vi. Only `G`
    /// used to take one.
    #[test]
    fn a_count_repeats_the_motion_it_precedes() {
        let (_d, mut app) =
            viewer_on(&(1..=80).map(|i| format!("line {i} word word\n")).collect::<String>());
        let line = |app: &App| match &app.popup {
            Popup::Viewer { line, .. } => *line,
            other => panic!("expected the viewer, got {other:?}"),
        };
        let col = |app: &App| match &app.popup {
            Popup::Viewer { col, .. } => *col,
            other => panic!("expected the viewer, got {other:?}"),
        };
        for k in ['3', 'j'] {
            app.handle_key(key(k)).unwrap();
        }
        assert_eq!(line(&app), 3, "3j");
        for k in ['2', 'k'] {
            app.handle_key(key(k)).unwrap();
        }
        assert_eq!(line(&app), 1, "2k");
        for k in ['5', 'l'] {
            app.handle_key(key(k)).unwrap();
        }
        assert_eq!(col(&app), 5, "5l");
        for k in ['2', 'w'] {
            app.handle_key(key(k)).unwrap();
        }
        assert!(col(&app) > 5, "2w moved on: {}", col(&app));
        // `gg`, not a bare `g`: the prefix is vi's, and it leaves `gJ` room.
        for k in ['5', 'g', 'g'] {
            app.handle_key(key(k)).unwrap();
        }
        assert_eq!(line(&app), 4, "5gg is line 5");
    }

    /// The vim keys the viewer was missing: `*` searches the word under the
    /// cursor, `~` swaps its case, `>>` shifts a line by a tab stop, and `zz`
    /// puts the cursor's line in the middle of the window without moving it.
    #[test]
    fn star_tilde_shift_and_zz() {
        let (_d, mut app) = viewer_on("alpha beta\ngamma\nbeta again\n");
        app.handle_key(key('w')).unwrap(); // onto "beta"
        app.handle_key(key('*')).unwrap();
        match &app.popup {
            Popup::Viewer { find_query, line, .. } => {
                assert_eq!(find_query.as_deref(), Some("beta"), "the word under the cursor");
                assert_eq!(*line, 2, "and it jumped to the next one");
            }
            other => panic!("expected the viewer, got {other:?}"),
        }

        let (_d2, mut app2) = viewer_on("abc\n");
        app2.handle_key(key('~')).unwrap();
        assert_eq!(viewer_lines(&app2)[0], "Abc");
        app2.handle_key(key('~')).unwrap();
        assert_eq!(viewer_lines(&app2)[0], "ABc", "it walks along");

        app2.handle_key(key('>')).unwrap();
        app2.handle_key(key('>')).unwrap();
        assert!(viewer_lines(&app2)[0].starts_with("    "), "{:?}", viewer_lines(&app2));
        app2.handle_key(key('<')).unwrap();
        app2.handle_key(key('<')).unwrap();
        assert_eq!(viewer_lines(&app2)[0], "ABc", "and back");

        let (_d3, mut app3) =
            viewer_on(&(1..=200).map(|i| format!("l{i}\n")).collect::<String>());
        let _ = render(&mut app3, 100, 30);
        for k in ['1', '0', '0', 'G'] {
            app3.handle_key(key(k)).unwrap();
        }
        let _ = render(&mut app3, 100, 30);
        app3.handle_key(key('z')).unwrap();
        app3.handle_key(key('z')).unwrap();
        match &app3.popup {
            Popup::Viewer { line, scroll, .. } => {
                assert_eq!(*line, 99, "the cursor stayed");
                assert!(*scroll > 0 && *scroll < 99, "the line moved to the middle: {scroll}");
            }
            other => panic!("expected the viewer, got {other:?}"),
        }
    }

    /// The file closes with `:q`, as it does in vi — never with Esc, which is
    /// the key you press to mean "never mind" and must not also mean "put
    /// this away". The ✕ in the corner is the mouse's way out.
    #[test]
    fn only_q_and_the_button_close_the_viewer() {
        let (_d, mut app) = viewer_on("alpha\nbeta\n");
        let rows = render(&mut app, 100, 30);
        assert!(rows.iter().any(|r| r.contains('✕')), "the button is drawn:\n{rows:?}");

        // Esc says how to close rather than closing.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "Esc kept the file");
        assert!(app.message.as_deref().is_some_and(|m| m.contains(":q")), "{:?}", app.message);

        // A click on the ✕ closes it.
        let x = app.viewer_close_rect;
        assert!(x.width > 0, "the button has a place on screen");
        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: x.x,
            row: x.y,
            modifiers: KeyModifiers::NONE,
        });
        assert!(matches!(app.popup, Popup::None), "the button closed it");
    }

    /// Even a file cian cannot write closes with `:q` — the prompt used to be
    /// offered only on editable files, which after this change would have left
    /// a PDF or a docx with no way out at all.
    #[test]
    fn a_read_only_file_can_still_be_closed_and_refuses_to_be_written() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("bin.dat"), [0u8, 1, 2, 3]).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let i = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "bin.dat")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = i;
        app.handle_key(code(KeyCode::F(3))).unwrap();
        // Force the read-only case the document viewers produce.
        if let Popup::Viewer { editable, .. } = &mut app.popup {
            *editable = false;
        }
        for k in [':', 'w'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), ":w did not close it");
        assert!(app.message.is_some(), "…and said why it cannot be written");
        quit_viewer(&mut app);
        assert!(matches!(app.popup, Popup::None), ":q closes a read-only file");
    }

    /// vi's whole point: operators and motions multiply. `dw`, `d2w`, `d$`,
    /// `cw`, `yy`, `dj` — one grammar rather than a key per combination.
    #[test]
    fn operators_take_motions_and_counts() {
        let keys = |app: &mut App, s: &str| {
            for c in s.chars() {
                app.handle_key(key(c)).unwrap();
            }
        };
        // dw
        let (_d, mut app) = viewer_on("alpha beta gamma\nsecond\n");
        keys(&mut app, "dw");
        assert_eq!(viewer_lines(&app)[0], "beta gamma");
        // d2w from the start takes both words
        let (_d2, mut app2) = viewer_on("alpha beta gamma\n");
        keys(&mut app2, "d2w");
        assert_eq!(viewer_lines(&app2)[0], "gamma");
        // d$ to the end of the line, including the last character
        let (_d3, mut app3) = viewer_on("alpha beta\n");
        keys(&mut app3, "ld$");
        assert_eq!(viewer_lines(&app3)[0], "a");
        // dd and 2dd
        let (_d4, mut app4) = viewer_on("one\ntwo\nthree\nfour\n");
        keys(&mut app4, "dd");
        assert_eq!(viewer_lines(&app4), ["two", "three", "four"]);
        keys(&mut app4, "2dd");
        assert_eq!(viewer_lines(&app4), ["four"]);
        // dj takes both lines, whatever the column
        let (_d5, mut app5) = viewer_on("one\ntwo\nthree\n");
        keys(&mut app5, "lldj");
        assert_eq!(viewer_lines(&app5), ["three"]);
        // cw deletes the word and leaves the editor open to type
        let (_d6, mut app6) = viewer_on("alpha beta\n");
        keys(&mut app6, "cw");
        assert!(matches!(app6.popup, Popup::Viewer { editing: true, .. }), "c opens the editor");
        // vi's one special case: `cw` changes the word, not the space after
        // it — it behaves like `ce`.
        assert_eq!(viewer_lines(&app6)[0], " beta");
        // yy copies a line without changing anything
        let (_d7, mut app7) = viewer_on("one\ntwo\n");
        keys(&mut app7, "yy");
        assert_eq!(viewer_lines(&app7), ["one", "two"], "yank changes nothing");
        assert_eq!(app7.yank.as_deref(), Some("one\n"));
    }

    /// `f`, `t` and the pair `;` `,` — and `df,`, which is the operator and
    /// the motion together.
    #[test]
    fn find_char_moves_and_can_be_operated_on() {
        let (_d, mut app) = viewer_on("one,two,three\n");
        let col = |app: &App| match &app.popup {
            Popup::Viewer { col, .. } => *col,
            other => panic!("expected the viewer, got {other:?}"),
        };
        app.handle_key(key('f')).unwrap();
        app.handle_key(key(',')).unwrap();
        assert_eq!(col(&app), 3, "f, landed on the comma");
        app.handle_key(key(';')).unwrap();
        assert_eq!(col(&app), 7, "; repeated it");
        app.handle_key(key(',')).unwrap();
        assert_eq!(col(&app), 3, ", went back");
        // `t` stops before it.
        let (_d2, mut app2) = viewer_on("one,two\n");
        app2.handle_key(key('t')).unwrap();
        app2.handle_key(key(',')).unwrap();
        assert_eq!(col(&app2), 2, "t, stopped short");
        // `df,` deletes up to and including the comma.
        let (_d3, mut app3) = viewer_on("one,two\n");
        for c in "df,".chars() {
            app3.handle_key(key(c)).unwrap();
        }
        assert_eq!(viewer_lines(&app3)[0], "two");
    }

    /// Text objects: `ciw`, `di"`, `da(` — the other half of the grammar.
    #[test]
    fn text_objects_are_operated_on() {
        let keys = |app: &mut App, s: &str| {
            for c in s.chars() {
                app.handle_key(key(c)).unwrap();
            }
        };
        let (_d, mut app) = viewer_on("alpha beta gamma\n");
        keys(&mut app, "wdiw");
        assert_eq!(viewer_lines(&app)[0], "alpha  gamma", "diw took the word only");

        let (_d2, mut app2) = viewer_on("alpha beta gamma\n");
        keys(&mut app2, "wdaw");
        assert_eq!(viewer_lines(&app2)[0], "alpha gamma", "daw took its space too");

        let (_d3, mut app3) = viewer_on("value = \"some text\";\n");
        keys(&mut app3, "10ldi\"");
        assert_eq!(viewer_lines(&app3)[0], "value = \"\";", "di\" emptied the quotes");

        let (_d4, mut app4) = viewer_on("call(one, two);\n");
        keys(&mut app4, "6lda(");
        assert_eq!(viewer_lines(&app4)[0], "call;", "da( took the brackets with it");

        let (_d5, mut app5) = viewer_on("fn f() {\n    body();\n}\n");
        keys(&mut app5, "jdi{");
        assert_eq!(viewer_lines(&app5), ["fn f() {", "}"], "di{{ emptied the block");
    }

    /// Marks, the jump list and `.` — the three things that make a vi you can
    /// live in rather than one you can type in.
    #[test]
    fn marks_jumps_and_dot_repeat() {
        let keys = |app: &mut App, s: &str| {
            for c in s.chars() {
                app.handle_key(key(c)).unwrap();
            }
        };
        let at = |app: &App| match &app.popup {
            Popup::Viewer { line, .. } => *line,
            other => panic!("expected the viewer, got {other:?}"),
        };
        let body: String = (1..=60).map(|i| format!("line {i} alpha beta\n")).collect();
        let (_d, mut app) = viewer_on(&body);

        // `ma` here, wander off, `'a` back.
        keys(&mut app, "5jma");
        assert_eq!(at(&app), 5);
        keys(&mut app, "20j");
        assert_eq!(at(&app), 25);
        keys(&mut app, "'a");
        assert_eq!(at(&app), 5, "'a came back to the mark");
        // A mark that was never set says so rather than jumping somewhere.
        keys(&mut app, "'z");
        assert_eq!(at(&app), 5);
        assert!(app.message.as_deref().is_some_and(|m| m.contains('z')), "{:?}", app.message);

        // `G` is a jump: Ctrl+O goes back to where it started, Ctrl+I forward.
        keys(&mut app, "G");
        assert_eq!(at(&app), 59);
        app.handle_key(KeyEvent::new(KeyCode::Char('o'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(at(&app), 5, "Ctrl+O went back");
        app.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::CONTROL)).unwrap();
        assert_eq!(at(&app), 59, "Ctrl+I forward again");

        // `.` repeats a change, including what was typed into the editor.
        let (_d2, mut app2) = viewer_on("alpha beta\ngamma delta\n");
        keys(&mut app2, "cwX");
        app2.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(viewer_lines(&app2)[0], "X beta", "cw then typing");
        keys(&mut app2, "j0");
        keys(&mut app2, ".");
        assert_eq!(viewer_lines(&app2)[1], "X delta", ". did it again, here");

        // …and a plain `x` repeats too.
        let (_d3, mut app3) = viewer_on("abcdef\n");
        keys(&mut app3, "x");
        keys(&mut app3, "..");
        assert_eq!(viewer_lines(&app3)[0], "def", "x then two dots");
    }

    /// `:g/re/d` drops the lines that match, `:v/re/d` the ones that do not —
    /// the two halves of reading a log.
    #[test]
    fn global_delete_keeps_or_drops_matching_lines() {
        let (_d, mut app) = viewer_on("INFO one\nERROR two\nINFO three\nERROR four\n");
        for k in ":g/ERROR/d".chars() {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["INFO one", "INFO three"]);
        assert!(app.message.as_deref().is_some_and(|m| m.contains('2')), "{:?}", app.message);

        // …and one undo puts them all back.
        app.handle_key(key('u')).unwrap();
        assert_eq!(viewer_lines(&app).len(), 4, "one undo step");

        // `:v` keeps only what matches.
        for k in ":v/ERROR/d".chars() {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["ERROR two", "ERROR four"]);
    }

    /// Shift+Tab steps between the file and the panes, and opens an empty one
    /// when there is nothing to step back into — which is what makes the
    /// viewer somewhere to start writing rather than only somewhere to read.
    #[test]
    fn a_new_file_starts_empty_and_takes_a_name_when_saved() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // `:new` gives an empty, unnamed file, docked where you were. It used
        // to be what Shift+Tab did with nothing to step back into; Shift+Tab
        // is the tab strip now.
        app.command_buffer = "new".into();
        app.run_command();
        match &app.popup {
            Popup::Viewer { path, view, editable, .. } => {
                assert!(path.as_os_str().is_empty(), "no name yet");
                assert!(*editable, "and it can be typed into");
                assert_eq!(view.lines.len(), 1);
            }
            other => panic!("expected an empty viewer, got {other:?}"),
        }
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left), "in the pane you were in");

        app.handle_key(key('i')).unwrap();
        for c in "hello".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(viewer_lines(&app)[0], "hello");

        // `:w` alone will not guess a name; `:w <name>` writes and adopts it.
        for k in [':', 'w'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(app.message.as_deref().is_some_and(|m| m.contains(":w")), "{:?}", app.message);
        for k in ":w note.md".chars() {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let written = app.active_pane().unwrap().cwd.join("note.md");
        assert!(written.exists(), "written to the pane's folder: {:?} — {:?}", written, app.message);
        assert_eq!(std::fs::read_to_string(&written).unwrap(), "hello\n");
        match &app.popup {
            Popup::Viewer { path, title, dirty, .. } => {
                assert_eq!(path, &written, "it adopted the name");
                assert_eq!(title, "note.md");
                assert!(!*dirty, "and is saved");
            }
            other => panic!("expected the viewer, got {other:?}"),
        }
    }

    /// The state file holds more than one thing now, so writing one value must
    /// not lose the others.
    #[test]
    fn the_state_file_keeps_the_values_it_already_had() {
        let before = "# cian runtime state — managed by cian (see :where)\ntheme = \"nord\"\n";
        let after = crate::state_with(before, "font_level", "15");
        assert_eq!(crate::state_get_in(&after, "theme").as_deref(), Some("nord"), "{after}");
        assert_eq!(crate::state_get_in(&after, "font_level").as_deref(), Some("15"));
        // Setting it again replaces the line rather than adding a second one.
        let again = crate::state_with(&after, "font_level", "16");
        assert_eq!(again.matches("font_level").count(), 1, "{again}");
        assert_eq!(crate::state_get_in(&again, "font_level").as_deref(), Some("16"));
        assert_eq!(crate::state_get_in(&again, "theme").as_deref(), Some("nord"));
        // …and a file that never had a header gets one.
        let fresh = crate::state_with("", "theme", "dracula");
        assert!(fresh.starts_with("# cian runtime state"), "{fresh}");
    }

    /// `Enter` reads the file where its listing was — the *same* viewer,
    /// docked in that pane, with everything it can do. `F3` gives the same
    /// file the whole window; `:q` closes it and the listing is there again.
    #[test]
    fn enter_docks_the_panel_in_the_pane_and_f12_zooms_it() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "alpha\nbeta\n").unwrap();
        let body: String = (1..=200).map(|i| format!("line {i}\n")).collect();
        std::fs::write(d.path().join("b.log"), &body).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let at = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "b.log")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = at;

        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the viewer opened");
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left), "docked in this pane");

        // It is drawn in the pane, not over the window: the other pane still
        // lists its files beside it.
        let rows = render(&mut app, 120, 30);
        let screen = rows.join("\n");
        assert!(screen.contains("line 1"), "the file is in the pane:\n{screen}");
        assert!(screen.contains("a.txt"), "and the other pane still lists files");
        assert!(app.viewer_frame.width < 70, "it takes the pane's width: {:?}", app.viewer_frame);

        // Everything the viewer can do, it can do here — vi motions and all.
        for k in ['1', '0', '0', 'G'] {
            app.handle_key(key(k)).unwrap();
        }
        match &app.popup {
            Popup::Viewer { line, .. } => assert_eq!(*line, 99, "100G in a docked file"),
            other => panic!("expected the viewer, got {other:?}"),
        }

        // Everything the panel has to say is along the foot of the window: its
        // keys on the hint bar, its mode and position on the status bar.
        let rows = render(&mut app, 120, 30);
        let bottom = rows[rows.len().saturating_sub(2)].clone();
        let status = rows[rows.len() - 1].clone();
        assert!(
            bottom.contains("search") || bottom.contains("検索"),
            "the file's hints are on the bottom bar: {bottom:?}",
        );
        assert!(status.contains("READ"), "the mode is on the status bar: {status:?}");
        assert!(status.contains("100:1"), "…and where the cursor is: {status:?}");
        // Not in the panel's own frame any more.
        let framed = rows.iter().take(rows.len() - 3).any(|r| r.contains("READ"));
        assert!(!framed, "the frame gave the badge up:\n{rows:#?}");

        // The `:` line is cian's own, so it has the width of the window.
        app.handle_key(key(':')).unwrap();
        let rows = render(&mut app, 120, 30);
        let prompt = rows[rows.len().saturating_sub(3)].clone();
        assert!(prompt.contains(":_"), "the prompt is on cian's prompt row: {prompt:?}");
        assert!(
            rows[rows.len() - 1].contains("COMMAND"),
            "and the mode says so: {:?}",
            rows[rows.len() - 1],
        );
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // Tab crosses to the listing beside it; the file stays.
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Right);
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the file is still open");
        app.handle_key(key('j')).unwrap();
        assert!(app.right.active_ref().cursor > 0, "j moved the listing's cursor");
        let rows = render(&mut app, 120, 30);
        let bottom = rows[rows.len().saturating_sub(2)].clone();
        assert!(
            !(bottom.contains("whole window") || bottom.contains("全画面へ")),
            "the file's hints stepped aside: {bottom:?}",
        );
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Left, "and back to the file");

        // F12 (and F3, which used to mean this) zooms the pane it is docked
        // in, so the panel fills the window without being a second mode.
        app.handle_key(code(KeyCode::F(12))).unwrap();
        assert!(app.zoomed, "the pane zoomed");
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left), "still the same docked panel");
        let full = render(&mut app, 120, 30);
        assert!(full.iter().any(|r| r.contains("line 100")), "still the same place in it");
        assert!(app.viewer_frame.width > 100, "and it has the window now");
        app.handle_key(code(KeyCode::F(12))).unwrap();
        assert!(!app.zoomed, "and back to the pane");

        // `:q` closes it and the listing is back.
        for k in [':', 'q'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::None));
        let back = render(&mut app, 120, 30);
        assert!(back.iter().any(|r| r.contains("b.log")), "the listing is there");
    }

    /// The panel is one surface among the window's, not a dialog over them:
    /// a click on the listing beside it moves the focus there, `Shift+H` /
    /// `Shift+L` / `Shift+J` move it while reading, and `F3` reads a file in
    /// the *other* pane rather than opening a second kind of window.
    #[test]
    fn the_panel_gives_the_focus_up_to_a_click_and_to_shift_hjl() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "alpha\nbeta\n").unwrap();
        std::fs::write(d.path().join("b.txt"), "gamma\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let _ = render(&mut app, 120, 30);
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left));

        // A click on the listing beside it takes the focus.
        let r = app.layout_rects.right;
        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: r.x + 3,
            row: r.y + 3,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(app.focused, FocusedPane::Right, "the click moved the focus");
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the file is still open");

        // Shift+H comes back to it, Shift+J goes down to the shell.
        app.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(app.focused, FocusedPane::Left);
        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(app.focused, FocusedPane::Shell, "Shift+J while reading");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // …but not while editing: there `H` is a character.
        app.focus(FocusedPane::Left);
        app.handle_key(key('i')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(app.focused, FocusedPane::Left, "the editor kept the key");
        assert!(viewer_lines(&app)[0].contains('L'), "…and typed it: {:?}", viewer_lines(&app));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        quit_viewer_discarding(&mut app);

        // `F3` reads the file under the cursor in the *other* pane.
        app.focus(FocusedPane::Left);
        let at = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "b.txt")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = at;
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert_eq!(app.viewer_dock, Some(FocusedPane::Right), "opened over there");
        assert_eq!(app.focused, FocusedPane::Right, "and the focus followed it");
        assert_eq!(viewer_lines(&app), ["gamma"], "the file the cursor was on");
        let rows = render(&mut app, 120, 30);
        assert!(
            rows.iter().any(|r| r.contains("a.txt")),
            "the listing is still there, on the left:\n{rows:#?}",
        );
    }

    /// `F3` into a pane that is already reading something adds a tab there
    /// rather than replacing what is open — and it used to do nothing at all,
    /// because a leftover "F3 means full window" branch cleared the dock and
    /// returned before opening anything.
    #[test]
    fn f3_into_a_busy_pane_opens_another_tab() {
        let d = tempfile::tempdir().unwrap();
        for (n, b) in [("a.txt", "AAA\n"), ("b.txt", "BBB\n")] {
            std::fs::write(d.path().join(n), b).unwrap();
        }
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let go = |app: &mut App, n: &str| {
            app.focus(FocusedPane::Left);
            let at =
                app.active_pane().unwrap().entries.iter().position(|e| e.name == n).unwrap();
            app.active_pane_mut().unwrap().cursor = at;
            app.handle_key(code(KeyCode::F(3))).unwrap();
        };

        go(&mut app, "a.txt");
        assert_eq!(app.viewer_dock, Some(FocusedPane::Right));
        assert_eq!(viewer_lines(&app), ["AAA"]);

        go(&mut app, "b.txt");
        assert_eq!(viewer_lines(&app), ["BBB"], "the second one is what is being read");
        assert_eq!(app.viewer_tab_count(), 2, "and the first is still open, as a tab");
        assert_eq!(app.viewer_dock, Some(FocusedPane::Right), "in the same pane");

        // Shift+F2 steps back to the one that was there.
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(viewer_lines(&app), ["AAA"], "the tab strip has both");
    }

    /// The panel's frame goes quiet when the keyboard is somewhere else — a
    /// panel that keeps its mode colour while the keys go elsewhere looks
    /// live and is not.
    #[test]
    fn the_panels_frame_says_whether_it_has_the_keyboard() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let buf = render_buf(&mut app, 120, 30);
        let f = app.viewer_frame;
        let border = buf[(f.x, f.y)].fg;
        app.handle_key(code(KeyCode::Tab)).unwrap(); // focus the listing beside it
        let buf = render_buf(&mut app, 120, 30);
        let quiet = buf[(f.x, f.y)].fg;
        assert_ne!(border, quiet, "the frame changed colour when it lost the keyboard");
        assert_eq!(quiet, crate::theme::theme().border, "…to the colour an unfocused pane wears");
    }

    /// The borders resize while the panel is docked — with the mouse and
    /// with Ctrl+Shift+arrows. Both belong to the window's layout, so neither
    /// is the panel's to swallow.
    #[test]
    fn the_panes_still_resize_while_the_panel_is_open() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let _ = render(&mut app, 120, 30);
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the panel is open");
        let before = app.layout_rects.left.width;

        // Ctrl+Shift+Left narrows the left pane.
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::CONTROL | KeyModifiers::SHIFT))
            .unwrap();
        let _ = render(&mut app, 120, 30);
        assert!(app.layout_rects.left.width < before, "the keyboard resized it");

        // And the seam between the panes can still be grabbed and dragged.
        // The seam between the two panes: tall and narrow. (The other one is
        // the horizontal seam above the shell.)
        let seam = app
            .dividers
            .iter()
            .find(|d| d.zone.width <= 2 && d.zone.height > 2)
            .map(|d| d.zone)
            .expect("a vertical seam to grab");
        let narrowed = app.layout_rects.left.width;
        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: seam.x,
            row: seam.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert!(
            app.drag.is_some(),
            "the border was grabbed rather than the panel (seam {seam:?}, dividers {:?})",
            app.dividers.iter().map(|d| d.zone).collect::<Vec<_>>(),
        );
        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Drag(crossterm::event::MouseButton::Left),
            column: seam.x + 12,
            row: seam.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        let _ = render(&mut app, 120, 30);
        assert!(app.layout_rects.left.width > narrowed, "and dragging moved it");
    }

    /// The replace bar: two fields, three switches, and both ways of running
    /// it. A bar rather than a dialog so the file stays in view — watching
    /// each match land is what makes replace usable.
    #[test]
    fn the_replace_bar_replaces_one_at_a_time_and_all_at_once() {
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let alt = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
        let lines = |app: &App| match &app.popup {
            Popup::Viewer { view, .. } => view.lines.clone(),
            other => panic!("expected the panel, got {other:?}"),
        };

        let (_d, mut app) = viewer_on("cat CAT\ncattle\ncat\n");
        app.handle_key(ctrl('h')).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { replace: Some(_), .. }), "the bar opened");

        for c in "cat".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Tab)).unwrap();
        for c in "dog".chars() {
            app.handle_key(key(c)).unwrap();
        }
        // It is on the line `:` and `/` use, with what was typed in it.
        let bar = crate::render::editor_prompt(&app.popup, app.lang).unwrap();
        assert!(bar.contains("cat") && bar.contains("dog"), "the bar shows both: {bar}");

        // Enter takes the first match and stops on it.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(lines(&app)[0], "dog CAT", "one replaced, the rest untouched");

        // Shift+Enter takes the rest. Case-insensitive by default, so CAT goes
        // too — and `cattle` with it, since nothing said whole words.
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(lines(&app), vec!["dog dog", "dogtle", "dog"]);

        // Whole words only leaves `dogtle` alone. (Alt, not Ctrl: a letter has
        // to stay a letter in a text field.)
        let (_d, mut app) = viewer_on("cat cattle\n");
        app.handle_key(ctrl('h')).unwrap();
        for c in "cat".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Tab)).unwrap();
        for c in "dog".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(alt('w')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(lines(&app), vec!["dog cattle"], "whole words only");

        // Case sensitivity, and the switch showing in the bar.
        let (_d, mut app) = viewer_on("cat CAT\n");
        app.handle_key(ctrl('h')).unwrap();
        for c in "cat".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(alt('c')).unwrap();
        app.handle_key(code(KeyCode::Tab)).unwrap();
        for c in "dog".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(lines(&app), vec!["dog CAT"], "CAT is a different word now");

        // A regex, and a replacement carrying an escape.
        let (_d, mut app) = viewer_on("ORA-1234 here\n");
        app.handle_key(ctrl('h')).unwrap();
        for c in r"ORA-\d+".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(alt('r')).unwrap(); // wildcard
        app.handle_key(alt('r')).unwrap(); // regex
        app.handle_key(code(KeyCode::Tab)).unwrap();
        for c in "E".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(lines(&app), vec!["E here"], "the regex matched");

        // Esc closes it and changes nothing.
        app.handle_key(ctrl('h')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { replace: None, .. }), "the bar closed");
        assert_eq!(lines(&app), vec!["E here"]);
    }

    /// Clicking into a line of Japanese lands on the character that was
    /// clicked. A full-width character is one buffer character but two drawn
    /// columns; counting every character as one column put the cursor a
    /// character further left for every wide one before it, so a drag over a
    /// Japanese line selected somewhere else entirely.
    #[test]
    fn a_click_lands_where_it_was_aimed_on_a_wide_line() {
        let (_d, mut app) = viewer_on("あいうえお\nabcde\n");
        let _ = render(&mut app, 100, 30);
        let body = app.viewer_rect;
        let text_x = body.x + app.viewer_gutter;
        let click = |app: &mut App, cells: u16, kind| {
            app.handle_mouse(crossterm::event::MouseEvent {
                kind,
                column: text_x + cells,
                row: body.y,
                modifiers: KeyModifiers::NONE,
            });
        };
        use crossterm::event::{MouseButton, MouseEventKind};
        let at = |app: &App| match &app.popup {
            Popup::Viewer { col, .. } => *col,
            other => panic!("expected the panel, got {other:?}"),
        };

        // Cell 0 and 1 are both あ; cells 2 and 3 are い; 8 and 9 are お.
        for (cell, want) in [(0u16, 0usize), (1, 0), (2, 1), (3, 1), (8, 4), (9, 4)] {
            click(&mut app, cell, MouseEventKind::Down(MouseButton::Left));
            assert_eq!(at(&app), want, "cell {cell} is character {want}");
        }

        // And a drag from あ to う selects those three characters, not one.
        click(&mut app, 0, MouseEventKind::Down(MouseButton::Left));
        click(&mut app, 5, MouseEventKind::Drag(MouseButton::Left));
        match &app.popup {
            Popup::Viewer { anchor, line, col, visual: Some(ViewVisual::Char), .. } => {
                assert_eq!((*anchor, (*line, *col)), ((0, 0), (0, 2)), "あ through う");
            }
            other => panic!("expected a character selection, got {other:?}"),
        }
    }

    /// `x` over a selection cuts: what it took goes where `p` looks for it.
    /// It used to simply vanish, so `x` then `p` pasted whatever had been
    /// copied before — the last thing anyone means by cut and paste.
    #[test]
    fn what_x_cuts_is_what_p_puts_back() {
        for (start, expect) in [
            ('V', vec!["two", "one", "three"]),
            // `p` puts it after the cursor, which is where vi puts it: the
            // line is "e", the cut was "on", and it lands after the e.
            ('v', vec!["eon", "two", "three"]),
        ] {
            let (_d, mut app) = viewer_on("one\ntwo\nthree\n");
            app.handle_key(key(start)).unwrap();
            if start == 'v' {
                // `v` then `l` selects "on"; the linewise case takes the line.
                app.handle_key(key('l')).unwrap();
            }
            app.handle_key(key('x')).unwrap();
            assert!(app.yank.is_some(), "the cut text is on the clipboard");
            app.handle_key(key('p')).unwrap();
            assert_eq!(viewer_lines(&app), expect, "started with {start}");
        }

        // The operator form too: `dd` then `p` puts the line back below.
        let (_d, mut app) = viewer_on("one\ntwo\n");
        app.handle_key(key('d')).unwrap();
        app.handle_key(key('d')).unwrap();
        assert_eq!(viewer_lines(&app), vec!["two"]);
        app.handle_key(key('p')).unwrap();
        assert_eq!(viewer_lines(&app), vec!["two", "one"]);
    }

    /// The line-transform verbs act on a selection when there is one, and on
    /// the whole file when there is not. `:lf` and `:crlf` are the exception,
    /// and have to be: a line ending is a property of the file, not of a run
    /// of lines inside it.
    #[test]
    fn the_transforms_follow_the_selection_and_the_endings_do_not() {
        // `:han` on two selected lines of three.
        let (_d, mut app) = viewer_on("ＡＢＣ\nＤＥＦ\nＧＨＩ\n");
        app.handle_key(key('V')).unwrap();
        app.handle_key(key('j')).unwrap();
        app.command_buffer.clear();
        app.handle_key(key(':')).unwrap();
        for c in "han".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), vec!["ABC", "DEF", "ＧＨＩ"], "only the selection");
        assert!(
            app.message.as_deref().is_some_and(|m| m.contains("selection")),
            "and it says so: {:?}",
            app.message,
        );

        // With nothing selected it is the whole file.
        let (_d, mut app) = viewer_on("ＡＢＣ\nＤＥＦ\n");
        app.handle_key(key(':')).unwrap();
        for c in "han".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), vec!["ABC", "DEF"], "the whole file");

        // `:crlf` is the file's, selection or no selection.
        let (_d, mut app) = viewer_on("one\ntwo\nthree\n");
        app.handle_key(key('V')).unwrap();
        app.handle_key(key(':')).unwrap();
        for c in "crlf".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        match &app.popup {
            Popup::Viewer { view, .. } => {
                assert_eq!(view.eol, cian_core::viewer::Eol::Crlf, "every line of it");
            }
            other => panic!("expected the panel, got {other:?}"),
        }
    }

    /// `viw` and its family select the object rather than typing it. Text
    /// objects only ran after an operator, so over a selection `v` `i` `w`
    /// was read as "enter insert, type a w" — and put a `w` in the file.
    #[test]
    fn a_text_object_over_a_selection_selects_it() {
        let press = |app: &mut App, keys: &str| {
            for c in keys.chars() {
                app.handle_key(key(c)).unwrap();
            }
        };
        for (setup, keys, want) in [
            ("All done\n", "viwy", "All"),
            ("All done\n", "wviwy", "done"),
            ("say \"hi there\" now\n", "fhvi\"y", "hi there"),
            ("call(one, two)\n", "fovi(y", "one, two"),
            ("x 'abc' y\n", "fava'y", "'abc'"),
        ] {
            let (_d, mut app) = viewer_on(setup);
            press(&mut app, keys);
            assert_eq!(app.yank.as_deref(), Some(want), "{keys} on {setup:?}");
            assert_eq!(
                viewer_lines(&app)[0],
                setup.trim_end_matches('\n'),
                "and nothing was typed into the file",
            );
        }

        // …and an operator over the selection still acts on it.
        let (_d, mut app) = viewer_on("All done\n");
        press(&mut app, "viwd");
        assert_eq!(viewer_lines(&app)[0], " done");
    }

    /// A copy the system clipboard refused is still cian's copy: `p` pastes
    /// what was just taken rather than whatever the clipboard was holding
    /// from before. The failure used to be discarded, which made a copy look
    /// like it had worked and the paste produce something else entirely.
    #[test]
    fn a_refused_clipboard_does_not_paste_something_older() {
        let (_d, mut app) = viewer_on("All done\n");
        // `viewer_on` runs without a system clipboard, which is exactly the
        // "it would not take it" case.
        assert!(app.clipboard.is_none());
        for c in "viwy".chars() {
            app.handle_key(key(c)).unwrap();
        }
        assert_eq!(app.yank.as_deref(), Some("All"));
        assert!(!app.yank_on_clipboard, "the clipboard did not take it");
        // No clipboard service at all is not a problem worth a sentence: `p`
        // pastes from cian's own copy there and always has.
        assert_eq!(app.message.as_deref(), Some("copied"));
        app.handle_key(key('$')).unwrap();
        app.handle_key(key('p')).unwrap();
        assert_eq!(viewer_lines(&app)[0], "All doneAll", "p pasted what was copied");
    }

    /// A line wider than the panel scrolls sideways under the cursor, and
    /// says how much is off screen. It used to simply run off the edge: the
    /// cursor kept moving and the text stopped.
    #[test]
    fn a_long_line_follows_the_cursor_sideways() {
        // 200 columns of it, in a panel about 90 wide.
        let long: String = (0..40).map(|i| format!("word{i:02} ")).collect();
        let (_d, mut app) = viewer_on(&format!("{long}\nshort\n"));
        let seen = |app: &mut App| render(app, 100, 30).join("\n");
        let hscroll = |app: &App| match &app.popup {
            Popup::Viewer { hscroll, .. } => *hscroll,
            other => panic!("expected the panel, got {other:?}"),
        };

        let screen = seen(&mut app);
        assert_eq!(hscroll(&app), 0, "starts at the left");
        assert!(screen.contains("word00"), "the head of the line is shown");
        assert!(!screen.contains("word39"), "and the tail is not");

        // `$` goes to the end of it; the view has to follow.
        app.handle_key(key('$')).unwrap();
        let screen = seen(&mut app);
        assert!(hscroll(&app) > 0, "scrolled sideways");
        assert!(screen.contains("word39"), "the tail is shown now:\n{screen}");
        assert!(!screen.contains("word00"), "and the head has gone by");

        // …and back again.
        app.handle_key(key('0')).unwrap();
        let screen = seen(&mut app);
        assert_eq!(hscroll(&app), 0, "back to the left");
        assert!(screen.contains("word00"));

        // The line number is still there — the gutter does not scroll with
        // the text.
        assert!(screen.lines().any(|l| l.contains(" 1 ")), "gutter kept:\n{screen}");
    }

    /// Nothing is ever drawn over a frame, whatever is in it. A wide
    /// character that will not fit before the border is left out — the border
    /// is the thing that has to be right, and half a character is not one.
    #[test]
    fn a_wide_character_never_eats_the_border() {
        // Names of every length around the pane's right edge, so one of them
        // lands with a full-width character straddling it.
        let names: Vec<String> = (1..=30).map(|n| format!("{}.txt", "あ".repeat(n))).collect();
        let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
        let (_d, mut app) = app_with(&refs);
        // Past the "starting up" splash, which is drawn over both panes.
        app.startup_at = std::time::Instant::now() - std::time::Duration::from_secs(30);
        let buf = render_buf(&mut app, 100, 40);
        for (name, r) in
            [("left", app.layout_rects.left), ("right", app.layout_rects.right)]
        {
            for y in r.y + 1..r.y + r.height - 1 {
                for (edge, x) in [("left", r.x), ("right", r.x + r.width - 1)] {
                    let sym = buf[(x, y)].symbol();
                    assert!(
                        sym == "│" || sym == "┃" || sym == "║",
                        "{name} pane's {edge} border at row {y} is {sym:?}",
                    );
                }
            }
        }

        // …and the shell panel, drawn by the terminal widget rather than by
        // cian, with wide characters running exactly to its edge and past it.
        let dir = tempfile::tempdir().unwrap();
        let sh = cian_pty::default_shell();
        let session = cian_pty::PtySession::new(dir.path(), &sh, 24, 80).unwrap();
        app.shell.tabs.push(ShellTab::new(session));
        app.shell.active = 0;
        app.preview_on = false;
        app.focus(FocusedPane::Shell);
        let _ = render(&mut app, 100, 40);
        let cols = app.shell.cols as usize;
        if let Some(s) = app.shell.active_session() {
            // Exactly to the edge, then one narrow character shifting the next
            // line so a wide one straddles it.
            let text =
                format!("{}\r\nx{}Z\r\n", "あ".repeat(cols / 2), "あ".repeat(cols / 2));
            s.parser().lock().unwrap().process(text.as_bytes());
        }
        let buf = render_buf(&mut app, 100, 40);
        let r = app.layout_rects.shell;
        for y in r.y + 1..r.y + r.height - 1 {
            for (edge, x) in [("left", r.x), ("right", r.x + r.width - 1)] {
                let sym = buf[(x, y)].symbol();
                assert!(
                    sym == "│" || sym == "┃" || sym == "║",
                    "shell's {edge} border at row {y} is {sym:?}",
                );
            }
        }
    }

    /// A picture still draws when the terminal has no image protocol — the
    /// half-block renderer is the fallback, and it has to actually run.
    #[test]
    fn an_image_previews_without_a_terminal_protocol() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbImage::from_pixel(8, 8, image::Rgb([200, 40, 40]))
            .save(dir.path().join("shot.png"))
            .unwrap();
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let i = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "shot.png")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = i;
        assert!(crate::preview::preview_target(&app).is_ok(), "an image is previewable");

        let buf = render_buf(&mut app, 100, 30);
        let sh = app.layout_rects.shell;
        let painted = (sh.y + 1..sh.y + sh.height - 1)
            .flat_map(|y| (sh.x + 1..sh.x + sh.width - 1).map(move |x| (x, y)))
            .filter(|(x, y)| !buf[(*x, *y)].symbol().trim().is_empty())
            .count();
        assert!(painted > 20, "the picture is drawn: {painted} cells");
    }

    /// A long prompt stays readable. The chat's input drew one row per typed
    /// line and let a long one run off the right-hand edge; the AI-command
    /// dialog sized its box from the unwrapped text and cut the rest off the
    /// bottom. A prompt you cannot read back is one you cannot correct.
    #[test]
    fn a_long_prompt_is_visible_in_full() {
        let long: String = (0..24).map(|i| format!("word{i:02} ")).collect();
        assert!(long.len() > 150, "longer than any dialog is wide");

        // The chat.
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_ai_chat(ChatMode::Ai, vec![], false);
        for c in long.chars() {
            app.handle_key(key(c)).unwrap();
        }
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("word00"), "the start is there:\n{screen}");
        assert!(screen.contains("word23"), "and so is the end — where the caret is");

        // The AI-command dialog.
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::TextInput {
            title: " command from a description ".into(),
            prompt: "what should it do?".into(),
            buffer: long.clone(),
            kind: InputKind::AiShellCmd,
            cursor: long.chars().count(),
        };
        let screen = render(&mut app, 100, 30).join("\n");
        assert!(screen.contains("word00"), "the start is there:\n{screen}");
        assert!(screen.contains("word23"), "and the end:\n{screen}");
    }

    /// A bookmark that could not be written says so. Adding one reported a
    /// failed save; deleting one and making a group did not — the list on
    /// screen changed, the file did not, and the next launch had the old
    /// bookmarks back with no hint why.
    #[test]
    fn a_bookmark_that_cannot_be_saved_says_so() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // A path that cannot be written to: the "directory" is a file.
        let blocked = app.active_pane().unwrap().cwd.join("a.txt").join("shortcuts.lua");
        app.shortcuts.path = blocked;
        app.shortcuts.entries = vec![
            Shortcut { name: "one".into(), target: Some("/tmp".into()), children: None },
            Shortcut { name: "two".into(), target: Some("/tmp".into()), children: None },
        ];

        // Delete the first — the path that used to swallow it.
        app.popup = Popup::Shortcuts {
            entries: app.shortcuts.entries.clone(),
            cursor: 0,
            path: vec![],
        };
        app.handle_key(key('d')).unwrap();
        match &app.popup {
            Popup::Notice { lines } => {
                let text = lines.join(" ");
                assert!(
                    text.contains("could not be saved") || text.contains("保存できませんでした"),
                    "it says so: {text}",
                );
                assert!(text.contains(":where"), "and where it would have gone: {text}");
            }
            other => panic!("expected a notice, got {other:?}"),
        }
    }

    /// `preview_skip` names the kinds the cursor-follow preview leaves alone.
    /// A `.vsix` is a zip of an editor extension: listing one means unpacking
    /// it, which stalls the panel for a file nobody wanted to look inside.
    #[test]
    fn preview_skip_leaves_those_kinds_alone() {
        let dir = tempfile::tempdir().unwrap();
        for n in ["ext.vsix", "disc.ISO", "notes.txt"] {
            std::fs::write(dir.path().join(n), b"x").unwrap();
        }
        let p = dir.path().to_path_buf();
        let mut config = en_config();
        // Written any of the three ways someone would write it.
        config.options.preview_skip =
            vec!["vsix".into(), ".iso".into(), "TAR.GZ".into()];
        let mut app = App::new(p.clone(), p, config).unwrap();

        let at = |app: &mut App, name: &str| {
            let i = app
                .active_pane()
                .unwrap()
                .entries
                .iter()
                .position(|e| e.name == name)
                .unwrap();
            app.active_pane_mut().unwrap().cursor = i;
            crate::preview::preview_target(app)
        };

        assert!(at(&mut app, "notes.txt").is_ok(), "an ordinary file still previews");
        let e = at(&mut app, "ext.vsix").unwrap_err();
        assert!(e.contains("preview_skip"), "and says why: {e:?}");
        // Case does not matter, on the file or in the config.
        assert!(at(&mut app, "disc.ISO").is_err(), "matched whatever the case");

        // Some kinds are skipped without being configured at all: a `.vsix`
        // is an editor extension, and unpacking one to list it stalls the
        // panel for something nobody is looking at the folder for.
        let dir2 = tempfile::tempdir().unwrap();
        for n in ["ext.vsix", "disc.iso", "lib.whl", "paper.pdf", "archive.zip", "notes.txt", "shot.png"] {
            std::fs::write(dir2.path().join(n), b"x").unwrap();
        }
        let p2 = dir2.path().to_path_buf();
        let mut plain = App::new(p2.clone(), p2, en_config()).unwrap();
        for skipped in ["ext.vsix", "disc.iso", "lib.whl", "paper.pdf"] {
            assert!(at(&mut plain, skipped).is_err(), "{skipped} is skipped by default");
        }
        // …but a plain archive is one someone is browsing on purpose.
        assert!(at(&mut plain, "archive.zip").is_ok(), "a zip still previews");
        assert!(at(&mut plain, "notes.txt").is_ok());
        // …and so does an image, which is the whole point of a preview.
        assert!(at(&mut plain, "shot.png").is_ok(), "a picture still previews");
    }

    /// The shell keeps what has gone past, with its colours, and can be
    /// scrolled back through it. The parser was built with a scrollback of
    /// zero, so a line leaving the top of the panel was simply gone.
    #[test]
    fn the_shell_can_be_scrolled_back_through_what_went_past() {
        let dir = tempfile::tempdir().unwrap();
        let sh = cian_pty::default_shell();
        let s = cian_pty::PtySession::new(dir.path(), &sh, 10, 80).unwrap();
        s.parser().lock().unwrap().process(
            (1..=200).map(|i| format!("line {i}\r\n")).collect::<String>().as_bytes(),
        );
        let seen = |s: &cian_pty::PtySession| s.parser().lock().unwrap().screen().contents();
        assert!(seen(&s).contains("line 200"), "the end is on screen");
        assert!(!seen(&s).contains("line 100"), "the middle is not");
        assert_eq!(s.scrollback_pos(), 0, "and it is live");

        // Back past the height of the screen — which used to panic, and is
        // why cian briefly kept a plain-text history of its own.
        assert_eq!(s.scroll_back(120), 120, "120 rows back");
        assert!(seen(&s).contains("line 72"), "which is up there:\n{}", seen(&s));
        assert!(!seen(&s).contains("line 200"), "the end has gone off the bottom");

        // Forward again, and to the end.
        s.scroll_back(-60);
        assert_eq!(s.scrollback_pos(), 60);
        s.scroll_to_bottom();
        assert_eq!(s.scrollback_pos(), 0);
        assert!(seen(&s).contains("line 200"), "back to live output");

        // It stops at both ends rather than running off them.
        s.scroll_back(-10);
        assert_eq!(s.scrollback_pos(), 0, "cannot scroll past the end");
        let far = s.scroll_back(isize::MAX / 2);
        assert!(far > 100 && far < 10_000, "clamped to what there is: {far}");
    }

    /// The wheel over the shell scrolls it, and typing comes back to the end
    /// — typing into a screen that is not the current one is how commands end
    /// up somewhere nobody was looking.
    #[test]
    fn the_wheel_scrolls_the_shell_and_typing_returns_to_it() {
        use crossterm::event::MouseEventKind;
        let (_d, mut app) = app_with(&["a.txt"]);
        let _ = render(&mut app, 100, 30);
        let Some(s) = app.shell.active_session() else {
            return; // no shell on this machine; the unit test above covers it
        };
        s.parser().lock().unwrap().process(
            (1..=200).map(|i| format!("line {i}\r\n")).collect::<String>().as_bytes(),
        );
        let shell = app.layout_rects.shell;
        let wheel = |app: &mut App, kind| {
            app.handle_mouse(crossterm::event::MouseEvent {
                kind,
                column: shell.x + 4,
                row: shell.y + 2,
                modifiers: KeyModifiers::NONE,
            });
        };
        wheel(&mut app, MouseEventKind::ScrollUp);
        wheel(&mut app, MouseEventKind::ScrollUp);
        let at = app.shell.active_session().map(|s| s.scrollback_pos()).unwrap_or(0);
        assert_eq!(at, 6, "six rows back, three to a notch");

        // …and the wheel does not steal the focus: reading is not choosing
        // where to type.
        assert_ne!(app.focused, FocusedPane::Shell, "focus stayed on the listing");

        // Typing into the shell brings it back to live output.
        app.focus(FocusedPane::Shell);
        app.handle_key(key('x')).unwrap();
        assert_eq!(
            app.shell.active_session().map(|s| s.scrollback_pos()),
            Some(0),
            "typing returned to the end",
        );
    }

    /// The wheel moves the view, both ways, and takes the cursor only when it
    /// would otherwise be left off screen.
    #[test]
    fn the_wheel_scrolls_the_panel_in_both_directions() {
        use crossterm::event::{MouseButton, MouseEventKind};
        let long: String = (0..40).map(|i| format!("word{i:02} ")).collect();
        let body = format!("{long}\n{}", "filler\n".repeat(60));
        let (_d, mut app) = viewer_on(&body);
        let _ = render(&mut app, 100, 30);
        let wheel = |app: &mut App, kind| {
            let r = app.viewer_rect;
            app.handle_mouse(crossterm::event::MouseEvent {
                kind,
                column: r.x + 4,
                row: r.y + 2,
                modifiers: KeyModifiers::NONE,
            });
        };
        let at = |app: &App| match &app.popup {
            Popup::Viewer { scroll, hscroll, line, .. } => (*scroll, *hscroll, *line),
            other => panic!("expected the panel, got {other:?}"),
        };

        // Down and back up.
        wheel(&mut app, MouseEventKind::ScrollDown);
        assert_eq!(at(&app).0, 3, "the view moved, three lines");
        assert!(at(&app).2 >= 3, "and the cursor came along rather than scrolling away");
        wheel(&mut app, MouseEventKind::ScrollUp);
        assert_eq!(at(&app).0, 0);

        // Sideways, for the terminals that report it.
        wheel(&mut app, MouseEventKind::ScrollRight);
        assert_eq!(at(&app).1, 3, "three columns right");
        wheel(&mut app, MouseEventKind::ScrollLeft);
        assert_eq!(at(&app).1, 0);

        // The wheel does not move the cursor while it stays in view: a flick
        // over a file should not change where typing would land.
        let before = at(&app).2;
        wheel(&mut app, MouseEventKind::ScrollDown);
        wheel(&mut app, MouseEventKind::ScrollUp);
        assert_eq!(at(&app).2, before, "the cursor stayed put");

        // A click still places it.
        app.handle_mouse(crossterm::event::MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: app.viewer_rect.x + app.viewer_gutter + 2,
            row: app.viewer_rect.y + 1,
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(at(&app).2, at(&app).0 + 1, "the row that was clicked");
    }

    /// Both bars are drawn on the frame, and only when there is something off
    /// screen to report.
    #[test]
    fn the_panel_says_how_much_is_off_screen() {
        let bars = |app: &mut App| {
            let buf = render_buf(app, 100, 20);
            let f = app.viewer_frame;
            let right: String = (f.y..f.y + f.height)
                .map(|y| buf[(f.x + f.width - 1, y)].symbol().to_string())
                .collect();
            let bottom: String = (f.x..f.x + f.width)
                .map(|x| buf[(x, f.y + f.height - 1)].symbol().to_string())
                .collect();
            (right, bottom)
        };

        // A short, narrow file: nothing to say, so nothing is drawn.
        let (_d, mut app) = viewer_on("one\ntwo\n");
        let (right, bottom) = bars(&mut app);
        assert!(!right.contains('┃'), "no vertical bar: {right:?}");
        assert!(!bottom.contains('━'), "no horizontal bar: {bottom:?}");

        // Taller than the panel: a bar down the right border.
        let (_d, mut app) = viewer_on(&"line\n".repeat(200));
        let (right, _) = bars(&mut app);
        assert!(right.contains('┃'), "a vertical bar: {right:?}");

        // Wider than the panel: a bar along the bottom border.
        let long: String = (0..40).map(|i| format!("word{i:02} ")).collect();
        let (_d, mut app) = viewer_on(&format!("{long}\n"));
        let (_, bottom) = bars(&mut app);
        assert!(bottom.contains('━'), "a horizontal bar: {bottom:?}");
    }

    /// The whole operator/object grid, since `viw` turned out to be broken
    /// and the only way to know the rest are not is to press them. Each row
    /// is: the keys, the buffer they leave, what went to the clipboard, and
    /// whether it ended up typing.
    #[test]
    fn every_operator_and_object_pairing() {
        let run = |setup: &str, keys: &str| -> (String, String, bool) {
            let (_d, mut app) = viewer_on(&format!("{setup}\n"));
            for c in keys.chars() {
                app.handle_key(key(c)).unwrap();
            }
            let editing = matches!(app.popup, Popup::Viewer { editing: true, .. });
            (viewer_lines(&app).join("|"), app.yank.clone().unwrap_or_default(), editing)
        };
        // (buffer, keys) -> (buffer after, yanked, left typing)
        let cases: &[(&str, &str, &str, &str, bool)] = &[
            // Quotes and brackets, inside and around.
            ("say \"hi there\" now", "fhci\"", "say \"\" now", "hi there", true),
            ("say \"hi there\" now", "fhdi\"", "say \"\" now", "hi there", false),
            ("say \"hi there\" now", "fhyi\"", "say \"hi there\" now", "hi there", false),
            ("say \"hi there\" now", "fhda\"", "say  now", "\"hi there\"", false),
            ("x 'abc' y", "faci'", "x '' y", "abc", true),
            ("x 'abc' y", "fada'", "x  y", "'abc'", false),
            ("say `x` now", "fxci`", "say `` now", "x", true),
            ("call(one, two) end", "foci(", "call() end", "one, two", true),
            // The closing half of a pair names the same object.
            ("call(one, two) end", "fodi)", "call() end", "one, two", false),
            ("call(one, two) end", "foya(", "call(one, two) end", "(one, two)", false),
            ("arr[1] end", "f1di[", "arr[] end", "1", false),
            ("map{a: 1} end", "f1di{", "map{} end", "a: 1", false),
            ("map{a: 1} end", "f1ca{", "map end", "{a: 1}", true),
            ("tag<b> end", "fbdi<", "tag<> end", "b", false),
            // Words, inside and around.
            ("All done here", "ciw", " done here", "All", true),
            ("All done here", "diw", " done here", "All", false),
            ("All done here", "yiw", "All done here", "All", false),
            ("All done here", "caw", "done here", "All ", true),
            ("All done here", "daw", "done here", "All ", false),
            // Operator plus motion, with and without a count.
            ("All done here", "cw", " done here", "All", true),
            ("All done here", "dw", "done here", "All ", false),
            ("All done here", "d2w", "here", "All done ", false),
            ("All done here", "2dw", "here", "All done ", false),
            ("All done here", "c2w", " here", "All done", true),
            ("All done here", "de", " done here", "All", false),
            ("All done here", "d$", "", "All done here", false),
            ("All done here", "dtd", "done here", "All ", false),
            // Line-wise.
            ("one\ntwo\nthree", "dd", "two|three", "one\n", false),
            ("one\ntwo\nthree", "2dd", "three", "one\ntwo\n", false),
            ("one\ntwo\nthree", "dj", "three", "one\ntwo\n", false),
            ("one\ntwo\nthree", "cc", "|two|three", "one\n", true),
            ("one\ntwo\nthree", "yy", "one|two|three", "one\n", false),
            // An object spanning lines is line-wise, as it is in vi: the
            // brackets stay where they are rather than meeting on one line.
            ("call(\n  one,\n)", "jdi(", "call(|)", "  one,\n", false),
            // Over a selection the object is what to select — the case that
            // was typing a `w` into the file.
            ("say \"hi\" now", "fhvi\"y", "say \"hi\" now", "hi", false),
            ("x 'abc' y", "fava'y", "x 'abc' y", "'abc'", false),
            ("All done here", "vawy", "All done here", "All ", false),
        ];
        for (setup, keys, buffer, yanked, typing) in cases {
            let got = run(setup, keys);
            assert_eq!(
                (got.0.as_str(), got.1.as_str(), got.2),
                (*buffer, *yanked, *typing),
                "{keys} on {setup:?}",
            );
        }

        // An operator followed by another operator is not a command. vi drops
        // it; what must not happen is the machine staying armed, so that the
        // next key is read as the tail of something abandoned.
        let (_d, mut app) = viewer_on("All done here\n");
        for c in "dc".chars() {
            app.handle_key(key(c)).unwrap();
        }
        assert!(matches!(app.popup, Popup::Viewer { pending: None, .. }), "nothing left armed");
        assert!(app.vim_obj.is_none() && app.vim_wait.is_none());
        app.handle_key(key('x')).unwrap();
        assert_eq!(viewer_lines(&app)[0], "ll done here", "and x is x again");
    }

    /// The vi keys the panel was missing, and the two it had but could not
    /// reach. Everything here was reported by using it.
    #[test]
    fn the_vi_keys_that_were_missing_or_unreachable() {
        let at = |app: &App| match &app.popup {
            Popup::Viewer { line, col, editing, scroll, .. } => (*line, *col, *editing, *scroll),
            other => panic!("expected the panel, got {other:?}"),
        };
        let press = |app: &mut App, keys: &str| {
            for c in keys.chars() {
                app.handle_key(key(c)).unwrap();
            }
        };

        // `zt` — the `t` was being read as the start of a find-till motion,
        // so the fold prefix never saw it and nothing scrolled.
        let body = format!("{}{}", "alpha beta gamma\nsecond line\nthird\n", "x\n".repeat(40));
        let (_d, mut app) = viewer_on(&body);
        press(&mut app, "jjzt");
        assert_eq!(at(&app).3, 2, "zt put the cursor's line at the top");
        press(&mut app, "zz");
        assert!(at(&app).3 < 2, "zz centred it again");

        // `s` and `S` — substitute a character, and a line.
        let (_d, mut app) = viewer_on("alpha beta\nsecond\n");
        press(&mut app, "s");
        assert_eq!(viewer_lines(&app)[0], "lpha beta");
        assert!(at(&app).2, "and it is typing");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        press(&mut app, "S");
        assert_eq!(viewer_lines(&app)[0], "", "S emptied the line");
        assert!(at(&app).2);
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // `A` — the end of the line. It was the AI's key once and never came
        // back. `C` changes to the end of the line.
        let (_d, mut app) = viewer_on("alpha beta\n");
        press(&mut app, "A");
        assert_eq!(at(&app).1, 10, "A went to the end");
        assert!(at(&app).2);
        app.handle_key(code(KeyCode::Esc)).unwrap();
        press(&mut app, "0llC");
        assert_eq!(viewer_lines(&app)[0], "al", "C took the rest of the line");
        assert_eq!(app.yank.as_deref(), Some("pha beta"), "and kept it");

        // `r` stamps one character, `3r` three; `R` overwrites until Esc.
        let (_d, mut app) = viewer_on("abcdef\n");
        press(&mut app, "rZ");
        assert_eq!(viewer_lines(&app)[0], "Zbcdef");
        press(&mut app, "0");
        press(&mut app, "3rY");
        assert_eq!(viewer_lines(&app)[0], "YYYdef", "3rY overwrote three");
        press(&mut app, "0Rxy");
        assert_eq!(viewer_lines(&app)[0], "xyYdef", "R overwrote rather than pushed");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        press(&mut app, "0ix");
        assert_eq!(viewer_lines(&app)[0], "xxyYdef", "and insert inserts again");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // `:combine` joins with a space, `gJ` without, and a count takes
        // more lines. `J` is the window's key for the shell below.
        let combine = |app: &mut App, cmd: &str| {
            app.handle_key(key(':')).unwrap();
            for c in cmd.chars() {
                app.handle_key(key(c)).unwrap();
            }
            app.handle_key(code(KeyCode::Enter)).unwrap();
        };
        let (_d, mut app) = viewer_on("one\ntwo\nthree\nfour\n");
        combine(&mut app, "combine");
        assert_eq!(viewer_lines(&app)[0], "one two");
        press(&mut app, "gJ");
        assert_eq!(viewer_lines(&app)[0], "one twothree");
        let (_d, mut app) = viewer_on("one\ntwo\nthree\nfour\n");
        combine(&mut app, "combine 3");
        assert_eq!(viewer_lines(&app), vec!["one two three", "four"], "three lines");
        let (_d, mut app) = viewer_on("one\ntwo\n");
        combine(&mut app, "combine!");
        assert_eq!(viewer_lines(&app), vec!["onetwo"], "the ! form adds no space");

        // W / E / B are the WORD forms: a word stops at punctuation, a WORD
        // runs to the next space.
        let (_d, mut app) = viewer_on("one two.three four\n");
        press(&mut app, "w");
        assert_eq!(at(&app).1, 4, "w to `two`");
        press(&mut app, "w");
        assert_eq!(at(&app).1, 7, "…and w stops at the dot");
        press(&mut app, "0W");
        assert_eq!(at(&app).1, 4, "W to `two.three`");
        press(&mut app, "W");
        assert_eq!(at(&app).1, 14, "…and W skips over the dot to `four`");
        press(&mut app, "0E");
        assert_eq!(at(&app).1, 2, "E to the end of `one`");
        press(&mut app, "E");
        assert_eq!(at(&app).1, 12, "…then the end of `two.three`");
        press(&mut app, "$B");
        assert_eq!(at(&app).1, 14, "B to the start of `four`");
        press(&mut app, "B");
        assert_eq!(at(&app).1, 4, "…then over the whole of `two.three`");
        press(&mut app, "$ge");
        assert_eq!(at(&app).1, 12, "ge back to the end of the previous word");
        // …and they take an operator: `dW` eats the punctuation with it.
        let (_d, mut app) = viewer_on("one two.three four\n");
        press(&mut app, "wdW");
        assert_eq!(viewer_lines(&app)[0], "one four");
        let (_d, mut app) = viewer_on("one two.three four\n");
        press(&mut app, "wdw");
        assert_eq!(viewer_lines(&app)[0], "one .three four", "dw stops at the dot");

        // `gg` is the top; a bare `g` is a prefix now and jumps nowhere.
        let (_d, mut app) = viewer_on("one\ntwo\nthree\n");
        press(&mut app, "jjg");
        assert_eq!(at(&app).0, 2, "g on its own waits");
        press(&mut app, "g");
        assert_eq!(at(&app).0, 0, "gg is the top");

        // `ca'` and friends — the quote was being eaten by the mark handler,
        // which reads `'a` as a jump.
        for (setup, keys, want) in [
            ("x 'abc' y\n", "faca'", "x  y"),
            ("x 'abc' y\n", "faci'", "x '' y"),
            ("say `x` now\n", "fxci`", "say `` now"),
            ("call(one, two)\n", "fodi(", "call()"),
        ] {
            let (_d, mut app) = viewer_on(setup);
            press(&mut app, keys);
            assert_eq!(viewer_lines(&app)[0], want, "{keys} on {setup:?}");
        }

        // A rectangle, `$`, then `A`: the same text on the end of lines that
        // are not the same length.
        let (_d, mut app) = viewer_on("one\nthirteen\nfive\n");
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)).unwrap();
        press(&mut app, "jj$A");
        press(&mut app, ";");
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), vec!["one;", "thirteen;", "five;"], "ragged right");
    }

    /// The wildcard mode: `crm*ne` finds `crmaine`, which is what a `*` in a
    /// search box is nearly always meant to say. It is its own mode rather
    /// than a change to the regex one — Alt+r cycles as typed → wildcard →
    /// regex — because `\d*` has to keep meaning what it says.
    #[test]
    fn the_wildcard_mode_reads_a_star_the_way_a_search_box_does() {
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let alt = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
        let (_d, mut app) = viewer_on("crmaine\ncrmne\n");
        app.handle_key(ctrl('h')).unwrap();
        for c in "crm*ne".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(alt('r')).unwrap(); // as typed → wildcard
        let bar = crate::render::editor_prompt(&app.popup, app.lang).unwrap();
        assert!(bar.contains("wildcard"), "the bar names the mode: {bar}");
        app.handle_key(code(KeyCode::Tab)).unwrap();
        for c in "X".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(viewer_lines(&app), vec!["X", "X"], "both, empty run included");

        // One more press is a real regex, where the same text means something
        // else and says so.
        let (_d, mut app) = viewer_on("crmaine\n");
        app.handle_key(ctrl('h')).unwrap();
        for c in "crm*ne".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(alt('r')).unwrap();
        app.handle_key(alt('r')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert!(
            app.message.as_deref().is_some_and(|m| m.contains("crm.*ne")),
            "{:?}",
            app.message,
        );

        // And a third press is back to as-typed, where `*` is a star.
        let (_d, mut app) = viewer_on("a*b\naxb\n");
        app.handle_key(ctrl('h')).unwrap();
        for c in "a*b".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Tab)).unwrap();
        app.handle_key(key('Z')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(viewer_lines(&app), vec!["Z", "axb"], "the literal star only");
    }

    /// A regex that finds nothing says why, when the reason is the usual one.
    /// `crm*ne` is "cr, any number of m, then ne" — it does not match
    /// `crmaine`, and looks like it should.
    #[test]
    fn a_regex_that_finds_nothing_says_what_the_star_means() {
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let (_d, mut app) = viewer_on("crmaine\ncrmaine\n");
        app.handle_key(ctrl('h')).unwrap();
        for c in "crm*ne".chars() {
            app.handle_key(key(c)).unwrap();
        }
        let alt_r = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::ALT);
        app.handle_key(alt_r).unwrap(); // wildcard
        app.handle_key(alt_r).unwrap(); // regex
        app.handle_key(code(KeyCode::Tab)).unwrap();
        for c in "x".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains("no matches"), "it did not match: {msg}");
        assert!(msg.contains("crm.*ne"), "and it says what to type: {msg}");
        assert_eq!(viewer_lines(&app), vec!["crmaine", "crmaine"], "nothing changed");

        // The pattern it suggests does match.
        app.handle_key(code(KeyCode::Backspace)).unwrap(); // the replacement
        app.handle_key(code(KeyCode::Tab)).unwrap();
        for _ in 0..6 {
            app.handle_key(code(KeyCode::Backspace)).unwrap();
        }
        for c in "crm.*ne".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Tab)).unwrap();
        app.handle_key(key('x')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert_eq!(viewer_lines(&app), vec!["x", "x"]);
    }

    /// `:replace` is the same bar, for the terminal that keeps Ctrl.
    #[test]
    fn replace_is_reachable_without_ctrl() {
        let (_d, mut app) = viewer_on("one\n");
        app.handle_key(key(':')).unwrap();
        for c in "replace".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { replace: Some(_), .. }));
    }

    /// Tab crosses the window; Shift+Tab steps the tab strip of whatever has
    /// the focus. Between two listings, between a listing and a file open in
    /// the editor panel, and between two of those panels — one key, because
    /// they are all just "the other side".
    #[test]
    fn tab_crosses_the_window_and_shift_tab_walks_the_tabs() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt", "b.txt"], &["c.txt"]);
        let _ = render(&mut app, 120, 30);

        // Listing ↔ listing.
        assert_eq!(app.focused, FocusedPane::Left);
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Right);
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Left);

        // Shift+Tab is this pane's own tabs, not the other pane.
        app.handle_key(key('t')).unwrap(); // a second tab here
        assert_eq!(app.left.tabs.len(), 2, "two tabs open");
        let before = app.left.active;
        app.handle_key(code(KeyCode::BackTab)).unwrap();
        assert_ne!(app.left.active, before, "Shift+Tab stepped the tab strip");
        assert_eq!(app.focused, FocusedPane::Left, "and left the focus where it was");

        // Listing ↔ editor panel.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left), "a file open on the left");
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Right, "crossed to the listing");
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the file stayed open");
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Left, "and back into the panel");

        // Panel ↔ panel: open one on the other side too.
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Right);
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.viewer_dock, Some(FocusedPane::Right), "and one open here");
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_eq!(app.focused, FocusedPane::Left, "Tab crosses between two panels too");

        // The shell is not on the Tab circuit: Shift+J is how you get there.
        app.handle_key(code(KeyCode::Tab)).unwrap();
        app.handle_key(code(KeyCode::Tab)).unwrap();
        assert_ne!(app.focused, FocusedPane::Shell, "Tab never lands on the shell");
    }

    /// Ctrl+G opens the grep, as it does in Sakura. Ctrl+F was already the
    /// key here; the two are the same prompt, so neither has to be the one
    /// remembered.
    #[test]
    fn ctrl_g_greps_the_way_ctrl_f_does() {
        for k in ['f', 'g'] {
            let (_d, mut app) = app_with(&["a.txt"]);
            app.handle_key(KeyEvent::new(KeyCode::Char(k), KeyModifiers::CONTROL)).unwrap();
            assert!(
                matches!(
                    app.popup,
                    Popup::TextInput { kind: InputKind::GrepRecursive, .. }
                ),
                "Ctrl+{k} opened the grep, got {:?}",
                app.popup,
            );
        }
    }

    /// The seven keys every editor shares — save, copy, cut, paste, undo,
    /// redo, select all — mean the same thing in all three of the panel's
    /// modes. A key you have to change modes to use is a key nobody reaches
    /// for, so they are handled ahead of the mode dispatch.
    #[test]
    fn the_editor_shortcuts_work_in_every_mode() {
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let text = |app: &App| match &app.popup {
            Popup::Viewer { view, .. } => view.lines.join("\n"),
            other => panic!("expected the panel, got {other:?}"),
        };

        // READ mode. Ctrl+X with no selection takes the line the cursor is
        // on, as an editor with these keys does.
        let (_d, mut app) = viewer_on("one\ntwo\nthree\n");
        app.handle_key(ctrl('x')).unwrap();
        assert_eq!(text(&app), "two\nthree", "Ctrl+X cut the cursor's line");
        assert_eq!(app.yank.as_deref(), Some("one\n"), "and it is on the clipboard");

        app.handle_key(ctrl('z')).unwrap();
        assert_eq!(text(&app), "one\ntwo\nthree", "Ctrl+Z put it back");
        app.handle_key(ctrl('y')).unwrap();
        assert_eq!(text(&app), "two\nthree", "Ctrl+Y took it away again");
        // vim's own name for the same step.
        app.handle_key(ctrl('z')).unwrap();
        app.handle_key(ctrl('r')).unwrap();
        assert_eq!(text(&app), "two\nthree", "Ctrl+R is redo too");

        app.handle_key(ctrl('v')).unwrap();
        assert_eq!(text(&app), "two\none\nthree", "Ctrl+V pasted it back");

        // VISUAL mode: Ctrl+C takes exactly what is selected.
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('V')).unwrap();
        app.handle_key(key('j')).unwrap();
        app.handle_key(ctrl('c')).unwrap();
        assert_eq!(app.yank.as_deref(), Some("two\none\n"), "the selection, not the file");

        // Ctrl+A selects all of it, whatever the mode.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        app.handle_key(ctrl('a')).unwrap();
        assert!(
            matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Line), .. }),
            "Ctrl+A selected the file",
        );
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // EDIT mode. The same keys, without leaving it.
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('i')).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { editing: true, .. }), "in the editor");
        for c in "hello".chars() {
            app.handle_key(key(c)).unwrap();
        }
        assert!(text(&app).starts_with("hello"), "typed: {:?}", text(&app));
        app.handle_key(ctrl('z')).unwrap();
        assert!(!text(&app).starts_with("hello"), "Ctrl+Z undid the insert while editing");
        app.handle_key(ctrl('y')).unwrap();
        assert!(text(&app).starts_with("hello"), "and Ctrl+Y redid it");
        assert!(matches!(app.popup, Popup::Viewer { editing: true, .. }), "still editing");
        app.handle_key(ctrl('s')).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { dirty: false, .. }), "Ctrl+S saved");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // A new edit throws the undone branch away, as vim does.
        app.handle_key(ctrl('z')).unwrap();
        let before = text(&app);
        app.handle_key(key('x')).unwrap();
        app.handle_key(ctrl('y')).unwrap();
        assert_ne!(text(&app), before, "the forked branch did not come back");
        assert_eq!(
            app.message.as_deref(),
            Some("already at newest change"),
            "and it says so",
        );
    }

    /// Ctrl+V pastes now, so the rectangle it used to start is on vim's own
    /// synonym for it — plus Alt+v and `:block`, which no terminal can take.
    #[test]
    fn the_rectangle_kept_the_keys_that_are_not_ctrl_v() {
        for start in [
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
            KeyEvent::new(KeyCode::Char('v'), KeyModifiers::ALT),
        ] {
            let (_d, mut app) = viewer_on("abcd\nefgh\n");
            app.handle_key(start).unwrap();
            assert!(
                matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Block), .. }),
                "{start:?} starts a rectangle",
            );
        }
        let (_d, mut app) = viewer_on("abcd\nefgh\n");
        app.handle_key(key(':')).unwrap();
        for c in "block".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Block), .. }));
    }

    /// Focus follows the mouse to the panel as well as away from it. Clicking
    /// the panel from another pane used to do nothing at all: the panel's own
    /// mouse handling only runs for the focused pane, so the click was
    /// swallowed on the way in.
    #[test]
    fn clicking_the_docked_panel_focuses_it() {
        let (_d, mut app) = app_with(&["a.txt", "b.log"]);
        let at = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "b.log")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = at;
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left), "docked on the left");
        let _ = render(&mut app, 120, 30);
        let frame = app.viewer_frame;

        let click = |app: &mut App, column: u16, row: u16| {
            app.handle_mouse(crossterm::event::MouseEvent {
                kind: crossterm::event::MouseEventKind::Down(
                    crossterm::event::MouseButton::Left,
                ),
                column,
                row,
                modifiers: KeyModifiers::NONE,
            });
        };

        // Away: the listing beside it takes the focus.
        let (right, shell) = (app.layout_rects.right, app.layout_rects.shell);
        click(&mut app, right.x + 4, right.y + 3);
        assert_eq!(app.focused, FocusedPane::Right, "the listing took it");

        // …and back. This is the direction that did not work.
        click(&mut app, frame.x + 4, frame.y + 3);
        assert_eq!(app.focused, FocusedPane::Left, "the panel took it back");

        // From the shell, too.
        click(&mut app, shell.x + 4, shell.y + 1);
        assert_eq!(app.focused, FocusedPane::Shell);
        click(&mut app, frame.x + 4, frame.y + 3);
        assert_eq!(app.focused, FocusedPane::Left, "and back from the shell");
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the file is still open");
    }

    /// F3 gave the panel the whole window, which is what F12 does. One key for
    /// that is enough, and F3 is the listings' — it opens a file in the other
    /// pane.
    #[test]
    fn f3_is_not_a_second_way_to_fill_the_window() {
        let (_d, mut app) = app_with(&["a.txt", "b.log"]);
        let at = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "b.log")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = at;
        app.handle_key(code(KeyCode::Enter)).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        assert!(!app.zoomed, "F3 does not zoom the panel");

        // F12 still does.
        app.handle_key(code(KeyCode::F(12))).unwrap();
        assert!(app.zoomed, "F12 does");

        // And it is not offered along the bottom any more.
        app.handle_key(code(KeyCode::F(12))).unwrap();
        let rows = render(&mut app, 120, 30);
        let bottom = rows[rows.len().saturating_sub(2)].clone();
        assert!(
            !bottom.contains("whole window") && !bottom.contains("全画面へ"),
            "the hint went with it: {bottom:?}",
        );
    }

    /// cian is written in Japanese first: with no `lang` in the config the
    /// interface is Japanese, and `lang = "en"` is what asks for English.
    /// (It was the other way round, which meant the people it was written
    /// for had to configure their own language.)
    #[test]
    fn the_interface_is_japanese_unless_asked() {
        let (_d, app) = app_with_lang(&["a.txt"], "ja");
        assert_eq!(app.lang, Lang::Ja);
        assert_eq!(app.menu_lang, Lang::Ja, "and the menus follow it");

        // The real default: nothing set at all.
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"").unwrap();
        let p = dir.path().to_path_buf();
        let mut app =
            App::new(p.clone(), p, cian_lua::Config::default()).unwrap();
        assert_eq!(app.lang, Lang::Ja, "no config, Japanese");
        assert_eq!(app.menu_lang, Lang::Ja);
        // Wide characters take two cells, so the rendered rows read "名 前";
        // the spacing is the terminal's, not the string's.
        let screen: String =
            render(&mut app, 120, 30).join("\n").chars().filter(|c| *c != ' ').collect();
        assert!(screen.contains("名前"), "the listing is in Japanese:\n{screen}");

        // And English is one option away.
        let (_d, app) = app_with_lang(&["a.txt"], "en");
        assert_eq!(app.lang, Lang::En);
    }

    /// A menu opened from the docked panel leaves the file stashed behind it,
    /// and the stash was drawn over the whole window: opening the menu looked
    /// like the panel had maximised itself, and Esc "restored" it.
    #[test]
    fn the_menu_does_not_move_the_docked_panel() {
        let (_d, mut app) = app_with(&["a.txt", "b.log"]);
        let at = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "b.log")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = at;
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.viewer_dock, Some(FocusedPane::Left), "docked in this pane");
        let _ = render(&mut app, 120, 30);
        let docked_frame = app.viewer_frame;

        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::ContextMenu { .. }), "the menu opened");
        assert!(app.viewer_return.is_some(), "with the file waiting behind it");

        // The panel stays where it was: the pane beside it still lists files,
        // which it cannot do if the panel has taken the window.
        let rows = render(&mut app, 120, 30);
        let screen = rows.join("\n");
        assert!(screen.contains("b.log"), "the file is still shown:\n{screen}");
        assert!(screen.contains("Name"), "the other pane's listing is intact:\n{screen}");
        assert!(screen.contains("a.txt"), "with its files on it:\n{screen}");

        // Esc puts the menu away and changes nothing else.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "the file is back");
        let _ = render(&mut app, 120, 30);
        assert_eq!(app.viewer_frame, docked_frame, "in the same place it was");
    }

    /// Dialogs follow the theme now — a light theme's menus are light — so
    /// everything drawn on them has to read on them. They were painted for a
    /// dark surface: fixed greys, the theme accent used as body text, the
    /// chat's own cyan. On a light dialog those ran from 1.0:1 to 3.2:1.
    ///
    /// Two things are checked, on every preset in the gallery. That the text
    /// reads — 4.0:1, measured against the cell it actually sits on, which
    /// for a row under the cursor is the selection and not the dialog. And
    /// that the cell was painted at all: `Clear` empties cells without
    /// colouring them, so a dialog with no surface of its own showed the
    /// terminal's background — the `?` manual and Z's jump list did exactly
    /// that, and no contrast check would ever have caught it.
    #[test]
    fn every_popup_reads_on_the_theme_it_is_drawn_on() {
        use crate::theme::{set_theme, ResolvedTheme};
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut bad: Vec<String> = Vec::new();
        // Every preset in the gallery, not a sample of them: the light ones
        // are where this goes wrong, and "which light ones" is not something
        // to keep in step by hand.
        for name in crate::theme::THEME_NAMES {
            let t = crate::theme::theme_preset(name).unwrap();
            set_theme(t);
            for what in ["manual", "panel-help", "palette", "chat", "notice", "toggles", "gallery", "jump",
                "listings", "ssh-users", "snippets", "local-dest", "find", "history",
                "bookmarks", "macros", "sort", "encoding", "op-queue", "ai-history",
                "commit", "input", "quit", "menu", "pane-bg", "report", "archive",
                "git-log", "disk-usage"]
            {
                let (_d, mut app) = app_with(&["a.txt", "b.rs"]);
                match what {
                    // `?` in the panes.
                    "manual" => {
                        app.handle_key(key('?')).unwrap();
                    }
                    // `?` in the text editor panel.
                    "panel-help" => {
                        app.handle_key(code(KeyCode::Enter)).unwrap();
                        app.handle_key(code(KeyCode::F(12))).unwrap();
                        app.handle_key(key('?')).unwrap();
                    }
                    "palette" => {
                        app.handle_key(key('C')).unwrap();
                    }
                    // Z's directory jump is the same popup with other items
                    // in it; it is listed separately because it is the one
                    // the missing surface was noticed on.
                    "jump" => app.start_fuzzy_jump(),
                    "chat" => app.start_ai_chat(
                        ChatMode::Ai,
                        vec![
                            ChatMsg { user: true, text: "hello".into() },
                            ChatMsg { user: false, text: "a reply".into() },
                        ],
                        false,
                    ),
                    "notice" => {
                        app.command_buffer = "ls".into();
                        app.run_command();
                    }
                    "gallery" => app.start_theme_picker(),
                    // The rest are built straight from their variants: they
                    // need remote hosts, a git repo or a finished search to
                    // reach by key, and what is under test is only the paint.
                    "listings" => {
                        app.popup = Popup::SshHosts { cursor: 0, filter: String::new() }
                    }
                    "ssh-users" => app.popup = Popup::SshUsers { host: 0, cursor: 0 },
                    "snippets" => {
                        app.popup = Popup::Snippets { cursor: 0, filter: String::new() }
                    }
                    "local-dest" => {
                        app.popup =
                            Popup::LocalDest { files: vec!["one.txt".into()], cursor: 0 }
                    }
                    "find" => {
                        app.popup = Popup::FindResults {
                            hits: vec![cian_core::search::Hit {
                                path: "/tmp/a.txt".into(),
                                rel: "a.txt".into(),
                                is_dir: false,
                                line: Some((3, "a matching line".into())),
                            }],
                            cursor: 0,
                            scroll: 0,
                            by_ai: false,
                        }
                    }
                    "history" => {
                        app.popup =
                            Popup::History { entries: vec!["/tmp".into()], cursor: 0 }
                    }
                    "bookmarks" => {
                        app.popup = Popup::Shortcuts {
                            entries: vec![Shortcut {
                                name: "home".into(),
                                target: Some("/tmp".into()),
                                children: None,
                            }],
                            cursor: 0,
                            path: vec![],
                        }
                    }
                    "macros" => {
                        app.popup =
                            Popup::Macros { cursor: 0, names: vec!["build".into()] }
                    }
                    "sort" => app.popup = Popup::SortPicker { cursor: 0 },
                    "encoding" => {
                        app.popup =
                            Popup::EncodingPicker { cursor: 0, target: EncTarget::Shell }
                    }
                    "op-queue" => app.popup = Popup::OpQueue { cursor: 0 },
                    "ai-history" => app.popup = Popup::AiHistory { cursor: 0 },
                    "commit" => {
                        app.popup = Popup::CommitMessage {
                            buffer: "fix the thing".into(),
                            stat: " 1 file changed".into(),
                            dir: "/tmp".into(),
                            editing: false,
                        }
                    }
                    "input" => {
                        app.popup = Popup::TextInput {
                            title: " rename ".into(),
                            prompt: "new name".into(),
                            buffer: "a.txt".into(),
                            kind: InputKind::Rename { original: "a.txt".into() },
                            cursor: 5,
                        }
                    }
                    "quit" => app.popup = Popup::ConfirmQuit,
                    "menu" => app.open_context_menu(4, 4),
                    "pane-bg" => {
                        app.popup =
                            Popup::ColorPicker { pane: FocusedPane::Left, cursor: 0 }
                    }
                    "report" => {
                        app.popup = Popup::Report {
                            title: " report ".into(),
                            lines: vec!["one line of it".into(), "and another".into()],
                            scroll: 0,
                            back: Box::new(Popup::None),
                        }
                    }
                    "archive" => {
                        app.popup = Popup::Archive {
                            path: "/tmp/a.zip".into(),
                            members: vec![cian_core::archive::Member {
                                name: "inside.txt".into(),
                                is_dir: false,
                                size: 100,
                                compressed: 40,
                            }],
                            cursor: 0,
                            scroll: 0,
                        }
                    }
                    "git-log" => {
                        app.popup = Popup::GitLog {
                            title: " log ".into(),
                            dir: "/tmp".into(),
                            commits: vec![cian_core::git::Commit {
                                hash: "abc1234".into(),
                                date: "2026-08-11".into(),
                                author: "someone".into(),
                                subject: "a commit subject".into(),
                            }],
                            cursor: 0,
                            scroll: 0,
                            vcs: Vcs::Git,
                        }
                    }
                    "disk-usage" => {
                        app.popup = Popup::DiskUsage {
                            dir: "/tmp".into(),
                            entries: vec![cian_core::du::DuEntry {
                                name: "big".into(),
                                path: "/tmp/big".into(),
                                size: 4096,
                                is_dir: true,
                            }],
                            total: 4096,
                            cursor: 0,
                            scroll: 0,
                        }
                    }
                    _ => {
                        app.handle_key(key('T')).unwrap();
                    }
                }
                let buf = render_buf(&mut app, 110, 30);
                for y in 0..buf.area.height {
                    for x in 0..buf.area.width {
                        let c = &buf[(x, y)];
                        // An unpainted cell is the bug this sweep missed the
                        // first time: `Clear` empties cells without colouring
                        // them, so the dialog showed the terminal's own
                        // background — which passes any contrast check and
                        // follows no theme at all. A theme that paints a
                        // background paints every cell of the window, and
                        // every glyph on it has a colour of its own.
                        let written = !c.symbol().trim().is_empty();
                        // The right half of a wide glyph is left blank and
                        // unstyled by ratatui; the terminal paints it from
                        // the left half, so it is not a gap.
                        let wide_tail = !written
                            && x > 0
                            && crate::util::width(buf[(x - 1, y)].symbol()) == 2;
                        if t.base_bg.is_some()
                            && !wide_tail
                            && (matches!(c.bg, Color::Reset)
                                || (written && matches!(c.fg, Color::Reset)))
                        {
                            bad.push(format!(
                                "{:?} {what}: {:?} at ({x},{y}) is unpainted — {:?} on {:?}",
                                t.accent,
                                c.symbol(),
                                c.fg,
                                c.bg,
                            ));
                            continue;
                        }
                        if !c.symbol().chars().all(char::is_alphanumeric) || !written {
                            continue;
                        }
                        if matches!(c.fg, Color::Reset) || matches!(c.bg, Color::Reset) {
                            continue;
                        }
                        let cr = crate::render::contrast_ratio(c.fg, c.bg);
                        if cr < 4.0 {
                            bad.push(format!(
                                "{:?} {what}: {:?} at ({x},{y}) — {:?} on {:?} is {cr:.2}:1",
                                t.accent,
                                c.symbol(),
                                c.fg,
                                c.bg,
                            ));
                        }
                    }
                }
            }
        }
        set_theme(ResolvedTheme::DARK);
        let n = bad.len();
        bad.dedup();
        bad.truncate(40);
        assert!(bad.is_empty(), "{n} unreadable cells:\n{}", bad.join("\n"));
    }

    /// Every preset in the gallery resolves, and every one of them paints a
    /// dialog surface on the same side of the line as its page — a light
    /// theme with a dark menu is the thing this replaced.
    #[test]
    fn every_preset_resolves_and_its_dialogs_match_its_page() {
        use crate::theme::{theme_preset, THEME_NAMES};
        let lum = |c: Color| match c {
            Color::Rgb(r, g, b) => (299 * r as i32 + 587 * g as i32 + 114 * b as i32) / 1000,
            _ => -1,
        };
        for name in THEME_NAMES {
            let t = theme_preset(name).unwrap_or_else(|| panic!("{name} does not resolve"));
            let Some(base) = t.base_bg else { continue }; // `default` keeps the terminal's
            let (page, dialog) = (lum(base), lum(t.popup_bg));
            assert!(
                (page > 140) == (dialog > 140),
                "{name}: a {} page with a {} dialog",
                if page > 140 { "light" } else { "dark" },
                if dialog > 140 { "light" } else { "dark" },
            );
        }
        // …and the five that were asked for are among them.
        for name in ["monokai-pro", "ayu-dark", "ayu-light", "bluloco-light", "bearded", "nord"] {
            assert!(THEME_NAMES.contains(&name), "{name} is in the gallery");
            assert!(theme_preset(name).is_some(), "{name} resolves");
        }
    }

    /// Backspace in a search listing means the same as Esc. A set of results
    /// has no parent directory to climb to, so climbing to one is a surprise.
    #[test]
    fn backspace_leaves_a_search_listing_rather_than_wandering_off() {
        let d = tempfile::tempdir().unwrap();
        let sub = d.path().join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("hit.txt"), "x\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p.clone(), en_config()).unwrap();

        app.start_find("hit", cian_core::search::Mode::Name);
        drain_find(&mut app);
        app.handle_key(key('p')).unwrap(); // panelize
        assert!(app.active_pane().unwrap().is_flat(), "the pane is a result listing");

        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert!(!app.active_pane().unwrap().is_flat(), "back to a folder");
        assert_eq!(
            app.active_pane().unwrap().cwd.canonicalize().unwrap(),
            p.canonicalize().unwrap(),
            "the same folder, not its parent",
        );
    }

    /// `:r` after a search takes the pattern with it, so a replace is the
    /// replacement text and nothing else. (It was the bare `r`, which is vi's
    /// replace-one-character.)
    #[test]
    fn r_replaces_what_the_search_just_found() {
        let (_d, mut app) = viewer_on("alpha bravo\nbravo charlie\n");
        let colon_r = |app: &mut App| {
            app.handle_key(key(':')).unwrap();
            app.handle_key(key('r')).unwrap();
            app.handle_key(code(KeyCode::Enter)).unwrap();
        };
        // Nothing searched for yet: it says so rather than opening a prompt
        // with nothing in it.
        colon_r(&mut app);
        assert!(matches!(app.popup, Popup::Viewer { sub_input: None, .. }));
        assert!(app.message.as_deref().unwrap_or("").contains('/'));

        app.handle_key(key('/')).unwrap();
        for c in "bravo".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        colon_r(&mut app);
        assert!(
            matches!(&app.popup, Popup::Viewer { sub_input: Some(s), .. } if s == "s/bravo/"),
            "seeded with what was searched for: {:?}",
            app.popup,
        );
        for c in "BRAVO/g".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["alpha BRAVO", "BRAVO charlie"]);

        // A pattern full of slashes gets a delimiter that is not one.
        let (_d2, mut app) = viewer_on("/usr/local/bin\n");
        app.handle_key(key('/')).unwrap();
        for c in "/usr/".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        colon_r(&mut app);
        let Popup::Viewer { sub_input: Some(seed), .. } = &app.popup else { panic!("no prompt") };
        assert!(!seed.starts_with("s/"), "a slash delimiter would break it: {seed:?}");
        assert!(seed.contains("/usr/"), "the pattern is intact: {seed:?}");
    }

    /// Whether a tab-separated file lines up is arithmetic, and the terminal's
    /// font has no say in it. Checked in the cell buffer so "it looks off" can
    /// be told apart from "it is off".
    #[test]
    fn a_tab_separated_file_lines_up_at_the_right_stop() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("t.tsv"), "col1\tcol2\tcol3\nあ\tい\tう\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.show_ws = false;
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "t.tsv").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();

        // Where a marker sits on the screen, as a column number.
        let col_of = |app: &mut App, needle: &str, nth: usize| -> usize {
            let rows = render(app, 120, 20);
            let row = rows.iter().filter(|r| r.contains(needle)).nth(nth).expect("row");
            // Cells, not bytes: the row holds `あ`, which is three bytes and
            // one cell (the backend writes a wide char's second cell as a
            // space of its own).
            let at = row.find(needle).expect("column");
            row[..at].chars().count()
        };

        // Stops every four: `col1` fills one exactly, so its tab moves on to
        // the next — the field after it lands at eight, while a two-column
        // `あ` in the same place lands at four. They cannot line up.
        cian_core::viewer::set_tab_width(4);
        let (a, b) = (col_of(&mut app, "col2", 0), col_of(&mut app, "い", 0));
        assert_ne!(a, b, "four columns is too narrow for this file, by arithmetic");

        // Eight is wide enough for both, so they do.
        cian_core::viewer::set_tab_width(8);
        assert_eq!(
            col_of(&mut app, "col2", 0),
            col_of(&mut app, "い", 0),
            "the second field starts in the same column on both rows",
        );
        assert_eq!(
            col_of(&mut app, "col3", 0),
            col_of(&mut app, "う", 0),
            "and so does the third",
        );
        cian_core::viewer::set_tab_width(4);
    }

    /// The reports from the second pass: a tab drawn outside the viewer moved
    /// the terminal's cursor instead of the text (which left the Makefile on
    /// screen underneath the next preview), a rectangle reached past its own
    /// right edge into a half-covered character, and `I`/`A` did nothing on a
    /// line selection.
    #[test]
    fn tabs_blocks_and_line_selections_all_stay_inside_their_lines() {
        // A tab never reaches the screen as a tab outside the viewer.
        let out = crate::util::plain("a\tb");
        assert_eq!(out, "a   b", "expanded to the next stop");
        assert!(!crate::util::plain("x\u{7}y").contains('\u{7}'), "and no other control code");

        // The block stops at the last character wholly inside it.
        let (_d, mut app) = viewer_on("## 事前準備\n- ふたつめ\n");
        // Ctrl+Q, not Ctrl+V: the latter pastes now, as it does everywhere
        // else, and vim's own synonym is what starts a rectangle.
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)).unwrap();
        if let Popup::Viewer { line, col, .. } = &mut app.popup {
            *line = 1;
            *col = 2; // the `ふ`, which ends at column 4
        }
        app.handle_key(key('d')).unwrap();
        assert_eq!(viewer_lines(&app), ["事前準備", "たつめ"], "`事` was only half inside");

        // I and A on a line selection: the start of every line, and each
        // line's own end — no squaring off.
        let (_d2, mut app) = viewer_on("one\nlonger line\n\n");
        app.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT)).unwrap();
        if let Popup::Viewer { line, .. } = &mut app.popup {
            *line = 2;
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { block_input: Some(_), .. }), "asks for the text");
        app.handle_key(key(',')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["one,", "longer line,", ","], "each line's own end");

        app.handle_key(KeyEvent::new(KeyCode::Char('V'), KeyModifiers::SHIFT)).unwrap();
        if let Popup::Viewer { line, .. } = &mut app.popup {
            *line = 1;
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT)).unwrap();
        for c in "# ".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["# one,", "# longer line,", ","]);
    }

    /// The preview panel changes contents on every cursor move, so anything
    /// it fails to wipe reads as part of the next file.
    #[test]
    fn the_preview_panel_does_not_keep_the_last_file_underneath() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("a-long.txt"),
            (1..=40).map(|i| format!("LONGFILE line {i}\n")).collect::<String>(),
        )
        .unwrap();
        std::fs::write(d.path().join("b-short.txt"), "SHORTFILE only line\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        assert!(app.preview_on, "the preview is on by default");
        // Past the startup splash, which would otherwise cover the panel.
        app.startup_at = std::time::Instant::now() - std::time::Duration::from_secs(5);

        let show = |app: &mut App, name: &str| {
            app.active_pane_mut().unwrap().cursor =
                app.active_pane().unwrap().entries.iter().position(|e| e.name == name).unwrap();
            render(app, 120, 40).join("\n")
        };

        let long = show(&mut app, "a-long.txt");
        assert!(long.contains("LONGFILE line 10"), "the long file previews");
        let short = show(&mut app, "b-short.txt");
        assert!(short.contains("SHORTFILE"), "the short file previews");
        assert!(
            !short.contains("LONGFILE"),
            "the previous file is still on screen underneath:\n{short}",
        );
    }

    /// A message the panel raises goes to cian's own status line, along the
    /// bottom of the window, where every other message in the program
    /// appears — never into the panel.
    ///
    /// It used to take the panel's footer, and docked there is no footer to
    /// take: the line was drawn over the *text*, without clearing it, so
    /// "copied" appeared with a couple of the file's own characters trailing
    /// after it.
    #[test]
    fn a_message_goes_to_the_status_line_and_not_into_the_file() {
        let (_d, mut app) = viewer_on("one\ntwo\n");
        // The panel's own last row carries a message it raised; the window's
        // hint bar carries its keys; its prompt line carries what is typed.
        let panel_last = |app: &mut App| {
            let rows = render(app, 100, 30);
            // Three rows of window furniture below the panel: prompt (when
            // one is open), hints, status. Without a prompt that is two.
            let n = rows.len();
            rows[n - 4].clone()
        };
        let hint_bar = |app: &mut App| {
            let rows = render(app, 100, 30);
            rows[rows.len() - 2].clone()
        };

        // A message raised by this keystroke is on the status line…
        app.handle_key(key(']')).unwrap();
        app.handle_key(key(']')).unwrap();
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains("utline") || msg.contains("アウトライン"), "{msg:?}");
        let rows = render(&mut app, 100, 30);
        assert!(
            rows[rows.len() - 1].contains(&msg),
            "on cian's status line: {:?}",
            rows[rows.len() - 1],
        );
        // …and nowhere inside the panel, where it would be painted over the
        // file with whatever was already on that row left beside it.
        let last = rows.len() - 1;
        assert!(
            !rows.iter().take(last).any(|r| r.contains(&msg)),
            "not in the panel:\n{rows:#?}",
        );
        // The panel's own last row is still the panel's.
        let m = panel_last(&mut app);
        assert!(!m.contains(&msg), "the footer kept its own text: {m:?}");

        // The hints are untouched by any of it.
        app.handle_key(key('j')).unwrap();
        let f = hint_bar(&mut app);
        assert!(f.contains("search") || f.contains("検索"), "hints are there: {f:?}");
        assert!(app.message.is_some(), "the status line still has it");

        // The `:` prompt goes on cian's own prompt line, above the hints, and
        // the hints stay readable beside it.
        app.handle_key(key('/')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        app.handle_key(key(':')).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { sub_input: Some(_), .. }));
        let rows = render(&mut app, 100, 30);
        let hints = rows[rows.len() - 2].clone();
        let prompt = rows[rows.len() - 3].clone();
        assert!(prompt.contains("s/old/new/"), "the command line is visible: {prompt:?}");
        assert!(
            hints.contains("search") || hints.contains("検索"),
            "and the hints keep their own row: {hints:?}"
        );
    }

    /// `/` gets the same treatment as `:`: a prompt line above the hints, and
    /// the text gives up the row so nothing of the file is covered by it.
    #[test]
    fn the_viewer_search_prompt_sits_above_the_hints() {
        let (_d, mut app) = viewer_on("alpha\nbeta\ngamma\n");
        let before = render(&mut app, 100, 30);
        app.handle_key(key('/')).unwrap();
        app.handle_key(key('b')).unwrap();
        let rows = render(&mut app, 100, 30);
        let hints = rows[rows.len() - 2].clone();
        let prompt = rows[rows.len() - 3].clone();
        assert!(prompt.contains("/b_"), "what is being typed: {prompt:?}");
        assert!(
            hints.contains("search") || hints.contains("検索"),
            "the hints are still there: {hints:?}"
        );
        // The last line of the file must not be hidden behind the new row.
        assert!(before.iter().any(|r| r.contains("gamma")));
        assert!(rows.iter().any(|r| r.contains("gamma")), "the text kept its lines");
    }

    /// A binding can name its modifiers, so a shortcut whose Ctrl key the
    /// terminal keeps can be moved somewhere the terminal will deliver.
    #[test]
    fn a_keymap_entry_can_carry_a_modifier() {
        use crate::theme::parse_key_spec;
        assert_eq!(parse_key_spec("x"), Some(('x', KeyModifiers::NONE)));
        assert_eq!(parse_key_spec("alt+g"), Some(('g', KeyModifiers::ALT)));
        assert_eq!(parse_key_spec("ctrl+f"), Some(('f', KeyModifiers::CONTROL)));
        assert_eq!(parse_key_spec(" Option+G "), Some(('G', KeyModifiers::ALT)));
        // Shift folds into the character: terminals disagree about reporting
        // both, and the uppercase letter already says it.
        assert_eq!(parse_key_spec("shift+s"), Some(('S', KeyModifiers::NONE)));
        for bad in ["", "alt+", "hyper+g", "alt+gg", "+"] {
            assert!(parse_key_spec(bad).is_none(), "{bad:?} should be refused");
        }

        // …and it drives the real key handling.
        let (_d, mut app) = app_with_keymaps(&["a.txt"], vec![("alt+g", "grep_recursive".into())]);
        app.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT)).unwrap();
        assert!(
            matches!(&app.popup, Popup::TextInput { kind: InputKind::GrepRecursive, .. }),
            "Alt+g opened the grep prompt, got {:?}",
            app.popup,
        );
        // The unmodified key is untouched.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        app.handle_key(key('g')).unwrap();
        assert!(!matches!(&app.popup, Popup::TextInput { kind: InputKind::GrepRecursive, .. }));
    }

    /// Every Ctrl shortcut in the viewer needs a route that a terminal cannot
    /// intercept: iTerm2 keeps Ctrl+F for its own find bar and macOS takes
    /// Ctrl+Q for zoom, so a file that can be edited but not saved is a real
    /// possibility on a stock Mac.
    #[test]
    fn the_viewer_can_be_driven_without_a_single_ctrl_key() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("note.txt");
        std::fs::write(&f, "one\ntwo\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let open = |app: &mut App| {
            app.active_pane_mut().unwrap().cursor =
                app.active_pane().unwrap().entries.iter().position(|e| e.name == "note.txt").unwrap();
            app.handle_key(code(KeyCode::F(3))).unwrap();
        };
        let cmd = |app: &mut App, c: &str| {
            app.handle_key(key(':')).unwrap();
            if let Popup::Viewer { sub_input, .. } = &mut app.popup {
                *sub_input = Some(c.into());
            }
            app.handle_key(code(KeyCode::Enter)).unwrap();
        };

        // `:block` reaches the rectangle without Ctrl+V or Ctrl+Q.
        open(&mut app);
        cmd(&mut app, "block");
        assert!(matches!(app.popup, Popup::Viewer { visual: Some(ViewVisual::Block), .. }));
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // `:w` saves without Ctrl+S.
        app.handle_key(key('x')).unwrap(); // delete a character
        assert!(matches!(app.popup, Popup::Viewer { dirty: true, .. }));
        cmd(&mut app, "w");
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "ne\ntwo\n");
        assert!(matches!(app.popup, Popup::Viewer { dirty: false, .. }));

        // `:q` refuses to drop unsaved work; `:q!` says to anyway.
        app.handle_key(key('x')).unwrap();
        cmd(&mut app, "q");
        assert!(matches!(app.popup, Popup::Viewer { .. }), "still open");
        assert!(app.message.as_deref().unwrap_or("").contains(":q!"));
        cmd(&mut app, "q!");
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "ne\ntwo\n", "discarded, not written");

        // `:wq` writes and then closes.
        open(&mut app);
        app.handle_key(key('x')).unwrap();
        cmd(&mut app, "wq");
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "e\ntwo\n");
    }

    /// A message must be readable on a narrow terminal, where the status
    /// chips it shares a row with would otherwise push it off the edge — the
    /// reason `:keys`, "unknown command" and every other answer appeared to
    /// do nothing at all.
    #[test]
    fn a_message_is_never_the_thing_that_falls_off_the_status_line() {
        let d = tempfile::tempdir().unwrap();
        // A long path, so the chips have plenty to say.
        let deep = d.path().join("a-fairly-long-directory-name").join("and-another-one-here");
        std::fs::create_dir_all(&deep).unwrap();
        std::fs::write(deep.join("f.txt"), "x\n").unwrap();
        let mut app = App::new(deep.clone(), deep, en_config()).unwrap();

        app.mode = Mode::Command;
        app.command_buffer = "keys".into();
        app.run_command();
        for w in [60u16, 80, 120] {
            let screen = render(&mut app, w, 24).join("\n");
            assert!(screen.contains("showing every key"), "at {w} columns the answer is off screen");
        }

        // …and the report it turns on is readable too.
        app.handle_key(key('j')).unwrap();
        let screen = render(&mut app, 60, 24).join("\n");
        assert!(screen.contains("key: Char('j')"), "the key report is off screen: {screen}");

        // An unknown command says so rather than appearing to do nothing.
        app.mode = Mode::Command;
        app.command_buffer = "nosuchcommand".into();
        app.run_command();
        let screen = render(&mut app, 60, 24).join("\n");
        assert!(screen.contains("unknown command"), "{screen}");
    }

    /// The reported problems, each pinned so it cannot come back:
    /// `:` opened with `s/` already typed so no word command was reachable;
    /// `]]` disagreed with the screen in the Markdown preview; Space did not
    /// fold; and a message the viewer raised was drawn outside its own border.
    #[test]
    fn the_viewer_command_line_and_outline_answer_where_you_are_looking() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("doc.md"),
            "# One\n\nsome prose that is long enough to wrap once the width gets small\n\n## Two\n\nmore prose\n\n# Three\n\nlast\n",
        )
        .unwrap();
        std::fs::write(d.path().join("plain.txt"), "nothing here\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let open = |app: &mut App, name: &str| {
            app.active_pane_mut().unwrap().cursor =
                app.active_pane().unwrap().entries.iter().position(|e| e.name == name).unwrap();
            app.handle_key(code(KeyCode::F(3))).unwrap();
            let _ = render(app, 100, 30);
        };
        let at = |app: &App| match &app.popup {
            Popup::Viewer { line, .. } => *line,
            other => panic!("not a viewer: {other:?}"),
        };

        open(&mut app, "doc.md");
        // The prompt opens empty, so a word command is typable, and it works
        // in the preview — where `:outline` is most wanted.
        app.handle_key(key(':')).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { sub_input: Some(s), preview: true, .. } if s.is_empty()));
        for c in "outline".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { shape: Some(sh), .. } if !sh.shown));
        app.handle_key(key(':')).unwrap();
        for c in "outline".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        // In the preview, `]]` lands on the line that *shows* the next
        // heading — the rendered document has neither the same count of lines
        // as the source nor the same order.
        let _ = render(&mut app, 100, 30);
        let shown = |app: &mut App| {
            let l = at(app);
            match &app.popup {
                Popup::Viewer { view, .. } => view.lines[l].clone(),
                other => panic!("not a viewer: {other:?}"),
            }
        };
        for _ in 0..2 {
            app.handle_key(key(']')).unwrap();
        }
        assert!(shown(&mut app).contains("Two"), "got {:?}", shown(&mut app));
        for _ in 0..2 {
            app.handle_key(key(']')).unwrap();
        }
        assert!(shown(&mut app).contains("Three"), "got {:?}", shown(&mut app));
        for _ in 0..2 {
            app.handle_key(key('[')).unwrap();
        }
        assert!(shown(&mut app).contains("Two"), "back: got {:?}", shown(&mut app));

        // Space folds, in the source.
        app.toggle_markdown_preview();
        let _ = render(&mut app, 100, 30);
        if let Popup::Viewer { line, .. } = &mut app.popup {
            *line = 2; // inside the first section
        }
        app.handle_key(key(' ')).unwrap();
        let folds = |app: &App| match &app.popup {
            Popup::Viewer { shape, .. } => shape.as_deref().unwrap().folds.iter().copied().collect::<Vec<_>>(),
            other => panic!("not a viewer: {other:?}"),
        };
        assert_eq!(folds(&app), [0], "Space folded the section");
        app.handle_key(key(' ')).unwrap();
        assert!(folds(&app).is_empty(), "and unfolded it");

        // zA is the whole file as one switch, either way round.
        app.handle_key(key('z')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        let all = folds(&app);
        assert!(all.len() >= 2, "everything closed: {all:?}");
        app.handle_key(key('z')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        assert!(folds(&app).is_empty(), "and everything open again");

        // A file with no outline says so, on the viewer's own footer rather
        // than on the status line hiding behind it.
        quit_viewer(&mut app);
        open(&mut app, "plain.txt");
        for _ in 0..2 {
            app.handle_key(key(']')).unwrap();
        }
        let screen = render(&mut app, 100, 30);
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains("outline") || msg.contains("アウトライン"), "{msg:?}");
        // On cian's status line, and only there.
        let last = screen.len() - 1;
        assert!(screen[last].contains(&msg), "on the status line: {:?}", screen[last]);
        assert!(
            !screen.iter().take(last).any(|r| r.contains(&msg)),
            "and not painted over the file:\n{screen:#?}",
        );
    }

    /// Folding: za closes the section the cursor is in, the lines under it
    /// stop being drawn, the cursor comes out with them, and zR/zM work on the
    /// lot. The outline and the folds are the same information read two ways.
    #[test]
    fn folds_hide_a_section_and_take_the_cursor_with_them() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("doc.md"),
            "# One\nunder one\nstill one\n# Two\nunder two\n# Three\nunder three\n",
        )
        .unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "doc.md").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        // Markdown opens in preview; folding belongs to the source. The
        // whitespace marks are not what this is about.
        app.toggle_markdown_preview();
        app.show_ws = false;
        let _ = render(&mut app, 120, 30);

        let folds = |app: &App| match &app.popup {
            Popup::Viewer { shape, .. } => shape.as_deref().unwrap().folds.iter().copied().collect::<Vec<_>>(),
            other => panic!("not a viewer: {other:?}"),
        };
        let at = |app: &App| match &app.popup {
            Popup::Viewer { line, .. } => *line,
            other => panic!("not a viewer: {other:?}"),
        };
        let put = |app: &mut App, l: usize| {
            if let Popup::Viewer { line, .. } = &mut app.popup {
                *line = l;
            }
        };
        let za = |app: &mut App| {
            app.handle_key(key('z')).unwrap();
            app.handle_key(key('a')).unwrap();
        };

        // From inside the first section, za closes the section — not the line.
        put(&mut app, 1);
        za(&mut app);
        assert_eq!(folds(&app), [0]);
        assert_eq!(at(&app), 0, "the cursor came out onto the heading");
        let _ = render(&mut app, 120, 30);

        // The hidden lines are no longer drawn *in the panel*. (The cursor
        // preview under it is a different surface showing the same file, and
        // it does not fold.)
        let screen = |app: &mut App| -> String {
            let rows = render(app, 120, 30);
            let f = app.viewer_frame;
            rows.iter()
                .skip(f.y as usize)
                .take(f.height as usize)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        };
        let shown = screen(&mut app);
        assert!(shown.contains("# One") && shown.contains("# Two"));
        assert!(!shown.contains("under one"), "the folded lines are gone from the panel");

        // Pressing it again opens it.
        za(&mut app);
        assert!(folds(&app).is_empty());
        assert!(screen(&mut app).contains("under one"));

        // Clicking the marker in the gutter is the same as za on that line.
        let g = app.viewer_gutter;
        let b = app.viewer_rect;
        app.handle_mouse(crossterm::event::MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: b.x + g - 2,
            row: b.y + 3, // the "# Two" heading, with everything open
            modifiers: KeyModifiers::NONE,
        });
        assert_eq!(folds(&app), [3], "clicking the marker closed that section");
        let _ = render(&mut app, 120, 30);
        app.handle_key(key('z')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)).unwrap();

        // zM closes everything with something in it, zR opens the lot.
        app.handle_key(key('z')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('M'), KeyModifiers::SHIFT)).unwrap();
        assert_eq!(folds(&app), [0, 3, 5]);
        let shut = screen(&mut app);
        assert!(!shut.contains("under two") && !shut.contains("under three"));
        assert!(shut.contains("# Three"), "every heading is still there to open");
        app.handle_key(key('z')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::SHIFT)).unwrap();
        assert!(folds(&app).is_empty());

        // A file with nothing to fold says so instead of doing nothing.
        quit_viewer(&mut app);
        std::fs::write(d.path().join("flat.txt"), "a\nb\n").unwrap();
        let _ = app.active_file_tabs_mut().map(|t| t.active_mut().reload());
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "flat.txt").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        za(&mut app);
        assert!(app.message.as_deref().unwrap_or("").contains("fold"));
    }

    /// Rectangular editing: Ctrl+V marks a block, then `d` cuts it, and
    /// `I` / `A` / `c` type once and land on every line.
    #[test]
    fn block_selection_can_be_edited_not_just_copied() {
        // Move the cursor to (line, col) without relying on key counts.
        let put = |app: &mut App, l: usize, c: usize| {
            if let Popup::Viewer { line, col, .. } = &mut app.popup {
                *line = l;
                *col = c;
            }
        };
        let block = |app: &mut App, from: (usize, usize), to: (usize, usize)| {
            put(app, from.0, from.1);
            // Ctrl+Q, not Ctrl+V: the latter pastes now, as it does everywhere
        // else, and vim's own synonym is what starts a rectangle.
        app.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL)).unwrap();
            put(app, to.0, to.1);
        };

        // d cuts the rectangle out of every line it covers.
        let (_d, mut app) = viewer_on("abcdef\nabcdef\nabcdef\n");
        block(&mut app, (0, 2), (2, 3));
        app.handle_key(key('d')).unwrap();
        assert_eq!(viewer_lines(&app), ["abef", "abef", "abef"]);
        app.handle_key(key('u')).unwrap();
        assert_eq!(viewer_lines(&app), ["abcdef", "abcdef", "abcdef"], "one undo step");

        // I inserts down the left edge, once typed.
        let (_d2, mut app) = viewer_on("one\ntwo\nthree\n");
        block(&mut app, (0, 0), (2, 0));
        app.handle_key(KeyEvent::new(KeyCode::Char('I'), KeyModifiers::SHIFT)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { block_input: Some(_), .. }), "asks for the text");
        for c in "# ".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["# one", "# two", "# three"]);

        // A appends at the right edge, padding the short lines so it lines up.
        let (_d3, mut app) = viewer_on("ab\nabcd\n");
        block(&mut app, (0, 0), (1, 2));
        app.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT)).unwrap();
        app.handle_key(key('|')).unwrap();
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["ab |", "abc|d"], "padded to the column");

        // Mixed widths: the rectangle is rectangular on screen, so the same
        // columns come out of every line whatever it is made of.
        let (_dw, mut app) = viewer_on("あいうえ\nabcdefgh\nあbcう\n");
        // From the second character of line 1 (columns 2-3) down to the `う`
        // on line 3 (columns 4-5): columns 2..6 on every line between.
        block(&mut app, (0, 1), (2, 3));
        app.handle_key(key('d')).unwrap();
        assert_eq!(viewer_lines(&app), ["あえ", "abgh", "あ"]);

        // c replaces what the rectangle covers.
        let (_d4, mut app) = viewer_on("id=001\nid=002\n");
        block(&mut app, (0, 3), (1, 5));
        app.handle_key(key('c')).unwrap();
        for c in "999".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(viewer_lines(&app), ["id=999", "id=999"]);

        // Esc abandons a prompt without touching the buffer.
        let (_d5, mut app) = viewer_on("keep\nkeep\n");
        block(&mut app, (0, 0), (1, 1));
        app.handle_key(key('c')).unwrap();
        app.handle_key(key('X')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(viewer_lines(&app), ["keep", "keep"], "Esc changes nothing");
    }

    /// The hex editor: `i` on a binary view, hex digits overwrite the byte
    /// under the cursor, Ctrl+S saves — with a `.bak` of the original — and
    /// `u` walks the whole session back.
    #[test]
    fn hex_edit_overwrites_a_byte_and_saves_with_backup() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("blob.bin");
        // NULs make the sniffer call it binary; first byte is 0x41 ('A').
        let mut bytes = vec![0x41u8, 0x42, 0x00, 0x00, 0x43];
        std::fs::write(&file, &bytes).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "blob.bin").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        assert!(
            matches!(&app.popup, Popup::Viewer { view, editable: true, .. }
                if view.kind == cian_core::viewer::ViewKind::Binary),
            "binary views are hex-editable"
        );

        // i → editing; "ff" overwrites byte 0 nibble by nibble.
        app.handle_key(key('i')).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { editing: true, .. }));
        app.handle_key(key('f')).unwrap();
        app.handle_key(key('f')).unwrap();
        match &app.popup {
            Popup::Viewer { view, dirty, .. } => {
                assert_eq!(view.raw_bytes()[0], 0xFF, "byte overwritten");
                assert!(view.lines[0].contains("ff"), "dump line re-rendered");
                assert!(*dirty);
            }
            _ => unreachable!(),
        }

        // u restores the original buffer.
        app.handle_key(key('u')).unwrap();
        match &app.popup {
            Popup::Viewer { view, dirty, .. } => {
                assert_eq!(view.raw_bytes()[0], 0x41, "undo restored the bytes");
                assert!(!dirty, "back to the original → clean");
            }
            _ => unreachable!(),
        }

        // Edit again and save: the file changes, a .bak keeps the original.
        app.handle_key(key('f')).unwrap();
        app.handle_key(key('f')).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL)).unwrap();
        bytes[0] = 0xFF;
        assert_eq!(std::fs::read(&file).unwrap(), bytes, "patched in place, same size");
        assert_eq!(
            std::fs::read(d.path().join("blob.bin.bak")).unwrap()[0],
            0x41,
            "the .bak holds the original"
        );
    }

    /// A BOM'd file wears a badge in the viewer, and `:nobom` strips UTF-8
    /// BOMs while refusing to touch UTF-16 ones.
    #[test]
    fn bom_badge_and_nobom_strip() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("bommed.txt"), b"\xEF\xBB\xBFhello\n").unwrap();
        std::fs::write(d.path().join("plain.txt"), b"hello\n").unwrap();
        // UTF-16LE with BOM: FF FE + "hi" in LE code units.
        std::fs::write(d.path().join("wide.txt"), b"\xFF\xFEh\x00i\x00").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "bommed.txt").unwrap();
        }
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let out = render(&mut app, 100, 30).join("\n");
        assert!(out.contains("UTF-8 BOM"), "the badge shows: {out}");
        app.handle_key(code(KeyCode::Esc)).unwrap();

        // Mark all three and strip.
        {
            let pane = app.active_pane_mut().unwrap();
            for i in 0..pane.entries.len() {
                pane.set_mark_at(i);
            }
        }
        app.start_nobom();
        assert!(matches!(app.popup, Popup::ConfirmNoBom { .. }), "asks first");
        app.handle_key(key('y')).unwrap();
        assert_eq!(std::fs::read(d.path().join("bommed.txt")).unwrap(), b"hello\n", "BOM gone");
        assert_eq!(std::fs::read(d.path().join("plain.txt")).unwrap(), b"hello\n", "untouched");
        assert_eq!(
            std::fs::read(d.path().join("wide.txt")).unwrap(),
            b"\xFF\xFEh\x00i\x00",
            "UTF-16 BOM kept — it is load-bearing"
        );
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains('1') && (msg.contains("UTF-16") || msg.contains("stripped")), "{msg}");
    }

    /// Ops queue instead of refusing: a second start_op while one runs waits
    /// its turn and starts automatically when the runner finishes.
    #[test]
    fn a_second_op_queues_and_runs_after_the_first() {
        use std::sync::atomic::AtomicUsize;
        let (_d, mut app) = app_with(&[]);
        let order = Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
        let (o1, o2) = (Arc::clone(&order), Arc::clone(&order));
        let _ = AtomicUsize::new(0);
        app.start_op("copying", move |_ctl| {
            std::thread::sleep(Duration::from_millis(80));
            o1.lock().unwrap().push(1);
            OpReport { ok: 1, ..Default::default() }
        });
        app.start_op("copying", move |_ctl| {
            o2.lock().unwrap().push(2);
            OpReport { ok: 1, ..Default::default() }
        });
        assert_eq!(app.op_queue.len(), 1, "second op waits in line");
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains("queued") || msg.contains("キュー"), "{msg}");
        // Drain the runner; the queued op must start on its own and finish.
        for _ in 0..600 {
            app.poll_op_job();
            if app.op_job.is_none() && app.op_queue.is_empty() && order.lock().unwrap().len() == 2 {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(*order.lock().unwrap(), vec![1, 2], "ran in order, automatically");
    }

    /// A failed transfer re-runs by itself; local ops never do.
    #[test]
    fn transfers_auto_retry_on_failure() {
        use std::sync::atomic::AtomicUsize;
        let (_d, mut app) = app_with(&[]);
        let runs = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&runs);
        app.start_op("uploading", move |_ctl| {
            let n = r.fetch_add(1, Ordering::SeqCst);
            if n == 0 {
                let mut rep = OpReport::default();
                rep.note_error("connection reset".to_string());
                rep
            } else {
                OpReport { ok: 1, ..Default::default() }
            }
        });
        drain_op_job(&mut app);
        assert_eq!(runs.load(Ordering::SeqCst), 2, "one failure, one successful retry");

        // A local op with the same failure shape runs exactly once.
        let runs = Arc::new(AtomicUsize::new(0));
        let r = Arc::clone(&runs);
        app.start_op("copying", move |_ctl| {
            r.fetch_add(1, Ordering::SeqCst);
            let mut rep = OpReport::default();
            rep.note_error("nope".to_string());
            rep
        });
        drain_op_job(&mut app);
        assert_eq!(runs.load(Ordering::SeqCst), 1, "local failures are not retried");
    }

    /// A worker deaf to its cancel flag can be abandoned: the queue moves on
    /// even though the thread is still wedged.
    #[test]
    fn an_abandoned_op_frees_the_queue() {
        let (_d, mut app) = app_with(&[]);
        // A worker that blocks forever (a stand-in for a wedged syscall).
        let (_hold_tx, hold_rx) = std::sync::mpsc::channel::<()>();
        let hold = std::sync::Mutex::new(Some(hold_rx));
        app.start_op("uploading", move |_ctl| {
            if let Some(rx) = hold.lock().unwrap().take() {
                let _ = rx.recv(); // never resolves; _hold_tx lives in the test
            }
            OpReport::default()
        });
        let ran = Arc::new(std::sync::Mutex::new(false));
        let flag = Arc::clone(&ran);
        app.start_op("copying", move |_ctl| {
            *flag.lock().unwrap() = true;
            OpReport { ok: 1, ..Default::default() }
        });
        assert_eq!(app.op_queue.len(), 1);
        // Ask it to stop (it will not), then abandon.
        app.cancel_op_job();
        assert!(app.op_job.as_ref().unwrap().cancel_requested.is_some());
        app.abandon_op();
        assert!(app.message.clone().unwrap_or_default().contains("abandon")
            || app.message.clone().unwrap_or_default().contains("見捨て"));
        drain_op_job(&mut app);
        assert!(*ran.lock().unwrap(), "the queued op ran despite the wedged one");
    }

    /// `b` tucks the progress popup away and the keyboard works again while
    /// the op runs in the background.
    #[test]
    fn the_progress_popup_can_be_backgrounded() {
        let (_d, mut app) = app_with(&["a.txt", "b.txt"]);
        app.start_op("copying", move |_ctl| {
            std::thread::sleep(Duration::from_millis(150));
            OpReport { ok: 1, ..Default::default() }
        });
        // While the bar shows, ordinary keys are owned by it…
        let before = app.active_pane().unwrap().cursor;
        app.handle_key(key('j')).unwrap();
        assert_eq!(app.active_pane().unwrap().cursor, before, "modal while the bar shows");
        // …`b` backgrounds it, and the same key now moves the cursor.
        app.handle_key(key('b')).unwrap();
        assert!(app.op_bar_hidden);
        app.handle_key(key('j')).unwrap();
        assert_ne!(app.active_pane().unwrap().cursor, before, "keyboard is live again");
        drain_op_job(&mut app);
        assert!(!app.op_bar_hidden, "reset once the queue drains");
    }

    /// Regression: a keypress arriving in the tiny window after a background op
    /// finished but before its result was polled used to be swallowed by the
    /// "Esc only while an op runs" gate — so a second copy right after the first
    /// appeared to need two presses. handle_key must land a finished op first.
    #[test]
    fn a_key_right_after_an_op_finishes_is_not_swallowed() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Start a trivial op and let its worker report Done — but do NOT poll it,
        // exactly as the event loop leaves it while blocked on the next input.
        app.start_op("copying", |_ctl| cian_core::ops::OpReport::default());
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(app.op_job.is_some(), "job still flagged in-flight (unpolled)");

        // The next keypress must be acted on, not eaten: `c` opens the copy
        // confirmation, and the finished op is landed in the same step.
        app.handle_key(key('c')).unwrap();
        assert!(app.op_job.is_none(), "the finished op was landed, not left blocking");
        assert!(
            matches!(app.popup, Popup::ConfirmTransfer { .. }),
            "the copy key was handled: {:?}",
            app.popup
        );
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
        let mut config = en_config();
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
    fn explain_diff_opens_the_chat_with_the_diff() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = en_config();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.ai_ready = Some(true);

        let result = cian_core::diff::diff_lines(
            &["let x = 1;".to_string()],
            &["let x = 2;".to_string()],
        );
        let folded = cian_core::diff::fold(&result.rows, cian_core::diff::CONTEXT);
        app.popup = Popup::Diff {
            left: "a".into(),
            right: "b".into(),
            left_path: "a".into(),
            right_path: "b".into(),
            encoding: cian_core::viewer::TextEncoding::Utf8,
            result,
            folded,
            fold: true,
            scroll: 0,
            find: None,
            find_input: None,
        };
        app.explain_diff();
        match &app.popup {
            Popup::AiChat { log, pending, .. } => {
                assert!(*pending, "the request is in flight");
                assert!(log.iter().any(|m| m.user && m.text == "Explain this diff"));
            }
            other => panic!("expected the chat, got {:?}", other),
        }
        assert!(app.ai_job.is_some(), "a request was fired");
    }

    #[test]
    fn triage_log_reads_the_selected_file_and_opens_chat() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.log"), "INFO ok\nERROR boom\n").unwrap();
        let p = dir.path().to_path_buf();
        let mut config = en_config();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.ai_ready = Some(true);
        if let Some(t) = app.active_file_tabs_mut() {
            let pane = t.active_mut();
            let i = pane.entries.iter().position(|e| e.name == "app.log").unwrap();
            pane.cursor = i;
        }
        app.triage_log();
        match &app.popup {
            Popup::AiChat { log, pending, skin, .. } => {
                assert!(*pending);
                assert!(log.iter().any(|m| m.user && m.text.contains("app.log")), "names the log: {:?}", log);
                // The window is named for the action that opened it, and says
                // the local model is answering — not crmaine.
                assert_eq!(skin.title, "Triage this log");
                assert!(skin.simple, "the local model answers, so the window wears AI - simple");
            }
            other => panic!("expected the chat, got {:?}", other),
        }
    }

    /// A crmaine corpus tool streams *crmaine's* answer into a chat whose typed
    /// follow-ups go to the local model. The window must keep crmaine's name,
    /// so the reply is never credited to AI - simple.
    #[test]
    fn a_crmaine_tool_chat_keeps_crmaines_name() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.start_ai_chat_as(
            ChatMode::Ai,
            ChatSkin { title: "crmaine - Impact".into(), simple: false },
            vec![ChatMsg { user: true, text: "Impact: x".into() }],
            true,
        );
        match &app.popup {
            Popup::AiChat { skin, mode, .. } => {
                assert_eq!(skin.title, "crmaine - Impact");
                assert!(!skin.simple, "crmaine answered it");
                assert_eq!(*mode, ChatMode::Ai, "follow-ups still route to the local model");
            }
            other => panic!("expected the chat, got {:?}", other),
        }
    }

    /// The retrieval trace is read *about* a conversation, so closing it must
    /// put that conversation back rather than dump the user in the file pane.
    #[test]
    fn a_report_raised_over_the_chat_gives_the_chat_back() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_ai_chat(ChatMode::Rag, vec![ChatMsg { user: true, text: "RAG: q".into() }], false);
        let chat = std::mem::replace(&mut app.popup, Popup::None);
        app.popup = Popup::Report {
            title: " what RAG retrieved ".into(),
            lines: (0..40).map(|i| format!("line {i}")).collect(),
            scroll: 0,
            back: Box::new(chat),
        };
        // It scrolls like the manual…
        app.handle_key(key('j')).unwrap();
        app.handle_key(key('d')).unwrap();
        let Popup::Report { scroll, .. } = &app.popup else { panic!("expected the report") };
        assert_eq!(*scroll, 11);
        // …and Esc lands back in the conversation it explains.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        match &app.popup {
            Popup::AiChat { log, .. } => assert!(log[0].text.contains("RAG: q")),
            other => panic!("expected the chat back, got {:?}", other),
        }
    }

    /// The report draws under its own title (not the manual's), and the ranking
    /// reads as a ranking on an 80-column terminal.
    #[test]
    fn the_retrieval_trace_draws_as_a_scrolling_report() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let d = crmaine::DebugSearch {
            hits: vec![
                crmaine::DebugHit {
                    file: "keihi_rule.md".into(),
                    score: 18.4213,
                    chunk_id: Some(12),
                    preview: "出張費は帰着日の翌月10日までに精算してください。".into(),
                },
                crmaine::DebugHit {
                    file: "faq.md".into(),
                    score: 4.6,
                    chunk_id: Some(3),
                    preview: "経費の締めについてよくある質問".into(),
                },
            ],
            tokens: vec!["出張".into(), "費".into(), "精算".into(), "期限".into()],
            token_count: 4,
        };
        let lines = crmaine::debug_report_lines(Lang::Ja, "出張費の精算期限は？", "C:\\idx", false, &d, 72);
        app.popup = Popup::Report {
            title: " RAG が拾った断片 ".into(),
            lines,
            scroll: 0,
            back: Box::new(Popup::None),
        };
        // The test backend pads wide characters with a blank cell, so the
        // assertions stay on the ASCII parts of each line.
        let rows = render(&mut app, 80, 30);
        let screen = rows.join("\n");
        assert!(screen.contains("RAG"), "its own title, not the manual's:\n{screen}");
        assert!(!screen.contains("manual"), "the manual's title must not leak in:\n{screen}");
        assert!(screen.contains("C:\\idx"), "which index answered:\n{screen}");
        assert!(screen.contains("BM25"), "says the scores are raw:\n{screen}");
        assert!(screen.contains("keihi_rule.md #12"), "the top hit:\n{screen}");
        assert!(screen.contains("18.4213") && screen.contains("4.6000"), "the scores:\n{screen}");
        // The top hit fills its bar and the weaker one does not — the shape of
        // the ranking is the thing you read first.
        assert!(screen.contains(&"█".repeat(16)), "top hit's bar:\n{screen}");
        assert!(screen.contains("████············"), "a quarter-height bar:\n{screen}");
        assert!(rows.iter().any(|r| r.contains("Esc")), "says how to leave:\n{screen}");
    }

    /// Opened from the command line there is nothing underneath, so Esc closes.
    #[test]
    fn a_report_with_nothing_behind_it_just_closes() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.popup = Popup::Report {
            title: " r ".into(),
            lines: vec!["x".into()],
            scroll: 0,
            back: Box::new(Popup::None),
        };
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::None));
    }

    /// `:ragdebug` with no argument means "the question I just asked". With no
    /// question behind it, it says how to use it instead of asking for nothing.
    #[test]
    fn ragdebug_with_no_argument_needs_a_previous_question() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.start_debug_search("");
        assert!(app.debug_search_rx.is_none(), "nothing was sent");
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains(":ragdebug"), "explains itself: {msg}");
        // With a remembered question it gets as far as needing crmaine's config
        // (which this app has none of), rather than complaining about the query.
        app.crmaine_last_question = Some("expenses deadline".into());
        app.start_debug_search("");
        assert!(app.message.clone().unwrap_or_default().contains("crmaine"));
    }

    #[test]
    fn pasted_images_ride_along_with_a_chat_turn_only() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().to_path_buf();
        let mut config = en_config();
        config.ai = Some(cian_lua::AiOptions {
            python: "python3".into(),
            auth_mode: "mock".into(),
            ..Default::default()
        });
        let mut app = App::new(p.clone(), p, config).unwrap();
        app.ai_ready = Some(true);

        // A structured purpose parses its reply and has no attachment UI, so it
        // must leave a pending image alone rather than consuming it.
        app.chat_attachments.push(std::path::PathBuf::from("/tmp/shot.png"));
        app.ai_request(AiPurpose::ShellCommand, "sys".into(), "usr".into());
        assert_eq!(app.chat_attachments.len(), 1, "a shell-command request keeps the image");

        // A chat turn takes them, so the same image isn't sent twice.
        app.ai_job = None;
        app.ai_request(AiPurpose::Chat, "sys".into(), "usr".into());
        assert!(app.chat_attachments.is_empty(), "the chat turn took the image");

        // Starting a fresh conversation drops anything pasted for the old one.
        app.chat_attachments.push(std::path::PathBuf::from("/tmp/shot.png"));
        app.start_ai_chat(ChatMode::Ai, Vec::new(), false);
        assert!(app.chat_attachments.is_empty(), "a new chat starts empty");
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
            mode: ChatMode::Ai,
            skin: ChatSkin::of(ChatMode::Ai),
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

        let mut app = App::new(dir.clone(), dir.clone(), en_config()).unwrap();
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
        let mut app = App::new(dir.clone(), dir, en_config()).unwrap();
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

        let mut app = App::new(dir.clone(), dir.clone(), en_config()).unwrap();
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
        let mut config = en_config();
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
            by_ai: true,
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
        let mut config = en_config();
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

        for k in [':', 's', 'u', 'm', 'm', 'a', 'r', 'y'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
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

        let mut config = en_config();
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
        let mut config = en_config();
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
                ("x", "delete".into()), // bind a new key to an action
                ("d", "none".into()),   // and turn the default off
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
        // reload_config re-applies the theme into the process-wide global, so it
        // must not race the theme tests.
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (_d, mut app) = app_with(&["a.rs"]);
        // No user binding yet: `x` is not delete.
        assert!(!app.keymap.contains_key(&('x', KeyModifiers::NONE)));
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
        assert_eq!(app.keymap.get(&('x', KeyModifiers::NONE)), Some(&Action::Delete), "reload bound x live");
    }

    #[test]
    fn a_newly_named_action_is_bindable() {
        // `sort` had no bindable name before; confirm it now resolves and works.
        assert_eq!(action_from_name("sort"), Some(Action::Sort));
        let (_d, mut app) = app_with_keymaps(&["a.rs"], vec![("S", "sort".into())]);
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
    /// Render, and hand back the cell under the viewer's cursor with its
    /// colours — the only way to catch "the character is there but painted the
    /// same shade as the block behind it".
    fn cursor_cell(app: &mut App, w: u16, h: u16) -> (String, Color, Color) {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        let (line, col) = match &app.popup {
            Popup::Viewer { line, col, .. } => (*line, *col),
            other => panic!("not a viewer: {other:?}"),
        };
        let b = app.viewer_rect;
        let x = b.x + app.viewer_gutter + col as u16;
        let y = b.y + line as u16;
        let c = &buf[(x, y)];
        (c.symbol().to_string(), c.fg, c.bg)
    }

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

    /// A long cwd in the pane title is shortened from the middle: the tail is
    /// what identifies a path, and clipping at the border lost exactly that.
    #[test]
    fn title_keeps_the_tail_of_a_long_path() {
        let d = tempfile::tempdir().unwrap();
        let mut deep = d.path().to_path_buf();
        for part in ["very-long-segment-one", "very-long-segment-two", "very-long-segment-three", "destination"] {
            deep.push(part);
        }
        std::fs::create_dir_all(&deep).unwrap();
        let mut app = App::new(deep.clone(), deep, en_config()).unwrap();
        let out = render(&mut app, 80, 20);
        let title = &out[0];
        assert!(title.contains('…'), "long path was middle-truncated: {title}");
        assert!(title.contains("destination"), "the identifying tail survives: {title}");
    }

    /// The visible-window optimization must render the same rows ratatui would:
    /// the cursor stays on screen and far-away rows are excluded.
    #[test]
    fn big_directory_windows_to_the_cursor() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..500 {
            std::fs::write(d.path().join(format!("file_{i:04}.rs")), b"x").unwrap();
        }
        // The right pane opens an empty dir so only the left column shows file_*.
        let empty = tempfile::tempdir().unwrap();
        let config = en_config();
        let mut app = App::new(d.path().to_path_buf(), empty.path().to_path_buf(), config).unwrap();
        app.focus(FocusedPane::Left);
        let idx = app
            .active_pane()
            .unwrap()
            .entries
            .iter()
            .position(|e| e.name == "file_0400.rs")
            .unwrap();
        app.active_pane_mut().unwrap().cursor = idx;
        let joined = render(&mut app, 120, 50).join("\n");
        assert!(joined.contains("file_0400.rs"), "the cursor row must be visible");
        assert!(joined.contains("file_0399.rs"), "its neighbour is on screen too");
        assert!(!joined.contains("file_0000.rs"), "rows far above are windowed out");
        assert!(!joined.contains("file_0499.rs"), "rows far below are windowed out");
    }

    /// Micro-bench (run with `--ignored --nocapture`): time N renders of a pane
    /// holding a large directory, cursor parked deep in the list. Prints the
    /// per-frame cost so the windowing optimization can be measured.
    #[test]
    #[ignore]
    fn bench_render_big_directory() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..5000 {
            std::fs::write(d.path().join(format!("file_{i:05}.rs")), b"x").unwrap();
        }
        let mut config = en_config();
        config.options.home = Some(d.path().display().to_string());
        let mut app = App::new(d.path().to_path_buf(), d.path().to_path_buf(), config).unwrap();
        // Park the cursor deep so the visible window is far from the top.
        app.active_pane_mut().unwrap().cursor = 4000;
        let mut terminal = Terminal::new(TestBackend::new(120, 50)).unwrap();
        // Warm up.
        terminal.draw(|f| draw(f, &mut app)).unwrap();
        let n = 400;
        let start = std::time::Instant::now();
        for _ in 0..n {
            terminal.draw(|f| draw(f, &mut app)).unwrap();
        }
        let per = start.elapsed() / n;
        println!("bench_render_big_directory: {per:?}/frame over {n} frames (5000 entries)");
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

    /// Dragging inside a pane only moves the cursor. It used to rubber-band
    /// the marks, which fought the deliberate marking Space and visual mode
    /// already do — and turned every slightly-shaky click into a reshuffle.
    #[test]
    fn dragging_inside_a_pane_only_moves_the_cursor() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt", "b.txt", "c.txt"], &[]);
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, left.y + 3));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), left.x + 3, left.y + 5));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), left.x + 3, left.y + 5));
        assert_eq!(app.active_pane().unwrap().mark_count(), 0, "a drag marks nothing");
        assert!(matches!(app.popup, Popup::None), "and starts no transfer");
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
        keymap.insert(('x', KeyModifiers::NONE), Action::Delete);
        keymap.insert(('g', KeyModifiers::ALT), Action::GrepRecursive);
        let text = manual_lines(&keymap, Lang::En).join("\n");
        assert!(text.contains("d, x"), "user-bound key missing from manual:\n{}", text);
        assert!(text.contains("Alt+g"), "a modified binding is named in full:\n{}", text);
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
            en_config(),
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
    fn clipboard_keys_follow_windows_and_c_is_copy_to_other_pane() {
        let ctrl = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        let (_l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        app.focus(FocusedPane::Left);
        app.active_pane_mut().unwrap().cursor = 1; // a.txt (0 is `..`)

        // Ctrl+C → Windows-style file-clipboard copy.
        app.handle_key(ctrl('c')).unwrap();
        assert!(matches!(app.file_clip, Some(FileClipboard { op: ClipOp::Copy, .. })), "Ctrl+C copies");

        // Ctrl+X → cut.
        app.handle_key(ctrl('x')).unwrap();
        assert!(matches!(app.file_clip, Some(FileClipboard { op: ClipOp::Cut, .. })), "Ctrl+X cuts");

        // `c` is now "copy to the other pane" (a transfer), not the clipboard.
        app.file_clip = None;
        app.handle_key(key('c')).unwrap();
        assert!(matches!(app.popup, Popup::ConfirmTransfer { op: PendingOp::Copy, .. }), "c copies to the other pane");
        assert!(app.file_clip.is_none(), "c does not touch the file clipboard");
    }

    #[test]
    fn y_and_ctrl_v_both_paste() {
        let ctrl_v = KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL);
        for trigger in [key('y'), ctrl_v] {
            let (_l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
            app.focus(FocusedPane::Right); // paste into the (empty) right pane
            // Nothing on the clipboard yet → paste reports it (proves it routed
            // to paste_clip rather than a copy/transfer).
            app.handle_key(trigger).unwrap();
            assert_eq!(app.message.as_deref(), Some("clipboard has no files"), "paste ran for {trigger:?}");
        }
    }

    #[test]
    fn paste_is_always_offered() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        let _ = render(&mut app, 100, 40);

        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::PasteHere), "offered with nothing held");
        app.popup = Popup::None;

        app.clip_targets(ClipOp::Copy);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::PasteHere), "and still offered once held");
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
    fn file_menu_offers_the_os_actions_group() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::OsMenu), "file menu offers the OS group");
        assert!(MenuItem::OsMenu.is_group());
        let kids = app.submenu_children(MenuItem::OsMenu).expect("group has children");
        assert_eq!(
            kids,
            vec![
                MenuItem::OpenDefault,
                MenuItem::OpenWithOs,
                MenuItem::RevealInOs,
                MenuItem::PropertiesOs,
                MenuItem::Back,
            ]
        );
        // The reveal/properties labels adapt to the host OS and are never blank.
        for it in [MenuItem::RevealInOs, MenuItem::PropertiesOs, MenuItem::OpenWithOs] {
            assert!(!it.label(Lang::En).is_empty());
            assert!(!it.label(Lang::Ja).is_empty());
        }
    }

    #[test]
    fn the_os_group_is_absent_from_the_shell_menu() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(!items.contains(&MenuItem::OsMenu), "the OS group is file-pane only");
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
    fn file_menu_zone_order_is_consistent() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Left);
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        let pos = |m: MenuItem| items.iter().position(|i| *i == m).expect("item present");
        // Shortcuts joins the launcher cluster at the top, above the file ops.
        assert!(pos(MenuItem::Shortcuts) < pos(MenuItem::Copy));
        // Copy / paste cluster, then the connect block, then appearance.
        assert!(pos(MenuItem::Copy) < pos(MenuItem::PasteHere));
        assert!(pos(MenuItem::PasteHere) < pos(MenuItem::Ssh));
        assert!(pos(MenuItem::Ssh) < pos(MenuItem::Background));
        // Appearance block in the shared order: background, theme, language.
        assert!(pos(MenuItem::Background) < pos(MenuItem::ThemePick));
        assert!(pos(MenuItem::ThemePick) < pos(MenuItem::Lang));
        // OS group stays last before the footer.
        assert!(pos(MenuItem::Lang) < pos(MenuItem::OsMenu));
        assert!(pos(MenuItem::OsMenu) < pos(MenuItem::Quit));
    }

    #[test]
    fn shell_can_reach_the_command_line() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.focus(FocusedPane::Shell);
        // Ctrl+Enter from the shell opens cian's `:` command line (typing `:`
        // there would just go to the terminal).
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::CONTROL)).unwrap();
        assert_eq!(app.mode, Mode::Command);
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.mode, Mode::Normal);

        // The shell menu also offers it, for terminals that can't report Ctrl+Enter.
        app.open_context_menu(5, 5);
        let Popup::ContextMenu { items, .. } = &app.popup else { panic!("no menu") };
        assert!(items.contains(&MenuItem::CommandInput), "shell menu offers Command…");
    }

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
        // No SSH hosts configured here, so Transfer ▸ is omitted. The zones:
        // pane action (Paste), shell groups (Session / Window), the shared
        // connect + appearance blocks, then the footer.
        assert_eq!(
            core,
            vec![
                MenuItem::CommandInput,
                MenuItem::Paste,
                MenuItem::SessionMenu,
                MenuItem::WindowMenu,
                MenuItem::Ssh,
                MenuItem::RemotePane,
                MenuItem::Background,
                MenuItem::ThemePick,
                MenuItem::Lang,
                MenuItem::Quit,
                MenuItem::Manual
            ]
        );
    }

    #[test]
    fn theme_picker_previews_live_and_esc_restores() {
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (_d, mut app) = app_with(&["a.txt"]);
        let before = crate::theme::theme();
        app.start_theme_picker();
        assert!(matches!(app.popup, Popup::ThemePicker { .. }));
        // Moving the cursor applies the previewed preset to the live global.
        app.handle_key(code(KeyCode::Char('j'))).unwrap();
        assert_ne!(crate::theme::theme(), before, "preview should swap the theme");
        // Esc restores whatever was active on entry.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(crate::theme::theme(), before, "cancel restores the original");
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn pane_theme_override_is_scoped_and_clearable() {
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (_d, mut app) = app_with(&["a.txt"]);
        let app_theme = crate::theme::theme();
        // Right pane (side 1) gallery: preview leaves the global app theme alone.
        app.start_pane_theme_picker(1);
        app.handle_key(code(KeyCode::Char('j'))).unwrap();
        assert_eq!(crate::theme::theme(), app_theme, "pane preview must not touch the global");
        assert!(app.pane_theme[1].is_some(), "the right pane gained an override");
        assert!(app.pane_theme[0].is_none(), "the left pane is untouched");
        // Keep it.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        let kept = app.pane_theme[1].clone();
        assert!(kept.is_some());
        // Reopen and clear with `x` → follows the app again.
        app.start_pane_theme_picker(1);
        app.handle_key(code(KeyCode::Char('x'))).unwrap();
        assert!(app.pane_theme[1].is_none(), "x clears the override");
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn pane_theme_picker_esc_restores_previous_override() {
        let (_d, mut app) = app_with(&["a.txt"]);
        app.pane_theme[0] = Some("nord".to_string());
        app.start_pane_theme_picker(0);
        app.handle_key(code(KeyCode::Char('j'))).unwrap();
        app.handle_key(code(KeyCode::Char('j'))).unwrap();
        // Cancel → the pane's prior override comes back.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(app.pane_theme[0].as_deref(), Some("nord"));
    }

    #[test]
    fn saved_theme_round_trips_through_the_state_format() {
        use crate::state_get_in;
        // The exact shape save_theme_pref writes (comment header + quoted value).
        let body = "# cian runtime state — managed by cian (see :where)\ntheme = \"dracula\"\n";
        assert_eq!(state_get_in(body, "theme").as_deref(), Some("dracula"));
        // Tolerant of spacing and missing quotes; comments and blanks ignored.
        assert_eq!(state_get_in("theme=nord", "theme").as_deref(), Some("nord"));
        assert_eq!(state_get_in("  theme   =   \"one-dark\"  ", "theme").as_deref(), Some("one-dark"));
        assert_eq!(state_get_in("# theme = \"ignored\"\n", "theme").as_deref(), None);
        assert_eq!(state_get_in("theme = \"\"\n", "theme").as_deref(), None);
        assert_eq!(state_get_in("nothing here", "theme").as_deref(), None);
    }

    #[test]
    fn surface_follows_light_and_dark_themes() {
        use crate::theme::{set_theme, surface, ResolvedTheme};
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // The dark default paints no base, so surfaces fall back to the (dark)
        // popup background — the menu / viewer stay dark.
        set_theme(ResolvedTheme::DARK);
        assert_eq!(surface(), ResolvedTheme::DARK.popup_bg);
        // A light theme has a light base_bg, so the menu / viewer go light and
        // their readable_on text turns dark.
        set_theme(ResolvedTheme::GITHUB_LIGHT);
        assert_eq!(surface(), ResolvedTheme::GITHUB_LIGHT.base_bg.unwrap());
        assert_eq!(crate::render::readable_on(surface()), Color::Rgb(30, 32, 40), "dark text on a light menu");
        set_theme(ResolvedTheme::DARK);
    }

    /// The crosshair has to step *away* from the surface, not always darker:
    /// a fixed dark tint turned a light theme's cursor line into a black bar
    /// with the text still on it.
    #[test]
    fn the_crosshair_shade_follows_the_theme() {
        use crate::render::shade_of_surface;
        use crate::theme::{set_theme, surface, ResolvedTheme};
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lum = |c: Color| match c {
            Color::Rgb(r, g, b) => 299 * r as i32 + 587 * g as i32 + 114 * b as i32,
            _ => 0,
        };

        set_theme(ResolvedTheme::DARK);
        assert!(lum(shade_of_surface(40)) > lum(surface()), "lighter on a dark theme");
        set_theme(ResolvedTheme::GITHUB_LIGHT);
        assert!(lum(shade_of_surface(40)) < lum(surface()), "darker on a light one");
        // The cursor cell is the page's own two colours swapped, so it stays
        // legible whatever the line under it is tinted to — the one thing that
        // must never wash out.
        for t in [ResolvedTheme::DARK, ResolvedTheme::GITHUB_LIGHT] {
            set_theme(t);
            let (fg, bg) = (surface(), crate::render::readable_on(surface()));
            assert!(
                (lum(fg) - lum(bg)).abs() > 100_000,
                "the cursor stands off its own background",
            );
            assert!(
                (lum(bg) - lum(shade_of_surface(28))).abs() > 50_000,
                "…and off the tint of the line it sits on",
            );
        }
        set_theme(ResolvedTheme::DARK);
    }

    /// cian's chrome — the status bar, the pane tabs and column headings, the
    /// viewer's badge / tab strip / hint bar / prompt — was written as fixed
    /// colours chosen against a dark page: black on a chip, white on the
    /// status bar, the theme's border grey on a column heading. On a light
    /// theme those are their own background with words in it (the status bar
    /// scored 1.06:1 — white on near-white). Every letter of the chrome has
    /// to stand off what it is painted on, whatever the theme.
    #[test]
    fn the_chrome_reads_on_every_theme() {
        use crate::theme::{set_theme, ResolvedTheme};
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());

        for t in
            [ResolvedTheme::DARK, ResolvedTheme::GITHUB_LIGHT, ResolvedTheme::SOLARIZED_LIGHT]
        {
            set_theme(t);
            // stage 0 = the panes and the bars under them; 1 = the viewer over
            // them (two files, so the tab strip is drawn); 2 = with `:` open.
            for stage in 0..3 {
                let d = tempfile::tempdir().unwrap();
                std::fs::write(d.path().join("a.txt"), "AAA\n").unwrap();
                std::fs::write(d.path().join("b.txt"), "BBB\n").unwrap();
                let p = d.path().to_path_buf();
                let mut app = App::new(p.clone(), p, en_config()).unwrap();
                for n in ["a.txt", "b.txt"] {
                    let path = app
                        .active_pane()
                        .unwrap()
                        .entries
                        .iter()
                        .find(|e| e.name == n)
                        .unwrap()
                        .path
                        .clone();
                    app.active_pane_mut().unwrap().marks.insert(path);
                }
                if stage >= 1 {
                    app.handle_key(code(KeyCode::F(3))).unwrap();
                }
                if stage == 2 {
                    app.handle_key(key(':')).unwrap();
                }
                let buf = render_buf(&mut app, 100, 30);
                let h = buf.area.height;
                let row_text = |y: u16| {
                    (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect::<String>()
                };

                // The bars along the bottom and the pane chrome at the top run
                // the full width; the viewer's own chrome is checked only
                // within the viewer, since the panes show at the edges of the
                // same rows and are drawn by someone else.
                // Rows 0-2 are the panes' own chrome (tabs, column headings)
                // only while the viewer is not covering them; the bars along
                // the bottom are there in every stage.
                let full: Vec<u16> = if stage == 0 {
                    vec![0, 1, 2, h - 2, h - 1]
                } else {
                    vec![h - 2, h - 1]
                };
                let viewer_rows: Vec<u16> = (0..h)
                    .filter(|y| {
                        let r = row_text(*y);
                        r.contains("READ") || r.contains("COMMAND") || r.contains("1 a.txt")
                            || r.contains("search") || r.contains("s/old/new/")
                    })
                    .collect();
                let (vx0, vx1) =
                    (app.viewer_rect.x, app.viewer_rect.x + app.viewer_rect.width);
                let mut checked = 0;
                for y in 0..h {
                    let (x0, x1) = if full.contains(&y) {
                        (0, buf.area.width)
                    } else if viewer_rows.contains(&y) {
                        (vx0, vx1)
                    } else {
                        continue;
                    };
                    for x in x0..x1 {
                        let c = &buf[(x, y)];
                        // Letters and digits only: borders, separators and
                        // glyphs are decoration, and are meant to be quieter.
                        if !c.symbol().chars().all(char::is_alphanumeric)
                            || c.symbol().trim().is_empty()
                        {
                            continue;
                        }
                        // A Reset fg/bg is the terminal's own colour and
                        // cannot be measured from here.
                        if matches!(c.fg, Color::Reset) || matches!(c.bg, Color::Reset) {
                            continue;
                        }
                        checked += 1;
                        // WCAG's own measure rather than a luminance
                        // difference: the pairs that failed here score
                        // respectably by luminance and are still unreadable.
                        // 4.0 is a shade under the 4.5 wanted for body text,
                        // which is fair for bold chrome a few characters long.
                        let cr = crate::render::contrast_ratio(c.fg, c.bg);
                        assert!(
                            cr >= 4.0,
                            "{:?} stage{stage}: {:?} at ({x},{y}) — {:?} on {:?} is {cr:.2}:1",
                            t.accent,
                            c.symbol(),
                            c.fg,
                            c.bg,
                        );
                    }
                }
                assert!(checked > 20, "found almost no chrome to check ({checked} cells)");
            }
        }
        set_theme(ResolvedTheme::DARK);
    }

    /// Syntax colours were all chosen against a dark page. On a light one the
    /// pale ones — plain text most of all — were two shades from the paper,
    /// and the cursor line's tint underneath finished the job.
    #[test]
    fn syntax_colours_stay_legible_on_a_light_theme() {
        use crate::render::{hl_style_for, readable_on};
        use crate::theme::{set_theme, surface, ResolvedTheme};
        use cian_core::highlight::Category as C;
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lum = |c: Color| match c {
            Color::Rgb(r, g, b) => (299 * r as i32 + 587 * g as i32 + 114 * b as i32) / 1000,
            _ => 0,
        };

        for t in [ResolvedTheme::DARK, ResolvedTheme::GITHUB_LIGHT] {
            set_theme(t);
            let page = lum(surface());
            for cat in [C::Plain, C::Keyword, C::Type, C::Str, C::Comment, C::Number, C::Tag, C::Attr] {
                let fg = hl_style_for(cat);
                assert!(
                    (lum(fg) - page).abs() >= 80,
                    "{cat:?} is only {} from the page",
                    (lum(fg) - page).abs(),
                );
            }
            // Plain text is not a syntax colour at all — it is whatever reads
            // on this page.
            assert_eq!(hl_style_for(C::Plain), readable_on(surface()));
        }
        set_theme(ResolvedTheme::DARK);
    }

    /// The cursor cell has to be readable on every theme. It was built as the
    /// page's two colours swapped and then had the body colour put back on top
    /// of it, which made the character the same near-black as its own block —
    /// a solid square with the letter painted out inside.
    #[test]
    fn the_cursor_cell_never_paints_out_its_own_character() {
        use crate::theme::{set_theme, ResolvedTheme};
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let lum = |c: Color| match c {
            Color::Rgb(r, g, b) => (299 * r as i32 + 587 * g as i32 + 114 * b as i32) / 1000,
            _ => 0,
        };
        for t in [ResolvedTheme::DARK, ResolvedTheme::GITHUB_LIGHT] {
            set_theme(t);
            let (_d, mut app) = viewer_on("    println!();\n");
            app.show_ws = false;
            if let Popup::Viewer { col, .. } = &mut app.popup {
                *col = 4; // the `p`, not the indent
            }
            let (sym, fg, bg) = cursor_cell(&mut app, 100, 20);
            assert_eq!(sym, "p", "the cursor is on the character we think");
            assert!(
                (lum(fg) - lum(bg)).abs() > 90,
                "the character reads against its own block: {fg:?} on {bg:?}",
            );
        }
        set_theme(ResolvedTheme::DARK);
    }

    #[test]
    fn theme_set_by_name_sticks() {
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // set_theme_by_name persists to state.toml; point the config dir at a
        // tempdir so the test never clobbers the real ~/.config/cian.
        let cfg = tempfile::tempdir().unwrap();
        std::env::set_var("CIAN_CONFIG_DIR", cfg.path());
        let (_d, mut app) = app_with(&["a.txt"]);
        app.set_theme_by_name("dracula");
        assert_eq!(crate::theme::theme(), crate::theme::ResolvedTheme::DRACULA);
        assert_eq!(app.theme_name, "dracula");
        // Restore the default so other tests reading the global are unaffected.
        crate::theme::set_theme(crate::theme::ResolvedTheme::DARK);
        std::env::remove_var("CIAN_CONFIG_DIR");
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
        assert_eq!(p.locals.len(), 1);
    }

    /// Multi-file upload asks for each file's chmod in turn: a valid mode
    /// advances, an invalid one re-asks the same file (without losing the
    /// upload), and a blank keeps the server default.
    #[test]
    fn upload_chmod_is_per_file_and_reprompts_on_error() {
        let (_d, mut app) = app_with(&["a.txt"]);
        // Stand in a pending 3-file upload directly (skips the network browser).
        app.scp_pending = Some(crate::ScpPending {
            target: cian_scp::Target {
                host: "h".into(),
                port: 22,
                user: "u".into(),
                password: "p".into(),
            },
            label: "u@h".into(),
            locals: vec![
                std::path::PathBuf::from("/tmp/one.txt"),
                std::path::PathBuf::from("/tmp/two.txt"),
                std::path::PathBuf::from("/tmp/three.txt"),
            ],
        });
        app.scp_upload_modes.clear();

        let set_buf = |app: &mut App, s: &str| {
            if let Popup::TextInput { buffer, cursor, .. } = &mut app.popup {
                *buffer = s.to_string();
                *cursor = buffer.chars().count();
            } else {
                panic!("expected a chmod TextInput, got {:?}", app.popup);
            }
        };

        app.prompt_upload_chmod("/dest".into(), 0);
        match &app.popup {
            Popup::TextInput { kind: InputKind::UploadChmod { idx: 0, .. }, title, .. } => {
                assert!(title.contains("1/3"), "shows file 1 of 3: {title}");
            }
            other => panic!("expected file-1 chmod prompt, got {:?}", other),
        }

        // File 1: a valid mode advances to file 2.
        set_buf(&mut app, "755");
        app.finish_text_input().unwrap();
        assert_eq!(app.scp_upload_modes, vec![Some(0o755)]);
        assert!(matches!(app.popup, Popup::TextInput { kind: InputKind::UploadChmod { idx: 1, .. }, .. }));

        // File 2: an invalid mode re-asks the same file and keeps the pending upload.
        set_buf(&mut app, "zzz");
        app.finish_text_input().unwrap();
        assert!(app.message.as_deref().unwrap_or("").contains("invalid chmod"));
        assert_eq!(app.scp_upload_modes, vec![Some(0o755)], "no mode recorded for the bad entry");
        assert!(matches!(app.popup, Popup::TextInput { kind: InputKind::UploadChmod { idx: 1, .. }, .. }));
        assert!(app.scp_pending.is_some(), "the upload is not dropped on a bad mode");

        // File 2 again: blank keeps the server default and advances to file 3.
        set_buf(&mut app, "");
        app.finish_text_input().unwrap();
        assert_eq!(app.scp_upload_modes, vec![Some(0o755), None]);
        assert!(matches!(app.popup, Popup::TextInput { kind: InputKind::UploadChmod { idx: 2, .. }, .. }));
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
        let mut config = en_config();
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
        let mut config = en_config();
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
                let row = rect.y + 2 + off;
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
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y + 2));
        assert_eq!(app.active_pane().unwrap().cursor, 0);
        app.popup = Popup::None;

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), rect.x + 3, rect.y + 3));
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

    /// The exact split sequence a 2×2 grid macro issues (Cgrid4: pane2 splits
    /// pane1 right, pane3 splits pane1 down, pane4 splits pane2 down) must build
    /// a real grid — a left/right split whose two columns are each split into
    /// rows — not four side-by-side columns.
    #[test]
    fn macro_grid_from_targets_build_a_2x2() {
        let dir = tempfile::tempdir().unwrap();
        let sh = cian_pty::default_shell();
        let mk = || cian_pty::PtySession::new(dir.path(), &sh, 24, 80).unwrap();

        let mut tab = ShellTab::new(mk());
        let mut leaf_ids = vec![tab.active]; // pane 1
        tab.split_from(leaf_ids[0], SplitDir::LeftRight, 50, mk()); // pane2 from=1 right
        leaf_ids.push(tab.active);
        tab.split_from(leaf_ids[0], SplitDir::TopBottom, 50, mk()); // pane3 from=1 down
        leaf_ids.push(tab.active);
        tab.split_from(leaf_ids[1], SplitDir::TopBottom, 50, mk()); // pane4 from=2 down
        leaf_ids.push(tab.active);

        assert_eq!(tab.leaves().len(), 4, "four panes");
        let Some(Node::Split { dir, first, second, .. }) = tab.nodes.get(tab.root).and_then(|n| n.as_ref())
        else {
            panic!("root should be a split");
        };
        assert_eq!(*dir, SplitDir::LeftRight, "the outer split makes two columns");
        for (label, child) in [("left", *first), ("right", *second)] {
            match tab.nodes.get(child).and_then(|n| n.as_ref()) {
                Some(Node::Split { dir, .. }) => {
                    assert_eq!(*dir, SplitDir::TopBottom, "{label} column is split into rows");
                }
                _ => panic!("{label} column should be a top/bottom split"),
            }
        }
    }

    /// `new_tab_running` (behind "Edit in new tab") must actually open a tab; the
    /// command is delivered when the tab's shell lands.
    #[test]
    fn new_tab_running_opens_a_new_tab() {
        let (_d, mut app) = app_with(&["a.txt"]);
        let cwd = app.active_pane().unwrap().cwd.clone();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        app.shell.ensure(&cwd);
        while app.shell.count() == 0 {
            app.shell.poll_pending();
            assert!(std::time::Instant::now() < deadline, "first tab never spawned");
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        let before = app.shell.count();
        app.shell.new_tab_running(&cwd, "echo hi".into());
        while app.shell.count() == before {
            app.shell.poll_pending();
            assert!(std::time::Instant::now() < deadline, "new tab never opened");
            std::thread::sleep(std::time::Duration::from_millis(3));
        }
        assert_eq!(app.shell.count(), before + 1, "a fresh tab opened for the editor");
    }

    /// End-to-end: drive a 2×2 grid macro through the real tick loop (async PTY
    /// spawns and all) and confirm the *built* layout is a grid, not four
    /// columns — the actual #1 report. This exercises the leaf-id bookkeeping
    /// that the synchronous tree test cannot.
    #[test]
    fn macro_builds_a_real_2x2_grid_end_to_end() {
        use cian_lua::macros::{Macro, PaneStep, Split};
        let pane = |from: Option<usize>, dir: Split| PaneStep { dir, from, ..Default::default() };
        let m = Macro {
            name: "grid".into(),
            sync: false,
            zoom: false,
            script: None,
            panes: vec![
                pane(None, Split::Right),    // pane 1 (the shell you're on)
                pane(Some(1), Split::Right), // pane 2: split pane 1 → right
                pane(Some(1), Split::Down),  // pane 3: split pane 1 → down
                pane(Some(2), Split::Down),  // pane 4: split pane 2 → down
            ],
        };

        let (_d, mut app) = app_with(&["a.txt"]);
        app.begin_macro(&m);
        let start = std::time::Instant::now();
        while app.macro_run.is_some() {
            app.shell.poll_pending();
            app.tick_macro();
            assert!(start.elapsed() < std::time::Duration::from_secs(20), "macro did not finish");
            std::thread::sleep(std::time::Duration::from_millis(3));
        }

        let tab = app.shell.active_tab().expect("a shell tab");
        assert_eq!(tab.leaves().len(), 4, "the macro built four panes");
        let Some(Node::Split { dir, first, second, .. }) = tab.nodes.get(tab.root).and_then(|n| n.as_ref())
        else {
            panic!("root should be a split");
        };
        assert_eq!(*dir, SplitDir::LeftRight, "two columns, not four");
        for (label, child) in [("left", *first), ("right", *second)] {
            match tab.nodes.get(child).and_then(|n| n.as_ref()) {
                Some(Node::Split { dir, .. }) => {
                    assert_eq!(*dir, SplitDir::TopBottom, "{label} column split into two rows");
                }
                _ => panic!("{label} column should be a top/bottom split (got a bare pane)"),
            }
        }
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
        // The theme decides whether an untouched cell is Reset at all (a light
        // theme paints the whole surface), so this holds the theme still while
        // it looks — otherwise a theme test running beside it decides the
        // answer.
        use crate::theme::{set_theme, ResolvedTheme};
        let _g = THEME_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_theme(ResolvedTheme::DARK);
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
        let mut config = en_config();
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
        let mut config = en_config();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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

    /// The same lesson, for the other half of a column. `truncate` counted
    /// characters while `pad_to` counted cells, so a Japanese filename in the
    /// file pane was cut to 28 characters — 56 columns — and shoved the size
    /// and date columns off the right edge.
    #[test]
    fn truncation_counts_cells_too_so_columns_line_up() {
        assert_eq!(truncate("report.txt", 20), "report.txt", "shorter than the budget: untouched");
        // A wide character cannot always land exactly on the budget (five of
        // them plus the ellipsis is 11 of 12), so the guarantee is "no wider" —
        // which is why `fit` pads afterwards rather than trusting the cut.
        assert!(width(&truncate("第四四半期の報告書.txt", 12)) <= 12, "never wider than asked");
        assert!(truncate("第四四半期の報告書.txt", 12).ends_with('…'), "marked as cut");
        // A budget that cannot fit even one wide character still holds the line.
        assert_eq!(truncate("日本語", 1), "…");

        // What the file pane actually does: every name occupies the same width,
        // whatever script it is written in.
        for name in ["report_final.txt", "第四四半期の報告書.txt", "設計メモ.md", "a"] {
            assert_eq!(width(&fit(name, 12)), 12, "column width for {name}");
        }
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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

    /// Grep, then replace across everything it matched: the preview must show
    /// what each line becomes, Space must be able to spare one, and nothing may
    /// reach the disk until Enter.
    #[test]
    fn a_grep_can_be_replaced_across_every_file_it_matched() {
        let d = tempfile::tempdir().unwrap();
        let a = d.path().join("a.txt");
        let b = d.path().join("b.log");
        // CRLF and a tab, so the write path is held to the file it was given.
        std::fs::write(&a, "ORA-600 first\r\nfine\r\nORA-600 third\r\n").unwrap();
        std::fs::write(&b, b"col\tORA-600\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        app.start_find("ORA-600", cian_core::search::Mode::Content);
        drain_find(&mut app);
        assert!(matches!(&app.popup, Popup::FindResults { hits, .. } if hits.len() == 3));

        // `r` asks only for the replacement — the pattern is the one on screen.
        app.handle_key(key('r')).unwrap();
        let Popup::TextInput { kind, .. } = &app.popup else { panic!("no prompt") };
        assert!(matches!(kind, InputKind::GrepReplaceWith { paths, .. } if paths.len() == 2));
        for c in "ORA-7445".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();

        let Popup::GrepReplace(plan) = &app.popup else { panic!("no preview: {:?}", app.popup) };
        assert_eq!(plan.changes.len(), 3, "one row per changed line");
        assert!(plan.changes.iter().all(|c| c.picked));
        // The row order follows the walk, which the filesystem decides; find
        // the rows by what they say instead.
        let row = |plan: &crate::ReplacePlan, before: &str| {
            plan.changes.iter().position(|c| c.before == before).expect("row for {before}")
        };
        assert_eq!(plan.changes[row(plan, "ORA-600 first")].after, "ORA-7445 first");
        assert_eq!(plan.changes[row(plan, "col\tORA-600")].after, "col\tORA-7445");
        assert_eq!(
            std::fs::read(&a).unwrap(),
            b"ORA-600 first\r\nfine\r\nORA-600 third\r\n",
            "the preview must not have written anything",
        );

        // Space spares one line — and steps on, so a run can be unchecked by
        // holding it down.
        let spare = row(plan, "ORA-600 third");
        if let Popup::GrepReplace(plan) = &mut app.popup {
            plan.cursor = spare;
        }
        app.handle_key(key(' ')).unwrap();
        let Popup::GrepReplace(plan) = &app.popup else { panic!("preview gone") };
        assert!(!plan.changes[spare].picked);
        assert!(plan.cursor > spare || spare == plan.changes.len() - 1);

        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::None));
        assert_eq!(
            std::fs::read(&a).unwrap(),
            b"ORA-7445 first\r\nfine\r\nORA-600 third\r\n",
            "CRLF kept, and the unchecked line left alone",
        );
        assert_eq!(std::fs::read(&b).unwrap(), b"col\tORA-7445\n", "the tab survived");
        assert!(app.message.as_deref().unwrap_or("").contains("2 line(s) in 2 file(s)"));
    }

    /// Esc from the preview is free, and a name search has nothing to replace.
    #[test]
    fn a_grep_replace_can_always_be_backed_out_of() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("keep.txt");
        std::fs::write(&f, "TARGET\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        // A name search refuses, rather than replacing filenames by surprise.
        app.start_find("keep", cian_core::search::Mode::Name);
        drain_find(&mut app);
        app.handle_key(key('r')).unwrap();
        assert!(matches!(app.popup, Popup::FindResults { .. }), "still the results");
        assert!(app.message.as_deref().unwrap_or("").contains("grep"));

        app.start_find("TARGET", cian_core::search::Mode::Content);
        drain_find(&mut app);
        app.handle_key(key('r')).unwrap();
        for c in "GONE".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::GrepReplace(_)));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(std::fs::read_to_string(&f).unwrap(), "TARGET\n", "Esc wrote nothing");
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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

        // Closing the viewer returns to the grep results, not to nothing.
        quit_viewer(&mut app);
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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

    /// Wait until a branch view / panelize has installed its flat listing.
    /// `drain_find` cannot be used: routing to a pane releases the job the moment
    /// it completes, so there is no lingering `done` for it to observe.
    fn drain_until_flat(app: &mut App) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            app.poll_find_job();
            if app.active_pane().map(|p| p.is_flat()).unwrap_or(false) {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("branch view did not build");
    }

    #[test]
    fn b_flattens_the_subtree_into_the_pane_and_toggles_back() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        app.handle_key(key('b')).unwrap();
        drain_until_flat(&mut app);

        let pane = app.active_pane().unwrap();
        assert!(pane.is_flat());
        // Every file in the tree, folders excluded, shown by relative path.
        let mut names: Vec<String> =
            pane.entries.iter().filter(|e| !e.is_parent).map(|e| e.name.clone()).collect();
        names.sort();
        assert_eq!(
            names,
            vec!["build/main.o", "readme.md", "src/deep/main.rs", "src/main.rs"]
        );
        assert!(pane.entries.iter().all(|e| !e.is_parent), "no `..` row in a flat view");

        // `b` again leaves the view, back to the real directory listing.
        app.handle_key(key('b')).unwrap();
        let pane = app.active_pane().unwrap();
        assert!(!pane.is_flat());
        assert!(pane.entries.iter().any(|e| e.name == "src" && e.is_dir), "real dirs are back");
    }

    #[test]
    fn p_panelizes_search_results_into_the_pane() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        app.start_find("main", cian_core::search::Mode::Name);
        drain_find(&mut app);
        // main.rs (×2) + build/main.o = 3 name matches.
        let Popup::FindResults { hits, .. } = &app.popup else { panic!("no results") };
        assert_eq!(hits.len(), 3);

        app.handle_key(key('p')).unwrap();
        assert!(matches!(app.popup, Popup::None), "panelize closes the popup");
        assert!(app.find_job.is_none(), "and releases the worker");
        let pane = app.active_pane().unwrap();
        assert!(pane.is_flat());
        assert_eq!(pane.entries.iter().filter(|e| !e.is_parent).count(), 3);
    }

    #[test]
    fn a_search_with_no_matches_says_so_rather_than_hanging() {
        let d = find_tree();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 3));
        assert!(app.file_drag.is_some(), "pressing on an entry arms a drag");

        app.handle_mouse(mouse(
            MouseEventKind::Drag(MouseButton::Left),
            right.x + 5,
            right.y + 3,
        ));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 3));

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

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 3));
        app.handle_mouse(mouse(MouseEventKind::Drag(MouseButton::Left), right.x + 5, right.y + 3));
        let mut up = mouse(MouseEventKind::Up(MouseButton::Left), right.x + 5, right.y + 3);
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
        let mut app = App::new(start.clone(), start, en_config()).unwrap();
        let _ = render(&mut app, 100, 40);
        let left = app.layout_rects.left;
        // The first row is `..`; a single click steps up to the parent.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 3, left.y + 2));
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

        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), left.x + 5, left.y + 3));
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
        let app = App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
        let mut config = en_config();
        config.options.home = Some(d.path().display().to_string());
        assert_eq!(default_home(&config), d.path());

        // A configured but missing directory falls through (to Desktop/home/.).
        let mut config = en_config();
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
        let mut app = App::new(p.clone(), p.clone(), en_config()).unwrap();
        let _ = render(&mut app, 100, 40);
        let r = app.layout_rects.left;
        // Row 1 is the `..` row; "sub" (dirs first) is on row 2.
        let (cx, cy) = (r.x + 3, r.y + 3);
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
        let mut app = App::new(p.clone(), p.clone(), en_config()).unwrap();
        let _ = render(&mut app, 100, 40);
        let root = app.left.active_ref().cwd.clone();
        let r = app.layout_rects.left;
        // Row 2 is "sub"; row 1 is the `..` row (which would navigate up).
        let (cx, cy) = (r.x + 3, r.y + 3);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        app.handle_mouse(mouse(MouseEventKind::Up(MouseButton::Left), cx, cy));
        // Age the first click past the double-click window.
        app.last_click = Some((Instant::now() - Duration::from_secs(2), cy));
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), cx, cy));
        assert_eq!(app.left.active_ref().cwd, root,
            "a slow second click just selects, does not enter");
    }

    /// `:preview` borrows the shell panel for a cursor-follow preview: file
    /// contents while a file pane has focus, the real shell as soon as the
    /// shell takes focus — and the preview cache follows the cursor.
    #[test]
    fn preview_borrows_the_shell_panel_and_follows_the_cursor() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("hello.txt"), "alpha bravo preview-me\n").unwrap();
        std::fs::write(d.path().join("other.txt"), "charlie delta other-one\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "hello.txt").unwrap();
        }
        assert!(app.preview_on, "preview is on out of the box");
        let out = render(&mut app, 110, 36).join("\n");
        assert!(out.contains("⌥ preview"), "panel is labelled: {out}");
        assert!(out.contains("preview-me"), "shows the cursor file's text");
        assert!(!out.contains("other-one"), "not the other file");

        // Cursor moves → the preview follows.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "other.txt").unwrap();
        }
        let out = render(&mut app, 110, 36).join("\n");
        assert!(out.contains("other-one"), "follows the cursor: {out}");

        // Shell focus gets the real shell back.
        app.focus(FocusedPane::Shell);
        let out = render(&mut app, 110, 36).join("\n");
        assert!(!out.contains("⌥ preview"), "shell focus shows the shell");

        // And off means off, whatever has focus (the toggle flips on → off).
        app.focus(FocusedPane::Left);
        app.toggle_preview();
        let out = render(&mut app, 110, 36).join("\n");
        assert!(!out.contains("⌥ preview"));
    }

    /// Moving off an image asks the main loop for a full terminal clear.
    /// Terminal graphics are painted outside the cell buffer, so without it
    /// the picture stays on screen over the next file — which looked exactly
    /// like "the file after a png has no preview".
    #[test]
    fn leaving_an_image_preview_asks_for_a_clear() {
        let d = tempfile::tempdir().unwrap();
        // A real 2x2 PNG, plus a text file to move onto.
        let png: &[u8] = &[0x89,0x50,0x4E,0x47,0x0D,0x0A,0x1A,0x0A,0,0,0,0x0D,0x49,0x48,0x44,0x52,
            0,0,0,2,0,0,0,2,8,2,0,0,0,0xFD,0xD4,0x9A,0x73,0,0,0,0x16,0x49,0x44,0x41,0x54,
            0x78,0x9C,0x62,0xF8,0xCF,0xC0,0,0,0x03,0x01,0x01,0,0x18,0xDD,0x8D,0xB0,
            0,0,0,0,0x49,0x45,0x4E,0x44,0xAE,0x42,0x60,0x82];
        std::fs::write(d.path().join("pic.png"), png).unwrap();
        std::fs::write(d.path().join("after.txt"), "plain text after the picture\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        let go = |app: &mut App, name: &str| {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == name).unwrap();
        };
        go(&mut app, "pic.png");
        let _ = render(&mut app, 100, 30);
        app.full_clear = false; // ignore anything the first frame asked for

        // Onto the text file: the loop must be told to wipe first.
        go(&mut app, "after.txt");
        let out = render(&mut app, 100, 30).join("\n");
        assert!(app.full_clear, "leaving an image requests a clear");
        assert!(out.contains("plain text after"), "and the text is drawn: {out}");

        // Text to text costs nothing.
        app.full_clear = false;
        go(&mut app, "pic.png");
        let _ = render(&mut app, 100, 30);
        app.full_clear = false;
        go(&mut app, "after.txt");
        let _ = render(&mut app, 100, 30);
        assert!(app.full_clear);
        app.full_clear = false;
        let _ = render(&mut app, 100, 30);
        assert!(!app.full_clear, "a steady text preview asks for no clears");
    }

    /// A directory under the cursor previews as its listing.
    #[test]
    fn preview_lists_a_directory() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        std::fs::write(d.path().join("sub/inside.txt"), "x").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "sub").unwrap();
        }
        app.preview_on = true;
        let out = render(&mut app, 110, 36).join("\n");
        assert!(out.contains("inside.txt"), "directory listing shown: {out}");
    }

    /// Clicking a column header sorts by it; clicking it again flips the
    /// direction — how column headers behave everywhere else.
    #[test]
    fn clicking_a_column_header_sorts_and_flips() {
        let (_d, mut app) = app_with(&["small.txt", "big.txt"]);
        std::fs::write(_d.path().join("big.txt"), "x".repeat(5000)).unwrap();
        if let Some(p) = app.active_pane_mut() {
            let _ = p.reload();
        }
        let _ = render(&mut app, 100, 40);
        let (pane, key, r) = app
            .sort_rects
            .iter()
            .copied()
            .find(|(p, k, _)| *p == FocusedPane::Left && *k == cian_core::SortKey::Size)
            .expect("the Size header is clickable");
        assert_eq!(pane, FocusedPane::Left);
        assert_eq!(key, cian_core::SortKey::Size);
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), r.x, r.y));
        assert_eq!(app.active_pane().unwrap().sort.key, cian_core::SortKey::Size);
        assert!(!app.active_pane().unwrap().sort.reverse, "first click: ascending");
        let _ = render(&mut app, 100, 40); // rects rebuilt with the new sort glyph
        let (_, _, r) = app
            .sort_rects
            .iter()
            .copied()
            .find(|(p, k, _)| *p == FocusedPane::Left && *k == cian_core::SortKey::Size)
            .unwrap();
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), r.x, r.y));
        assert!(app.active_pane().unwrap().sort.reverse, "second click flips");
    }

    /// Clicking a path segment in the title jumps to that ancestor directory.
    #[test]
    fn clicking_a_breadcrumb_segment_navigates_up() {
        let d = tempfile::tempdir().unwrap();
        let deep = d.path().join("alpha").join("beta");
        std::fs::create_dir_all(&deep).unwrap();
        let mut app = App::new(deep.clone(), deep.clone(), en_config()).unwrap();
        let _ = render(&mut app, 120, 40);
        // strip=1 is the parent of the cwd ("alpha").
        let (_, _, r) = app
            .crumb_rects
            .iter()
            .copied()
            .find(|(p, strip, _)| *p == FocusedPane::Left && *strip == 1)
            .expect("the parent segment is clickable");
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Left), r.x, r.y));
        assert!(
            app.left.active_ref().cwd.ends_with("alpha"),
            "clicked one level up: {:?}",
            app.left.active_ref().cwd
        );
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
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
            App::new(l.path().to_path_buf(), r.path().to_path_buf(), en_config())
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
        app.start_history();
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
        app.start_history();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        app.handle_key(code(KeyCode::F(3))).unwrap();
        // A .md file opens straight into rendered preview.
        assert!(matches!(&app.popup, Popup::Viewer { markdown: true, preview: true, .. }), "opened in preview");
        // The render swaps the rendered document into view.lines (and fills the
        // per-char style grid) so the whole viewer works over the preview.
        let _ = render(&mut app, 100, 30);
        if let Popup::Viewer { view, md_styles, source, .. } = &app.popup {
            let flat = view.lines.join("\n");
            assert!(flat.contains("mermaid flow"), "mermaid flow is rendered");
            assert!(flat.contains('▶'), "the flow shows an arrow edge");
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

        // Ctrl+E toggles to raw source (view.lines becomes the file text
        // again); `:preview` does the same where Ctrl is not deliverable.
        app.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::CONTROL)).unwrap();
        let _ = render(&mut app, 100, 30);
        if let Popup::Viewer { view, md_styles, preview, .. } = &app.popup {
            assert!(!*preview, "toggled to source");
            assert!(md_styles.is_empty(), "styles dropped in source mode");
            assert!(view.lines.iter().any(|l| l == "# Title"), "shows raw source");
        } else {
            panic!("not a viewer");
        }
        app.handle_key(key(':')).unwrap();
        if let Popup::Viewer { sub_input, .. } = &mut app.popup {
            *sub_input = Some("preview".into());
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { preview: true, .. }), "back to preview");
        // Esc peels state: the still-active search clears first (viewer stays),
        // then a second Esc closes.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        match &app.popup {
            Popup::Viewer { find_query, .. } => assert!(find_query.is_none(), "search cleared, not closed"),
            _ => panic!("first Esc should have kept the viewer open"),
        }
        quit_viewer(&mut app);
        assert!(matches!(app.popup, Popup::None), ":q closes it");
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
            en_config(),
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
        let mut cfg = en_config();
        cfg.options.lang = Some("en".into());
        cfg.options.menu_lang = Some("ja".into());
        let app = App::new(p.clone(), p, cfg).unwrap();
        assert_eq!(app.lang, Lang::En, "the rest of the UI stays English");
        assert_eq!(app.menu_lang, Lang::Ja, "menu + manual follow menu_lang");

        // Unset menu_lang follows lang.
        let d2 = tempfile::tempdir().unwrap();
        let p2 = d2.path().to_path_buf();
        let mut cfg2 = en_config();
        cfg2.options.lang = Some("ja".into());
        let app2 = App::new(p2.clone(), p2, cfg2).unwrap();
        assert_eq!(app2.menu_lang, Lang::Ja, "falls back to lang when unset");
    }

    #[test]
    fn where_shows_config_paths() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        assert_eq!(menu_label_parts("AI - crmaine ▸"), ("AI - crmaine ▸", ""));
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
        let mut cfg = en_config();
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
        let mut app = App::new(p.clone(), p.clone(), en_config()).unwrap();
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
            en_config(),
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
    fn recent_files_dedupe_and_skip_remote_temp() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        app.note_recent_file(std::path::Path::new("/proj/a.rs"));
        app.note_recent_file(std::path::Path::new("/proj/b.rs"));
        app.note_recent_file(std::path::Path::new("/proj/a.rs")); // re-open moves to front
        assert_eq!(app.recent_files.len(), 2, "duplicate collapsed");
        assert_eq!(app.recent_files[0], std::path::PathBuf::from("/proj/a.rs"), "most recent first");

        // A downloaded remote temp is not a reopenable local file.
        app.note_recent_file(std::path::Path::new("/tmp/cian-remote/x.log"));
        assert_eq!(app.recent_files.len(), 2, "remote temp not recorded");
    }

    #[test]
    fn ai_history_archives_reopens_and_forgets() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

        // A RAG chat with an answer in it.
        app.popup = Popup::AiChat {
            input: String::new(),
            log: vec![
                ChatMsg { user: true, text: "first question".into() },
                ChatMsg { user: false, text: "an answer".into() },
            ],
            scroll: 0,
            pending: false,
            sel: None,
            mode: ChatMode::Rag,
            skin: ChatSkin::of(ChatMode::Rag),
        };
        app.open_ai_history();
        assert!(matches!(app.popup, Popup::AiHistory { .. }), "history picker opens");
        assert_eq!(app.ai_history.len(), 1, "current conversation archived");
        assert_eq!(app.ai_history[0].mode(), ChatMode::Rag, "backend remembered");
        assert_eq!(App::ai_history_title(app.ai_history[0].log()), "first question");

        // Reopening restores the backend, so a follow-up still goes to RAG.
        app.load_ai_conversation(0);
        assert!(matches!(app.popup, Popup::AiChat { mode: ChatMode::Rag, .. }), "mode restored");
        app.open_ai_history();
        assert_eq!(app.ai_history.len(), 1, "identical snapshot deduped");

        // A chat with no answer is not worth archiving.
        app.popup = Popup::AiChat {
            input: String::new(),
            log: vec![ChatMsg { user: true, text: "unanswered".into() }],
            scroll: 0,
            pending: true,
            sel: None,
            mode: ChatMode::Ai,
            skin: ChatSkin::of(ChatMode::Ai),
        };
        app.archive_current_ai_chat();
        assert_eq!(app.ai_history.len(), 1, "answerless chat not archived");

        // Reopen, then forget it.
        app.load_ai_conversation(0);
        assert!(matches!(app.popup, Popup::AiChat { .. }), "conversation reopened");
        app.popup = Popup::AiHistory { cursor: 0 };
        app.delete_ai_conversation(0);
        assert!(app.ai_history.is_empty(), "conversation forgotten");
    }

    #[test]
    fn folder_sync_one_way_copies_source_and_keeps_dest_only() {
        use std::sync::Arc;
        let l = tempfile::tempdir().unwrap();
        let r = tempfile::tempdir().unwrap();
        std::fs::write(l.path().join("only_left.txt"), b"L").unwrap();
        std::fs::write(l.path().join("both.txt"), b"AAA").unwrap();
        std::fs::write(r.path().join("both.txt"), b"BBB").unwrap();
        std::fs::write(r.path().join("only_right.txt"), b"R").unwrap();
        // A whole subtree present only on the left copies as one entry.
        std::fs::create_dir(l.path().join("newdir")).unwrap();
        std::fs::write(l.path().join("newdir").join("deep.txt"), b"D").unwrap();

        let mut app = App::new(
            l.path().to_path_buf(),
            r.path().to_path_buf(),
            en_config(),
        )
        .unwrap();

        let cancel = Arc::new(AtomicBool::new(false));
        let diff = cian_core::dirdiff::compare(l.path(), r.path(), &cancel, &mut |_| {});
        app.popup = Popup::DirCompare {
            left: "L".into(), right: "R".into(),
            left_root: l.path().to_path_buf(), right_root: r.path().to_path_buf(),
            entries: diff.entries, cursor: 0, scroll: 0, truncated: false,
        };

        // Sync left → right: everything the left has, none of it deleted.
        app.dir_compare_sync(true);
        let Popup::ConfirmDirSync { ops, extra, to_right, .. } = &app.popup else {
            panic!("expected a sync confirmation, got {:?}", app.popup);
        };
        assert!(*to_right);
        assert_eq!(*extra, 1, "only_right.txt is destination-only");
        assert_eq!(ops.len(), 3, "only_left.txt + both.txt + newdir/");
        app.confirm_dir_sync();
        assert!(app.op_job.is_some(), "sync runs on the worker");
        let start = Instant::now();
        while app.op_job.is_some() && start.elapsed() < Duration::from_secs(5) {
            app.poll_op_job();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(r.path().join("only_left.txt").exists(), "source-only copied");
        assert_eq!(std::fs::read(r.path().join("both.txt")).unwrap(), b"AAA", "differing overwritten");
        assert_eq!(std::fs::read(r.path().join("newdir").join("deep.txt")).unwrap(), b"D", "subtree copied");
        assert!(r.path().join("only_right.txt").exists(), "destination-only kept, never deleted");

        // Running it again finds nothing to do.
        let diff2 = cian_core::dirdiff::compare(l.path(), r.path(), &cancel, &mut |_| {});
        app.popup = Popup::DirCompare {
            left: "L".into(), right: "R".into(),
            left_root: l.path().to_path_buf(), right_root: r.path().to_path_buf(),
            entries: diff2.entries, cursor: 0, scroll: 0, truncated: false,
        };
        app.dir_compare_sync(true);
        assert!(matches!(app.popup, Popup::DirCompare { .. }), "nothing to sync leaves the compare up");
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

        let mut app = App::new(dir.clone(), dir.clone(), en_config()).unwrap();
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
        quit_viewer(&mut app);

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
        quit_viewer(&mut app);

        // F3 then B toggles blame.
        app.handle_key(code(KeyCode::F(3))).unwrap();
        for k in [':', 'b', 'l', 'a', 'm', 'e'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        match &app.popup {
            Popup::Viewer { blame, .. } => assert!(!blame.is_empty(), "blame computed"),
            _ => panic!("not a viewer"),
        }
    }

    #[test]
    fn disk_usage_cache_populates_for_the_active_pane() {
        let d = tempfile::tempdir().unwrap();
        let p = std::fs::canonicalize(d.path()).unwrap();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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

        let mut app = App::new(wc.clone(), wc.clone(), en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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

        // Esc leaves edit mode; `:q` closes (nothing unsaved now).
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { editing: false, .. }));
        quit_viewer_discarding(&mut app);
        assert!(matches!(app.popup, Popup::None));
    }

    /// Open note.txt ("alpha…delta") in the viewer, cursor on line 0.
    fn viewer_on(lines: &str) -> (tempfile::TempDir, App) {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("note.txt"), lines).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        // No system clipboard: these tests run in parallel on one machine and
        // would otherwise yank and paste through the *developer's* clipboard,
        // reading each other's copies. cian's own yank is the path that has to
        // work anyway — it is what a machine over SSH has.
        app.clipboard = None;
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "note.txt").unwrap();
        // Enter opens the panel where the file was listed; F12 gives it the
        // window, which is the shape most of these tests are about.
        app.handle_key(code(KeyCode::Enter)).unwrap();
        app.handle_key(code(KeyCode::F(12))).unwrap();
        let _ = render(&mut app, 100, 30);
        (d, app)
    }

    fn viewer_lines(app: &App) -> Vec<String> {
        match &app.popup {
            Popup::Viewer { view, .. } => view.lines.clone(),
            other => panic!("not a viewer: {other:?}"),
        }
    }

    /// The normal-mode change set: dd/x/J/D mutate in place, o opens a line
    /// and drops into insert, and `u` walks it all back — one unit per change,
    /// with `dirty` clearing once the stack drains to the original.
    #[test]
    fn viewer_normal_mode_operators_edit_and_undo() {
        let (_d, mut app) = viewer_on("alpha\nbravo\ncharlie\n");

        // dd deletes the line under the cursor.
        app.handle_key(key('d')).unwrap();
        app.handle_key(key('d')).unwrap();
        assert_eq!(viewer_lines(&app), ["bravo", "charlie"], "dd removed line 0");

        // x deletes the character under the cursor.
        app.handle_key(key('x')).unwrap();
        assert_eq!(viewer_lines(&app)[0], "ravo", "x ate the b");

        // `gJ` joins the next line up. (`J` is the window's key for the
        // shell below; `:combine` is the one that adds a space.)
        app.handle_key(key('g')).unwrap();
        app.handle_key(key('J')).unwrap();
        assert_eq!(viewer_lines(&app), ["ravocharlie"], "gJ joined");

        // o opens a line below and enters insert mode; typing lands there.
        app.handle_key(key('o')).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { editing: true, .. }), "o → insert mode");
        app.handle_key(key('z')).unwrap();
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert_eq!(viewer_lines(&app), ["ravocharlie", "z"]);

        // u, u, u, u: each change was one unit (the whole o-session is one).
        for expect in [
            vec!["ravocharlie".to_string()],
            vec!["ravo".into(), "charlie".into()],
            vec!["bravo".into(), "charlie".into()],
            vec!["alpha".into(), "bravo".into(), "charlie".into()],
        ] {
            app.handle_key(key('u')).unwrap();
            assert_eq!(viewer_lines(&app), expect);
        }
        assert!(
            matches!(app.popup, Popup::Viewer { dirty: false, .. }),
            "undone to the original → clean, so Esc closes without a warning"
        );

        // One more u: nothing left, and it says so rather than scrolling.
        app.handle_key(key('u')).unwrap();
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains("oldest") || msg.contains("戻れません"), "says so: {msg}");
    }

    /// V + d deletes the selected lines; v + d splices within lines.
    #[test]
    fn viewer_visual_delete() {
        let (_d, mut app) = viewer_on("one\ntwo\nthree\nfour\n");
        // V j d: delete lines 0-1.
        app.handle_key(key('V')).unwrap();
        app.handle_key(key('j')).unwrap();
        app.handle_key(key('d')).unwrap();
        assert_eq!(viewer_lines(&app), ["three", "four"]);

        // v l l d on "three": delete chars 0..=2 → "ee".
        app.handle_key(key('v')).unwrap();
        app.handle_key(key('l')).unwrap();
        app.handle_key(key('l')).unwrap();
        app.handle_key(key('d')).unwrap();
        assert_eq!(viewer_lines(&app), ["ee", "four"]);

        // u twice restores everything.
        app.handle_key(key('u')).unwrap();
        app.handle_key(key('u')).unwrap();
        assert_eq!(viewer_lines(&app), ["one", "two", "three", "four"]);
    }

    /// d and u still scroll on a non-editable view (here: the hex dump), so
    /// the pager reflexes survive where there is nothing to edit.
    #[test]
    fn viewer_d_and_u_still_scroll_where_not_editable() {
        let d = tempfile::tempdir().unwrap();
        // A binary file (NUL bytes) opens as a hex dump, which is not editable.
        let mut bytes = vec![0u8; 4096];
        bytes[1] = 1;
        std::fs::write(d.path().join("blob.bin"), &bytes).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "blob.bin").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        assert!(
            matches!(&app.popup, Popup::Viewer { view, editable: true, .. }
                if view.kind == cian_core::viewer::ViewKind::Binary),
            "a hex dump is editable — but as hex (i), not with the text operators"
        );
        let before = match &app.popup {
            Popup::Viewer { line, .. } => *line,
            _ => unreachable!(),
        };
        // `d` is vi's operator now, not a scroll key — Ctrl+D scrolls.
        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)).unwrap();
        let after = match &app.popup {
            Popup::Viewer { line, .. } => *line,
            _ => unreachable!(),
        };
        assert!(after > before, "Ctrl+D scrolled half a page");
    }

    #[test]
    fn the_viewer_refuses_to_drop_unsaved_edits() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "x\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "a.txt").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);
        app.handle_key(key('i')).unwrap();
        app.handle_key(key('z')).unwrap(); // dirty
        app.handle_key(code(KeyCode::Esc)).unwrap(); // leave edit mode
        // `:q` won't discard unsaved work…
        quit_viewer(&mut app);
        assert!(matches!(app.popup, Popup::Viewer { dirty: true, .. }), "still open, warned");
        // …but `:q!` does.
        quit_viewer_discarding(&mut app);
        assert!(matches!(app.popup, Popup::None), ":q! discards and closes");
    }

    #[test]
    fn viewer_esc_clears_search_before_closing() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "alpha\nbeta\ngamma\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.active_pane_mut().unwrap().cursor =
            app.active_pane().unwrap().entries.iter().position(|e| e.name == "a.txt").unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();
        let _ = render(&mut app, 100, 30);

        // Run a `/` search, then Esc: it clears the search and the viewer stays.
        app.handle_key(key('/')).unwrap();
        for c in "beta".chars() { app.handle_key(key(c)).unwrap(); }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { find_query: Some(_), .. }), "search active");
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(&app.popup, Popup::Viewer { find_query: None, .. }), "Esc cleared the search");

        // A second Esc does *not* close it — that is `:q`, as it is in vi —
        // and it says so rather than doing nothing.
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "Esc does not close the file");
        assert!(
            app.message.as_deref().is_some_and(|m| m.contains(":q")),
            "it says how: {:?}",
            app.message,
        );
        for k in [':', 'q'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert!(matches!(app.popup, Popup::None), ":q closes it");
    }

    /// A zip with a small tree, for the archive-browse tests.
    fn make_browse_zip(dir: &std::path::Path) -> PathBuf {
        use std::io::Write;
        let path = dir.join("bundle.zip");
        let f = std::fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        w.start_file("top.txt", opts).unwrap();
        w.write_all(b"top level\n").unwrap();
        w.start_file("docs/readme.md", opts).unwrap();
        w.write_all(b"# hello from inside\n").unwrap();
        w.start_file("docs/deep/note.txt", opts).unwrap();
        w.write_all(b"deep note\n").unwrap();
        w.finish().unwrap();
        path
    }

    /// No keystroke may end the session. `l` inside an archive used to reach
    /// the local-directory navigation and hand it a member path, whose
    /// read_dir failure propagated all the way out of the event loop and
    /// killed cian — with an unsaved-work-shaped hole where a message belonged.
    #[test]
    fn keys_inside_an_archive_never_kill_the_session() {
        let d = tempfile::tempdir().unwrap();
        make_browse_zip(d.path());
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "bundle.zip").unwrap();
        }
        app.activate_selected().unwrap();
        assert!(app.active_pane().unwrap().archive_view().is_some());
        // Every plain letter, on every row, including the ones that used to
        // walk into the filesystem with a path that only exists in the zip.
        for row in 0..app.active_pane().unwrap().entries.len() {
            app.active_pane_mut().unwrap().cursor = row;
            for c in "abcdefghijklmnopqrstuvwxyz-".chars() {
                assert!(app.handle_key(key(c)).is_ok(), "key {c:?} on row {row} returned an error");
                if app.active_pane().map(|p| p.archive_view().is_none()).unwrap_or(true) {
                    // A key legitimately left the archive; go back in and carry on.
                    app.popup = Popup::None;
                    let pane = app.active_pane_mut().unwrap();
                    if let Some(i) = pane.entries.iter().position(|e| e.name == "bundle.zip") {
                        pane.cursor = i;
                    }
                    app.activate_selected().unwrap();
                }
                app.popup = Popup::None;
            }
        }
    }

    /// Alt+←/→ (and Alt+h/l) are the browser arrows over this pane's history,
    /// and `-` stays unbound so a stray dash never navigates.
    #[test]
    fn alt_arrows_walk_the_directory_history() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("sub")).unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p.clone(), en_config()).unwrap();
        let root = app.active_pane().unwrap().cwd.clone();

        // Go into sub/, then back, then forward again.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "sub").unwrap();
        }
        app.activate_selected().unwrap();
        assert!(app.active_pane().unwrap().cwd.ends_with("sub"));

        let alt = |c: char| KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT);
        app.handle_key(alt('h')).unwrap();
        assert_eq!(app.active_pane().unwrap().cwd, root, "Alt+h went back");
        app.handle_key(alt('l')).unwrap();
        assert!(app.active_pane().unwrap().cwd.ends_with("sub"), "Alt+l went forward");

        // The arrows are the same pair.
        app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)).unwrap();
        assert_eq!(app.active_pane().unwrap().cwd, root);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)).unwrap();
        assert!(app.active_pane().unwrap().cwd.ends_with("sub"));

        // Going somewhere new ends the forward branch.
        app.handle_key(alt('h')).unwrap();
        assert!(!app.active_pane().unwrap().forward.is_empty(), "forward is armed");
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "sub").unwrap();
        }
        app.activate_selected().unwrap();
        assert!(app.active_pane().unwrap().forward.is_empty(), "a new step drops forward");

        // `-` is unbound; Backspace still goes up.
        let before = app.active_pane().unwrap().cwd.clone();
        app.handle_key(key('-')).unwrap();
        assert_eq!(app.active_pane().unwrap().cwd, before, "`-` is unbound");
        app.handle_key(code(KeyCode::Backspace)).unwrap();
        assert_ne!(app.active_pane().unwrap().cwd, before, "Backspace still goes up");
    }


    /// A file dragged from Finder/Explorer onto the terminal arrives as a
    /// paste; cian turns it into a move into the focused pane, asking first.
    #[test]
    fn a_dropped_file_becomes_a_move_into_this_pane() {
        let (l, r, mut app) = app_two_dirs(&["victim.txt"], &[]);
        app.focus(FocusedPane::Right);
        let src = l.path().join("victim.txt");

        // The shape iTerm2 sends for a drag.
        let dropped = src.display().to_string().replace(' ', "\\ ");
        assert!(app.accept_drop(&dropped), "recognised as a drop");
        match &app.popup {
            Popup::ConfirmTransfer { op, targets, dest } => {
                assert!(matches!(op, PendingOp::Move), "a drop moves");
                assert_eq!(targets, &vec![src.clone()]);
                // Compare by the final component: the pane canonicalises
                // (/var → /private/var on macOS) and the tempdir does not.
                assert_eq!(dest.file_name(), r.path().file_name());
            }
            other => panic!("expected the transfer confirm, got {other:?}"),
        }
        // Confirming actually moves it.
        app.handle_key(key('y')).unwrap();
        drain_op_job(&mut app);
        assert!(r.path().join("victim.txt").exists(), "landed in the right pane");
        assert!(!src.exists(), "and left the left pane");
    }

    /// Ordinary pastes must still be pastes — the drop path only claims text
    /// that is entirely real files, and never while something is being typed.
    #[test]
    fn a_drop_never_steals_an_ordinary_paste() {
        let (_l, _r, mut app) = app_two_dirs(&["a.txt"], &[]);
        assert!(!app.accept_drop("just some words"), "prose is a paste");

        // Even a real path, while a text field is open, belongs to the field.
        let real = _l.path().join("a.txt").display().to_string();
        app.start_rename();
        assert!(!app.accept_drop(&real), "a text field keeps its paste");
        app.popup = Popup::None;

        // And the shell keeps its own — dropping a file on a terminal to get
        // its path onto the command line predates cian.
        app.focus(FocusedPane::Shell);
        assert!(!app.accept_drop(&real), "the shell keeps its paste");
    }

    /// Inside an archive the hint bar names archive keys — and says outright
    /// when the format is read-only, since the keys that would write are the
    /// ones a filer user reaches for first.
    #[test]
    fn the_hint_bar_changes_inside_an_archive() {
        let d = tempfile::tempdir().unwrap();
        make_browse_zip(d.path());
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        let plain: Vec<&str> = crate::render::key_hints(&app).iter().map(|(k, _)| *k).collect();
        assert!(plain.contains(&"S-J"), "the ordinary bar leads with pane keys");

        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "bundle.zip").unwrap();
        }
        app.activate_selected().unwrap();
        let hints = crate::render::key_hints(&app);
        let keys: Vec<&str> = hints.iter().map(|(k, _)| *k).collect();
        assert!(keys.contains(&"Enter/l") && keys.contains(&"-/h"), "navigation named: {keys:?}");
        assert!(keys.contains(&"F3"), "member viewing named");
        assert!(keys.contains(&"F2") && keys.contains(&"d"), "zip is writable, so say so: {keys:?}");
    }

    /// Enter on a zip browses into it like a folder: members list, subdirs
    /// descend, `..` climbs, and past the root you are back on the archive.
    #[test]
    fn enter_browses_into_an_archive_and_out_again() {
        let d = tempfile::tempdir().unwrap();
        make_browse_zip(d.path());
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "bundle.zip").unwrap();
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane().unwrap();
            assert!(pane.archive_view().is_some(), "entered the archive");
            let names: Vec<&str> = pane.entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, vec!["..", "docs", "top.txt"], "root listing");
        }
        // Descend into docs/, then docs/deep/.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "docs").unwrap();
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane().unwrap();
            assert_eq!(pane.archive_view().unwrap().1, "docs/");
            let names: Vec<&str> = pane.entries.iter().map(|e| e.name.as_str()).collect();
            assert_eq!(names, vec!["..", "deep", "readme.md"]);
        }
        // `..` climbs back to the root; cursor lands on the dir we left.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = 0;
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane().unwrap();
            assert_eq!(pane.archive_view().unwrap().1, "");
            assert_eq!(pane.selected().unwrap().name, "docs", "cursor on the dir we left");
        }
        // `..` at the root leaves the archive, cursor on the zip itself.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = 0;
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane().unwrap();
            assert!(pane.archive_view().is_none(), "left the archive");
            assert_eq!(pane.selected().unwrap().name, "bundle.zip");
        }
    }

    /// F3 on a member extracts to a temp file and opens the normal viewer;
    /// markdown members even get their preview.
    #[test]
    fn f3_views_an_archive_member() {
        let d = tempfile::tempdir().unwrap();
        make_browse_zip(d.path());
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "bundle.zip").unwrap();
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "top.txt").unwrap();
        }
        app.handle_key(code(KeyCode::F(3))).unwrap();
        match &app.popup {
            Popup::Viewer { view, title, .. } => {
                assert!(view.lines.join("\n").contains("top level"), "member content shown");
                assert!(title.contains("bundle.zip"), "title names the archive: {title}");
            }
            other => panic!("expected the viewer, got {other:?}"),
        }
    }

    /// Copying from inside an archive extracts to the other pane, relative to
    /// the directory being browsed.
    #[test]
    fn copy_out_of_an_archive_extracts_to_the_other_pane() {
        let (l, r, mut app) = app_two_dirs(&[], &[]);
        let zip = make_browse_zip(l.path());
        let _ = zip;
        if let Some(pane) = app.active_pane_mut() {
            let _ = pane.reload();
            pane.cursor = pane.entries.iter().position(|e| e.name == "bundle.zip").unwrap();
        }
        app.activate_selected().unwrap();
        // Into docs/, then copy readme.md across.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "docs").unwrap();
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "readme.md").unwrap();
        }
        app.start_transfer(PendingOp::Copy);
        drain_op_job(&mut app);
        assert!(
            r.path().join("readme.md").exists(),
            "extracted relative to docs/, not the whole tree"
        );
        assert!(!r.path().join("docs").exists(), "no rebuilt docs/ directory");

        // A directory row extracts everything under it, keeping its own name.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = 0; // `..` → back to root
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "docs").unwrap();
        }
        app.start_transfer(PendingOp::Copy);
        drain_op_job(&mut app);
        assert!(r.path().join("docs/deep/note.txt").exists(), "subtree extracted");

        // Move is refused while archives are read-only.
        app.start_transfer(PendingOp::Move);
        let msg = app.message.clone().unwrap_or_default();
        assert!(msg.contains("read-only") || msg.contains("読み取り専用"), "{msg}");
    }

    /// The write side, end to end: copy INTO the zip from the other pane,
    /// rename a member, delete a member — each confirmed, run on the worker,
    /// and reflected in the refreshed listing.
    #[test]
    fn zip_add_rename_delete_from_the_panes() {
        let (l, r, mut app) = app_two_dirs(&[], &["fresh.txt"]);
        let zip = make_browse_zip(l.path());
        std::fs::write(r.path().join("fresh.txt"), "fresh body").unwrap();
        // Left pane: into the zip's docs/ directory.
        if let Some(pane) = app.active_pane_mut() {
            let _ = pane.reload();
            pane.cursor = pane.entries.iter().position(|e| e.name == "bundle.zip").unwrap();
        }
        app.activate_selected().unwrap();
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "docs").unwrap();
        }
        app.activate_selected().unwrap();

        // Right pane copies fresh.txt toward the left → confirm → into docs/.
        app.focus(FocusedPane::Right);
        if let Some(pane) = app.active_pane_mut() {
            let _ = pane.reload();
            pane.cursor = pane.entries.iter().position(|e| e.name == "fresh.txt").unwrap();
        }
        app.start_transfer(PendingOp::Copy);
        assert!(
            matches!(app.popup, Popup::ConfirmZipAdd { .. }),
            "asks before writing into the zip: {:?}",
            app.popup
        );
        app.handle_key(key('y')).unwrap();
        drain_op_job(&mut app);
        let names: Vec<String> = cian_core::archive::list(&zip)
            .unwrap()
            .into_iter()
            .map(|m| m.name)
            .collect();
        assert!(names.contains(&"docs/fresh.txt".to_string()), "added under docs/: {names:?}");

        // The left pane (still inside docs/) sees the new member.
        app.focus(FocusedPane::Left);
        let listed: Vec<String> =
            app.active_pane().unwrap().entries.iter().map(|e| e.name.clone()).collect();
        assert!(listed.contains(&"fresh.txt".to_string()), "listing refreshed: {listed:?}");

        // Rename it (F2 path) …
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "fresh.txt").unwrap();
        }
        app.start_rename();
        assert!(
            matches!(&app.popup, Popup::TextInput { kind: InputKind::RenameZipMember { .. }, .. }),
            "member rename prompt: {:?}",
            app.popup
        );
        // Clear the seeded name, type the new one, Enter.
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL)).unwrap();
        for c in "renamed.txt".chars() {
            app.handle_key(key(c)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        drain_op_job(&mut app);
        let names: Vec<String> =
            cian_core::archive::list(&zip).unwrap().into_iter().map(|m| m.name).collect();
        assert!(names.contains(&"docs/renamed.txt".to_string()), "renamed: {names:?}");
        assert!(!names.contains(&"docs/fresh.txt".to_string()));

        // …and delete it.
        {
            let pane = app.active_pane_mut().unwrap();
            pane.cursor = pane.entries.iter().position(|e| e.name == "renamed.txt").unwrap();
        }
        app.start_delete();
        assert!(matches!(app.popup, Popup::ConfirmZipDelete { .. }), "{:?}", app.popup);
        app.handle_key(key('y')).unwrap();
        drain_op_job(&mut app);
        let names: Vec<String> =
            cian_core::archive::list(&zip).unwrap().into_iter().map(|m| m.name).collect();
        assert!(!names.contains(&"docs/renamed.txt".to_string()), "deleted: {names:?}");
        // Untouched members survived all three rewrites.
        assert!(names.contains(&"docs/deep/note.txt".to_string()));
        assert!(names.contains(&"top.txt".to_string()));
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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
        quit_viewer(&mut app);
        assert!(matches!(app.popup, Popup::None));
    }

    #[test]
    fn the_viewer_line_visual_selects_and_copies_a_range() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(d.path().join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let p = d.path().to_path_buf();
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        // Right-click opens the menu, with the viewer put aside rather than
        // closed — the selection is still there behind it.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), x0 + 8, body.y));
        assert!(matches!(app.popup, Popup::ContextMenu { .. }), "the menu opened");
        assert!(app.viewer_return.is_some(), "the file is waiting behind it");

        // Copy is in it, and means the selection.
        let at = match &app.popup {
            Popup::ContextMenu { items, .. } => {
                items.iter().position(|i| matches!(i, MenuItem::Copy)).expect("Copy is in the menu")
            }
            _ => unreachable!(),
        };
        if let Popup::ContextMenu { cursor, .. } = &mut app.popup {
            *cursor = at;
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
        assert_eq!(app.message.as_deref(), Some("copied"));
        assert!(matches!(app.popup, Popup::Viewer { .. }), "and the file came back");

        // Esc out of the menu puts the file back untouched.
        app.handle_mouse(mouse(MouseEventKind::Down(MouseButton::Right), x0 + 8, body.y));
        app.handle_key(code(KeyCode::Esc)).unwrap();
        assert!(matches!(app.popup, Popup::Viewer { .. }), "back to the file");
        assert!(app.viewer_return.is_none(), "and nothing left waiting");
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        app.handle_key(code(KeyCode::F(3))).unwrap();

        // `e` opens the picker (a list), not an immediate cycle.
        for k in [':', 'e', 'n', 'c'] {
            app.handle_key(key(k)).unwrap();
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
        // Open the file, then Shift+Enter for the viewer's menu, and take the
        // item that reveals it. (Shift+Enter used to reveal it directly; it is
        // the keyboard's right-click now, and revealing moved into the menu.)
        app.open_viewer_at(&d.path().join("sub").join("deep.txt"), "deep.txt", 0);
        let _ = render(&mut app, 100, 30);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT)).unwrap();
        let at = match &app.popup {
            Popup::ContextMenu { items, .. } => items
                .iter()
                .position(|i| matches!(i, MenuItem::RevealInPane))
                .expect("the menu offers it"),
            other => panic!("expected the viewer's menu, got {other:?}"),
        };
        if let Popup::ContextMenu { cursor, .. } = &mut app.popup {
            *cursor = at;
        }
        app.handle_key(code(KeyCode::Enter)).unwrap();
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
        // Closing still returns to the (stepped) results list.
        quit_viewer(&mut app);
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();

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
            en_config(),
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
        let mut app = App::new(p.clone(), p, en_config()).unwrap();
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
