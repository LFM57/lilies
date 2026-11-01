#![deny(unsafe_code)]

use crate::runtime::{camel_to_kebab, get_inline_style_prop, update_inline_style, JsRuntime};
use lilies_dom::Document;
use lilies_html::parse_html;

#[test]
fn test_eval_expressions() {
    let doc = Document::new();
    let mut runtime = JsRuntime::new(doc);

    let result = runtime.eval("1 + 2").expect("eval failed");
    assert_eq!(result.as_number(), Some(3.0));

    let str_result = runtime.eval("'hello' + ' ' + 'world'").expect("eval failed");
    assert_eq!(
        str_result.as_string().map(|s| s.to_std_string_escaped()),
        Some("hello world".to_string())
    );
}

#[test]
fn test_document_get_element_by_id_and_text_content() {
    let html = r#"<html><body><h1 id="heading">Static Heading</h1></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let initial_eval = runtime
        .eval(
            r#"
        var heading = document.getElementById("heading");
        heading.textContent;
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        initial_eval.as_string().map(|s| s.to_std_string_escaped()),
        Some("Static Heading".to_string())
    );

    // Now mutate textContent
    runtime
        .eval(
            r#"
        heading.textContent = "Dynamic Heading (Set by JS)";
    "#,
        )
        .expect("eval failed");

    assert!(runtime.is_dirty(), "Runtime should be marked dirty after textContent mutation");

    // Verify DOM arena was updated
    let heading_id = runtime.find_element_by_id("heading").expect("heading node must exist");
    assert_eq!(
        runtime.doc().text_content(heading_id),
        "Dynamic Heading (Set by JS)"
    );

    // Verify getter returns updated value
    let updated_eval = runtime.eval("heading.textContent;").expect("eval failed");
    assert_eq!(
        updated_eval.as_string().map(|s| s.to_std_string_escaped()),
        Some("Dynamic Heading (Set by JS)".to_string())
    );

    // Non-existent element returns null
    let null_eval = runtime
        .eval("document.getElementById('non_existent');")
        .expect("eval failed");
    assert!(null_eval.is_null());
}

#[test]
fn test_style_mutation_via_js() {
    let html = r#"<html><body><button id="btn" style="background-color: #3b82f6; color: white;">Button</button></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    // Check reading initial style properties
    let color_eval = runtime
        .eval(
            r#"
        var btn = document.getElementById("btn");
        btn.style.color;
    "#,
        )
        .expect("eval failed");
    assert_eq!(
        color_eval.as_string().map(|s| s.to_std_string_escaped()),
        Some("white".to_string())
    );

    // Mutate color and backgroundColor
    runtime
        .eval(
            r##"
        btn.style.color = "red";
        btn.style.backgroundColor = "#ff0000";
    "##,
        )
        .expect("eval failed");

    assert!(runtime.is_dirty(), "Runtime should be marked dirty after style mutation");

    // Check DOM arena inline style attribute
    let btn_id = runtime.find_element_by_id("btn").expect("btn node must exist");
    let doc = runtime.doc();
    let style_attr = doc
        .node(btn_id)
        .and_then(|n| n.as_element())
        .and_then(|el| el.get_attribute("style"))
        .expect("style attribute must exist");

    assert!(style_attr.contains("color: red"), "Style must contain color: red: {style_attr}");
    assert!(
        style_attr.contains("background-color: #ff0000"),
        "Style must contain background-color: #ff0000: {style_attr}"
    );

    drop(doc);

    // Reading style property back through JS
    let new_color = runtime.eval("btn.style.color;").expect("eval failed");
    assert_eq!(
        new_color.as_string().map(|s| s.to_std_string_escaped()),
        Some("red".to_string())
    );

    let new_bg = runtime.eval("btn.style.backgroundColor;").expect("eval failed");
    assert_eq!(
        new_bg.as_string().map(|s| s.to_std_string_escaped()),
        Some("#ff0000".to_string())
    );
}

#[test]
fn test_add_event_listener_and_dispatch_click() {
    let html = r#"<html><body>
        <p id="counter-text">Count: 0</p>
        <button id="btn">Click Me!</button>
    </body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    runtime
        .eval(
            r#"
        var count = 0;
        var btn = document.getElementById("btn");
        var counter = document.getElementById("counter-text");
        btn.addEventListener("click", function() {
            count = count + 1;
            counter.textContent = "Count: " + count;
        });
    "#,
        )
        .expect("eval failed");

    let btn_id = runtime.find_element_by_id("btn").expect("btn node must exist");
    let counter_id = runtime.find_element_by_id("counter-text").expect("counter node must exist");

    assert_eq!(runtime.doc().text_content(counter_id), "Count: 0");

    // Reset dirty flag before dispatch
    runtime.reset_dirty();
    assert!(!runtime.is_dirty());

    // Dispatch first click
    runtime.dispatch_click(btn_id).expect("dispatch_click failed");
    assert!(runtime.is_dirty(), "Runtime should be marked dirty after click handler mutated DOM");
    assert_eq!(runtime.doc().text_content(counter_id), "Count: 1");

    // Dispatch second click
    runtime.reset_dirty();
    runtime.dispatch_click(btn_id).expect("dispatch_click failed");
    assert!(runtime.is_dirty());
    assert_eq!(runtime.doc().text_content(counter_id), "Count: 2");

    // Dispatch third click
    runtime.reset_dirty();
    runtime.dispatch_click(btn_id).expect("dispatch_click failed");
    assert!(runtime.is_dirty());
    assert_eq!(runtime.doc().text_content(counter_id), "Count: 3");
}

#[test]
fn test_set_attribute_and_get_attribute() {
    let html = r#"<html><body><div id="box"></div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    runtime
        .eval(
            r#"
        var box = document.getElementById("box");
        box.setAttribute("data-test", "val123");
    "#,
        )
        .expect("eval failed");

    assert!(runtime.is_dirty());
    let box_id = runtime.find_element_by_id("box").expect("box must exist");
    assert_eq!(
        runtime.doc().node(box_id).unwrap().as_element().unwrap().get_attribute("data-test"),
        Some("val123")
    );

    let get_attr_eval = runtime.eval("box.getAttribute('data-test');").expect("eval failed");
    assert_eq!(
        get_attr_eval.as_string().map(|s| s.to_std_string_escaped()),
        Some("val123".to_string())
    );
}

#[test]
fn test_helpers_camel_to_kebab_and_style_helpers() {
    assert_eq!(camel_to_kebab("color"), "color");
    assert_eq!(camel_to_kebab("backgroundColor"), "background-color");
    assert_eq!(camel_to_kebab("fontSize"), "font-size");
    assert_eq!(camel_to_kebab("background-color"), "background-color");

    let initial = "color: blue; padding: 10px;";
    let updated = update_inline_style(Some(initial), "color", "red");
    assert!(updated.contains("color: red;"));
    assert!(updated.contains("padding: 10px;"));

    assert_eq!(get_inline_style_prop(Some(&updated), "color"), Some("red".to_string()));
    assert_eq!(get_inline_style_prop(Some(&updated), "padding"), Some("10px".to_string()));
    assert_eq!(get_inline_style_prop(Some(&updated), "margin"), None);
}
