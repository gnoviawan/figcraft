#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use figcraft_engine::Session;
use serde_json::{Value, json};

/// A 39-step chat mockup from gnoviawan/termul#1424, verbatim. Its parent is `"<MAIN>"`.
const ISSUE_1424: &str = include_str!("fixtures/issue_1424_batch.json");

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
    assert!(err.starts_with("step 35 (node.create \"Placeholder\"): "), "{err}");
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
