use figcraft_engine::Session;
use serde_json::{Value, json};

/// A 39-step chat mockup from gnoviawan/termul#1424, verbatim. Its parent is `"<MAIN>"`.
const ISSUE_1424: &str = include_str!("../fixtures/issue_1424_batch.json");

fn main_frame(s: &mut Session) -> Value {
    s.execute("node.create", &json!({"type": "frame", "width": 912, "height": 630, "props": {"layout": "vertical", "gap": 1}})).unwrap()["id"].clone()
}

fn issue_1424_steps(main: &Value) -> Value {
    let mut steps: Value = serde_json::from_str(ISSUE_1424).unwrap();
    for step in steps.as_array_mut().unwrap() {
        if step["params"]["parent"] == "<MAIN>" {
            step["params"]["parent"] = main.clone();
        }
    }
    steps
}

#[test]
fn a_failing_step_is_named_in_the_error() {
    let mut s = Session::new();
    let err = s
        .execute(
            "command.batch",
            &json!({"commands": [
                {"command": "node.create", "params": {"type": "frame", "width": 10, "height": 10}},
                {"command": "no.such.command", "params": {}}
            ]}),
        )
        .unwrap_err();
    assert_eq!(err.to_string(), "step 1 (no.such.command): unknown command `no.such.command`");
}

#[test]
fn the_issue_1424_batch_names_the_step_whose_parent_has_no_auto_layout() {
    let mut s = Session::new();
    let main = main_frame(&mut s);
    let err = s.execute("command.batch", &json!({"commands": issue_1424_steps(&main)})).unwrap_err().to_string();
    assert!(err.contains("step 35 (node.create \"Placeholder\"): "), "{err}");
    assert!(err.contains("Fill needs an auto layout parent"), "{err}");
    let main = figcraft_engine::params::id_of(&main).unwrap();
    assert!(s.doc().unwrap().node(main).unwrap().children.is_empty(), "the batch rolled back");
}

/// The batch's last four steps point one step too early: at "Flex spacer" (33) instead of
/// "Composer" (34), and at "Model" (36) instead of "Send" (37). With those fixed it builds.
#[test]
fn the_issue_1424_batch_builds_with_its_composer_references_fixed() {
    let mut s = Session::new();
    let main = main_frame(&mut s);
    let mut steps = issue_1424_steps(&main);
    for (step, parent) in [(35, 34), (36, 34), (37, 34), (38, 37)] {
        steps[step]["params"]["parent"] = json!({"$step": parent});
    }
    let results = s.execute("command.batch", &json!({"commands": steps})).unwrap();
    let id = |step: usize| figcraft_engine::params::id_of(&results[step]["id"]).unwrap();
    let doc = s.doc().unwrap();
    let composer = doc.node(id(34)).unwrap();
    assert_eq!(composer.name, "Composer");
    assert_eq!(composer.children, [id(35), id(36), id(37)]);
    assert_eq!(doc.node(id(38)).unwrap().parent, Some(id(37)));
}

#[test]
fn a_failing_named_step_is_named_by_its_as_and_its_layer_name() {
    let mut s = Session::new();
    let err = s
        .execute(
            "command.batch",
            &json!({"commands": [
                {"command": "node.create", "params": {"type": "frame", "width": 10, "height": 10}, "as": "hero"},
                {"command": "node.create", "params": {"type": "text", "name": "Hero Title", "parent": {"$name": "hero"}, "text": "Hi", "props": {"sizingH": "fill"}}, "as": "hero-title"}
            ]}),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("step 1 \"hero-title\" (node.create \"Hero Title\"): "), "{err}");
    assert!(err.contains("Fill needs an auto layout parent"), "{err}");
}

/// A reference that doesn't resolve names its step the way a failed command does
/// (gnoviawan/termul#1424): the index, the `as` name, the command and the layer name. The error
/// stays `command.batch`'s. These are the cases figcraft#13's verifier ran, with `$name` and `$ref`.
#[test]
fn a_reference_that_does_not_resolve_names_its_step() {
    let frame = |name: &str, r#as: Option<&str>, parent: Option<Value>| {
        let mut step = json!({"command": "node.create", "params": {"type": "frame", "name": name}});
        if let Some(r#as) = r#as {
            step["as"] = json!(r#as);
        }
        if let Some(parent) = parent {
            step["params"]["parent"] = parent;
        }
        step
    };
    let set = |r: Value| json!({"command": "node.set", "params": {"ids": [r], "props": {"name": "x"}}});
    for key in ["$name", "$ref"] {
        let r = |name: Value| json!({key: name});
        let cases = [
            (
                json!([frame("A", Some("a"), None), frame("B", None, Some(r(json!("zz"))))]),
                format!("step 1 (node.create \"B\"): `{key}` \"zz\" names no step (\"a\" is step 0)"),
            ),
            (
                json!([frame("A", None, None), frame("B", None, Some(r(json!("a"))))]),
                format!("step 1 (node.create \"B\"): `{key}` \"a\" names no step (no step has an `as` name)"),
            ),
            (
                json!([frame("A", Some("a"), Some(r(json!("b")))), frame("B", Some("b"), None)]),
                format!("step 0 \"a\" (node.create \"A\"): `{key}` \"b\" is step 1, not an earlier step"),
            ),
            (
                json!([frame("A", Some("a"), Some(r(json!("a"))))]),
                format!("step 0 \"a\" (node.create \"A\"): `{key}` \"a\" is step 0, not an earlier step"),
            ),
            (json!([frame("A", Some("a"), None), set(r(json!(0)))]), format!("step 1 (node.set): `{key}` must be a step name (a string)")),
            (
                json!([frame("A", Some("a"), None), set(json!({key: "a", "extra": true}))]),
                format!("step 1 (node.set): a `{key}` reference takes only `field`, not `extra`"),
            ),
            (
                json!([frame("A", Some("a"), None), set(json!({key: "a", "field": "nope"}))]),
                "step 1 (node.set): step 0 (\"a\")'s result has no `nope`".into(),
            ),
        ];
        for (commands, msg) in cases {
            let err = Session::new().execute("command.batch", &json!({"commands": commands})).unwrap_err().to_string();
            assert_eq!(err, format!("invalid parameters for `command.batch`: {msg}"));
        }
    }
    let err =
        Session::new().execute("command.batch", &json!({"commands": [frame("A", None, None), set(json!({"$step": 3}))]})).unwrap_err().to_string();
    assert_eq!(err, "invalid parameters for `command.batch`: step 1 (node.set): `$step` 3 must be an earlier step (0 to 0)");
}

/// A failing nested batch names the outer step as well as its own.
#[test]
fn a_failing_nested_batch_names_the_outer_step_too() {
    let err = Session::new()
        .execute(
            "command.batch",
            &json!({"commands": [
                {"command": "node.create", "as": "a", "params": {"type": "frame", "name": "A"}},
                {"command": "command.batch", "as": "inner", "params": {"commands": [
                    {"command": "node.create", "params": {"type": "frame", "name": "I"}},
                    {"command": "no.such.command", "params": {}}
                ]}}
            ]}),
        )
        .unwrap_err()
        .to_string();
    assert_eq!(err, "step 1 \"inner\" (command.batch): step 1 (no.such.command): unknown command `no.such.command`");
}
