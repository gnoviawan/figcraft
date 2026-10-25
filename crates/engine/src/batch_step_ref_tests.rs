use serde_json::json;

use crate::{EngineError, Session};

fn id(value: &serde_json::Value) -> crate::NodeId {
    crate::params::id_of(&value["id"]).unwrap()
}

#[test]
fn later_steps_target_layers_created_by_earlier_steps() {
    let mut s = Session::new();
    let results = s
        .execute(
            "command.batch",
            &json!({
                "label": "Card",
                "commands": [
                    {"command": "node.create", "params": {"type": "frame", "x": 0, "y": 0, "width": 200, "height": 100}},
                    {"command": "node.create", "params": {"type": "rectangle", "parent": {"$step": 0}, "x": 10, "y": 10, "width": 20, "height": 20}},
                    {"command": "node.set", "params": {"ids": [{"$step": 1}], "props": {"name": "Swatch"}}}
                ]
            }),
        )
        .unwrap();

    let frame = id(&results[0]);
    let rect = id(&results[1]);
    let doc = s.doc().unwrap();
    assert_eq!(doc.node(rect).unwrap().parent, Some(frame));
    assert_eq!(doc.node(rect).unwrap().name, "Swatch");
    assert_eq!(s.ds().unwrap().history.undo.len(), 1, "still one undo step");
}

#[test]
fn field_picks_another_part_of_the_result() {
    let mut s = Session::new();
    let results = s
        .execute(
            "command.batch",
            &json!({"commands": [
                {"command": "node.create", "params": {"type": "frame", "width": 50, "height": 50}},
                {"command": "node.create", "params": {"type": "text", "text": "hi", "parent": {"$step": 0, "field": "id"}}}
            ]}),
        )
        .unwrap();
    assert_eq!(s.doc().unwrap().node(id(&results[1])).unwrap().parent, Some(id(&results[0])));
}

#[test]
fn bad_references_fail_and_roll_back() {
    let mut s = Session::new();
    let before = s.doc().unwrap().node(s.page().unwrap()).unwrap().children.len();
    let frame = json!({"command": "node.create", "params": {"type": "frame", "width": 10, "height": 10}});
    for (bad, needle) in [
        (json!({"$step": 1}), "earlier step"),
        (json!({"$step": 5}), "earlier step"),
        (json!({"$step": -1}), "step index"),
        (json!({"$step": "0"}), "step index"),
        (json!({"$step": 0, "field": "nope"}), "no `nope`"),
        (json!({"$step": 0, "field": 3}), "`field` must be a string"),
        (json!({"$step": 0, "extra": true}), "not `extra`"),
    ] {
        let r = s.execute("command.batch", &json!({"commands": [frame, {"command": "node.set", "params": {"ids": [bad], "props": {"name": "x"}}}]}));
        match r {
            Err(EngineError::Step { step: 1, source, .. }) => match *source {
                EngineError::BadParams { msg, .. } => assert!(msg.contains(needle), "{msg} should mention {needle}"),
                other => panic!("expected BadParams for {bad}, got {other:?}"),
            },
            other => panic!("expected BadParams for {bad}, got {other:?}"),
        }
        assert_eq!(s.doc().unwrap().node(s.page().unwrap()).unwrap().children.len(), before, "rolled back");
    }
}

#[test]
fn deep_params_are_refused_not_overflowed() {
    let mut s = Session::new();
    let mut deep = json!(1);
    for _ in 0..200 {
        deep = json!([deep]);
    }
    let r = s.execute("command.batch", &json!({"commands": [{"command": "select.none", "params": {"x": deep}}]}));
    assert!(matches!(&r, Err(EngineError::Step { step: 0, source, .. }) if matches!(**source, EngineError::BadParams { .. })), "{r:?}");
}

#[test]
fn text_that_looks_like_a_reference_is_left_alone() {
    let mut s = Session::new();
    let results = s
        .execute("command.batch", &json!({"commands": [{"command": "node.create", "params": {"type": "text", "text": "$step 0 {\"$step\": 0}"}}]}))
        .unwrap();
    let n = s.doc().unwrap().node(id(&results[0])).unwrap().clone();
    assert!(matches!(n.kind, figcraft_doc::NodeKind::Text(t) if t.characters == "$step 0 {\"$step\": 0}"));
}

#[test]
fn a_nested_batch_refers_to_its_own_steps() {
    let mut s = Session::new();
    let frame = |name: &str| json!({"command": "node.create", "params": {"type": "frame", "name": name, "width": 100, "height": 100}});
    let results = s
        .execute(
            "command.batch",
            &json!({"commands": [
                frame("Outer"),
                {"command": "command.batch", "params": {"commands": [
                    frame("Inner"),
                    {"command": "node.create", "params": {"type": "rectangle", "parent": {"$step": 0}, "width": 10, "height": 10}}
                ]}}
            ]}),
        )
        .unwrap();
    let inner = &results[1];
    let rect = s.doc().unwrap().node(id(&inner[1])).unwrap().clone();
    assert_eq!(rect.parent, Some(id(&inner[0])), "in the nested batch's own first frame, not the outer one");
}

fn frame(name: &str) -> serde_json::Value {
    json!({"command": "node.create", "params": {"type": "frame", "name": name, "width": 100, "height": 100}})
}

fn named(step: serde_json::Value, name: &str) -> serde_json::Value {
    let mut step = step;
    step["as"] = json!(name);
    step
}

/// The `BadParams` message of a batch that failed at `step`, after checking it rolled back.
fn failed_at(s: &mut Session, step: usize, commands: serde_json::Value) -> String {
    let before = s.doc().unwrap().node(s.page().unwrap()).unwrap().children.len();
    let r = s.execute("command.batch", &json!({"commands": commands}));
    assert_eq!(s.doc().unwrap().node(s.page().unwrap()).unwrap().children.len(), before, "rolled back");
    match r {
        Err(EngineError::Step { step: at, source, .. }) if at == step => match *source {
            EngineError::BadParams { msg, .. } => msg,
            other => panic!("expected BadParams at step {step}, got {other:?}"),
        },
        other => panic!("expected a failure at step {step}, got {other:?}"),
    }
}

#[test]
fn named_steps_are_referred_to_by_name_and_listed_in_the_result() {
    let mut s = Session::new();
    let out = s
        .execute(
            "command.batch",
            &json!({"commands": [
                named(frame("Card"), "card"),
                named(json!({"command": "node.create", "params": {"type": "text", "text": "Hi", "parent": {"$name": "card"}}}), "title"),
                {"command": "node.set", "params": {"ids": [{"$name": "title"}], "props": {"name": "Title"}}}
            ]}),
        )
        .unwrap();

    let results = out["results"].as_array().unwrap();
    assert_eq!(results.len(), 3);
    assert_eq!(out["names"], json!({"card": results[0]["id"], "title": results[1]["id"]}));
    let doc = s.doc().unwrap();
    let title = doc.node(id(&results[1])).unwrap();
    assert_eq!(title.parent, Some(id(&results[0])));
    assert_eq!(title.name, "Title");
    assert_eq!(s.ds().unwrap().history.undo.len(), 1, "still one undo step");
}

#[test]
fn a_name_reference_takes_field_like_a_step_reference() {
    let mut s = Session::new();
    let out = s
        .execute(
            "command.batch",
            &json!({"commands": [
                named(json!({"command": "node.create", "params": {"type": "component", "name": "Card", "width": 100, "height": 100}}), "card"),
                named(json!({"command": "node.create", "params": {"type": "text", "text": "Starter", "parent": {"$name": "card"}}}), "plan"),
                named(json!({"command": "component.addProperty", "params": {"id": {"$name": "card"}, "name": "Plan", "type": "text", "layer": {"$name": "plan"}}}), "plan-prop"),
                {"command": "node.create", "params": {"type": "text", "text": {"$name": "plan-prop", "field": "name"}}}
            ]}),
        )
        .unwrap();
    let label = s.doc().unwrap().node(id(&out["results"][3])).unwrap().clone();
    assert!(matches!(&label.kind, figcraft_doc::NodeKind::Text(t) if t.characters == out["results"][2]["name"]), "{:?}", label.kind);
}

#[test]
fn names_and_step_indexes_mix_in_one_batch() {
    let mut s = Session::new();
    let out = s
        .execute(
            "command.batch",
            &json!({"commands": [
                named(frame("Outer"), "outer"),
                frame("Unnamed"),
                {"command": "node.create", "params": {"type": "rectangle", "parent": {"$step": 0}, "width": 10, "height": 10}},
                {"command": "node.create", "params": {"type": "rectangle", "parent": {"$name": "outer"}, "width": 10, "height": 10}},
                {"command": "node.reparent", "params": {"ids": [{"$step": 1}], "parent": {"$name": "outer"}}}
            ]}),
        )
        .unwrap();
    let r = out["results"].as_array().unwrap();
    let doc = s.doc().unwrap();
    for child in [&r[1], &r[2], &r[3]] {
        assert_eq!(doc.node(id(child)).unwrap().parent, Some(id(&r[0])));
    }
    assert_eq!(out["names"], json!({"outer": r[0]["id"]}), "only named steps are listed");
}

#[test]
fn a_batch_without_names_still_returns_a_plain_array() {
    let mut s = Session::new();
    let out = s.execute("command.batch", &json!({"commands": [frame("A"), frame("B")]})).unwrap();
    assert_eq!(out.as_array().map(Vec::len), Some(2), "{out}");
}

#[test]
fn a_name_whose_step_returns_no_id_maps_to_null() {
    let mut s = Session::new();
    let out = s
        .execute(
            "command.batch",
            &json!({"commands": [frame("A"), named(json!({"command": "node.set", "params": {"ids": [{"$step": 0}], "props": {"name": "B"}}}), "rename")]}),
        )
        .unwrap();
    assert_eq!(out["names"], json!({"rename": null}));
}

#[test]
fn a_duplicate_name_fails_before_any_step_runs() {
    let mut s = Session::new();
    let fails_when_run = json!({"command": "node.set", "params": {"ids": [99999], "props": {"name": "x"}}});
    let msg = failed_at(&mut s, 2, json!([named(frame("A"), "card"), fails_when_run, named(frame("C"), "card")]));
    assert!(msg.contains("\"card\"") && msg.contains("step 0"), "{msg}");
}

#[test]
fn bad_names_and_name_references_fail_at_their_step_and_roll_back() {
    let mut s = Session::new();
    let set = |r: serde_json::Value| json!({"command": "node.set", "params": {"ids": [r], "props": {"name": "x"}}});
    for (commands, step, needle) in [
        (json!([named(frame("A"), "a"), set(json!({"$name": "later"})), named(frame("B"), "later")]), 1, "earlier step"),
        (json!([named(set(json!({"$name": "me"})), "me")]), 0, "earlier step"),
        (json!([named(frame("A"), "a"), named(frame("B"), "b"), set(json!({"$name": "nope"}))]), 2, "the names in this batch are \"a\", \"b\""),
        (json!([frame("A"), set(json!({"$name": "a"}))]), 1, "no step of this batch has a name"),
        (json!([named(frame("A"), "a"), set(json!({"$name": 0}))]), 1, "`$name` must be"),
        (json!([named(frame("A"), "a"), set(json!({"$name": "a", "field": "nope"}))]), 1, "no `nope`"),
        (json!([named(frame("A"), "a"), set(json!({"$name": "a", "extra": true}))]), 1, "not `extra`"),
        (json!([named(frame("A"), "a"), set(json!({"$name": "a", "$step": 0}))]), 1, "not `$name`"),
        (json!([named(frame("A"), "")]), 0, "`as` must be"),
        (json!([frame("A"), named(frame("B"), "b"), {"command": "node.create", "params": {"type": "frame"}, "as": 3}]), 2, "`as` must be"),
    ] {
        let msg = failed_at(&mut s, step, commands.clone());
        assert!(msg.contains(needle), "{commands}: {msg} should mention {needle}");
    }
}

#[test]
fn text_that_looks_like_a_name_reference_is_left_alone() {
    let mut s = Session::new();
    let text = "{\"$name\": \"card\"}";
    let out = s
        .execute("command.batch", &json!({"commands": [named(json!({"command": "node.create", "params": {"type": "text", "text": text}}), "card")]}))
        .unwrap();
    let n = s.doc().unwrap().node(id(&out["results"][0])).unwrap().clone();
    assert!(matches!(n.kind, figcraft_doc::NodeKind::Text(t) if t.characters == text));
}

#[test]
fn a_nested_batch_has_its_own_names() {
    let mut s = Session::new();
    let out = s
        .execute(
            "command.batch",
            &json!({"commands": [
                named(frame("Outer"), "box"),
                named(json!({"command": "command.batch", "params": {"commands": [
                    named(frame("Inner"), "box"),
                    {"command": "node.create", "params": {"type": "rectangle", "parent": {"$name": "box"}, "width": 10, "height": 10}}
                ]}}), "inner")
            ]}),
        )
        .unwrap();
    let inner = &out["results"][1];
    let rect = s.doc().unwrap().node(id(&inner["results"][1])).unwrap().clone();
    assert_eq!(rect.parent, Some(id(&inner["results"][0])), "in the nested batch's own box, not the outer one");
    assert_eq!(out["names"], json!({"box": out["results"][0]["id"], "inner": null}));
}

#[test]
fn an_unknown_name_lists_at_most_twenty_names() {
    let mut s = Session::new();
    let mut commands: Vec<serde_json::Value> = (0..25).map(|i| named(frame("F"), &format!("f{i}"))).collect();
    commands.push(json!({"command": "node.set", "params": {"ids": [{"$name": "nope"}], "props": {"name": "x"}}}));
    let msg = failed_at(&mut s, 25, json!(commands));
    assert!(msg.contains("\"f19\", …") && !msg.contains("\"f20\""), "{msg}");
}
