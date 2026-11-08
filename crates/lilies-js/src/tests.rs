#![deny(unsafe_code)]

use crate::runtime::{camel_to_kebab, get_inline_style_prop, update_inline_style, JsRuntime};
use lilies_dom::Document;
use lilies_html::parse_html;

#[test]
fn test_script_with_for_loop_and_comparisons() {
    let html = r#"
        <html>
            <body>
                <p id="counter">0</p>
                <script>
                    var sum = 0;
                    var arr = [10, 20, 30];
                    for (var i = 0; i < arr.length; i++) {
                        if (arr[i] < 25) {
                            sum += arr[i];
                        }
                    }
                    var p = document.getElementById("counter");
                    p.textContent = "Sum: " + sum;
                </script>
            </body>
        </html>
    "#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    // Extract and run script
    let script_node = runtime.find_element_by_id("counter").unwrap();
    let parent = runtime.doc().parent(script_node).unwrap();
    let children = runtime.doc().get_children(parent);
    let mut scripts_to_run = Vec::new();
    {
        let doc = runtime.doc();
        for child in children {
            if let Some(node) = doc.node(child) {
                if let Some(el) = node.as_element() {
                    if el.tag_name().eq_ignore_ascii_case("script") {
                        scripts_to_run.push(doc.text_content(child));
                    }
                }
            }
        }
    }
    for code in scripts_to_run {
        runtime.eval(&code).expect("eval script with comparisons");
    }

    let p_node = runtime.find_element_by_id("counter").unwrap();
    assert_eq!(runtime.doc().text_content(p_node), "Sum: 30");
}

#[test]
fn test_browser_globals_and_document_properties() {
    let html = r#"
        <html>
            <head><title>Test Page</title></head>
            <body>
                <form name="searchForm">
                    <input name="q" value="query test">
                    <button id="submitBtn">Search</button>
                </form>
            </body>
        </html>
    "#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval("window === globalThis && self === globalThis && top === globalThis")
        .unwrap();
    assert_eq!(res.as_boolean(), Some(true));

    let loc_res = runtime.eval("window.location.pathname").unwrap();
    assert_eq!(loc_res.as_string().map(|s| s.to_std_string_escaped()), Some("/".to_string()));

    let doc_elem = runtime.eval("document.documentElement.tagName").unwrap();
    assert_eq!(doc_elem.as_string().map(|s| s.to_std_string_escaped()), Some("HTML".to_string()));

    let body_elem = runtime.eval("document.body.tagName").unwrap();
    assert_eq!(body_elem.as_string().map(|s| s.to_std_string_escaped()), Some("BODY".to_string()));

    let btn = runtime.eval("document.getElementById('submitBtn').form.q.value").unwrap();
    assert_eq!(btn.as_string().map(|s| s.to_std_string_escaped()), Some("query test".to_string()));
}

#[test]
fn test_google_html_scripts() {
    let path = std::path::Path::new("/tmp/google.html");
    if !path.exists() {
        return;
    }
    let content = std::fs::read_to_string(path).unwrap();
    let doc = parse_html(&content);
    let mut scripts = Vec::new();
    let mut stack = vec![doc.root()];
    while let Some(node_id) = stack.pop() {
        if let Some(node) = doc.node(node_id) {
            if let Some(el) = node.as_element() {
                if el.tag_name().eq_ignore_ascii_case("script") {
                    scripts.push(doc.text_content(node_id));
                }
            }
        }
        for &child in doc.get_children(node_id).iter().rev() {
            stack.push(child);
        }
    }
    let mut runtime = JsRuntime::new(doc);
    for (i, s) in scripts.iter().enumerate() {
        runtime.eval(s).unwrap_or_else(|e| panic!("Script {} failed: {}", i, e));
    }
}

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

    // Clearing a property removes it without leaving `color: ;`
    let cleared = update_inline_style(Some(&updated), "color", "");
    assert!(!cleared.contains("color"));
    assert_eq!(get_inline_style_prop(Some(&cleared), "color"), None);
    assert_eq!(get_inline_style_prop(Some(&cleared), "padding"), Some("10px".to_string()));
}

#[test]
fn test_clear_inline_style_and_css_text() {
    let html = r#"<html><body><div id="box" style="width: 100px; color: red;"></div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    runtime
        .eval(
            r#"
        var box = document.getElementById("box");
        box.style.color = "";
    "#,
        )
        .expect("eval failed");

    let box_id = runtime.find_element_by_id("box").unwrap();
    let style_after_clear = runtime
        .doc()
        .node(box_id)
        .and_then(|n| n.as_element())
        .and_then(|el| el.get_attribute("style"))
        .unwrap()
        .to_string();
    assert_eq!(style_after_clear, "width: 100px;");

    let color_val = runtime.eval("box.style.color").unwrap();
    assert_eq!(color_val.as_string().map(|s| s.to_std_string_escaped()), Some(String::new()));

    runtime
        .eval(r#"box.style.cssText = "height: 50px; color: green;";"#)
        .expect("eval cssText failed");
    let css_text_val = runtime.eval("box.style.cssText").unwrap();
    assert_eq!(
        css_text_val.as_string().map(|s| s.to_std_string_escaped()),
        Some("height: 50px; color: green;".to_string())
    );
    let height_val = runtime.eval("box.style.height").unwrap();
    assert_eq!(
        height_val.as_string().map(|s| s.to_std_string_escaped()),
        Some("50px".to_string())
    );
}

#[test]
fn test_get_element_by_id_ignores_detached_nodes() {
    let html = r#"<html><body><div id="container"><span id="item">Old</span></div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var container = document.getElementById("container");
        var oldItem = document.getElementById("item");
        container.removeChild(oldItem);
        var afterRemove = document.getElementById("item");

        var detached = document.createElement("span");
        detached.id = "item";
        detached.textContent = "Detached";
        var beforeAppend = document.getElementById("item");

        container.appendChild(detached);
        var afterAppend = document.getElementById("item");

        [afterRemove === null, beforeAppend === null, afterAppend.textContent].join(",");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("true,true,Detached".to_string())
    );
}

#[test]
fn test_click_on_text_node_binds_this_to_listener_element_and_passes_event() {
    let html = r#"<html><body><div id="outer"><button id="btn">Click Me!</button></div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    runtime
        .eval(
            r#"
        var btn = document.getElementById("btn");
        var outer = document.getElementById("outer");
        var outerClicked = false;
        btn.addEventListener("click", function(e) {
            e.preventDefault();
            e.stopPropagation();
            this.style.backgroundColor = "red";
            this.textContent = "Clicked: " + e.target.id + ":" + e.currentTarget.id;
        });
        outer.addEventListener("click", function(e) {
            outerClicked = true;
        });
    "#,
        )
        .expect("eval failed");

    let btn_id = runtime.find_element_by_id("btn").unwrap();
    let text_node_id = runtime.doc().first_child(btn_id).unwrap();
    assert!(runtime.doc().node(text_node_id).unwrap().is_text());

    // Dispatch click on the inner Text node (just as hit_test returns when clicking button text)
    runtime
        .dispatch_click(text_node_id)
        .expect("dispatch_click on text node failed");

    assert_eq!(runtime.doc().text_content(btn_id), "Clicked: btn:btn");
    let bg = runtime.eval("btn.style.backgroundColor").unwrap();
    assert_eq!(
        bg.as_string().map(|s| s.to_std_string_escaped()),
        Some("red".to_string())
    );
    let outer_clicked = runtime.eval("outerClicked").unwrap();
    assert_eq!(outer_clicked.as_boolean(), Some(false));

    // Also verify mutating textContent directly on a Text node updates Text::data in place
    runtime
        .eval(r#"btn.firstChild.textContent = "Direct Text Node Update";"#)
        .expect("mutating firstChild.textContent failed");
    assert_eq!(
        runtime.doc().text_content(btn_id),
        "Direct Text Node Update"
    );
    let fc_id = runtime.doc().first_child(btn_id).unwrap();
    assert_eq!(runtime.doc().get_children(fc_id).len(), 0);
}

#[test]
fn test_append_child_and_insert_before_cycle_preserves_parent() {
    let html = r#"<html><body><div id="parent"><span id="child">Text</span></div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var parent = document.getElementById("parent");
        var child = document.getElementById("child");
        var caughtAppend = false;
        try {
            child.appendChild(parent);
        } catch (e) {
            caughtAppend = true;
        }
        var caughtInsert = false;
        try {
            child.insertBefore(parent, null);
        } catch (e) {
            caughtInsert = true;
        }
        [caughtAppend, caughtInsert, document.getElementById("parent") !== null, parent.parentNode.tagName].join(",");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("true,true,true,BODY".to_string())
    );
}

#[test]
fn test_dom_query_methods() {
    let html = r#"<html><body>
        <div id="container" class="main-box active">
            <p class="text highlight">First paragraph</p>
            <p class="text">Second paragraph</p>
            <form name="searchForm">
                <input name="query" value="rust" />
            </form>
        </div>
        <div id="footer">
            <span class="highlight">Footer text</span>
        </div>
    </body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r##"
        var container = document.querySelector("#container");
        var firstP = container.querySelector("p.highlight");
        var allP = container.querySelectorAll("p");
        var allHighlight = document.querySelectorAll(".highlight");
        var byClass = document.getElementsByClassName("highlight");
        var byName = document.getElementsByName("query");
        var containerP = container.getElementsByTagName("p");

        var pItem = allP.item(1);
        var matchesP = firstP.matches("p.highlight");
        var closestContainer = firstP.closest("#container");

        [
            container !== null,
            firstP.textContent,
            allP.length,
            pItem.textContent,
            allHighlight.length,
            byClass.length,
            byName.length,
            containerP.length,
            matchesP,
            closestContainer === container
        ].join("|");
    "##,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("true|First paragraph|2|Second paragraph|2|2|1|2|true|true".to_string())
    );
}

#[test]
fn test_style_proxy_methods_and_coercion() {
    let html = r#"<html><body><div id="box" style="color: red; font-size: 16px;">Hello</div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var box = document.getElementById("box");
        var initialColor = box.style.getPropertyValue("color");
        box.style.setProperty("background-color", "blue");
        var bg = box.style.getPropertyValue("background-color");
        var removed = box.style.removeProperty("color");
        var colorAfterRemove = box.style.getPropertyValue("color");
        var itemVal = box.style.item(0);
        var strVal = box.style.toString();
        var coerced = String(box.style);
        var hasProp = box.style.hasOwnProperty("color");

        [
            initialColor,
            bg,
            removed,
            colorAfterRemove,
            typeof itemVal,
            strVal,
            coerced,
            typeof hasProp
        ].join("|");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("red|blue|red||string|[object CSSStyleDeclaration]|[object CSSStyleDeclaration]|boolean".to_string())
    );
}

#[test]
fn test_get_computed_style_and_match_media() {
    let html = r#"<html><head><style>#card { display: flex; color: green; font-size: 20px; }</style></head><body><div id="card">Content</div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var card = document.getElementById("card");
        var computed = window.getComputedStyle(card);
        var display = computed.display;
        var color = computed.getPropertyValue("color");
        var fontSize = computed.fontSize;

        var mql = window.matchMedia("(prefers-color-scheme: dark)");
        var isDark = mql.matches;
        var listenerAdded = typeof mql.addEventListener === 'function';

        [
            display,
            color,
            fontSize,
            isDark,
            listenerAdded
        ].join("|");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("flex|rgb(0, 128, 0)|20px|false|true".to_string())
    );
}

#[test]
fn test_document_creation_and_helper_methods() {
    let doc = Document::new();
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var frag = document.createDocumentFragment();
        var comment = document.createComment("a test comment");
        var evt = document.createEvent("Event");
        evt.initEvent("custom", true, true);
        evt.preventDefault();
        var focused = document.hasFocus();

        [
            frag !== null,
            typeof frag.appendChild === 'function',
            comment !== null,
            evt.type,
            evt.bubbles,
            evt.defaultPrevented,
            focused,
            typeof document.write === 'function'
        ].join("|");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("true|true|true|custom|true|true|true|true".to_string())
    );
}

#[test]
fn test_element_manipulation_traversal_and_class_list() {
    let html = r#"<html><body><div id="parent"><span id="child" class="btn active">Item</span><p id="sibling">Other</p></div></body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var parent = document.getElementById("parent");
        var child = document.getElementById("child");
        var sibling = document.getElementById("sibling");

        // classList tests
        child.classList.add("primary", "highlight");
        var hasActive = child.classList.contains("active");
        child.classList.remove("active");
        var hasActiveAfterRemove = child.classList.contains("active");
        var toggledOff = child.classList.toggle("btn");
        var toggledOn = child.classList.toggle("btn");
        var classString = child.classList.toString();

        // contains tests
        var parentContainsChild = parent.contains(child);
        var childContainsParent = child.contains(parent);

        // cloneNode tests
        var cloned = child.cloneNode(true);
        var clonedHasClass = cloned.classList.contains("primary");

        // replaceChild tests
        parent.replaceChild(cloned, sibling);
        var siblingDetached = sibling.parentNode === null;

        // insertAdjacentHTML tests
        child.insertAdjacentHTML("afterend", "<b>Bold</b>");

        // remove test
        child.remove();
        var childRemoved = child.parentNode === null;

        [
            hasActive,
            hasActiveAfterRemove,
            toggledOff,
            toggledOn,
            classString.indexOf("primary") !== -1,
            parentContainsChild,
            childContainsParent,
            clonedHasClass,
            siblingDetached,
            childRemoved
        ].join("|");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("true|false|false|true|true|true|false|true|true|true".to_string())
    );
}

#[test]
fn test_form_submit_and_named_input_collision() {
    let html = r#"<html><body>
        <form id="loginForm">
            <input name="submit" value="Sign In" />
            <input name="reset" value="Clear" />
            <input name="username" value="alice" />
        </form>
    </body></html>"#;
    let doc = parse_html(html);
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var form = document.getElementById("loginForm");
        var isSubmitFn = typeof form.submit === 'function';
        var isResetFn = typeof form.reset === 'function';
        var submitInput = form.elements.submit;
        var usernameInput = form.username;

        // Ensure calling form.submit() does not throw TypeError
        var submitThrew = false;
        try {
            form.submit();
        } catch (e) {
            submitThrew = true;
        }

        [
            isSubmitFn,
            isResetFn,
            submitInput !== undefined,
            submitInput.value,
            usernameInput !== undefined,
            usernameInput.value,
            submitThrew
        ].join("|");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("true|true|true|Sign In|true|alice|false".to_string())
    );
}

#[test]
fn test_window_image_dom_element() {
    let doc = parse_html("<html><body></body></html>");
    let mut runtime = JsRuntime::new(doc);

    let res = runtime
        .eval(
            r#"
        var img = new Image(100, 200);
        img.src = "https://example.com/test.png";
        img.setAttribute("alt", "Test Image");
        img.classList.add("responsive");
        document.body.appendChild(img);

        var appended = document.body.querySelector("img");

        [
            img.tagName,
            img.src,
            img.getAttribute("alt"),
            img.classList.contains("responsive"),
            appended === img,
            typeof img.addEventListener === 'function'
        ].join("|");
    "#,
        )
        .expect("eval failed");

    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("IMG|https://example.com/test.png|Test Image|true|true|true".to_string())
    );
}

#[test]
fn test_window_and_document_event_listeners_and_bubbling() {
    let doc = parse_html(r#"<html><body><div id="wrapper"><button id="btn">Click me</button></div></body></html>"#);
    let mut runtime = JsRuntime::new(doc);

    runtime
        .eval(
            r#"
        var log = [];
        window.addEventListener("DOMContentLoaded", function(e) {
            log.push("window:domcontentloaded");
        });
        document.addEventListener("DOMContentLoaded", function(e) {
            log.push("doc:domcontentloaded");
        });
        window.addEventListener("load", function(e) {
            log.push("window:load");
        });

        var wrapper = document.getElementById("wrapper");
        var btn = document.getElementById("btn");

        wrapper.addEventListener("click", function(e) {
            log.push("wrapper:click");
        });
        document.addEventListener("click", function(e) {
            log.push("doc:click");
        });
        window.addEventListener("click", function(e) {
            log.push("window:click");
        });
        btn.addEventListener("click", function(e) {
            log.push("btn:click");
        });
    "#,
        )
        .expect("eval setup failed");

    // Test DOMContentLoaded dispatch
    runtime.dispatch_event(None, "DOMContentLoaded").unwrap();
    // Test load dispatch
    runtime.dispatch_event(None, "load").unwrap();

    let res = runtime.eval("log.join('|')").unwrap();
    assert_eq!(
        res.as_string().map(|s| s.to_std_string_escaped()),
        Some("doc:domcontentloaded|window:domcontentloaded|window:load".to_string())
    );

    // Test click bubbling: btn -> wrapper -> document -> window
    let btn_id = runtime.find_element_by_id("btn").unwrap();
    runtime.eval("log = [];").unwrap();
    let default_allowed = runtime.dispatch_click(btn_id).unwrap();
    assert!(default_allowed);

    let res2 = runtime.eval("log.join('|')").unwrap();
    assert_eq!(
        res2.as_string().map(|s| s.to_std_string_escaped()),
        Some("btn:click|wrapper:click|doc:click|window:click".to_string())
    );

    // Test removeEventListener on document and window
    runtime
        .eval(
            r#"
        log = [];
        var removeHandler = function() { log.push("never"); };
        document.addEventListener("click", removeHandler);
        document.removeEventListener("click", removeHandler);
        window.addEventListener("click", removeHandler);
        window.removeEventListener("click", removeHandler);
    "#,
        )
        .unwrap();
    runtime.dispatch_click(btn_id).unwrap();
    let res3 = runtime.eval("log.join('|')").unwrap();
    assert_eq!(
        res3.as_string().map(|s| s.to_std_string_escaped()),
        Some("btn:click|wrapper:click|doc:click|window:click".to_string())
    );

    // Test stopPropagation()
    runtime
        .eval(
            r#"
        log = [];
        btn.addEventListener("click", function(e) {
            e.stopPropagation();
            log.push("btn:stopped");
        });
    "#,
        )
        .unwrap();
    runtime.dispatch_click(btn_id).unwrap();
    let res4 = runtime.eval("log.join('|')").unwrap();
    assert_eq!(
        res4.as_string().map(|s| s.to_std_string_escaped()),
        Some("btn:click|btn:stopped".to_string())
    );

    // Test preventDefault()
    runtime
        .eval(
            r#"
        btn.addEventListener("click", function(e) {
            e.preventDefault();
        });
    "#,
        )
        .unwrap();
    let not_prevented = runtime.dispatch_click(btn_id).unwrap();
    assert!(!not_prevented);

    // Test dispatchEvent on element and document
    runtime
        .eval(
            r#"
        var customFired = false;
        btn.addEventListener("custom", function(e) {
            customFired = (e.type === "custom");
        });
        btn.dispatchEvent({ type: "custom" });
    "#,
        )
        .unwrap();
    let res5 = runtime.eval("customFired").unwrap();
    assert_eq!(res5.as_boolean(), Some(true));
}

#[test]
fn test_timers_and_request_animation_frame() {
    let doc = parse_html("<html><body></body></html>");
    let mut runtime = JsRuntime::new(doc);

    runtime
        .eval(
            r#"
        var results = [];
        var t1 = setTimeout(function() {
            results.push("timeout_0");
        }, 0);

        var t2 = setTimeout(function() {
            results.push("cancelled");
        }, 0);
        clearTimeout(t2);

        var count = 0;
        var intervalId = setInterval(function() {
            count++;
            results.push("interval_" + count);
            if (count >= 2) {
                clearInterval(intervalId);
            }
        }, 10);

        var rafTimestamp = -1;
        var rafId = requestAnimationFrame(function(ts) {
            rafTimestamp = ts;
            results.push("raf");
        });

        var cancelRafId = requestAnimationFrame(function() {
            results.push("raf_cancelled");
        });
        cancelAnimationFrame(cancelRafId);
    "#,
        )
        .expect("setup failed");

    // Initially nothing has run yet
    let initial_res = runtime.eval("results.join('|')").unwrap();
    assert_eq!(
        initial_res.as_string().map(|s| s.to_std_string_escaped()),
        Some("".to_string())
    );

    // Run ready timers (delay = 0)
    let fired = runtime.run_ready_timers().unwrap();
    assert!(fired >= 1);
    let after_timeout = runtime.eval("results.join('|')").unwrap();
    assert_eq!(
        after_timeout.as_string().map(|s| s.to_std_string_escaped()),
        Some("timeout_0".to_string())
    );

    // Run animation frames
    let rafs = runtime.run_animation_frames(123.45).unwrap();
    assert_eq!(rafs, 1); // 2 registered, 1 cancelled, 1 ran
    let after_raf = runtime.eval("results.join('|')").unwrap();
    assert_eq!(
        after_raf.as_string().map(|s| s.to_std_string_escaped()),
        Some("timeout_0|raf".to_string())
    );
    let ts_val = runtime.eval("rafTimestamp").unwrap();
    assert_eq!(ts_val.as_number(), Some(123.45));

    // Wait 15ms so interval fires first tick
    std::thread::sleep(std::time::Duration::from_millis(15));
    runtime.run_ready_timers().unwrap();
    let after_int1 = runtime.eval("results.join('|')").unwrap();
    assert_eq!(
        after_int1.as_string().map(|s| s.to_std_string_escaped()),
        Some("timeout_0|raf|interval_1".to_string())
    );

    // Wait 15ms so interval fires second tick and cancels itself
    std::thread::sleep(std::time::Duration::from_millis(15));
    runtime.run_ready_timers().unwrap();
    let after_int2 = runtime.eval("results.join('|')").unwrap();
    assert_eq!(
        after_int2.as_string().map(|s| s.to_std_string_escaped()),
        Some("timeout_0|raf|interval_1|interval_2".to_string())
    );

    // Wait another 15ms to verify interval is not called a 3rd time
    std::thread::sleep(std::time::Duration::from_millis(15));
    runtime.run_ready_timers().unwrap();
    let after_int3 = runtime.eval("results.join('|')").unwrap();
    assert_eq!(
        after_int3.as_string().map(|s| s.to_std_string_escaped()),
        Some("timeout_0|raf|interval_1|interval_2".to_string())
    );

    // Test setTimeout with string evaluation
    runtime
        .eval(
            r#"
        setTimeout("results.push('string_eval')", 0);
    "#,
        )
        .unwrap();
    runtime.run_ready_timers().unwrap();
    let after_str = runtime.eval("results.join('|')").unwrap();
    assert_eq!(
        after_str.as_string().map(|s| s.to_std_string_escaped()),
        Some("timeout_0|raf|interval_1|interval_2|string_eval".to_string())
    );
}

#[test]
fn test_inner_html_and_outer_html() {
    let doc = parse_html(r#"<html><body><div id="container"><div id="box"><p class="intro">Hello <span>world</span></p></div></div></body></html>"#);
    let mut runtime = JsRuntime::new(doc);

    // 1. Test innerHTML and outerHTML getters
    let res = runtime
        .eval(
            r#"
        var box = document.getElementById("box");
        var initialInner = box.innerHTML;
        var initialOuter = box.outerHTML;

        // 2. Test innerHTML setter
        box.innerHTML = '<h2>Title</h2><p>New content</p>';
        var newInner = box.innerHTML;
        var h2Text = box.querySelector("h2").textContent;
        var childCount = box.children.length;

        // 3. Test outerHTML setter
        box.outerHTML = '<section id="new-box"><span class="tag">Replaced</span></section>';
        var oldBoxIsNull = document.getElementById("box") === null;
        var newBox = document.getElementById("new-box");
        var newBoxTag = newBox ? newBox.tagName : "";
        var newBoxInner = newBox ? newBox.innerHTML : "";

        [
            initialInner,
            initialOuter,
            newInner,
            h2Text,
            childCount,
            oldBoxIsNull,
            newBoxTag,
            newBoxInner
        ].join("~~~");
    "#,
        )
        .expect("eval failed");

    let val = res.as_string().map(|s| s.to_std_string_escaped()).unwrap();
    let parts: Vec<&str> = val.split("~~~").collect();
    assert_eq!(parts[0], r#"<p class="intro">Hello <span>world</span></p>"#);
    assert_eq!(parts[1], r#"<div id="box"><p class="intro">Hello <span>world</span></p></div>"#);
    assert_eq!(parts[2], "<h2>Title</h2><p>New content</p>");
    assert_eq!(parts[3], "Title");
    assert_eq!(parts[4], "2");
    assert_eq!(parts[5], "true");
    assert_eq!(parts[6], "SECTION");
    assert_eq!(parts[7], r#"<span class="tag">Replaced</span>"#);
    assert!(runtime.is_dirty(), "Runtime must be marked dirty after DOM mutation via innerHTML/outerHTML");
}
