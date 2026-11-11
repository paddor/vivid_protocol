//! Strict document editing codec and stale-range validation.

// Rust guideline compliant 2026-10-07
use vivid_protocol::{
    cbor::Value,
    overlay::wire::{
        WindowAddress,
        text::editor::{Context, Edit, MAX_TEXT_BYTES, SetContext},
    },
};

fn context() -> Context {
    Context {
        editor: 3,
        revision: 9,
        text: "a😀éz".into(),
        cursor: 5,
        anchor: 7,
        sensitive: false,
    }
}
fn edit() -> Edit {
    Edit {
        address: WindowAddress {
            context_id: 2,
            surface_id: 7,
            generation: 1,
        },
        scene_revision: 4,
        editor: 3,
        revision: 9,
        replacement: Some(1..7),
        selection: None,
        commit: Some("好".into()),
        preedit: None,
    }
}
#[test]
fn strict_roundtrip_and_private_debug() {
    let edit = edit();
    let context = context();
    let request = SetContext {
        address: edit.address,
        scene_revision: 4,
        context: Some(context.clone()),
    };
    assert_eq!(
        SetContext::decode(7, &Value::Map(request.payload().unwrap())).unwrap(),
        request
    );
    assert_eq!(
        Edit::decode(7, &Value::Map(edit.payload().unwrap())).unwrap(),
        edit
    );
    edit.validate_context(&context).unwrap();
    assert!(!format!("{request:?}").contains(&context.text));
    assert!(!format!("{edit:?}").contains("好"));
    let mut payload = edit.payload().unwrap();
    payload.push((10, Value::Null));
    Edit::decode(7, &Value::Map(payload)).unwrap_err();
    Edit::decode(8, &Value::Map(edit.payload().unwrap())).unwrap_err();
}
#[test]
fn absolute_ranges_require_current_selection_and_utf8_boundaries() {
    let context = context();
    let mut edit = edit();
    edit.replacement = Some(2..7);
    edit.validate_context(&context).unwrap_err();
    edit.replacement = Some(1..5);
    edit.validate_context(&context).unwrap_err();
    edit.replacement = Some(1..7);
    edit.revision -= 1;
    edit.validate_context(&context).unwrap_err();
    edit.replacement = None;
    edit.validate_context(&context).unwrap();
    edit.revision = context.revision + 1;
    edit.validate_context(&context).unwrap_err();
    edit.revision = context.revision;
    edit.editor -= 1;
    edit.validate_context(&context).unwrap_err();
    edit.editor = context.editor;
    edit.commit = None;
    edit.selection = Some((7, 1));
    edit.validate_context(&context).unwrap();
    edit.selection = Some((3, 1));
    edit.validate_context(&context).unwrap_err();
}
#[test]
fn sensitive_context_and_text_limits_are_enforced() {
    let mut context = context();
    context.sensitive = true;
    context.validate().unwrap_err();
    context.text.clear();
    context.cursor = 0;
    context.anchor = 0;
    context.validate().unwrap();
    let mut edit = edit();
    edit.replacement = Some(0..0);
    edit.validate_context(&context).unwrap_err();
    edit.replacement = None;
    edit.validate_context(&context).unwrap();
    edit.commit = Some("x".repeat(MAX_TEXT_BYTES + 1));
    edit.validate().unwrap_err();
    edit.commit = None;
    edit.preedit = Some(("😀".into(), Some(1..4)));
    edit.validate().unwrap_err();
    edit.preedit = Some(("😀".into(), Some(0..4)));
    edit.validate().unwrap();
    context.text = "x".repeat(MAX_TEXT_BYTES + 1);
    context.sensitive = false;
    context.validate().unwrap_err();
}
