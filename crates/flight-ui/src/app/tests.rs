use super::*;
use eframe::egui::Color32;
fn test_context() -> egui::Context {
    let ctx = egui::Context::default();
    ctx.input_mut(|input| input.max_texture_side = 4096);
    ctx
}
fn finish_generation(app: &mut FlightApp, ctx: &egui::Context) {
    for _ in 0..6 {
        let Some(task) = app.task.as_ref() else { break };
        let result = match task.kind {
            Kind::Check(_) => json!({"scores":vec![vec![1u64;9]],"canProve":true}),
            Kind::Prove(q) => serde_json::to_value(proof(q + 1)).unwrap(),
            Kind::Import => panic!("unexpected import during generation"),
        };
        app.backend.reply(json!({"type":"result","result":result}));
        app.receive(ctx);
    }
    assert!(!app.busy(), "generation did not finish: {}", app.status);
    assert_eq!(app.status, "证书已生成");
    assert!(app.ready.is_some());
}
fn painted_label(output: &egui::FullOutput, label: &str) -> egui::Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.visual_bounding_rect())
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("missing label: {label}"))
}
fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}
#[test]
fn four_navigation_tabs_fit_and_preserve_generation_input() {
    for width in [320.0, 400.0, 1024.0] {
        let ctx = test_context();
        let mut app = FlightApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        assert_eq!(app.page, Page::Materials);
        app.team = "甲".into();
        let completed = proof(1);
        app.questions[0].proof = Some(completed.clone());
        app.questions[1].files = csv_pair("UNSAVED CSV CONTENTS");
        app.questions[1].checked = Some(vec![1; 9]);
        ctx.style_mut(|style| style.animation_time = 0.0);
        let draw = |app: &mut FlightApp, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 768.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("navigation")
                        .frame(egui::Frame::new().inner_margin(egui::Margin::symmetric(
                            if width < 600.0 { 12 } else { 24 },
                            18,
                        )))
                        .show(ctx, |ui| app.navigation(ui));
                    egui::CentralPanel::default().show(ctx, |_| {});
                },
            )
        };
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        let output = draw(&mut app, vec![]);
        let tabs = ["题目资料", "解答认证", "验证证书", "证书记录"]
            .map(|label| painted_label(&output, label));
        for rect in tabs {
            assert!(
                rect.left() >= 0.0 && rect.right() <= width,
                "{width}: clipped navigation at {rect:?}"
            );
            assert!((rect.center().y - tabs[0].center().y).abs() < 1.0);
        }
        for pair in tabs.windows(2) {
            assert!(pair[0].right() < pair[1].left());
        }
        for (label, page) in [
            ("解答认证", Page::Generate),
            ("题目资料", Page::Materials),
            ("证书记录", Page::Records),
            ("验证证书", Page::Verify),
            ("解答认证", Page::Generate),
        ] {
            let pos = painted_label(&draw(&mut app, vec![]), label).center();
            draw(
                &mut app,
                vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
            );
            draw(&mut app, vec![pointer_button(pos, false)]);
            assert_eq!(app.page, page, "{width}: {label}");
        }
        assert_eq!(app.team, "甲");
        assert_eq!(
            app.questions[0].proof.as_ref().unwrap().id(),
            completed.id()
        );
        assert_eq!(app.questions[1].checked, Some(vec![1; 9]));
        for file in &app.questions[1].files {
            assert!(file.name.starts_with("PRIVATE-"));
            assert_eq!(file.text, "UNSAVED CSV CONTENTS");
        }
    }
}
#[test]
fn record_actions_still_open_verification_and_generation() {
    for (label, destination) in [("查看", Page::Verify), ("更新", Page::Generate)] {
        let ctx = test_context();
        let mut app = FlightApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        let record = proof(2);
        app.page = Page::Records;
        let draw = |app: &mut FlightApp, events| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(400.0, 300.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let (rect, _) = ui
                            .allocate_exact_size(egui::vec2(296.0, 40.0), egui::Sense::hover());
                        app.record_actions(ui, &record, rect);
                    });
                },
            )
        };
        for _ in 0..3 {
            draw(&mut app, vec![]);
        }
        let pos = painted_label(&draw(&mut app, vec![]), label).center();
        draw(
            &mut app,
            vec![egui::Event::PointerMoved(pos), pointer_button(pos, true)],
        );
        draw(&mut app, vec![pointer_button(pos, false)]);
        assert_eq!(app.page, destination, "record action: {label}");
        assert!(app.reset_scroll);
        match destination {
            Page::Verify => assert_eq!(app.review.as_ref().unwrap().item.id(), record.id()),
            Page::Generate => {
                assert_eq!(app.editing_record, Some(record.id()));
                assert_eq!(app.questions[1].proof.as_ref().unwrap().id(), record.id());
            }
            _ => unreachable!(),
        }
    }
}
#[test]
fn storage_modal_blocks_navigation_and_close_has_a_full_hoverable_hit_area() {
    use eframe::App;
    let ctx = test_context();
    let mut app = FlightApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
    ctx.style_mut(|style| style.animation_time = 0.0);
    let draw = |app: &mut FlightApp, events| {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1024.0, 768.0),
                )),
                events,
                ..Default::default()
            },
            |ctx| app.update(ctx, &mut eframe::Frame::_new_kittest()),
        )
    };
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    let output = draw(&mut app, vec![]);
    let navigation = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "验证证书" => {
                Some(text.visual_bounding_rect().center())
            }
            _ => None,
        })
        .expect("navigation button");
    let pointer = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    app.storage_open = true;
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    draw(
        &mut app,
        vec![
            egui::Event::PointerMoved(navigation),
            pointer(navigation, true),
        ],
    );
    draw(&mut app, vec![pointer(navigation, false)]);
    assert_eq!(
        app.page,
        Page::Materials,
        "the modal must block background navigation"
    );
    app.storage_open = true;
    for _ in 0..3 {
        draw(&mut app, vec![]);
    }
    let close = ctx
        .read_response(egui::Id::new("storage-close"))
        .unwrap()
        .rect;
    assert_eq!(close.size(), egui::vec2(44.0, 44.0));
    let corner = close.left_top() + egui::vec2(3.0, 3.0);
    let hovered = draw(&mut app, vec![egui::Event::PointerMoved(corner)]);
    assert!(
        hovered.shapes.iter().any(|shape| matches!(
            &shape.shape, egui::Shape::Rect(rect)
                if rect.rect == close && rect.fill == Color32::from_rgb(231, 239, 235)
        )),
        "hover fills the whole close button"
    );
    draw(&mut app, vec![pointer(corner, true)]);
    draw(&mut app, vec![pointer(corner, false)]);
    assert!(
        !app.storage_open,
        "clicking outside the X but inside its button closes the modal"
    );
    draw(&mut app, vec![egui::Event::PointerMoved(navigation)]);
    draw(
        &mut app,
        vec![
            egui::Event::PointerMoved(navigation),
            pointer(navigation, true),
        ],
    );
    draw(&mut app, vec![pointer(navigation, false)]);
    assert_eq!(
        app.page,
        Page::Verify,
        "background navigation resumes after closing"
    );
}

#[test]
fn storage_window_fits_narrow_and_wide_views() {
    for width in [320.0, 400.0, 1024.0] {
        let ctx = test_context();
        let mut app = FlightApp::new(&eframe::CreationContext::_new_kittest(ctx.clone()));
        app.storage_open = true;
        app.storage_state = Some(json!({"available":true,"rows":[
            {"q":1,"bytes":228524311,"cached_bytes":228524311,"total_bytes":228524311},
            {"q":2,"bytes":4194304,"cached_bytes":4194304,"total_bytes":428391940},
            {"q":3,"bytes":0,"cached_bytes":0,"total_bytes":455938453}
        ]}));
        ctx.style_mut(|style| style.animation_time = 0.0);
        let mut output = None;
        for _ in 0..3 {
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 768.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::TopBottomPanel::top("navigation")
                        .frame(
                            egui::Frame::new()
                                .fill(Color32::WHITE)
                                .inner_margin(egui::Margin::symmetric(24, 18)),
                        )
                        .show(ctx, |ui| app.navigation(ui));
                    egui::CentralPanel::default().show(ctx, |_| {});
                    app.storage_window(ctx);
                },
            ));
        }
        fn texts(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
            match shape {
                egui::Shape::Text(text) => {
                    out.push((text.galley.text().to_owned(), text.visual_bounding_rect()))
                }
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        texts(shape, out);
                    }
                }
                _ => {}
            }
        }
        let output = output.unwrap();
        let mut labels = vec![];
        for shape in &output.shapes {
            texts(&shape.shape, &mut labels);
        }
        for expected in [
            "存储管理",
            "第一问",
            "第二问",
            "第三问",
            "已缓存 228.5 MB",
            "部分缓存 4.2 / 428.4 MB",
            "未缓存",
        ] {
            assert!(
                labels.iter().any(|(text, _)| text == expected),
                "{width}: missing {expected}"
            );
        }
        for (text, rect) in &labels {
            assert!(
                rect.left() >= -1.0 && rect.right() <= width + 1.0,
                "{width}: clipped {text} at {rect:?}"
            );
        }
    }
}
fn proof(q: usize) -> Verified {
    let c = flight_format::Certificate {
        metadata: flight_format::Metadata::new(
            q,
            "甲".into(),
            flight_format::nonce(&[q as u8; 16]),
        ),
        proof: STANDARD.encode([42; 128]),
        scores: [1; 9],
    };
    Verified {
        certificate: serde_json::to_value(flight_format::CertificateSet::new(vec![c]).unwrap())
            .unwrap(),
        scores: vec![vec![1; 9]],
    }
}
#[test]
fn incomplete_question_cannot_silently_disappear_from_export() {
    let mut questions: [Question; 3] = Default::default();
    questions[0].proof = Some(proof(1));
    questions[2].proof = Some(proof(3));
    let item = combine_questions(&questions, "甲").unwrap();
    assert!(item.score_for(1).is_some());
    assert!(item.score_for(2).is_none());
    assert!(item.score_for(3).is_some());
    questions[1].files[0] = CsvFile {
        name: "routes.csv".into(),
        text: "unfinished".into(),
    };
    assert!(combine_questions(&questions, "甲").is_none());
    // An explicit removal allows export of the two retained questions.
    questions[1] = Question::default();
    assert!(combine_questions(&questions, "甲").is_some());
    assert!(combine_questions(&questions, "乙").is_none());
    questions[0].changed();
    questions[0].files[0] = CsvFile {
        name: "new.csv".into(),
        text: "new".into(),
    };
    assert!(questions[2].proof.is_some());
    assert!(combine_questions(&questions, "甲").is_none());
    assert!(combine_questions(&Default::default(), "甲").is_none());
}

#[test]
fn batch_file_read_failures_survive_later_frames_and_successful_imports() {
    let ctx = test_context();
    let mut app = application(None);
    // Hold backend work while delivering one picker selection over two frames.
    app.storage_busy = true;
    app.file_events.lock().unwrap().extend([
        FileEvent::SelectionStarted { slot: 6 },
        FileEvent::LoadError {
            slot: 6,
            name: "oversized.png".into(),
            message: "证书图片不能超过 16 MB".into(),
        },
    ]);
    app.receive(&ctx);
    assert_eq!(app.import_failures.len(), 1);
    app.file_events.lock().unwrap().push(FileEvent::Loaded {
        slot: 6,
        name: "valid.png".into(),
        bytes: vec![],
    });
    app.receive(&ctx);
    assert_eq!(
        app.import_failures.len(),
        1,
        "later files must not reset the batch"
    );
    let pending = app.pending.pop_front().unwrap();
    // As in the recovery tests, this is an explicit synthetic engine result.
    app.accept_import(&pending, proof(1), &ctx).unwrap();
    assert!(app.review.is_some());
    assert_eq!(app.import_failures[0].name, "oversized.png");
    assert!(app.import_failures[0].message.contains("16 MB"));
    app.file_events
        .lock()
        .unwrap()
        .push(FileEvent::SelectionStarted { slot: 6 });
    app.receive(&ctx);
    assert!(
        app.import_failures.is_empty(),
        "a new selection resets old failures once"
    );
}

#[derive(Default)]
struct MemoryStorage {
    values: std::collections::HashMap<String, String>,
    flushes: usize,
}
impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }
    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.to_owned(), value);
    }
    fn flush(&mut self) {
        self.flushes += 1;
    }
}
fn application(storage: Option<&dyn eframe::Storage>) -> FlightApp {
    let mut cc = eframe::CreationContext::_new_kittest(test_context());
    cc.storage = storage;
    FlightApp::new(&cc)
}
fn distinct_proof(q: usize, n: u16, team: &str) -> Verified {
    let mut item = proof(q);
    let mut nonce = [0; 16];
    nonce[..2].copy_from_slice(&n.to_le_bytes());
    item.certificate["certificates"][0]["metadata"]["nonce"] =
        json!(flight_format::nonce(&nonce));
    item.certificate["certificates"][0]["metadata"]["team"] = json!(team);
    item
}
// Synthetic engine responses exercise UI state transitions only. They must
// never be accepted merely by loading storage: every test explicitly sends
// a verification success/failure through the same completion handlers.
fn verification_result(document: &Value) -> Verified {
    let set = flight_format::CertificateSet::from_value(document).unwrap();
    Verified {
        certificate: document.clone(),
        scores: set
            .certificates
            .iter()
            .map(|c| c.scores.iter().map(|v| *v as u64).collect())
            .collect(),
    }
}
fn finish_next_recovery(app: &mut FlightApp) {
    let p = app.pending.pop_front().unwrap();
    let result = verification_result(&p.imported.certificate);
    app.accept_import(&p, result, &test_context()).unwrap();
}
fn empty_batch() -> Batch {
    Batch {
        checks: VecDeque::new(),
        proofs: VecDeque::new(),
    }
}
fn csv_pair(marker: &str) -> [CsvFile; 2] {
    std::array::from_fn(|slot| CsvFile {
        name: format!("PRIVATE-{slot}.csv"),
        text: marker.to_owned(),
    })
}
#[test]
fn legacy_draft_must_not_force_unselected_questions_into_generation() {
    let ctx = test_context();
    let mut storage = MemoryStorage::default();
    // Exact RON stored by the removed draft implementation, including stale selections.
    storage.values.insert(
        LEGACY_DRAFT_KEY.into(),
        r#"(team:"甲",editing_record:None,active:(true,true,true),proofs:(None,None,None))"#
            .into(),
    );
    for mask in 1..=7 {
        let mut app = application(Some(&storage));
        app.team = "甲".into();
        for q in 0..3 {
            if mask & (1 << q) != 0 {
                app.accept_proof(q, proof(q + 1)).unwrap();
            }
        }
        app.generate_certificate(&ctx);
        assert_eq!(app.status, "证书已生成", "question mask {mask}");
        let item = app.ready.as_ref().unwrap();
        for q in 0..3 {
            assert_eq!(item.score_for(q + 1).is_some(), mask & (1 << q) != 0);
        }
    }
}
#[test]
fn csv_selection_generates_exactly_the_selected_question_subset() {
    let ctx = test_context();
    for mask in 0..=7 {
        let mut app = application(None);
        app.team = "甲".into();
        let selected: Vec<_> = (0..3).filter(|q| mask & (1 << q) != 0).collect();
        for &q in &selected {
            for slot in 0..2 {
                app.file_events.lock().unwrap().push(FileEvent::Loaded {
                    slot: q * 2 + slot,
                    name: format!("q{}-{slot}.csv", q + 1),
                    bytes: b"private CSV input".to_vec(),
                });
            }
        }
        app.receive(&ctx);
        app.generate_certificate(&ctx);
        if selected.is_empty() {
            assert!(app.error);
            assert!(app.backend.requests.is_empty());
            assert!(app.ready.is_none());
            continue;
        }
        finish_generation(&mut app, &ctx);
        let requests: Vec<_> = app
            .backend
            .requests
            .iter()
            .map(|r| (r["action"].as_str().unwrap(), r["q"].as_u64().unwrap()))
            .collect();
        let expected: Vec<_> = ["check", "prove"]
            .into_iter()
            .flat_map(|action| selected.iter().map(move |q| (action, (q + 1) as u64)))
            .collect();
        assert_eq!(requests, expected, "question mask {mask}");
        let item = app.ready.as_ref().unwrap();
        for q in 0..3 {
            assert_eq!(item.score_for(q + 1).is_some(), selected.contains(&q));
        }
    }
}
#[test]
fn incomplete_current_input_blocks_generation_until_explicitly_cleared() {
    let ctx = test_context();
    let mut app = application(None);
    app.team = "甲".into();
    app.accept_proof(0, proof(1)).unwrap();
    app.questions[1].files[0] = CsvFile {
        name: "routes.csv".into(),
        text: "unfinished current input".into(),
    };
    app.generate_certificate(&ctx);
    assert!(app.error && app.status.starts_with("第 2 问"));
    assert!(app.ready.is_none());
    assert!(app.backend.requests.is_empty());
    app.questions[1] = Question::default();
    app.invalidate_generated();
    app.generate_certificate(&ctx);
    assert_eq!(app.status, "证书已生成");
    assert!(app.ready.as_ref().unwrap().score_for(1).is_some());
    assert!(app.ready.as_ref().unwrap().score_for(2).is_none());
}
#[test]
fn record_order_survives_repeated_restart_and_in_flight_save() {
    let mut storage = MemoryStorage::default();
    let expected: Vec<_> = [3, 2, 1]
        .map(|n| distinct_proof(1, n, "甲").certificate)
        .to_vec();
    eframe::set_value(&mut storage, SAVE_KEY, &expected);
    for _ in 0..3 {
        let mut app = application(Some(&storage));
        assert!(
            app.records.is_empty(),
            "stored records require verification"
        );
        finish_next_recovery(&mut app);
        assert!(
            !app.checkpoint_pending,
            "restoring unchanged records does not rewrite storage"
        );
        let p = app.pending.pop_front().unwrap();
        app.task = Some(Task {
            kind: Kind::Import,
            import: Some(p),
        });
        app.save_state(&mut storage);
        assert_eq!(
            eframe::get_value::<Vec<Value>>(&storage, SAVE_KEY).unwrap(),
            expected
        );
        app.cancel(&test_context());
        app.flush_checkpoint(&mut storage);
        assert_eq!(
            eframe::get_value::<Vec<Value>>(&storage, SAVE_KEY).unwrap(),
            expected
        );
        while !app.pending.is_empty() {
            finish_next_recovery(&mut app);
        }
        assert_eq!(
            app.records
                .iter()
                .map(|r| r.certificate.clone())
                .collect::<Vec<_>>(),
            expected
        );
        app.save_state(&mut storage);
    }
}
#[test]
fn record_limit_keeps_newest_after_restoring_and_adding() {
    let mut storage = MemoryStorage::default();
    let expected: Vec<_> = (1..=MAX_RECORDS as u16)
        .rev()
        .map(|n| distinct_proof(1, n, "甲").certificate)
        .collect();
    eframe::set_value(&mut storage, SAVE_KEY, &expected);
    let mut app = application(Some(&storage));
    while !app.pending.is_empty() {
        finish_next_recovery(&mut app);
    }
    let newest = distinct_proof(1, 201, "甲");
    app.add_record(newest.clone());
    assert_eq!(app.records.len(), MAX_RECORDS);
    assert_eq!(app.records[0].id(), newest.id());
    assert_eq!(
        app.records.last().unwrap().id(),
        distinct_proof(1, 2, "甲").id()
    );
    let saved = app.stored_records();
    assert_eq!(saved[1], expected[0]);
    assert!(!saved.contains(expected.last().unwrap()));
}
#[test]
fn completed_questions_are_reused_after_cancel_or_failure_in_the_same_session() {
    let ctx = test_context();
    for cancel in [true, false] {
        let mut app = application(None);
        let original = distinct_proof(1, 10, "甲");
        app.add_record(original.clone());
        app.use_certificate(&original).unwrap();
        app.questions[0].changed();
        app.questions[0].files = csv_pair("PRIVATE REPLACEMENT Q1");
        app.questions[1].files = csv_pair("PRIVATE Q2");
        let completed = distinct_proof(1, 11, "甲");
        app.accept_proof(0, completed.clone()).unwrap();
        app.invalidate_generated();
        app.generate_certificate(&ctx);
        app.backend.reply(
            json!({"type":"result","result":{"scores":vec![vec![1u64;9]],"canProve":true}}),
        );
        app.receive(&ctx);
        assert!(matches!(app.task.as_ref().unwrap().kind, Kind::Prove(1)));
        if cancel {
            app.cancel(&ctx);
        } else {
            app.backend
                .reply(json!({"type":"error","message":"test engine failure"}));
            app.receive(&ctx);
        }
        assert!(!app.busy());
        assert!(app.ready.is_none());
        assert_eq!(
            app.questions[0].proof.as_ref().unwrap().id(),
            completed.id()
        );
        assert_eq!(app.records[0].id(), original.id());
        app.generate_certificate(&ctx);
        finish_generation(&mut app, &ctx);
        assert!(app.backend.requests.iter().all(|r| r["q"] == 2));
        assert_eq!(app.records.len(), 1);
        assert_ne!(app.records[0].id(), original.id());
        assert!(app.records[0].score_for(1).is_some());
        assert!(app.records[0].score_for(2).is_some());
        assert!(app.records[0].score_for(3).is_none());
    }
}
#[test]
fn restarting_restores_only_completed_records_and_an_empty_generation_form() {
    let mut storage = MemoryStorage::default();
    let mut app = application(None);
    let record = distinct_proof(3, 20, "甲");
    app.add_record(record.clone());
    app.use_certificate(&record).unwrap();
    app.questions[0].files = csv_pair("SECRET CSV CONTENTS");
    app.accept_proof(0, distinct_proof(1, 21, "甲")).unwrap();
    app.questions[1].files = csv_pair("SECRET INCOMPLETE GENERATION");
    app.save_state(&mut storage);
    let serialized = storage.values.values().cloned().collect::<String>();
    assert!(!serialized.contains("SECRET"));
    assert!(!serialized.contains("PRIVATE"));
    assert_eq!(storage.values[LEGACY_DRAFT_KEY], "");
    let mut reopened = application(Some(&storage));
    assert!(reopened.team.is_empty());
    assert!(reopened.editing_record.is_none());
    assert!(reopened.ready.is_none());
    assert!(reopened.questions.iter().all(|q| !q.active()));
    assert!(
        reopened.records.is_empty(),
        "saved records must be verified again"
    );
    assert_eq!(reopened.pending.len(), 1);
    finish_next_recovery(&mut reopened);
    assert_eq!(reopened.records[0].id(), record.id());
    assert!(reopened.questions.iter().all(|q| !q.active()));
}
#[test]
fn renaming_keeps_original_and_rejects_old_team_proofs() {
    let mut app = application(None);
    let original = distinct_proof(1, 50, "甲");
    app.add_record(original.clone());
    app.use_certificate(&original).unwrap();
    app.team = "乙".into();
    app.invalidate_generated();
    assert!(app.accept_proof(0, original.clone()).is_err());
    let renamed = distinct_proof(1, 51, "乙");
    app.accept_proof(0, renamed.clone()).unwrap();
    app.batch = Some(empty_batch());
    app.next_batch(&test_context());
    assert_eq!(app.records.len(), 2);
    assert_eq!(app.records[0].id(), renamed.id());
    assert_eq!(app.records[1].id(), original.id());
}
#[test]
fn successful_replacement_also_removes_original_waiting_for_reverification() {
    let ctx = test_context();
    let mut app = application(None);
    let original = distinct_proof(1, 60, "甲");
    app.team = "甲".into();
    app.editing_record = Some(original.id());
    app.pending.push_back(Pending {
        imported: Imported {
            certificate: original.certificate,
            original: None,
        },
        name: "已保存证书".into(),
        source: ImportSource::Record,
    });
    app.imports_paused = true;
    let completed = distinct_proof(1, 61, "甲");
    app.accept_proof(0, completed.clone()).unwrap();
    app.batch = Some(empty_batch());
    app.next_batch(&ctx);
    assert!(app.pending.is_empty());
    assert_eq!(app.stored_records(), vec![completed.certificate]);
}
