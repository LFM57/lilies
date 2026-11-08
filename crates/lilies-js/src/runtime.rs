#![deny(unsafe_code)]

use boa_engine::{
    js_string,
    object::{
        builtins::{JsArray, JsFunction},
        FunctionObjectBuilder, ObjectInitializer,
    },
    property::Attribute,
    Context, JsArgs, JsData, JsError, JsNativeError, JsResult, JsValue, NativeFunction, Source,
};
use boa_gc::{Finalize, Trace};
use lilies_css::{matches as css_matches, parse_selectors};
use lilies_dom::{Document, IndexMap, NodeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Converts a camelCase string (e.g. "backgroundColor", "fontSize") to kebab-case ("background-color", "font-size").
pub fn camel_to_kebab(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    for c in s.chars() {
        if c.is_ascii_uppercase() {
            result.push('-');
            result.push(c.to_ascii_lowercase());
        } else {
            result.push(c);
        }
    }
    result
}

/// Updates or inserts a CSS property declaration in an inline style string.
pub fn update_inline_style(existing: Option<&str>, prop: &str, val: &str) -> String {
    let mut entries: Vec<(String, String)> = Vec::new();
    if let Some(s) = existing {
        for part in s.split(';') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once(':') {
                let k = k.trim().to_ascii_lowercase();
                let v = v.trim().to_string();
                if !k.is_empty() && !v.is_empty() {
                    entries.push((k, v));
                }
            }
        }
    }

    let target_key = prop.to_ascii_lowercase();
    let val_trimmed = val.trim();
    if val_trimmed.is_empty() {
        entries.retain(|(k, _)| k != &target_key);
    } else {
        let mut updated = false;
        for (k, v) in &mut entries {
            if k == &target_key {
                *v = val_trimmed.to_string();
                updated = true;
                break;
            }
        }
        if !updated {
            entries.push((target_key, val_trimmed.to_string()));
        }
    }

    entries
        .into_iter()
        .map(|(k, v)| format!("{k}: {v};"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Retrieves the value of a CSS property from an inline style string if present.
pub fn get_inline_style_prop(existing: Option<&str>, prop: &str) -> Option<String> {
    let s = existing?;
    let target_key = prop.to_ascii_lowercase();
    for part in s.split(';') {
        let part = part.trim();
        if let Some((k, v)) = part.split_once(':') {
            if k.trim().eq_ignore_ascii_case(&target_key) {
                return Some(v.trim().to_string());
            }
        }
    }
    None
}

fn find_in_tree<F>(doc: &Document, mut predicate: F) -> Option<NodeId>
where
    F: FnMut(&lilies_dom::Element) -> bool,
{
    let mut stack = vec![doc.root()];
    while let Some(node_id) = stack.pop() {
        if let Some(node) = doc.node(node_id) {
            if let Some(el) = node.as_element() {
                if predicate(el) {
                    return Some(node_id);
                }
            }
        }
        for &child in doc.get_children(node_id).iter().rev() {
            stack.push(child);
        }
    }
    None
}

/// Finds an element node in the connected DOM tree matching the given `id` attribute.
pub fn find_element_by_id(doc: &Document, id: &str) -> Option<NodeId> {
    find_in_tree(doc, |el| el.id() == Some(id))
}

/// Finds the root `<html>` element in the connected document tree.
pub fn find_html_element(doc: &Document) -> Option<NodeId> {
    find_in_tree(doc, |el| el.tag_name().eq_ignore_ascii_case("html"))
}

/// Finds the `<body>` element in the connected document tree.
pub fn find_body_element(doc: &Document) -> Option<NodeId> {
    find_in_tree(doc, |el| el.tag_name().eq_ignore_ascii_case("body"))
}

/// Finds the `<head>` element in the connected document tree.
pub fn find_head_element(doc: &Document) -> Option<NodeId> {
    find_in_tree(doc, |el| el.tag_name().eq_ignore_ascii_case("head"))
}

/// Replaces all child nodes of `node_id` with a single new Text node containing `text`,
/// or directly mutates `Text::data` if `node_id` is already a `Text` node.
pub fn set_node_text_content(doc: &mut Document, node_id: NodeId, text: &str) {
    if let Some(node) = doc.node_mut(node_id) {
        if let Some(text_node) = node.as_text_mut() {
            text_node.data = text.to_string();
            return;
        }
    }
    let children = doc.get_children(node_id);
    for child in children {
        let _ = doc.remove_child(node_id, child);
    }
    if !text.is_empty() {
        let text_id = doc.create_text(text);
        let _ = doc.append_child(node_id, text_id);
    }
}

/// Native object data wrapping a DOM `NodeId` inside Boa JavaScript element objects.
#[derive(Debug, Clone, Copy, Trace, Finalize, JsData)]
#[boa_gc(empty_trace)]
pub struct ElementNode(pub NodeId);

/// Callback for a scheduled timer (either a JavaScript function or source script).
#[derive(Clone)]
pub enum TimerCallback {
    Function(JsFunction),
    Script(String),
}

/// A scheduled timer entry.
pub struct Timer {
    pub id: u32,
    pub callback: TimerCallback,
    pub target_time: Instant,
    pub interval: Option<Duration>,
    pub args: Vec<JsValue>,
}

/// A queued requestAnimationFrame callback.
pub struct AnimationFrame {
    pub id: u32,
    pub callback: JsFunction,
}

/// Internal state managed by the JavaScript runtime.
pub struct RuntimeInner {
    pub doc: Document,
    pub dirty: bool,
    pub click_listeners: HashMap<NodeId, Vec<JsFunction>>,
    pub node_cache: HashMap<NodeId, boa_engine::object::JsObject>,
    pub document_listeners: HashMap<String, Vec<JsFunction>>,
    pub window_listeners: HashMap<String, Vec<JsFunction>>,
    pub element_listeners: HashMap<(NodeId, String), Vec<JsFunction>>,
    pub start_time: Instant,
    pub next_timer_id: u32,
    pub timers: Vec<Timer>,
    pub cancelled_timers: HashSet<u32>,
    pub next_raf_id: u32,
    pub animation_frames: Vec<AnimationFrame>,
    pub cancelled_rafs: HashSet<u32>,
}

thread_local! {
    static CURRENT_RUNTIME: RefCell<Option<Rc<RefCell<RuntimeInner>>>> = const { RefCell::new(None) };
}

/// RAII guard ensuring the thread-local runtime pointer is restored on drop.
pub struct ActiveRuntimeGuard {
    prev: Option<Rc<RefCell<RuntimeInner>>>,
}

impl ActiveRuntimeGuard {
    pub fn new(inner: &Rc<RefCell<RuntimeInner>>) -> Self {
        let prev = CURRENT_RUNTIME.with(|c| c.replace(Some(inner.clone())));
        Self { prev }
    }
}

impl Drop for ActiveRuntimeGuard {
    fn drop(&mut self) {
        CURRENT_RUNTIME.with(|c| *c.borrow_mut() = self.prev.take());
    }
}

/// Runs a closure with access to the currently active `RuntimeInner`.
pub fn with_current_runtime<F, R>(f: F) -> JsResult<R>
where
    F: FnOnce(&mut RuntimeInner) -> JsResult<R>,
{
    CURRENT_RUNTIME.with(|c| {
        let opt = c.borrow();
        let rc = opt.as_ref().ok_or_else(|| {
            JsNativeError::typ()
                .with_message("No active DOM runtime context available")
        })?;
        let mut inner = rc.borrow_mut();
        f(&mut inner)
    })
}

fn get_node_id_from_this(this: &JsValue) -> Result<NodeId, JsNativeError> {
    this.as_object()
        .and_then(|obj| obj.downcast_ref::<ElementNode>())
        .map(|node| node.0)
        .ok_or_else(|| JsNativeError::typ().with_message("Invalid this for DOM Element"))
}

fn console_log(_this: &JsValue, args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        let s = match arg {
            JsValue::String(s) => s.to_std_string_escaped(),
            JsValue::Undefined => "undefined".to_string(),
            JsValue::Null => "null".to_string(),
            JsValue::Boolean(b) => b.to_string(),
            JsValue::Integer(i) => i.to_string(),
            JsValue::Rational(r) => r.to_string(),
            other => other.to_string(context)?.to_std_string_escaped(),
        };
        parts.push(s);
    }
    println!("{}", parts.join(" "));
    Ok(JsValue::undefined())
}

fn document_get_element_by_id(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let id_arg = args.get_or_undefined(0);
    let id_str = id_arg.to_string(context)?.to_std_string_escaped();

    let found_id = with_current_runtime(|inner| Ok(find_element_by_id(&inner.doc, &id_str)))?;

    if let Some(node_id) = found_id {
        let el_obj = get_or_create_element_object(node_id, context)?;
        Ok(el_obj.into())
    } else {
        Ok(JsValue::null())
    }
}

fn document_get_document_element(
    _this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let target = with_current_runtime(|inner| Ok(find_html_element(&inner.doc)))?;
    match target {
        Some(node_id) => {
            let obj = get_or_create_element_object(node_id, context)?;
            Ok(JsValue::new(obj))
        }
        None => Ok(JsValue::null()),
    }
}

fn document_get_body(
    _this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let target = with_current_runtime(|inner| Ok(find_body_element(&inner.doc)))?;
    match target {
        Some(node_id) => {
            let obj = get_or_create_element_object(node_id, context)?;
            Ok(JsValue::new(obj))
        }
        None => Ok(JsValue::null()),
    }
}

fn document_get_head(
    _this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let target = with_current_runtime(|inner| Ok(find_head_element(&inner.doc)))?;
    match target {
        Some(node_id) => {
            let obj = get_or_create_element_object(node_id, context)?;
            Ok(JsValue::new(obj))
        }
        None => Ok(JsValue::null()),
    }
}

fn document_create_element(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let tag = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let new_node_id = with_current_runtime(|inner| {
        inner.dirty = true;
        Ok(inner.doc.create_element(&tag, IndexMap::new()))
    })?;
    let obj = get_or_create_element_object(new_node_id, context)?;
    Ok(JsValue::new(obj))
}

fn document_create_text_node(
    _this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let text = args.get_or_undefined(0).to_string(_context)?.to_std_string_escaped();
    let new_node_id = with_current_runtime(|inner| {
        inner.dirty = true;
        Ok(inner.doc.create_text(&text))
    })?;
    let obj = get_or_create_element_object(new_node_id, _context)?;
    Ok(JsValue::new(obj))
}

fn document_create_comment(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let text = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let new_node_id = with_current_runtime(|inner| {
        inner.dirty = true;
        Ok(inner.doc.create_comment(&text))
    })?;
    let obj = get_or_create_element_object(new_node_id, context)?;
    Ok(JsValue::new(obj))
}

fn document_create_document_fragment(
    _this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let new_node_id = with_current_runtime(|inner| {
        inner.dirty = true;
        Ok(inner.doc.create_document_fragment())
    })?;
    let obj = get_or_create_element_object(new_node_id, context)?;
    Ok(JsValue::new(obj))
}

fn document_get_child_nodes(
    _this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let child_ids = with_current_runtime(|inner| Ok(inner.doc.get_children(inner.doc.root())))?;
    wrap_elements_as_collection(child_ids, context)
}

fn wrap_elements_as_collection(node_ids: Vec<NodeId>, context: &mut Context) -> JsResult<JsValue> {
    let array = JsArray::new(context);
    for node_id in node_ids {
        let obj = get_or_create_element_object(node_id, context)?;
        array.push(JsValue::new(obj), context)?;
    }

    let item_fn = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(|this, args, context| {
            let index = args.get_or_undefined(0).to_u32(context).unwrap_or(0);
            if let Some(obj) = this.as_object() {
                if let Ok(val) = obj.get(index, context) {
                    if !val.is_undefined() {
                        return Ok(val);
                    }
                }
            }
            Ok(JsValue::null())
        }),
    )
    .name(js_string!("item"))
    .length(1)
    .build();

    let array_obj: boa_engine::object::JsObject = array.into();
    let _ = array_obj.set(js_string!("item"), item_fn, false, context);
    Ok(JsValue::new(array_obj))
}

fn query_selector_internal(
    root: NodeId,
    include_root: bool,
    selector_str: &str,
    first_only: bool,
) -> JsResult<Vec<NodeId>> {
    let selectors = match parse_selectors(selector_str) {
        Ok(s) => s,
        Err(_) => return Ok(Vec::new()),
    };

    with_current_runtime(|inner| {
        let mut results = Vec::new();
        let mut stack = Vec::new();
        if include_root {
            stack.push(root);
        } else {
            for &child in inner.doc.get_children(root).iter().rev() {
                stack.push(child);
            }
        }

        while let Some(curr) = stack.pop() {
            if let Some(node) = inner.doc.node(curr) {
                if node.is_element() {
                    let matches_any = selectors
                        .iter()
                        .any(|s| css_matches(s, curr, inner.doc.arena()));
                    if matches_any {
                        results.push(curr);
                        if first_only {
                            break;
                        }
                    }
                }
            }
            for &child in inner.doc.get_children(curr).iter().rev() {
                stack.push(child);
            }
        }

        Ok(results)
    })
}

fn document_query_selector(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let selector_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let root = with_current_runtime(|inner| Ok(inner.doc.root()))?;
    let found = query_selector_internal(root, false, &selector_str, true)?;
    if let Some(&first) = found.first() {
        let obj = get_or_create_element_object(first, context)?;
        Ok(JsValue::new(obj))
    } else {
        Ok(JsValue::null())
    }
}

fn document_query_selector_all(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let selector_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let root = with_current_runtime(|inner| Ok(inner.doc.root()))?;
    let found = query_selector_internal(root, false, &selector_str, false)?;
    wrap_elements_as_collection(found, context)
}

fn element_query_selector(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let selector_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let found = query_selector_internal(node_id, false, &selector_str, true)?;
    if let Some(&first) = found.first() {
        let obj = get_or_create_element_object(first, context)?;
        Ok(JsValue::new(obj))
    } else {
        Ok(JsValue::null())
    }
}

fn element_query_selector_all(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let selector_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let found = query_selector_internal(node_id, false, &selector_str, false)?;
    wrap_elements_as_collection(found, context)
}

fn element_matches(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let selector_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let selectors = match parse_selectors(&selector_str) {
        Ok(s) => s,
        Err(_) => return Ok(JsValue::new(false)),
    };
    let matches_any = with_current_runtime(|inner| {
        Ok(selectors
            .iter()
            .any(|s| css_matches(s, node_id, inner.doc.arena())))
    })?;
    Ok(JsValue::new(matches_any))
}

fn element_closest(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let selector_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let selectors = match parse_selectors(&selector_str) {
        Ok(s) => s,
        Err(_) => return Ok(JsValue::null()),
    };

    let found_id = with_current_runtime(|inner| {
        let mut curr = Some(node_id);
        while let Some(id) = curr {
            if let Some(node) = inner.doc.node(id) {
                if node.is_element()
                    && selectors
                        .iter()
                        .any(|s| css_matches(s, id, inner.doc.arena()))
                {
                    return Ok(Some(id));
                }
                curr = node.parent;
            } else {
                break;
            }
        }
        Ok(None)
    })?;

    if let Some(matched_id) = found_id {
        let obj = get_or_create_element_object(matched_id, context)?;
        Ok(JsValue::new(obj))
    } else {
        Ok(JsValue::null())
    }
}

fn document_get_elements_by_class_name(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let class_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let classes: Vec<String> = class_str
        .split_ascii_whitespace()
        .map(String::from)
        .collect();
    let root = with_current_runtime(|inner| Ok(inner.doc.root()))?;
    let node_ids = with_current_runtime(|inner| {
        let mut results = Vec::new();
        if classes.is_empty() {
            return Ok(results);
        }
        let mut stack = Vec::new();
        for &c in inner.doc.get_children(root).iter().rev() {
            stack.push(c);
        }
        while let Some(curr) = stack.pop() {
            if let Some(node) = inner.doc.node(curr) {
                if let Some(el) = node.as_element() {
                    if classes.iter().all(|c| el.has_class(c)) {
                        results.push(curr);
                    }
                }
            }
            for &c in inner.doc.get_children(curr).iter().rev() {
                stack.push(c);
            }
        }
        Ok(results)
    })?;
    wrap_elements_as_collection(node_ids, context)
}

fn element_get_elements_by_class_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let class_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let classes: Vec<String> = class_str
        .split_ascii_whitespace()
        .map(String::from)
        .collect();
    let node_ids = with_current_runtime(|inner| {
        let mut results = Vec::new();
        if classes.is_empty() {
            return Ok(results);
        }
        let mut stack = Vec::new();
        for &c in inner.doc.get_children(node_id).iter().rev() {
            stack.push(c);
        }
        while let Some(curr) = stack.pop() {
            if let Some(node) = inner.doc.node(curr) {
                if let Some(el) = node.as_element() {
                    if classes.iter().all(|c| el.has_class(c)) {
                        results.push(curr);
                    }
                }
            }
            for &c in inner.doc.get_children(curr).iter().rev() {
                stack.push(c);
            }
        }
        Ok(results)
    })?;
    wrap_elements_as_collection(node_ids, context)
}

fn document_get_elements_by_name(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let name_str = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let root = with_current_runtime(|inner| Ok(inner.doc.root()))?;
    let node_ids = with_current_runtime(|inner| {
        let mut results = Vec::new();
        let mut stack = Vec::new();
        for &c in inner.doc.get_children(root).iter().rev() {
            stack.push(c);
        }
        while let Some(curr) = stack.pop() {
            if let Some(node) = inner.doc.node(curr) {
                if let Some(el) = node.as_element() {
                    if el.get_attribute("name") == Some(&name_str) {
                        results.push(curr);
                    }
                }
            }
            for &c in inner.doc.get_children(curr).iter().rev() {
                stack.push(c);
            }
        }
        Ok(results)
    })?;
    wrap_elements_as_collection(node_ids, context)
}

fn element_get_elements_by_tag_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let tag = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let match_all = tag == "*";
    let node_ids = with_current_runtime(|inner| {
        let mut results = Vec::new();
        let mut stack = Vec::new();
        for &child in inner.doc.get_children(node_id).iter().rev() {
            stack.push(child);
        }
        while let Some(curr) = stack.pop() {
            if let Some(node) = inner.doc.node(curr) {
                if let Some(el) = node.as_element() {
                    if match_all || el.tag_name().eq_ignore_ascii_case(&tag) {
                        results.push(curr);
                    }
                }
            }
            for &child in inner.doc.get_children(curr).iter().rev() {
                stack.push(child);
            }
        }
        Ok(results)
    })?;
    wrap_elements_as_collection(node_ids, context)
}

fn document_get_elements_by_tag_name(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let tag = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let match_all = tag == "*";
    let node_ids = with_current_runtime(|inner| {
        let mut result = Vec::new();
        let mut stack = vec![inner.doc.root()];
        while let Some(node_id) = stack.pop() {
            if let Some(node) = inner.doc.node(node_id) {
                if let Some(el) = node.as_element() {
                    if match_all || el.tag_name().eq_ignore_ascii_case(&tag) {
                        result.push(node_id);
                    }
                }
            }
            for &child in inner.doc.get_children(node_id).iter().rev() {
                stack.push(child);
            }
        }
        Ok(result)
    })?;

    wrap_elements_as_collection(node_ids, context)
}

fn element_get_text_content(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let text = with_current_runtime(|inner| Ok(inner.doc.text_content(node_id)))?;
    Ok(js_string!(text).into())
}

fn element_set_text_content(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let text = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();

    with_current_runtime(|inner| {
        set_node_text_content(&mut inner.doc, node_id, &text);
        inner.dirty = true;
        Ok(())
    })?;

    Ok(JsValue::undefined())
}

fn serialize_node_to_html(doc: &Document, node_id: NodeId, out: &mut String) {
    if let Some(node) = doc.node(node_id) {
        match &node.node_type {
            lilies_dom::NodeType::Element(el) => {
                let tag = el.tag_name().to_ascii_lowercase();
                out.push('<');
                out.push_str(&tag);
                for (name, val) in &el.attributes {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    for c in val.chars() {
                        match c {
                            '&' => out.push_str("&amp;"),
                            '"' => out.push_str("&quot;"),
                            '<' => out.push_str("&lt;"),
                            '>' => out.push_str("&gt;"),
                            _ => out.push(c),
                        }
                    }
                    out.push('"');
                }
                out.push('>');

                if !lilies_html::is_void_element(&tag) {
                    for child in doc.get_children(node_id) {
                        serialize_node_to_html(doc, child, out);
                    }
                    out.push_str("</");
                    out.push_str(&tag);
                    out.push('>');
                }
            }
            lilies_dom::NodeType::Text(t) => {
                let parent_is_raw = node
                    .parent
                    .and_then(|pid| doc.node(pid))
                    .and_then(|pn| pn.as_element())
                    .is_some_and(|pel| {
                        let pt = pel.tag_name();
                        pt.eq_ignore_ascii_case("script") || pt.eq_ignore_ascii_case("style")
                    });
                if parent_is_raw {
                    out.push_str(&t.data);
                } else {
                    for c in t.data.chars() {
                        match c {
                            '&' => out.push_str("&amp;"),
                            '<' => out.push_str("&lt;"),
                            '>' => out.push_str("&gt;"),
                            _ => out.push(c),
                        }
                    }
                }
            }
            lilies_dom::NodeType::Comment(c) => {
                out.push_str("<!--");
                out.push_str(c);
                out.push_str("-->");
            }
            lilies_dom::NodeType::DocumentFragment | lilies_dom::NodeType::Document => {
                for child in doc.get_children(node_id) {
                    serialize_node_to_html(doc, child, out);
                }
            }
            lilies_dom::NodeType::DocumentType(dt) => {
                out.push_str("<!DOCTYPE ");
                out.push_str(&dt.name);
                out.push('>');
            }
        }
    }
}

fn element_get_inner_html(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let html = with_current_runtime(|inner| {
        let mut out = String::new();
        for child in inner.doc.get_children(node_id) {
            serialize_node_to_html(&inner.doc, child, &mut out);
        }
        Ok(out)
    })?;
    Ok(JsValue::new(js_string!(html)))
}

fn element_set_inner_html(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let html_str = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();

    let parsed_doc = lilies_html::parse_html(&html_str);
    let fragment_children = parsed_doc.get_children(parsed_doc.root());

    with_current_runtime(|inner| {
        let existing = inner.doc.get_children(node_id);
        for child in existing {
            let _ = inner.doc.remove_child(node_id, child);
        }

        for child in fragment_children {
            if let Some(imported) = import_node_into_doc(&parsed_doc, child, &mut inner.doc) {
                let _ = inner.doc.append_child(node_id, imported);
            }
        }

        inner.dirty = true;
        Ok(())
    })?;

    Ok(JsValue::undefined())
}

fn element_get_outer_html(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let html = with_current_runtime(|inner| {
        let mut out = String::new();
        serialize_node_to_html(&inner.doc, node_id, &mut out);
        Ok(out)
    })?;
    Ok(JsValue::new(js_string!(html)))
}

fn element_set_outer_html(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let html_str = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();

    let parsed_doc = lilies_html::parse_html(&html_str);
    let fragment_children = parsed_doc.get_children(parsed_doc.root());

    with_current_runtime(|inner| {
        let parent = inner
            .doc
            .node(node_id)
            .and_then(|n| n.parent)
            .ok_or_else(|| {
                JsNativeError::typ().with_message("Cannot set outerHTML on element with no parent")
            })?;

        for child in fragment_children {
            if let Some(imported) = import_node_into_doc(&parsed_doc, child, &mut inner.doc) {
                let _ = inner.doc.insert_before(parent, imported, node_id);
            }
        }

        let _ = inner.doc.remove_child(parent, node_id);
        inner.dirty = true;
        Ok(())
    })?;

    Ok(JsValue::undefined())
}

fn element_set_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let value = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();

    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                el.set_attribute(name, value);
                inner.dirty = true;
            }
        }
        Ok(())
    })?;

    Ok(JsValue::undefined())
}

fn element_get_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();

    let val = with_current_runtime(|inner| {
        let v = inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|el| el.get_attribute(&name))
            .map(String::from);
        Ok(v)
    })?;

    match val {
        Some(v) => Ok(js_string!(v).into()),
        None => Ok(JsValue::null()),
    }
}

fn element_remove_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                el.remove_attribute(&name);
                inner.dirty = true;
            }
        }
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn element_has_attribute(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let name = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let has = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.has_attribute(&name)))
    })?;
    Ok(JsValue::new(has))
}

fn element_add_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let event_type = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let callback_val = args.get_or_undefined(1);

    let callback = callback_val
        .as_object()
        .and_then(|obj| JsFunction::from_object(obj.clone()))
        .ok_or_else(|| {
            JsNativeError::typ().with_message("addEventListener second argument must be a function")
        })?;

    let lower_type = event_type.to_ascii_lowercase();
    with_current_runtime(|inner| {
        if lower_type == "click" {
            inner.click_listeners.entry(node_id).or_default().push(callback.clone());
        }
        inner.element_listeners.entry((node_id, lower_type)).or_default().push(callback);
        Ok(())
    })?;

    Ok(JsValue::undefined())
}

fn element_remove_event_listener(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let event_type = args.get_or_undefined(0).to_string(context)?.to_std_string_escaped();
    let callback_val = args.get_or_undefined(1);

    let lower_type = event_type.to_ascii_lowercase();
    if let Some(target_obj) = callback_val.as_object() {
        with_current_runtime(|inner| {
            if lower_type == "click" {
                if let Some(list) = inner.click_listeners.get_mut(&node_id) {
                    list.retain(|f| !boa_engine::object::JsObject::equals(f, target_obj));
                }
            }
            if let Some(list) = inner.element_listeners.get_mut(&(node_id, lower_type)) {
                list.retain(|f| !boa_engine::object::JsObject::equals(f, target_obj));
            }
            Ok(())
        })?;
    }
    Ok(JsValue::undefined())
}

fn element_dispatch_event(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let event_obj = args.get_or_undefined(0);
    let event_type = if let Some(obj) = event_obj.as_object() {
        obj.get(js_string!("type"), context)?
            .to_string(context)?
            .to_std_string_escaped()
    } else {
        args.get_or_undefined(0)
            .to_string(context)
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|_| "custom".to_string())
    };
    let event_val = if event_obj.is_object() {
        Some(event_obj.clone())
    } else {
        None
    };
    let allowed = dispatch_event_with_context(Some(node_id), &event_type, event_val, context)?;
    Ok(JsValue::new(allowed))
}

fn document_add_event_listener(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_type = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .to_ascii_lowercase();
    let callback_val = args.get_or_undefined(1);
    let callback = callback_val
        .as_object()
        .and_then(|obj| JsFunction::from_object(obj.clone()))
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("addEventListener second argument must be a function")
        })?;

    with_current_runtime(|inner| {
        inner
            .document_listeners
            .entry(event_type)
            .or_default()
            .push(callback);
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn document_remove_event_listener(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_type = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .to_ascii_lowercase();
    let callback_val = args.get_or_undefined(1);
    if let Some(target_obj) = callback_val.as_object() {
        with_current_runtime(|inner| {
            if let Some(list) = inner.document_listeners.get_mut(&event_type) {
                list.retain(|f| !boa_engine::object::JsObject::equals(f, target_obj));
            }
            Ok(())
        })?;
    }
    Ok(JsValue::undefined())
}

fn document_dispatch_event(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_obj = args.get_or_undefined(0);
    let event_type = if let Some(obj) = event_obj.as_object() {
        obj.get(js_string!("type"), context)?
            .to_string(context)?
            .to_std_string_escaped()
    } else {
        args.get_or_undefined(0)
            .to_string(context)
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|_| "custom".to_string())
    };
    let event_val = if event_obj.is_object() {
        Some(event_obj.clone())
    } else {
        None
    };
    let allowed = dispatch_event_with_context(None, &event_type, event_val, context)?;
    Ok(JsValue::new(allowed))
}

fn window_add_event_listener(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_type = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .to_ascii_lowercase();
    let callback_val = args.get_or_undefined(1);
    let callback = callback_val
        .as_object()
        .and_then(|obj| JsFunction::from_object(obj.clone()))
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("addEventListener second argument must be a function")
        })?;

    with_current_runtime(|inner| {
        inner
            .window_listeners
            .entry(event_type)
            .or_default()
            .push(callback);
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn window_remove_event_listener(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_type = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .to_ascii_lowercase();
    let callback_val = args.get_or_undefined(1);
    if let Some(target_obj) = callback_val.as_object() {
        with_current_runtime(|inner| {
            if let Some(list) = inner.window_listeners.get_mut(&event_type) {
                list.retain(|f| !boa_engine::object::JsObject::equals(f, target_obj));
            }
            Ok(())
        })?;
    }
    Ok(JsValue::undefined())
}

fn window_dispatch_event(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let event_obj = args.get_or_undefined(0);
    let event_type = if let Some(obj) = event_obj.as_object() {
        obj.get(js_string!("type"), context)?
            .to_string(context)?
            .to_std_string_escaped()
    } else {
        args.get_or_undefined(0)
            .to_string(context)
            .map(|s| s.to_std_string_escaped())
            .unwrap_or_else(|_| "custom".to_string())
    };
    let event_val = if event_obj.is_object() {
        Some(event_obj.clone())
    } else {
        None
    };
    let allowed = dispatch_event_with_context(None, &event_type, event_val, context)?;
    Ok(JsValue::new(allowed))
}

fn native_performance_now(
    _this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let elapsed = with_current_runtime(|inner| {
        Ok(inner.start_time.elapsed().as_secs_f64() * 1000.0)
    })?;
    Ok(JsValue::new(elapsed))
}

fn native_set_timeout(
    _this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let cb_arg = args.get_or_undefined(0);
    let callback = if let Some(func_obj) = cb_arg.as_callable() {
        if let Some(func) = JsFunction::from_object(func_obj.clone()) {
            TimerCallback::Function(func)
        } else {
            return Ok(JsValue::new(0));
        }
    } else if let Some(s) = cb_arg.as_string() {
        TimerCallback::Script(s.to_std_string_escaped())
    } else {
        return Ok(JsValue::new(0));
    };

    let delay_ms = args
        .get(1)
        .and_then(|v| v.as_number())
        .unwrap_or(0.0)
        .max(0.0);
    let delay = Duration::from_millis(delay_ms as u64);

    let pass_args = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        Vec::new()
    };

    let timer_id = with_current_runtime(|inner| {
        let id = inner.next_timer_id;
        inner.next_timer_id = inner.next_timer_id.wrapping_add(1).max(1);
        let target_time = Instant::now() + delay;
        inner.timers.push(Timer {
            id,
            callback,
            target_time,
            interval: None,
            args: pass_args,
        });
        Ok(id)
    })?;

    Ok(JsValue::new(timer_id as i32))
}

fn native_clear_timeout(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let id_val = args.get_or_undefined(0).to_u32(context).unwrap_or(0);
    with_current_runtime(|inner| {
        inner.timers.retain(|t| t.id != id_val);
        inner.cancelled_timers.insert(id_val);
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn native_set_interval(
    _this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let cb_arg = args.get_or_undefined(0);
    let callback = if let Some(func_obj) = cb_arg.as_callable() {
        if let Some(func) = JsFunction::from_object(func_obj.clone()) {
            TimerCallback::Function(func)
        } else {
            return Ok(JsValue::new(0));
        }
    } else if let Some(s) = cb_arg.as_string() {
        TimerCallback::Script(s.to_std_string_escaped())
    } else {
        return Ok(JsValue::new(0));
    };

    let delay_ms = args
        .get(1)
        .and_then(|v| v.as_number())
        .unwrap_or(0.0)
        .max(1.0);
    let interval = Duration::from_millis(delay_ms as u64);

    let pass_args = if args.len() > 2 {
        args[2..].to_vec()
    } else {
        Vec::new()
    };

    let timer_id = with_current_runtime(|inner| {
        let id = inner.next_timer_id;
        inner.next_timer_id = inner.next_timer_id.wrapping_add(1).max(1);
        let target_time = Instant::now() + interval;
        inner.timers.push(Timer {
            id,
            callback,
            target_time,
            interval: Some(interval),
            args: pass_args,
        });
        Ok(id)
    })?;

    Ok(JsValue::new(timer_id as i32))
}

fn native_clear_interval(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    native_clear_timeout(this, args, context)
}

fn native_request_animation_frame(
    _this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let cb_arg = args.get_or_undefined(0);
    let func = cb_arg
        .as_object()
        .and_then(|obj| JsFunction::from_object(obj.clone()))
        .ok_or_else(|| {
            JsNativeError::typ()
                .with_message("requestAnimationFrame argument must be a function")
        })?;

    let raf_id = with_current_runtime(|inner| {
        let id = inner.next_raf_id;
        inner.next_raf_id = inner.next_raf_id.wrapping_add(1).max(1);
        inner.animation_frames.push(AnimationFrame {
            id,
            callback: func,
        });
        Ok(id)
    })?;

    Ok(JsValue::new(raf_id as i32))
}

fn native_cancel_animation_frame(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let id_val = args.get_or_undefined(0).to_u32(context).unwrap_or(0);
    with_current_runtime(|inner| {
        inner.animation_frames.retain(|r| r.id != id_val);
        inner.cancelled_rafs.insert(id_val);
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn element_append_child(
    this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let parent_id = get_node_id_from_this(this)?;
    let child_val = args.get_or_undefined(0);
    let child_id = get_node_id_from_this(child_val)?;
    with_current_runtime(|inner| {
        inner
            .doc
            .append_child(parent_id, child_id)
            .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
        inner.dirty = true;
        Ok(())
    })?;
    Ok(child_val.clone())
}

fn element_remove_child(
    this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let parent_id = get_node_id_from_this(this)?;
    let child_val = args.get_or_undefined(0);
    let child_id = get_node_id_from_this(child_val)?;
    with_current_runtime(|inner| {
        inner
            .doc
            .remove_child(parent_id, child_id)
            .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
        inner.dirty = true;
        Ok(())
    })?;
    Ok(child_val.clone())
}

fn element_insert_before(
    this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let parent_id = get_node_id_from_this(this)?;
    let new_child_val = args.get_or_undefined(0);
    let new_child_id = get_node_id_from_this(new_child_val)?;
    let ref_child_val = args.get_or_undefined(1);

    with_current_runtime(|inner| {
        if ref_child_val.is_null_or_undefined() {
            inner
                .doc
                .append_child(parent_id, new_child_id)
                .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
        } else {
            let ref_child_id = get_node_id_from_this(ref_child_val)?;
            inner
                .doc
                .insert_before(parent_id, new_child_id, ref_child_id)
                .map_err(|e| JsNativeError::error().with_message(e.to_string()))?;
        }
        inner.dirty = true;
        Ok(())
    })?;
    Ok(new_child_val.clone())
}

fn element_remove(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node(node_id) {
            if let Some(parent) = node.parent {
                let _ = inner.doc.remove_child(parent, node_id);
                inner.dirty = true;
            }
        }
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn element_replace_child(
    this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let parent_id = get_node_id_from_this(this)?;
    let new_child_val = args.get_or_undefined(0);
    let old_child_val = args.get_or_undefined(1);
    let new_child_id = get_node_id_from_this(new_child_val)?;
    let old_child_id = get_node_id_from_this(old_child_val)?;

    with_current_runtime(|inner| {
        inner
            .doc
            .replace_child(parent_id, new_child_id, old_child_id)
            .map_err(|e| JsNativeError::typ().with_message(format!("replaceChild failed: {e:?}")))?;
        inner.dirty = true;
        Ok(())
    })?;

    Ok(old_child_val.clone())
}

fn element_clone_node(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let deep = args.get_or_undefined(0).to_boolean();

    let cloned_id = with_current_runtime(|inner| {
        inner
            .doc
            .clone_node(node_id, deep)
            .ok_or_else(|| JsNativeError::typ().with_message("cloneNode failed").into())
    })?;

    let obj = get_or_create_element_object(cloned_id, context)?;
    Ok(JsValue::new(obj))
}

fn element_contains(
    this: &JsValue,
    args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let other_val = args.get_or_undefined(0);
    let Ok(other_id) = get_node_id_from_this(other_val) else {
        return Ok(JsValue::new(false));
    };

    let contains = with_current_runtime(|inner| {
        let mut curr = Some(other_id);
        while let Some(curr_id) = curr {
            if curr_id == node_id {
                return Ok(true);
            }
            curr = inner.doc.node(curr_id).and_then(|n| n.parent);
        }
        Ok(false)
    })?;

    Ok(JsValue::new(contains))
}

fn import_node_into_doc(
    src_doc: &Document,
    src_id: NodeId,
    target_doc: &mut Document,
) -> Option<NodeId> {
    let src_node = src_doc.node(src_id)?;
    let new_id = match &src_node.node_type {
        lilies_dom::NodeType::Element(el) => {
            target_doc.create_element(&el.tag_name, el.attributes.clone())
        }
        lilies_dom::NodeType::Text(t) => target_doc.create_text(&t.data),
        lilies_dom::NodeType::Comment(c) => target_doc.create_comment(c),
        lilies_dom::NodeType::DocumentFragment => target_doc.create_document_fragment(),
        lilies_dom::NodeType::DocumentType(dt) => {
            target_doc.create_doctype(&dt.name, dt.public_id.clone(), dt.system_id.clone())
        }
        lilies_dom::NodeType::Document => return None,
    };

    for child in src_doc.get_children(src_id) {
        if let Some(imported_child) = import_node_into_doc(src_doc, child, target_doc) {
            let _ = target_doc.append_child(new_id, imported_child);
        }
    }

    Some(new_id)
}

fn element_insert_adjacent_html(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let position = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped()
        .to_ascii_lowercase();
    let html_text = args
        .get_or_undefined(1)
        .to_string(context)?
        .to_std_string_escaped();

    let parsed_doc = lilies_html::parse_html(&html_text);
    let fragment_children = parsed_doc.get_children(parsed_doc.root());

    with_current_runtime(|inner| {
        let mut imported_nodes = Vec::new();
        for child in fragment_children {
            if let Some(cloned) = import_node_into_doc(&parsed_doc, child, &mut inner.doc) {
                imported_nodes.push(cloned);
            }
        }

        match position.as_str() {
            "beforebegin" => {
                if let Some(parent) = inner.doc.node(node_id).and_then(|n| n.parent) {
                    for imported in imported_nodes {
                        let _ = inner.doc.insert_before(parent, imported, node_id);
                    }
                    inner.dirty = true;
                }
            }
            "afterbegin" => {
                let first_child = inner.doc.node(node_id).and_then(|n| n.first_child);
                for imported in imported_nodes {
                    if let Some(fc) = first_child {
                        let _ = inner.doc.insert_before(node_id, imported, fc);
                    } else {
                        let _ = inner.doc.append_child(node_id, imported);
                    }
                }
                inner.dirty = true;
            }
            "beforeend" => {
                for imported in imported_nodes {
                    let _ = inner.doc.append_child(node_id, imported);
                }
                inner.dirty = true;
            }
            "afterend" => {
                if let Some(parent) = inner.doc.node(node_id).and_then(|n| n.parent) {
                    let next_sibling = inner.doc.node(node_id).and_then(|n| n.next_sibling);
                    for imported in imported_nodes {
                        if let Some(ns) = next_sibling {
                            let _ = inner.doc.insert_before(parent, imported, ns);
                        } else {
                            let _ = inner.doc.append_child(parent, imported);
                        }
                    }
                    inner.dirty = true;
                }
            }
            _ => {}
        }
        Ok(())
    })?;

    Ok(JsValue::undefined())
}

fn internal_class_list_add(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let tokens_str = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();
    let to_add: Vec<&str> = tokens_str.split_ascii_whitespace().collect();

    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                let mut classes: Vec<String> = el.classes().iter().map(|&s| s.to_string()).collect();
                let mut changed = false;
                for item in to_add {
                    if !classes.iter().any(|c| c == item) {
                        classes.push(item.to_string());
                        changed = true;
                    }
                }
                if changed {
                    el.set_attribute("class", classes.join(" "));
                    inner.dirty = true;
                }
            }
        }
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn internal_class_list_remove(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let tokens_str = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();
    let to_remove: Vec<&str> = tokens_str.split_ascii_whitespace().collect();

    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                let mut classes: Vec<String> = el.classes().iter().map(|&s| s.to_string()).collect();
                let initial_len = classes.len();
                classes.retain(|c| !to_remove.contains(&c.as_str()));
                if classes.len() != initial_len {
                    el.set_attribute("class", classes.join(" "));
                    inner.dirty = true;
                }
            }
        }
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn internal_class_list_toggle(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let token = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();
    let token_trimmed = token.trim().to_string();
    let force_val = args.get_or_undefined(2);
    let force_opt = if force_val.is_null_or_undefined() {
        None
    } else {
        Some(force_val.to_boolean())
    };

    let result = with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                let mut classes: Vec<String> = el.classes().iter().map(|&s| s.to_string()).collect();
                let currently_has = classes.contains(&token_trimmed);
                let should_have = force_opt.unwrap_or(!currently_has);

                if should_have && !currently_has {
                    classes.push(token_trimmed);
                    el.set_attribute("class", classes.join(" "));
                    inner.dirty = true;
                } else if !should_have && currently_has {
                    classes.retain(|c| c != &token_trimmed);
                    el.set_attribute("class", classes.join(" "));
                    inner.dirty = true;
                }
                return Ok(should_have);
            }
        }
        Ok(false)
    })?;
    Ok(JsValue::new(result))
}

fn internal_class_list_contains(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let token = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();
    let token_trimmed = token.trim();
    let has = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .is_some_and(|el| el.has_class(token_trimmed)))
    })?;
    Ok(JsValue::new(has))
}

fn internal_class_list_item(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let idx = args.get_or_undefined(1).to_u32(context).unwrap_or(0) as usize;
    let val = with_current_runtime(|inner| {
        let classes = inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .map(|el| el.classes())
            .unwrap_or_default();
        Ok(classes.get(idx).map(|&s| s.to_string()))
    })?;
    match val {
        Some(s) => Ok(js_string!(s).into()),
        None => Ok(JsValue::null()),
    }
}

fn internal_class_list_to_string(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let class_str = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|el| el.get_attribute("class"))
            .unwrap_or("")
            .to_string())
    })?;
    Ok(js_string!(class_str).into())
}

fn internal_class_list_length(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let len = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .map(|el| el.classes().len())
            .unwrap_or(0))
    })?;
    Ok(JsValue::new(len as i32))
}

fn element_get_parent_node(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let parent = with_current_runtime(|inner| {
        let p = inner.doc.parent(node_id);
        if let Some(p_id) = p {
            if p_id != inner.doc.root() {
                return Ok(Some(p_id));
            }
        }
        Ok(None)
    })?;
    match parent {
        Some(p_id) => {
            let obj = get_or_create_element_object(p_id, context)?;
            Ok(JsValue::new(obj))
        }
        None => Ok(JsValue::null()),
    }
}

fn element_get_children(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let child_ids = with_current_runtime(|inner| {
        let mut list = Vec::new();
        for &c in inner.doc.get_children(node_id).iter() {
            if let Some(n) = inner.doc.node(c) {
                if n.is_element() {
                    list.push(c);
                }
            }
        }
        Ok(list)
    })?;

    wrap_elements_as_collection(child_ids, context)
}

fn element_get_child_nodes(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let child_ids = with_current_runtime(|inner| Ok(inner.doc.get_children(node_id)))?;
    wrap_elements_as_collection(child_ids, context)
}

fn element_get_first_child(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let fc = with_current_runtime(|inner| Ok(inner.doc.first_child(node_id)))?;
    match fc {
        Some(cid) => {
            let obj = get_or_create_element_object(cid, context)?;
            Ok(JsValue::new(obj))
        }
        None => Ok(JsValue::null()),
    }
}

fn element_get_last_child(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let lc = with_current_runtime(|inner| Ok(inner.doc.last_child(node_id)))?;
    match lc {
        Some(cid) => {
            let obj = get_or_create_element_object(cid, context)?;
            Ok(JsValue::new(obj))
        }
        None => Ok(JsValue::null()),
    }
}

fn element_get_first_element_child(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let child_id = with_current_runtime(|inner| {
        for &cid in inner.doc.get_children(node_id).iter() {
            if inner.doc.node(cid).is_some_and(|n| n.is_element()) {
                return Ok(Some(cid));
            }
        }
        Ok(None)
    })?;
    match child_id {
        Some(cid) => Ok(JsValue::new(get_or_create_element_object(cid, context)?)),
        None => Ok(JsValue::null()),
    }
}

fn element_get_last_element_child(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let child_id = with_current_runtime(|inner| {
        for &cid in inner.doc.get_children(node_id).iter().rev() {
            if inner.doc.node(cid).is_some_and(|n| n.is_element()) {
                return Ok(Some(cid));
            }
        }
        Ok(None)
    })?;
    match child_id {
        Some(cid) => Ok(JsValue::new(get_or_create_element_object(cid, context)?)),
        None => Ok(JsValue::null()),
    }
}

fn element_get_next_sibling(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let next_id = with_current_runtime(|inner| Ok(inner.doc.next_sibling(node_id)))?;
    match next_id {
        Some(nid) => Ok(JsValue::new(get_or_create_element_object(nid, context)?)),
        None => Ok(JsValue::null()),
    }
}

fn element_get_previous_sibling(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let prev_id = with_current_runtime(|inner| Ok(inner.doc.prev_sibling(node_id)))?;
    match prev_id {
        Some(pid) => Ok(JsValue::new(get_or_create_element_object(pid, context)?)),
        None => Ok(JsValue::null()),
    }
}

fn element_get_next_element_sibling(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let next_el = with_current_runtime(|inner| {
        let mut curr = inner.doc.next_sibling(node_id);
        while let Some(cid) = curr {
            if inner.doc.node(cid).is_some_and(|n| n.is_element()) {
                return Ok(Some(cid));
            }
            curr = inner.doc.next_sibling(cid);
        }
        Ok(None)
    })?;
    match next_el {
        Some(nid) => Ok(JsValue::new(get_or_create_element_object(nid, context)?)),
        None => Ok(JsValue::null()),
    }
}

fn element_get_previous_element_sibling(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let prev_el = with_current_runtime(|inner| {
        let mut curr = inner.doc.prev_sibling(node_id);
        while let Some(cid) = curr {
            if inner.doc.node(cid).is_some_and(|n| n.is_element()) {
                return Ok(Some(cid));
            }
            curr = inner.doc.prev_sibling(cid);
        }
        Ok(None)
    })?;
    match prev_el {
        Some(pid) => Ok(JsValue::new(get_or_create_element_object(pid, context)?)),
        None => Ok(JsValue::null()),
    }
}

fn element_get_node_type(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let node_type = with_current_runtime(|inner| {
        let val = inner.doc.node(node_id).map(|n| match &n.node_type {
            lilies_dom::NodeType::Element(_) => 1,
            lilies_dom::NodeType::Text(_) => 3,
            lilies_dom::NodeType::Comment(_) => 8,
            lilies_dom::NodeType::Document => 9,
            lilies_dom::NodeType::DocumentType(_) => 10,
            lilies_dom::NodeType::DocumentFragment => 11,
        }).unwrap_or(1);
        Ok(val)
    })?;
    Ok(JsValue::new(node_type))
}

fn element_get_node_name(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let name = with_current_runtime(|inner| {
        let n_name = inner.doc.node(node_id).map(|n| match &n.node_type {
            lilies_dom::NodeType::Element(el) => el.tag_name().to_ascii_uppercase(),
            lilies_dom::NodeType::Text(_) => "#text".to_string(),
            lilies_dom::NodeType::Comment(_) => "#comment".to_string(),
            lilies_dom::NodeType::Document => "#document".to_string(),
            lilies_dom::NodeType::DocumentType(dt) => dt.name.clone(),
            lilies_dom::NodeType::DocumentFragment => "#document-fragment".to_string(),
        }).unwrap_or_default();
        Ok(n_name)
    })?;
    Ok(JsValue::new(js_string!(name)))
}

fn element_get_node_value(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let val = with_current_runtime(|inner| {
        let v = inner.doc.node(node_id).and_then(|n| match &n.node_type {
            lilies_dom::NodeType::Text(t) => Some(t.data.clone()),
            lilies_dom::NodeType::Comment(c) => Some(c.clone()),
            _ => None,
        });
        Ok(v)
    })?;
    match val {
        Some(s) => Ok(JsValue::new(js_string!(s))),
        None => Ok(JsValue::null()),
    }
}

fn element_set_node_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let val_str = args
        .get_or_undefined(0)
        .to_string(context)?
        .to_std_string_escaped();
    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            match &mut node.node_type {
                lilies_dom::NodeType::Text(t) => {
                    t.data = val_str;
                    inner.dirty = true;
                }
                lilies_dom::NodeType::Comment(c) => {
                    *c = val_str;
                    inner.dirty = true;
                }
                _ => {}
            }
        }
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn element_get_tag_name(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let tag = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .map(|e| e.tag_name().to_ascii_uppercase()))
    })?;
    match tag {
        Some(t) => Ok(JsValue::new(js_string!(t))),
        None => Ok(JsValue::undefined()),
    }
}

fn element_get_id(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let id_val = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|e| e.get_attribute("id"))
            .unwrap_or_default()
            .to_string())
    })?;
    Ok(JsValue::new(js_string!(id_val)))
}

fn element_set_id(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    element_set_attribute(this, &[JsValue::new(js_string!("id")), args.get_or_undefined(0).clone()], context)
}

fn element_get_class_name(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let class_val = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|e| e.get_attribute("class"))
            .unwrap_or_default()
            .to_string())
    })?;
    Ok(JsValue::new(js_string!(class_val)))
}

fn element_set_class_name(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    element_set_attribute(this, &[JsValue::new(js_string!("class")), args.get_or_undefined(0).clone()], context)
}

fn element_get_value(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let val = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|e| e.get_attribute("value"))
            .unwrap_or_default()
            .to_string())
    })?;
    Ok(JsValue::new(js_string!(val)))
}

fn element_set_value(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    element_set_attribute(this, &[JsValue::new(js_string!("value")), args.get_or_undefined(0).clone()], context)
}

fn element_get_checked(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let is_checked = with_current_runtime(|inner| {
        Ok(inner.doc.node(node_id).and_then(|n| n.as_element()).is_some_and(|e| e.has_attribute("checked")))
    })?;
    Ok(JsValue::new(is_checked))
}

fn element_set_checked(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let val = args.get_or_undefined(0).to_boolean();
    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                if val {
                    el.set_attribute("checked", "");
                } else {
                    el.remove_attribute("checked");
                }
                inner.dirty = true;
            }
        }
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn element_get_src(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let val = with_current_runtime(|inner| {
        let v = inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|el| el.get_attribute("src"))
            .map(String::from);
        Ok(v)
    })?;
    Ok(val.map(|s| js_string!(s).into()).unwrap_or_else(|| js_string!("").into()))
}

fn element_set_src(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    element_set_attribute(this, &[JsValue::new(js_string!("src")), args.get_or_undefined(0).clone()], context)
}

fn element_get_href(
    this: &JsValue,
    _args: &[JsValue],
    _context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let val = with_current_runtime(|inner| {
        let v = inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|el| el.get_attribute("href"))
            .map(String::from);
        Ok(v)
    })?;
    Ok(val.map(|s| js_string!(s).into()).unwrap_or_else(|| js_string!("").into()))
}

fn element_set_href(
    this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    element_set_attribute(this, &[JsValue::new(js_string!("href")), args.get_or_undefined(0).clone()], context)
}

fn element_get_disabled(this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let is_disabled = with_current_runtime(|inner| {
        Ok(inner.doc.node(node_id).and_then(|n| n.as_element()).is_some_and(|e| e.has_attribute("disabled")))
    })?;
    Ok(JsValue::new(is_disabled))
}

fn element_set_disabled(this: &JsValue, args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let val = args.get_or_undefined(0).to_boolean();
    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                if val {
                    el.set_attribute("disabled", "");
                } else {
                    el.remove_attribute("disabled");
                }
                inner.dirty = true;
            }
        }
        Ok(())
    })?;
    Ok(JsValue::undefined())
}

fn element_get_form(
    this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    let form_node_id = with_current_runtime(|inner| {
        let mut curr = inner.doc.parent(node_id);
        while let Some(pid) = curr {
            if let Some(node) = inner.doc.node(pid) {
                if let Some(el) = node.as_element() {
                    if el.tag_name().eq_ignore_ascii_case("form") {
                        return Ok(Some(pid));
                    }
                }
            }
            curr = inner.doc.parent(pid);
        }
        Ok(None)
    })?;
    match form_node_id {
        Some(fid) => {
            let obj = get_or_create_element_object(fid, context)?;
            Ok(JsValue::new(obj))
        }
        None => Ok(JsValue::null()),
    }
}

fn element_focus(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

fn element_blur(_this: &JsValue, _args: &[JsValue], _context: &mut Context) -> JsResult<JsValue> {
    Ok(JsValue::undefined())
}

fn element_click(this: &JsValue, _args: &[JsValue], context: &mut Context) -> JsResult<JsValue> {
    let node_id = get_node_id_from_this(this)?;
    dispatch_click_with_context(node_id, context)?;
    Ok(JsValue::undefined())
}

fn dispatch_click_with_context(node_id: NodeId, context: &mut Context) -> JsResult<bool> {
    let target_id = with_current_runtime(|inner| {
        let mut curr = Some(node_id);
        let mut found = node_id;
        while let Some(cid) = curr {
            if inner.doc.node(cid).is_some_and(|n| n.is_element()) {
                found = cid;
                break;
            }
            curr = inner.doc.parent(cid);
        }
        Ok(found)
    })?;

    let target_obj = get_or_create_element_object(target_id, context)?;
    let create_event_fn = context
        .global_object()
        .get(js_string!("__createClickEvent"), context)?;

    let event_val = if let Some(create_fn) = create_event_fn.as_callable() {
        create_fn.call(
            &JsValue::undefined(),
            &[
                JsValue::new(target_obj.clone()),
                JsValue::new(target_obj.clone()),
            ],
            context,
        )?
    } else {
        JsValue::undefined()
    };

    let mut ancestor_chain = Vec::new();
    let mut current = Some(node_id);
    while let Some(curr_id) = current {
        let (listeners, cached_obj, parent_id) = with_current_runtime(|inner| {
            let list = inner
                .click_listeners
                .get(&curr_id)
                .cloned()
                .unwrap_or_default();
            let cached = inner.node_cache.get(&curr_id).cloned();
            let parent = inner.doc.parent(curr_id);
            Ok((list, cached, parent))
        })?;

        let mut cbs = listeners;
        if let Some(cached_obj) = cached_obj {
            if let Ok(onclick_val) = cached_obj.get(js_string!("onclick"), context) {
                if let Some(cb_obj) = onclick_val.as_callable() {
                    if let Some(cb) = JsFunction::from_object(cb_obj.clone()) {
                        cbs.push(cb);
                    }
                }
            }
        }
        if !cbs.is_empty() {
            ancestor_chain.push((curr_id, cbs));
        }
        current = parent_id;
    }

    let mut stopped = false;
    for (curr_id, cbs) in ancestor_chain {
        let current_target_obj = get_or_create_element_object(curr_id, context)?;
        if let Some(ev_obj) = event_val.as_object() {
            let _ = ev_obj.set(
                js_string!("currentTarget"),
                current_target_obj.clone(),
                false,
                context,
            );
        }
        let this_val = JsValue::new(current_target_obj);

        for cb in cbs {
            cb.call(&this_val, std::slice::from_ref(&event_val), context)?;
            if let Some(ev_obj) = event_val.as_object() {
                if ev_obj
                    .get(js_string!("_stopped"), context)
                    .map(|v| v.to_boolean())
                    .unwrap_or(false)
                {
                    stopped = true;
                }
            }
        }
        if stopped {
            let default_prevented = if let Some(ev_obj) = event_val.as_object() {
                ev_obj
                    .get(js_string!("defaultPrevented"), context)
                    .map(|v| v.to_boolean())
                    .unwrap_or(false)
            } else {
                false
            };
            return Ok(!default_prevented);
        }
    }

    // Bubble to document
    let doc_cbs = with_current_runtime(|inner| {
        Ok(inner
            .document_listeners
            .get("click")
            .cloned()
            .unwrap_or_default())
    })?;
    let doc_obj = context
        .global_object()
        .get(js_string!("document"), context)?;
    if let Some(ev_obj) = event_val.as_object() {
        let _ = ev_obj.set(
            js_string!("currentTarget"),
            doc_obj.clone(),
            false,
            context,
        );
    }
    for cb in doc_cbs {
        cb.call(&doc_obj, std::slice::from_ref(&event_val), context)?;
        if let Some(ev_obj) = event_val.as_object() {
            if ev_obj
                .get(js_string!("_stopped"), context)
                .map(|v| v.to_boolean())
                .unwrap_or(false)
            {
                stopped = true;
            }
        }
    }
    if let Some(doc_o) = doc_obj.as_object() {
        if let Ok(onclick_val) = doc_o.get(js_string!("onclick"), context) {
            if let Some(cb_obj) = onclick_val.as_callable() {
                if let Some(cb) = JsFunction::from_object(cb_obj.clone()) {
                    let _ = cb.call(&doc_obj, std::slice::from_ref(&event_val), context);
                }
            }
        }
    }
    if stopped {
        let default_prevented = if let Some(ev_obj) = event_val.as_object() {
            ev_obj
                .get(js_string!("defaultPrevented"), context)
                .map(|v| v.to_boolean())
                .unwrap_or(false)
        } else {
            false
        };
        return Ok(!default_prevented);
    }

    // Bubble to window
    let win_cbs = with_current_runtime(|inner| {
        Ok(inner
            .window_listeners
            .get("click")
            .cloned()
            .unwrap_or_default())
    })?;
    let win_obj = context.global_object();
    if let Some(ev_obj) = event_val.as_object() {
        let _ = ev_obj.set(
            js_string!("currentTarget"),
            win_obj.clone(),
            false,
            context,
        );
    }
    let win_val = JsValue::new(win_obj.clone());
    for cb in win_cbs {
        cb.call(&win_val, std::slice::from_ref(&event_val), context)?;
    }
    if let Ok(onclick_val) = win_obj.get(js_string!("onclick"), context) {
        if let Some(cb_obj) = onclick_val.as_callable() {
            if let Some(cb) = JsFunction::from_object(cb_obj.clone()) {
                let _ = cb.call(&win_val, std::slice::from_ref(&event_val), context);
            }
        }
    }

    let default_prevented = if let Some(ev_obj) = event_val.as_object() {
        ev_obj
            .get(js_string!("defaultPrevented"), context)
            .map(|v| v.to_boolean())
            .unwrap_or(false)
    } else {
        false
    };

    Ok(!default_prevented)
}

fn dispatch_event_with_context(
    target: Option<NodeId>,
    event_type: &str,
    event_val_override: Option<JsValue>,
    context: &mut Context,
) -> JsResult<bool> {
    let lower_type = event_type.to_ascii_lowercase();
    if target.is_some() && lower_type == "click" && event_val_override.is_none() {
        return dispatch_click_with_context(target.unwrap(), context);
    }

    let doc_obj = context
        .global_object()
        .get(js_string!("document"), context)?;
    let win_obj = context.global_object();

    let target_val = match target {
        Some(node_id) => {
            let target_id = with_current_runtime(|inner| {
                let mut curr = Some(node_id);
                let mut found = node_id;
                while let Some(cid) = curr {
                    if inner.doc.node(cid).is_some_and(|n| n.is_element()) {
                        found = cid;
                        break;
                    }
                    curr = inner.doc.parent(cid);
                }
                Ok(found)
            })?;
            JsValue::new(get_or_create_element_object(target_id, context)?)
        }
        None => {
            if lower_type == "load" {
                JsValue::new(win_obj.clone())
            } else {
                doc_obj.clone()
            }
        }
    };

    let create_event_fn = context
        .global_object()
        .get(js_string!("__createCustomEvent"), context)?;

    let event_val = if let Some(ev) = event_val_override {
        if let Some(ev_obj) = ev.as_object() {
            let _ = ev_obj.set(js_string!("target"), target_val.clone(), false, context);
            let _ = ev_obj.set(js_string!("srcElement"), target_val.clone(), false, context);
        }
        ev
    } else if let Some(create_fn) = create_event_fn.as_callable() {
        create_fn.call(
            &JsValue::undefined(),
            &[
                JsValue::new(js_string!(event_type)),
                target_val.clone(),
                target_val.clone(),
            ],
            context,
        )?
    } else {
        JsValue::undefined()
    };

    let bubbles = if let Some(ev_obj) = event_val.as_object() {
        ev_obj
            .get(js_string!("bubbles"), context)
            .map(|v| v.to_boolean())
            .unwrap_or(true)
    } else {
        true
    };

    let mut stopped = false;

    if let Some(node_id) = target {
        let mut ancestor_chain = Vec::new();
        let mut current = Some(node_id);
        while let Some(curr_id) = current {
            let (listeners, cached_obj, parent_id) = with_current_runtime(|inner| {
                let list = inner
                    .element_listeners
                    .get(&(curr_id, lower_type.clone()))
                    .cloned()
                    .unwrap_or_default();
                let cached = inner.node_cache.get(&curr_id).cloned();
                let parent = inner.doc.parent(curr_id);
                Ok((list, cached, parent))
            })?;

            let mut cbs = listeners;
            if let Some(cached_obj) = cached_obj {
                let on_prop_name = format!("on{}", lower_type);
                if let Ok(on_val) = cached_obj.get(js_string!(on_prop_name), context) {
                    if let Some(cb_obj) = on_val.as_callable() {
                        if let Some(cb) = JsFunction::from_object(cb_obj.clone()) {
                            cbs.push(cb);
                        }
                    }
                }
            }
            if !cbs.is_empty() {
                ancestor_chain.push((curr_id, cbs));
            }
            current = if bubbles { parent_id } else { None };
        }

        for (curr_id, cbs) in ancestor_chain {
            let current_target_obj = get_or_create_element_object(curr_id, context)?;
            if let Some(ev_obj) = event_val.as_object() {
                let _ = ev_obj.set(
                    js_string!("currentTarget"),
                    current_target_obj.clone(),
                    false,
                    context,
                );
            }
            let this_val = JsValue::new(current_target_obj);

            for cb in cbs {
                cb.call(&this_val, std::slice::from_ref(&event_val), context)?;
                if let Some(ev_obj) = event_val.as_object() {
                    if ev_obj
                        .get(js_string!("_stopped"), context)
                        .map(|v| v.to_boolean())
                        .unwrap_or(false)
                    {
                        stopped = true;
                    }
                }
            }
            if stopped {
                let default_prevented = if let Some(ev_obj) = event_val.as_object() {
                    ev_obj
                        .get(js_string!("defaultPrevented"), context)
                        .map(|v| v.to_boolean())
                        .unwrap_or(false)
                } else {
                    false
                };
                return Ok(!default_prevented);
            }
        }
    }

    if !bubbles && target.is_some() {
        let default_prevented = if let Some(ev_obj) = event_val.as_object() {
            ev_obj
                .get(js_string!("defaultPrevented"), context)
                .map(|v| v.to_boolean())
                .unwrap_or(false)
        } else {
            false
        };
        return Ok(!default_prevented);
    }

    // Document listeners
    let doc_cbs = with_current_runtime(|inner| {
        Ok(inner
            .document_listeners
            .get(&lower_type)
            .cloned()
            .unwrap_or_default())
    })?;
    if !doc_cbs.is_empty() || doc_obj.as_object().is_some() {
        if let Some(ev_obj) = event_val.as_object() {
            let _ = ev_obj.set(
                js_string!("currentTarget"),
                doc_obj.clone(),
                false,
                context,
            );
        }
        for cb in doc_cbs {
            cb.call(&doc_obj, std::slice::from_ref(&event_val), context)?;
            if let Some(ev_obj) = event_val.as_object() {
                if ev_obj
                    .get(js_string!("_stopped"), context)
                    .map(|v| v.to_boolean())
                    .unwrap_or(false)
                {
                    stopped = true;
                }
            }
        }
        if let Some(doc_o) = doc_obj.as_object() {
            let on_prop = format!("on{}", lower_type);
            if let Ok(on_val) = doc_o.get(js_string!(on_prop), context) {
                if let Some(cb_obj) = on_val.as_callable() {
                    if let Some(cb) = JsFunction::from_object(cb_obj.clone()) {
                        let _ = cb.call(&doc_obj, std::slice::from_ref(&event_val), context);
                    }
                }
            }
        }
        if stopped {
            let default_prevented = if let Some(ev_obj) = event_val.as_object() {
                ev_obj
                    .get(js_string!("defaultPrevented"), context)
                    .map(|v| v.to_boolean())
                    .unwrap_or(false)
            } else {
                false
            };
            return Ok(!default_prevented);
        }
    }

    // Window listeners
    let win_cbs = with_current_runtime(|inner| {
        Ok(inner
            .window_listeners
            .get(&lower_type)
            .cloned()
            .unwrap_or_default())
    })?;
    let win_val = JsValue::new(win_obj.clone());
    if let Some(ev_obj) = event_val.as_object() {
        let _ = ev_obj.set(
            js_string!("currentTarget"),
            win_obj.clone(),
            false,
            context,
        );
    }
    for cb in win_cbs {
        cb.call(&win_val, std::slice::from_ref(&event_val), context)?;
    }
    let on_prop = format!("on{}", lower_type);
    if let Ok(on_val) = win_obj.get(js_string!(on_prop), context) {
        if let Some(cb_obj) = on_val.as_callable() {
            if let Some(cb) = JsFunction::from_object(cb_obj.clone()) {
                let _ = cb.call(&win_val, std::slice::from_ref(&event_val), context);
            }
        }
    }

    let default_prevented = if let Some(ev_obj) = event_val.as_object() {
        ev_obj
            .get(js_string!("defaultPrevented"), context)
            .map(|v| v.to_boolean())
            .unwrap_or(false)
    } else {
        false
    };

    Ok(!default_prevented)
}

fn element_get_bounding_client_rect(
    _this: &JsValue,
    _args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let rect = ObjectInitializer::new(context)
        .property(js_string!("x"), 0.0, Attribute::all())
        .property(js_string!("y"), 0.0, Attribute::all())
        .property(js_string!("top"), 0.0, Attribute::all())
        .property(js_string!("left"), 0.0, Attribute::all())
        .property(js_string!("right"), 800.0, Attribute::all())
        .property(js_string!("bottom"), 600.0, Attribute::all())
        .property(js_string!("width"), 800.0, Attribute::all())
        .property(js_string!("height"), 600.0, Attribute::all())
        .build();
    Ok(JsValue::new(rect))
}

fn internal_set_style(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let prop_raw = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();
    let val_str = args.get_or_undefined(2).to_string(context)?.to_std_string_escaped();

    if prop_raw == "cssText" {
        with_current_runtime(|inner| {
            if let Some(node) = inner.doc.node_mut(node_id) {
                if let Some(el) = node.as_element_mut() {
                    el.set_attribute("style", val_str);
                    inner.dirty = true;
                }
            }
            Ok(())
        })?;
        return Ok(JsValue::undefined());
    }

    let kebab_prop = camel_to_kebab(&prop_raw);

    with_current_runtime(|inner| {
        if let Some(node) = inner.doc.node_mut(node_id) {
            if let Some(el) = node.as_element_mut() {
                let existing = el.get_attribute("style");
                let new_style = update_inline_style(existing, &kebab_prop, &val_str);
                el.set_attribute("style", new_style);
                inner.dirty = true;
            }
        }
        Ok(())
    })?;

    Ok(JsValue::undefined())
}

fn internal_get_style(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let prop_raw = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();

    if prop_raw == "cssText" {
        let val = with_current_runtime(|inner| {
            let v = inner
                .doc
                .node(node_id)
                .and_then(|n| n.as_element())
                .and_then(|el| el.get_attribute("style"))
                .map(String::from);
            Ok(v)
        })?;
        return match val {
            Some(v) => Ok(js_string!(v).into()),
            None => Ok(js_string!("").into()),
        };
    }

    let kebab_prop = camel_to_kebab(&prop_raw);

    let val = with_current_runtime(|inner| {
        let v = inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .and_then(|el| el.get_attribute("style"))
            .and_then(|s| get_inline_style_prop(Some(s), &kebab_prop));
        Ok(v)
    })?;

    match val {
        Some(v) => Ok(js_string!(v).into()),
        None => Ok(js_string!("").into()),
    }
}

fn internal_get_computed_style(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let prop_raw = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();
    let kebab_prop = camel_to_kebab(&prop_raw);

    let val = with_current_runtime(|inner| {
        let style_tree = lilies_css::compute_styles_for_document(&inner.doc);
        let style = style_tree.get(node_id).cloned().unwrap_or_default();
        let prop_val = match kebab_prop.as_str() {
            "display" => match style.display {
                lilies_css::Display::Block => "block",
                lilies_css::Display::Inline => "inline",
                lilies_css::Display::Flex => "flex",
                lilies_css::Display::None => "none",
            }
            .to_string(),
            "width" => match style.width {
                lilies_css::LengthOrAuto::Px(v) => format!("{v}px"),
                lilies_css::LengthOrAuto::Auto => "auto".to_string(),
            },
            "height" => match style.height {
                lilies_css::LengthOrAuto::Px(v) => format!("{v}px"),
                lilies_css::LengthOrAuto::Auto => "auto".to_string(),
            },
            "color" => format!(
                "rgb({}, {}, {})",
                style.color.r, style.color.g, style.color.b
            ),
            "background-color" => format!(
                "rgba({}, {}, {}, {})",
                style.background_color.r,
                style.background_color.g,
                style.background_color.b,
                style.background_color.a as f32 / 255.0
            ),
            "font-size" => format!("{}px", style.font_size),
            "margin-top" => match style.margin_top {
                lilies_css::LengthOrAuto::Px(v) => format!("{v}px"),
                _ => "0px".to_string(),
            },
            "margin-right" => match style.margin_right {
                lilies_css::LengthOrAuto::Px(v) => format!("{v}px"),
                _ => "0px".to_string(),
            },
            "margin-bottom" => match style.margin_bottom {
                lilies_css::LengthOrAuto::Px(v) => format!("{v}px"),
                _ => "0px".to_string(),
            },
            "margin-left" => match style.margin_left {
                lilies_css::LengthOrAuto::Px(v) => format!("{v}px"),
                _ => "0px".to_string(),
            },
            "padding-top" => format!("{}px", style.padding_top),
            "padding-right" => format!("{}px", style.padding_right),
            "padding-bottom" => format!("{}px", style.padding_bottom),
            "padding-left" => format!("{}px", style.padding_left),
            "border-top-width" | "border-top" => format!("{}px", style.border_top),
            "border-right-width" | "border-right" => format!("{}px", style.border_right),
            "border-bottom-width" | "border-bottom" => format!("{}px", style.border_bottom),
            "border-left-width" | "border-left" => format!("{}px", style.border_left),
            "text-align" => match style.text_align {
                lilies_css::TextAlign::Left => "left",
                lilies_css::TextAlign::Center => "center",
                lilies_css::TextAlign::Right => "right",
            }
            .to_string(),
            _ => "".to_string(),
        };
        Ok(prop_val)
    })?;

    Ok(js_string!(val).into())
}

/// Returns the cached JavaScript Element object for `node_id`, or creates and caches a new one.
pub fn get_or_create_element_object(
    node_id: NodeId,
    context: &mut Context,
) -> JsResult<boa_engine::object::JsObject> {
    let cached = with_current_runtime(|inner| Ok(inner.node_cache.get(&node_id).cloned()))?;

    if let Some(obj) = cached {
        return Ok(obj);
    }

    let obj = create_element_object(node_id, context)?;

    with_current_runtime(|inner| {
        inner.node_cache.insert(node_id, obj.clone());
        Ok(())
    })?;

    Ok(obj)
}

/// Constructs a JavaScript Element object wrapping `node_id`.
pub fn create_element_object(
    node_id: NodeId,
    context: &mut Context,
) -> JsResult<boa_engine::object::JsObject> {
    let get_text = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_text_content))
        .name(js_string!("get textContent")).build();
    let set_text = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_text_content))
        .name(js_string!("set textContent")).length(1).build();

    let get_inner_html = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_inner_html))
        .name(js_string!("get innerHTML")).build();
    let set_inner_html = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_inner_html))
        .name(js_string!("set innerHTML")).length(1).build();

    let get_outer_html = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_outer_html))
        .name(js_string!("get outerHTML")).build();
    let set_outer_html = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_outer_html))
        .name(js_string!("set outerHTML")).length(1).build();

    let get_parent = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_parent_node))
        .name(js_string!("get parentNode")).build();
    let get_children = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_children))
        .name(js_string!("get children")).build();
    let get_child_nodes = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_child_nodes))
        .name(js_string!("get childNodes")).build();
    let get_first_child = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_first_child))
        .name(js_string!("get firstChild")).build();
    let get_last_child = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_last_child))
        .name(js_string!("get lastChild")).build();
    let get_first_element_child = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_first_element_child))
        .name(js_string!("get firstElementChild")).build();
    let get_last_element_child = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_last_element_child))
        .name(js_string!("get lastElementChild")).build();
    let get_next_sibling = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_next_sibling))
        .name(js_string!("get nextSibling")).build();
    let get_prev_sibling = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_previous_sibling))
        .name(js_string!("get previousSibling")).build();
    let get_next_element_sibling = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_next_element_sibling))
        .name(js_string!("get nextElementSibling")).build();
    let get_prev_element_sibling = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_previous_element_sibling))
        .name(js_string!("get previousElementSibling")).build();

    let get_node_type = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_node_type))
        .name(js_string!("get nodeType")).build();
    let get_node_name = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_node_name))
        .name(js_string!("get nodeName")).build();
    let get_node_value = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_node_value))
        .name(js_string!("get nodeValue")).build();
    let set_node_value = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_node_value))
        .name(js_string!("set nodeValue")).length(1).build();

    let get_tag_name = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_tag_name))
        .name(js_string!("get tagName")).build();

    let get_id = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_id))
        .name(js_string!("get id")).build();
    let set_id = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_id))
        .name(js_string!("set id")).length(1).build();

    let get_class = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_class_name))
        .name(js_string!("get className")).build();
    let set_class = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_class_name))
        .name(js_string!("set className")).length(1).build();

    let get_val = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_value))
        .name(js_string!("get value")).build();
    let set_val = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_value))
        .name(js_string!("set value")).length(1).build();

    let get_checked = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_checked))
        .name(js_string!("get checked")).build();
    let set_checked = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_checked))
        .name(js_string!("set checked")).length(1).build();

    let get_disabled = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_disabled))
        .name(js_string!("get disabled")).build();
    let set_disabled = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_disabled))
        .name(js_string!("set disabled")).length(1).build();

    let get_form = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_form))
        .name(js_string!("get form")).build();

    let get_src = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_src))
        .name(js_string!("get src")).build();
    let set_src = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_src))
        .name(js_string!("set src")).length(1).build();

    let get_href = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_href))
        .name(js_string!("get href")).build();
    let set_href = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_set_href))
        .name(js_string!("set href")).length(1).build();

    let dataset = ObjectInitializer::new(context).build();
    let mut obj_init = ObjectInitializer::with_native_data(ElementNode(node_id), context);
    obj_init
        .accessor(js_string!("textContent"), Some(get_text), Some(set_text), Attribute::all())
        .accessor(js_string!("innerHTML"), Some(get_inner_html), Some(set_inner_html), Attribute::all())
        .accessor(js_string!("outerHTML"), Some(get_outer_html), Some(set_outer_html), Attribute::all())
        .accessor(js_string!("src"), Some(get_src), Some(set_src), Attribute::all())
        .accessor(js_string!("href"), Some(get_href), Some(set_href), Attribute::all())
        .accessor(js_string!("parentNode"), Some(get_parent.clone()), None, Attribute::all())
        .accessor(js_string!("parentElement"), Some(get_parent), None, Attribute::all())
        .accessor(js_string!("children"), Some(get_children), None, Attribute::all())
        .accessor(js_string!("childNodes"), Some(get_child_nodes), None, Attribute::all())
        .accessor(js_string!("firstChild"), Some(get_first_child), None, Attribute::all())
        .accessor(js_string!("lastChild"), Some(get_last_child), None, Attribute::all())
        .accessor(js_string!("firstElementChild"), Some(get_first_element_child), None, Attribute::all())
        .accessor(js_string!("lastElementChild"), Some(get_last_element_child), None, Attribute::all())
        .accessor(js_string!("nextSibling"), Some(get_next_sibling), None, Attribute::all())
        .accessor(js_string!("previousSibling"), Some(get_prev_sibling), None, Attribute::all())
        .accessor(js_string!("nextElementSibling"), Some(get_next_element_sibling), None, Attribute::all())
        .accessor(js_string!("previousElementSibling"), Some(get_prev_element_sibling), None, Attribute::all())
        .accessor(js_string!("nodeType"), Some(get_node_type), None, Attribute::all())
        .accessor(js_string!("nodeName"), Some(get_node_name), None, Attribute::all())
        .accessor(js_string!("nodeValue"), Some(get_node_value), Some(set_node_value), Attribute::all())
        .accessor(js_string!("tagName"), Some(get_tag_name), None, Attribute::all())
        .accessor(js_string!("id"), Some(get_id), Some(set_id), Attribute::all())
        .accessor(js_string!("className"), Some(get_class), Some(set_class), Attribute::all())
        .accessor(js_string!("value"), Some(get_val), Some(set_val), Attribute::all())
        .accessor(js_string!("checked"), Some(get_checked), Some(set_checked), Attribute::all())
        .accessor(js_string!("disabled"), Some(get_disabled), Some(set_disabled), Attribute::all())
        .accessor(js_string!("form"), Some(get_form), None, Attribute::all())
        .function(NativeFunction::from_fn_ptr(element_set_attribute), js_string!("setAttribute"), 2)
        .function(NativeFunction::from_fn_ptr(element_get_attribute), js_string!("getAttribute"), 1)
        .function(NativeFunction::from_fn_ptr(element_remove_attribute), js_string!("removeAttribute"), 1)
        .function(NativeFunction::from_fn_ptr(element_has_attribute), js_string!("hasAttribute"), 1)
        .function(NativeFunction::from_fn_ptr(element_add_event_listener), js_string!("addEventListener"), 2)
        .function(NativeFunction::from_fn_ptr(element_remove_event_listener), js_string!("removeEventListener"), 2)
        .function(NativeFunction::from_fn_ptr(element_dispatch_event), js_string!("dispatchEvent"), 1)
        .function(NativeFunction::from_fn_ptr(element_append_child), js_string!("appendChild"), 1)
        .function(NativeFunction::from_fn_ptr(element_remove_child), js_string!("removeChild"), 1)
        .function(NativeFunction::from_fn_ptr(element_insert_before), js_string!("insertBefore"), 2)
        .function(NativeFunction::from_fn_ptr(element_focus), js_string!("focus"), 0)
        .function(NativeFunction::from_fn_ptr(element_blur), js_string!("blur"), 0)
        .function(NativeFunction::from_fn_ptr(element_click), js_string!("click"), 0)
        .function(NativeFunction::from_fn_ptr(element_get_bounding_client_rect), js_string!("getBoundingClientRect"), 0)
        .function(NativeFunction::from_fn_ptr(element_query_selector), js_string!("querySelector"), 1)
        .function(NativeFunction::from_fn_ptr(element_query_selector_all), js_string!("querySelectorAll"), 1)
        .function(NativeFunction::from_fn_ptr(element_matches), js_string!("matches"), 1)
        .function(NativeFunction::from_fn_ptr(element_closest), js_string!("closest"), 1)
        .function(NativeFunction::from_fn_ptr(element_get_elements_by_class_name), js_string!("getElementsByClassName"), 1)
        .function(NativeFunction::from_fn_ptr(element_get_elements_by_tag_name), js_string!("getElementsByTagName"), 1)
        .function(NativeFunction::from_fn_ptr(element_remove), js_string!("remove"), 0)
        .function(NativeFunction::from_fn_ptr(element_replace_child), js_string!("replaceChild"), 2)
        .function(NativeFunction::from_fn_ptr(element_clone_node), js_string!("cloneNode"), 1)
        .function(NativeFunction::from_fn_ptr(element_contains), js_string!("contains"), 1)
        .function(NativeFunction::from_fn_ptr(element_insert_adjacent_html), js_string!("insertAdjacentHTML"), 2)
        .property(js_string!("clientWidth"), 800, Attribute::all())
        .property(js_string!("clientHeight"), 600, Attribute::all())
        .property(js_string!("offsetWidth"), 800, Attribute::all())
        .property(js_string!("offsetHeight"), 600, Attribute::all())
        .property(js_string!("scrollWidth"), 800, Attribute::all())
        .property(js_string!("scrollHeight"), 600, Attribute::all())
        .property(js_string!("offsetTop"), 0, Attribute::all())
        .property(js_string!("offsetLeft"), 0, Attribute::all())
        .property(js_string!("clientTop"), 0, Attribute::all())
        .property(js_string!("clientLeft"), 0, Attribute::all())
        .property(js_string!("dataset"), dataset, Attribute::all());

    let obj = obj_init.build();
    obj.set(js_string!("__nodeId"), JsValue::new(node_id.0 as i64), false, context)?;
    let create_classlist_fn = context
        .global_object()
        .get(js_string!("__createClassList"), context)?;
    if let Some(create_fn) = create_classlist_fn.as_callable() {
        let cl = create_fn.call(
            &JsValue::undefined(),
            &[JsValue::new(node_id.0 as i64)],
            context,
        )?;
        obj.set(js_string!("classList"), cl, false, context)?;
    }

    // Create the style Proxy via the global helper
    let create_style_fn = context
        .global_object()
        .get(js_string!("__createStyleProxy"), context)?;
    if let Some(create_fn) = create_style_fn.as_callable() {
        let style_proxy = create_fn.call(
            &JsValue::undefined(),
            &[JsValue::new(node_id.0 as i64)],
            context,
        )?;
        obj.set(js_string!("style"), style_proxy, true, context)?;
    }

    // If element is a form, bind named child inputs
    let is_form = with_current_runtime(|inner| {
        Ok(inner
            .doc
            .node(node_id)
            .and_then(|n| n.as_element())
            .is_some_and(|e| e.tag_name().eq_ignore_ascii_case("form")))
    })?;

    if is_form {
        let submit_fn = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_fn_ptr(|_this, _args, _context| {
                Ok(JsValue::undefined())
            }),
        )
        .name(js_string!("submit"))
        .build();

        let reset_fn = FunctionObjectBuilder::new(
            context.realm(),
            NativeFunction::from_fn_ptr(|this, _args, _context| {
                let form_id = get_node_id_from_this(this)?;
                with_current_runtime(|inner| {
                    let mut stack = vec![form_id];
                    while let Some(curr) = stack.pop() {
                        if curr != form_id {
                            if let Some(node) = inner.doc.node_mut(curr) {
                                if let Some(el) = node.as_element_mut() {
                                    if el.tag_name().eq_ignore_ascii_case("input") {
                                        let default_val = el
                                            .get_attribute("value")
                                            .map(String::from)
                                            .unwrap_or_default();
                                        el.set_attribute("value", default_val);
                                        inner.dirty = true;
                                    }
                                }
                            }
                        }
                        for &c in inner.doc.get_children(curr).iter().rev() {
                            stack.push(c);
                        }
                    }
                    Ok(())
                })?;
                Ok(JsValue::undefined())
            }),
        )
        .name(js_string!("reset"))
        .build();

        obj.set(js_string!("submit"), submit_fn, false, context)?;
        obj.set(js_string!("reset"), reset_fn, false, context)?;

        let named_children = with_current_runtime(|inner| {
            let mut list = Vec::new();
            let mut stack = vec![node_id];
            while let Some(curr) = stack.pop() {
                if curr != node_id {
                    if let Some(node) = inner.doc.node(curr) {
                        if let Some(el) = node.as_element() {
                            if let Some(name) = el.get_attribute("name") {
                                list.push((curr, name.to_string()));
                            }
                        }
                    }
                }
                for &c in inner.doc.get_children(curr).iter().rev() {
                    stack.push(c);
                }
            }
            Ok(list)
        })?;

        let form_elements = JsArray::new(context);
        for (cid, name) in named_children {
            let child_obj = get_or_create_element_object(cid, context)?;
            form_elements.push(JsValue::new(child_obj.clone()), context)?;
            let _ = form_elements.set(js_string!(name.clone()), child_obj.clone(), false, context);
            if name != "submit" && name != "reset" {
                obj.set(js_string!(name), child_obj, false, context)?;
            }
        }
        obj.set(js_string!("elements"), form_elements, false, context)?;
    }

    Ok(obj)
}

/// A JavaScript runtime for lilies executing ECMAScript with a bridge to the DOM.
pub struct JsRuntime {
    context: Context,
    inner: Rc<RefCell<RuntimeInner>>,
}

impl JsRuntime {
    /// Creates a new `JsRuntime` initialized with the given DOM document.
    pub fn new(doc: Document) -> Self {
        let mut context = Context::default();
        let inner = Rc::new(RefCell::new(RuntimeInner {
            doc,
            dirty: false,
            click_listeners: HashMap::new(),
            node_cache: HashMap::new(),
            document_listeners: HashMap::new(),
            window_listeners: HashMap::new(),
            element_listeners: HashMap::new(),
            start_time: Instant::now(),
            next_timer_id: 1,
            timers: Vec::new(),
            cancelled_timers: HashSet::new(),
            next_raf_id: 1,
            animation_frames: Vec::new(),
            cancelled_rafs: HashSet::new(),
        }));

        let _guard = ActiveRuntimeGuard::new(&inner);

        // Global aliases: window, self, top, parent
        let global = context.global_object();
        let _ = context.register_global_property(js_string!("window"), global.clone(), Attribute::all());
        let _ = context.register_global_property(js_string!("self"), global.clone(), Attribute::all());
        let _ = context.register_global_property(js_string!("top"), global.clone(), Attribute::all());
        let _ = context.register_global_property(js_string!("parent"), global, Attribute::all());

        // 1. Register console
        let console = ObjectInitializer::new(&mut context)
            .function(NativeFunction::from_fn_ptr(console_log), js_string!("log"), 1)
            .function(NativeFunction::from_fn_ptr(console_log), js_string!("warn"), 1)
            .function(NativeFunction::from_fn_ptr(console_log), js_string!("error"), 1)
            .function(NativeFunction::from_fn_ptr(console_log), js_string!("info"), 1)
            .function(NativeFunction::from_fn_ptr(console_log), js_string!("debug"), 1)
            .build();
        let _ = context.register_global_property(
            js_string!("console"),
            console,
            Attribute::all(),
        );

        // 2. Register document
        let document = ObjectInitializer::new(&mut context)
            .function(
                NativeFunction::from_fn_ptr(document_get_element_by_id),
                js_string!("getElementById"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(document_query_selector),
                js_string!("querySelector"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(document_query_selector_all),
                js_string!("querySelectorAll"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(document_get_elements_by_class_name),
                js_string!("getElementsByClassName"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(document_get_elements_by_name),
                js_string!("getElementsByName"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(document_get_elements_by_tag_name),
                js_string!("getElementsByTagName"),
                1,
            )
            .function(
                NativeFunction::from_fn_ptr(document_add_event_listener),
                js_string!("addEventListener"),
                2,
            )
            .function(
                NativeFunction::from_fn_ptr(document_remove_event_listener),
                js_string!("removeEventListener"),
                2,
            )
            .function(
                NativeFunction::from_fn_ptr(document_dispatch_event),
                js_string!("dispatchEvent"),
                1,
            )
            .build();
        let _ = context.register_global_property(
            js_string!("document"),
            document.clone(),
            Attribute::all(),
        );

        // 3. Register internal DOM helpers and global event listeners
        let _ = context.register_global_callable(
            js_string!("addEventListener"),
            2,
            NativeFunction::from_fn_ptr(window_add_event_listener),
        );
        let _ = context.register_global_callable(
            js_string!("removeEventListener"),
            2,
            NativeFunction::from_fn_ptr(window_remove_event_listener),
        );
        let _ = context.register_global_callable(
            js_string!("dispatchEvent"),
            1,
            NativeFunction::from_fn_ptr(window_dispatch_event),
        );
        let _ = context.register_global_callable(
            js_string!("__window_add_event_listener"),
            2,
            NativeFunction::from_fn_ptr(window_add_event_listener),
        );
        let _ = context.register_global_callable(
            js_string!("__window_remove_event_listener"),
            2,
            NativeFunction::from_fn_ptr(window_remove_event_listener),
        );
        let _ = context.register_global_callable(
            js_string!("setTimeout"),
            2,
            NativeFunction::from_fn_ptr(native_set_timeout),
        );
        let _ = context.register_global_callable(
            js_string!("clearTimeout"),
            1,
            NativeFunction::from_fn_ptr(native_clear_timeout),
        );
        let _ = context.register_global_callable(
            js_string!("setInterval"),
            2,
            NativeFunction::from_fn_ptr(native_set_interval),
        );
        let _ = context.register_global_callable(
            js_string!("clearInterval"),
            1,
            NativeFunction::from_fn_ptr(native_clear_interval),
        );
        let _ = context.register_global_callable(
            js_string!("requestAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(native_request_animation_frame),
        );
        let _ = context.register_global_callable(
            js_string!("cancelAnimationFrame"),
            1,
            NativeFunction::from_fn_ptr(native_cancel_animation_frame),
        );
        let _ = context.register_global_callable(
            js_string!("__performance_now"),
            0,
            NativeFunction::from_fn_ptr(native_performance_now),
        );
        let _ = context.register_global_callable(
            js_string!("__set_style_internal"),
            3,
            NativeFunction::from_fn_ptr(internal_set_style),
        );
        let _ = context.register_global_callable(
            js_string!("__get_style_internal"),
            2,
            NativeFunction::from_fn_ptr(internal_get_style),
        );
        let _ = context.register_global_callable(
            js_string!("__get_computed_style_internal"),
            2,
            NativeFunction::from_fn_ptr(internal_get_computed_style),
        );
        let _ = context.register_global_callable(
            js_string!("__get_document_element"),
            0,
            NativeFunction::from_fn_ptr(document_get_document_element),
        );
        let _ = context.register_global_callable(
            js_string!("__get_document_body"),
            0,
            NativeFunction::from_fn_ptr(document_get_body),
        );
        let _ = context.register_global_callable(
            js_string!("__get_document_head"),
            0,
            NativeFunction::from_fn_ptr(document_get_head),
        );
        let _ = context.register_global_callable(
            js_string!("__create_element"),
            1,
            NativeFunction::from_fn_ptr(document_create_element),
        );
        let _ = context.register_global_callable(
            js_string!("__create_text_node"),
            1,
            NativeFunction::from_fn_ptr(document_create_text_node),
        );
        let _ = context.register_global_callable(
            js_string!("__create_comment"),
            1,
            NativeFunction::from_fn_ptr(document_create_comment),
        );
        let _ = context.register_global_callable(
            js_string!("__create_document_fragment"),
            0,
            NativeFunction::from_fn_ptr(document_create_document_fragment),
        );
        let _ = context.register_global_callable(
            js_string!("__get_document_child_nodes"),
            0,
            NativeFunction::from_fn_ptr(document_get_child_nodes),
        );
        let _ = context.register_global_callable(
            js_string!("__get_elements_by_tag_name"),
            1,
            NativeFunction::from_fn_ptr(document_get_elements_by_tag_name),
        );
        let _ = context.register_global_callable(
            js_string!("__class_list_add"),
            2,
            NativeFunction::from_fn_ptr(internal_class_list_add),
        );
        let _ = context.register_global_callable(
            js_string!("__class_list_remove"),
            2,
            NativeFunction::from_fn_ptr(internal_class_list_remove),
        );
        let _ = context.register_global_callable(
            js_string!("__class_list_toggle"),
            3,
            NativeFunction::from_fn_ptr(internal_class_list_toggle),
        );
        let _ = context.register_global_callable(
            js_string!("__class_list_contains"),
            2,
            NativeFunction::from_fn_ptr(internal_class_list_contains),
        );
        let _ = context.register_global_callable(
            js_string!("__class_list_item"),
            2,
            NativeFunction::from_fn_ptr(internal_class_list_item),
        );
        let _ = context.register_global_callable(
            js_string!("__class_list_to_string"),
            1,
            NativeFunction::from_fn_ptr(internal_class_list_to_string),
        );
        let _ = context.register_global_callable(
            js_string!("__class_list_length"),
            1,
            NativeFunction::from_fn_ptr(internal_class_list_length),
        );

        // Bind named forms to document (e.g. document.f)
        let named_forms = with_current_runtime(|inner| {
            let mut list = Vec::new();
            let mut stack = vec![inner.doc.root()];
            while let Some(node_id) = stack.pop() {
                if let Some(node) = inner.doc.node(node_id) {
                    if let Some(el) = node.as_element() {
                        if el.tag_name().eq_ignore_ascii_case("form") {
                            if let Some(name) = el.get_attribute("name") {
                                list.push((node_id, name.to_string()));
                            }
                        }
                    }
                }
                for &child in inner.doc.get_children(node_id).iter().rev() {
                    stack.push(child);
                }
            }
            Ok(list)
        })
        .unwrap_or_default();

        for (form_id, name) in named_forms {
            if let Ok(form_obj) = get_or_create_element_object(form_id, &mut context) {
                let _ = document.set(js_string!(name), form_obj, false, &mut context);
            }
        }

        let empty_images = JsArray::new(&mut context);
        let _ = document.set(js_string!("images"), empty_images, false, &mut context);

        // 4. Register browser environment preamble & style proxy factory
        let browser_preamble = r##"
            globalThis.__createStyleProxy = function(nodeId) {
                return new Proxy({}, {
                    get(target, prop) {
                        if (typeof prop === 'string') {
                            if (prop === 'setProperty') {
                                return function(k, v) { __set_style_internal(nodeId, k, v); };
                            }
                            if (prop === 'removeProperty') {
                                return function(k) {
                                    var old = __get_style_internal(nodeId, k);
                                    __set_style_internal(nodeId, k, "");
                                    return old;
                                };
                            }
                            if (prop === 'getPropertyValue') {
                                return function(k) { return __get_style_internal(nodeId, k); };
                            }
                            if (prop === 'item') {
                                return function(i) { return ""; };
                            }
                            if (prop === 'toString') {
                                return function() { return "[object CSSStyleDeclaration]"; };
                            }
                            if (prop === 'valueOf') {
                                return function() { return target; };
                            }
                            if (prop in Object.prototype) {
                                return Object.prototype[prop];
                            }
                            return __get_style_internal(nodeId, prop);
                        }
                        if (typeof prop === 'symbol') {
                            if (prop === Symbol.toPrimitive) {
                                return function(hint) { return "[object CSSStyleDeclaration]"; };
                            }
                            if (prop === Symbol.toStringTag) {
                                return "CSSStyleDeclaration";
                            }
                            return target[prop];
                        }
                        return target[prop];
                    },
                    set(target, prop, value) {
                        if (typeof prop === 'string') {
                            __set_style_internal(nodeId, prop, String(value));
                        }
                        target[prop] = value;
                        return true;
                    }
                });
            };

            globalThis.__createClassList = function(nodeId) {
                return {
                    add: function(...tokens) {
                        __class_list_add(nodeId, tokens.join(" "));
                    },
                    remove: function(...tokens) {
                        __class_list_remove(nodeId, tokens.join(" "));
                    },
                    toggle: function(token, force) {
                        return __class_list_toggle(nodeId, token, force !== undefined ? !!force : null);
                    },
                    contains: function(token) {
                        return __class_list_contains(nodeId, token);
                    },
                    item: function(idx) {
                        return __class_list_item(nodeId, idx);
                    },
                    toString: function() {
                        return __class_list_to_string(nodeId);
                    },
                    get length() {
                        return __class_list_length(nodeId);
                    }
                };
            };

            window.innerWidth = 1200;
            window.innerHeight = 800;
            window.outerWidth = 1200;
            window.outerHeight = 800;
            window.scrollX = 0;
            window.scrollY = 0;
            window.pageXOffset = 0;
            window.pageYOffset = 0;
            window.scrollTo = function() {};
            window.scrollBy = function() {};

            window.getComputedStyle = function(el) {
                if (!el || typeof el.__nodeId !== 'number') return null;
                var nodeId = el.__nodeId;
                return new Proxy({}, {
                    get(target, prop) {
                        if (typeof prop === 'string') {
                            if (prop === 'getPropertyValue') {
                                return function(k) { return __get_computed_style_internal(nodeId, k); };
                            }
                            if (prop === 'item') {
                                return function(i) { return ""; };
                            }
                            if (prop === 'toString') {
                                return function() { return "[object CSSStyleDeclaration]"; };
                            }
                            if (prop in Object.prototype) {
                                return Object.prototype[prop];
                            }
                            return __get_computed_style_internal(nodeId, prop);
                        }
                        if (typeof prop === 'symbol') {
                            if (prop === Symbol.toStringTag) return "CSSStyleDeclaration";
                            return target[prop];
                        }
                        return target[prop];
                    }
                });
            };

            window.matchMedia = function(mediaQuery) {
                var mq = String(mediaQuery || "");
                var matches = false;
                if (mq.indexOf("prefers-color-scheme") !== -1 && mq.indexOf("dark") !== -1) {
                    matches = false;
                }
                var minWidthMatch = mq.match(/min-width:\s*(\d+)px/);
                if (minWidthMatch) {
                    matches = window.innerWidth >= parseInt(minWidthMatch[1], 10);
                }
                var maxWidthMatch = mq.match(/max-width:\s*(\d+)px/);
                if (maxWidthMatch) {
                    matches = window.innerWidth <= parseInt(maxWidthMatch[1], 10);
                }

                var listeners = [];
                return {
                    media: mq,
                    matches: matches,
                    onchange: null,
                    addListener: function(fn) { if (typeof fn === 'function') listeners.push(fn); },
                    removeListener: function(fn) {
                        var idx = listeners.indexOf(fn);
                        if (idx !== -1) listeners.splice(idx, 1);
                    },
                    addEventListener: function(type, fn) {
                        if (type === 'change') this.addListener(fn);
                    },
                    removeEventListener: function(type, fn) {
                        if (type === 'change') this.removeListener(fn);
                    },
                    dispatchEvent: function(evt) { return true; }
                };
            };

            window.location = {
                href: "",
                search: "",
                pathname: "/",
                hash: "",
                host: "",
                hostname: "",
                origin: "",
                protocol: "https:",
                port: "",
                assign: function() {},
                replace: function() {},
                reload: function() {}
            };
            document.location = window.location;

            window.navigator = {
                userAgent: "Mozilla/5.0 (X11; Linux x86_64) lilies/0.1.0",
                language: "en-US",
                languages: ["en-US", "en"],
                onLine: true,
                platform: "Linux x86_64",
                cookieEnabled: true
            };

            window.screen = {
                width: 1920,
                height: 1080,
                availWidth: 1920,
                availHeight: 1080,
                colorDepth: 24,
                pixelDepth: 24
            };

            window.performance = {
                now: function() { return __performance_now(); },
                timeOrigin: Date.now(),
                timing: {
                    navigationStart: Date.now(),
                    fetchStart: Date.now(),
                    domLoading: Date.now(),
                    domInteractive: Date.now(),
                    domContentLoadedEventStart: Date.now(),
                    domContentLoadedEventEnd: Date.now(),
                    domComplete: Date.now(),
                    loadEventStart: Date.now(),
                    loadEventEnd: Date.now()
                }
            };
            globalThis.performance = window.performance;

            function __createStorage() {
                var store = Object.create(null);
                var storageObj = {
                    getItem: function(key) {
                        var k = String(key);
                        return (k in store) ? store[k] : null;
                    },
                    setItem: function(key, val) {
                        var k = String(key);
                        store[k] = String(val);
                    },
                    removeItem: function(key) {
                        var k = String(key);
                        delete store[k];
                    },
                    clear: function() {
                        store = Object.create(null);
                    },
                    key: function(idx) {
                        var keys = Object.keys(store);
                        return idx >= 0 && idx < keys.length ? keys[idx] : null;
                    },
                    get length() {
                        return Object.keys(store).length;
                    }
                };
                return new Proxy(storageObj, {
                    get: function(target, prop) {
                        if (typeof prop === 'string') {
                            if (prop in target) return target[prop];
                            return (prop in store) ? store[prop] : undefined;
                        }
                        return target[prop];
                    },
                    set: function(target, prop, value) {
                        if (typeof prop === 'string') {
                            if (prop in target) {
                                target[prop] = value;
                                return true;
                            }
                            store[prop] = String(value);
                            return true;
                        }
                        target[prop] = value;
                        return true;
                    },
                    deleteProperty: function(target, prop) {
                        if (typeof prop === 'string' && prop in store) {
                            delete store[prop];
                            return true;
                        }
                        delete target[prop];
                        return true;
                    }
                });
            }
            window.localStorage = __createStorage();
            window.sessionStorage = __createStorage();
            globalThis.localStorage = window.localStorage;
            globalThis.sessionStorage = window.sessionStorage;

            window.history = {
                length: 1,
                state: null,
                scrollRestoration: "auto",
                pushState: function(state, unused, url) {
                    this.state = state;
                    if (url) {
                        try {
                            var u = new URL(url, window.location.href || "https://localhost/");
                            window.location.href = u.href;
                            window.location.pathname = u.pathname;
                            window.location.search = u.search;
                            window.location.hash = u.hash;
                        } catch (e) {}
                    }
                },
                replaceState: function(state, unused, url) {
                    this.state = state;
                    if (url) {
                        try {
                            var u = new URL(url, window.location.href || "https://localhost/");
                            window.location.href = u.href;
                            window.location.pathname = u.pathname;
                            window.location.search = u.search;
                            window.location.hash = u.hash;
                        } catch (e) {}
                    }
                },
                back: function() {},
                forward: function() {},
                go: function(delta) {}
            };
            globalThis.history = window.history;

            var _b64chars = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
            window.btoa = function(input) {
                var str = String(input);
                var output = '';
                for (var i = 0; i < str.length; i += 3) {
                    var c1 = str.charCodeAt(i);
                    var c2 = i + 1 < str.length ? str.charCodeAt(i + 1) : NaN;
                    var c3 = i + 2 < str.length ? str.charCodeAt(i + 2) : NaN;
                    if (c1 > 255 || c2 > 255 || c3 > 255) {
                        throw new Error("InvalidCharacterError: String contains characters outside the Latin1 range");
                    }
                    var n = (c1 << 16) | (isNaN(c2) ? 0 : c2 << 8) | (isNaN(c3) ? 0 : c3);
                    output += _b64chars.charAt((n >> 18) & 63);
                    output += _b64chars.charAt((n >> 12) & 63);
                    output += isNaN(c2) ? '=' : _b64chars.charAt((n >> 6) & 63);
                    output += isNaN(c3) ? '=' : _b64chars.charAt(n & 63);
                }
                return output;
            };
            window.atob = function(input) {
                var str = String(input).replace(/[\t\n\f\r ]+/g, '');
                var output = '';
                for (var i = 0; i < str.length; i += 4) {
                    var enc1 = _b64chars.indexOf(str.charAt(i));
                    var enc2 = _b64chars.indexOf(str.charAt(i + 1));
                    var enc3 = str.charAt(i + 2) === '=' ? 64 : _b64chars.indexOf(str.charAt(i + 2));
                    var enc4 = str.charAt(i + 3) === '=' ? 64 : _b64chars.indexOf(str.charAt(i + 3));
                    if (enc1 === -1 || enc2 === -1 || (enc3 === -1 && enc3 !== 64) || (enc4 === -1 && enc4 !== 64)) {
                        throw new Error("InvalidCharacterError: Invalid base64 character");
                    }
                    var chr1 = (enc1 << 2) | (enc2 >> 4);
                    output += String.fromCharCode(chr1);
                    if (enc3 !== 64) {
                        var chr2 = ((enc2 & 15) << 4) | (enc3 >> 2);
                        output += String.fromCharCode(chr2);
                    }
                    if (enc4 !== 64) {
                        var chr3 = ((enc3 & 3) << 6) | enc4;
                        output += String.fromCharCode(chr3);
                    }
                }
                return output;
            };
            globalThis.btoa = window.btoa;
            globalThis.atob = window.atob;

            function URLSearchParams(init) {
                var params = [];
                if (typeof init === 'string') {
                    var str = init;
                    if (str.charAt(0) === '?') str = str.slice(1);
                    if (str.length > 0) {
                        var pairs = str.split('&');
                        for (var i = 0; i < pairs.length; i++) {
                            var p = pairs[i];
                            var eq = p.indexOf('=');
                            if (eq !== -1) {
                                params.push([decodeURIComponent(p.slice(0, eq).replace(/\+/g, ' ')), decodeURIComponent(p.slice(eq + 1).replace(/\+/g, ' '))]);
                            } else {
                                params.push([decodeURIComponent(p.replace(/\+/g, ' ')), '']);
                            }
                        }
                    }
                } else if (Array.isArray(init)) {
                    for (var i = 0; i < init.length; i++) {
                        params.push([String(init[i][0]), String(init[i][1])]);
                    }
                } else if (init && typeof init === 'object') {
                    for (var k in init) {
                        params.push([String(k), String(init[k])]);
                    }
                }
                this.append = function(name, value) {
                    params.push([String(name), String(value)]);
                };
                this.delete = function(name) {
                    var n = String(name);
                    params = params.filter(function(pair) { return pair[0] !== n; });
                };
                this.get = function(name) {
                    var n = String(name);
                    for (var i = 0; i < params.length; i++) {
                        if (params[i][0] === n) return params[i][1];
                    }
                    return null;
                };
                this.getAll = function(name) {
                    var n = String(name);
                    var res = [];
                    for (var i = 0; i < params.length; i++) {
                        if (params[i][0] === n) res.push(params[i][1]);
                    }
                    return res;
                };
                this.has = function(name) {
                    var n = String(name);
                    for (var i = 0; i < params.length; i++) {
                        if (params[i][0] === n) return true;
                    }
                    return false;
                };
                this.set = function(name, value) {
                    var n = String(name);
                    var v = String(value);
                    var found = false;
                    var newParams = [];
                    for (var i = 0; i < params.length; i++) {
                        if (params[i][0] === n) {
                            if (!found) {
                                newParams.push([n, v]);
                                found = true;
                            }
                        } else {
                            newParams.push(params[i]);
                        }
                    }
                    if (!found) newParams.push([n, v]);
                    params = newParams;
                };
                this.forEach = function(cb, thisArg) {
                    for (var i = 0; i < params.length; i++) {
                        cb.call(thisArg, params[i][1], params[i][0], this);
                    }
                };
                this.toString = function() {
                    return params.map(function(pair) {
                        return encodeURIComponent(pair[0]) + '=' + encodeURIComponent(pair[1]);
                    }).join('&');
                };
                this.entries = function*() {
                    for (var i = 0; i < params.length; i++) {
                        yield [params[i][0], params[i][1]];
                    }
                };
                this[Symbol.iterator] = this.entries;
                Object.defineProperty(this, 'size', {
                    get: function() { return params.length; }
                });
            }
            window.URLSearchParams = URLSearchParams;
            globalThis.URLSearchParams = URLSearchParams;

            function URL(url, base) {
                var rawUrl = String(url);
                var resolved;
                if (base !== undefined) {
                    var baseStr = String(base);
                    if (rawUrl.indexOf('://') !== -1) {
                        resolved = rawUrl;
                    } else if (rawUrl.startsWith('//')) {
                        var protoIdx = baseStr.indexOf('://');
                        var proto = protoIdx !== -1 ? baseStr.slice(0, protoIdx + 1) : 'https:';
                        resolved = proto + rawUrl;
                    } else if (rawUrl.startsWith('/')) {
                        var m = baseStr.match(/^([a-zA-Z]+:\/\/[^\/]+)/);
                        var origin = m ? m[1] : baseStr;
                        resolved = origin + rawUrl;
                    } else {
                        var slashIdx = baseStr.lastIndexOf('/');
                        if (slashIdx !== -1 && slashIdx > baseStr.indexOf('://') + 2) {
                            resolved = baseStr.slice(0, slashIdx + 1) + rawUrl;
                        } else {
                            resolved = baseStr + '/' + rawUrl;
                        }
                    }
                } else {
                    resolved = rawUrl;
                }

                var parseRegex = /^([a-zA-Z]+:)?(\/\/([^\/?#:]+)(?::(\d+))?)?([^?#]*)(\?[^#]*)?(#.*)?$/;
                var match = resolved.match(parseRegex) || [];

                var protocol = match[1] || "";
                var hostname = match[3] || "";
                var port = match[4] || "";
                var host = hostname + (port ? ":" + port : "");
                var origin = (protocol && host) ? (protocol + "//" + host) : "";
                var pathname = match[5] || "/";
                if (pathname.length === 0 && origin) pathname = "/";
                var search = match[6] || "";
                var hash = match[7] || "";

                var searchParams = new URLSearchParams(search);

                this.protocol = protocol;
                this.host = host;
                this.hostname = hostname;
                this.port = port;
                this.origin = origin;
                this.pathname = pathname;
                this.hash = hash;
                this.searchParams = searchParams;

                Object.defineProperty(this, 'search', {
                    get: function() {
                        var s = searchParams.toString();
                        return s ? "?" + s : "";
                    },
                    set: function(v) {
                        search = v;
                        this.searchParams = new URLSearchParams(v);
                    }
                });

                Object.defineProperty(this, 'href', {
                    get: function() {
                        var p = this.protocol ? this.protocol + "//" + this.host : "";
                        return p + this.pathname + this.search + this.hash;
                    },
                    set: function(v) {
                        var u = new URL(v);
                        this.protocol = u.protocol;
                        this.host = u.host;
                        this.hostname = u.hostname;
                        this.port = u.port;
                        this.origin = u.origin;
                        this.pathname = u.pathname;
                        this.search = u.search;
                        this.hash = u.hash;
                    }
                });

                this.toString = function() {
                    return this.href;
                };
                this.toJSON = function() {
                    return this.href;
                };
            }
            window.URL = URL;
            globalThis.URL = URL;

            function Event(type, options) {
                options = options || {};
                this.type = String(type || "");
                this.bubbles = !!options.bubbles;
                this.cancelable = !!options.cancelable;
                this.defaultPrevented = false;
                this.target = null;
                this.currentTarget = null;
                this.srcElement = null;
                this.cancelBubble = false;
                this._stopped = false;
                this.preventDefault = function() {
                    if (this.cancelable !== false) this.defaultPrevented = true;
                };
                this.stopPropagation = function() {
                    this._stopped = true;
                    this.cancelBubble = true;
                };
            }
            window.Event = Event;
            globalThis.Event = Event;

            function CustomEvent(type, options) {
                Event.call(this, type, options);
                this.detail = (options && options.detail !== undefined) ? options.detail : null;
            }
            CustomEvent.prototype = Object.create(Event.prototype);
            CustomEvent.prototype.constructor = CustomEvent;
            window.CustomEvent = CustomEvent;
            globalThis.CustomEvent = CustomEvent;

            window.fetch = function(url, options) {
                return Promise.resolve({
                    ok: true,
                    status: 200,
                    statusText: "OK",
                    url: String(url),
                    text: function() { return Promise.resolve(""); },
                    json: function() { return Promise.resolve({}); },
                    blob: function() { return Promise.resolve(null); },
                    headers: {
                        get: function() { return null; },
                        has: function() { return false; }
                    }
                });
            };
            globalThis.fetch = window.fetch;

            window.addEventListener = function(evt, handler) {
                __window_add_event_listener(evt, handler);
            };
            window.removeEventListener = function(evt, handler) {
                __window_remove_event_listener(evt, handler);
            };

            window.Image = function(width, height) {
                var img = __create_element("img");
                if (width !== undefined) {
                    img.setAttribute("width", String(width));
                    img.width = width;
                }
                if (height !== undefined) {
                    img.setAttribute("height", String(height));
                    img.height = height;
                }
                return img;
            };

            window.Node = {
                ELEMENT_NODE: 1,
                ATTRIBUTE_NODE: 2,
                TEXT_NODE: 3,
                CDATA_SECTION_NODE: 4,
                ENTITY_REFERENCE_NODE: 5,
                ENTITY_NODE: 6,
                PROCESSING_INSTRUCTION_NODE: 7,
                COMMENT_NODE: 8,
                DOCUMENT_NODE: 9,
                DOCUMENT_TYPE_NODE: 10,
                DOCUMENT_FRAGMENT_NODE: 11,
                NOTATION_NODE: 12
            };
            globalThis.Node = window.Node;

            document.defaultView = window;
            document.compatMode = "CSS1Compat";
            document.readyState = "complete";
            document.cookie = "";
            document.title = "";
            document.nodeType = 9;
            document.nodeName = "#document";
            document.nodeValue = null;

            Object.defineProperty(document, 'childNodes', {
                get: function() { return __get_document_child_nodes(); },
                configurable: true,
                enumerable: true
            });
            Object.defineProperty(document, 'firstChild', {
                get: function() {
                    var cn = this.childNodes;
                    return cn && cn.length > 0 ? cn[0] : null;
                },
                configurable: true,
                enumerable: true
            });
            Object.defineProperty(document, 'lastChild', {
                get: function() {
                    var cn = this.childNodes;
                    return cn && cn.length > 0 ? cn[cn.length - 1] : null;
                },
                configurable: true,
                enumerable: true
            });

            Object.defineProperty(document, 'documentElement', {
                get: function() { return __get_document_element(); },
                configurable: true,
                enumerable: true
            });
            Object.defineProperty(document, 'body', {
                get: function() { return __get_document_body(); },
                configurable: true,
                enumerable: true
            });
            Object.defineProperty(document, 'head', {
                get: function() { return __get_document_head(); },
                configurable: true,
                enumerable: true
            });

            document.createElement = function(tag) { return __create_element(tag); };
            document.createTextNode = function(text) { return __create_text_node(text); };
            document.createComment = function(text) { return __create_comment(text); };
            document.createDocumentFragment = function() { return __create_document_fragment(); };
            document.createEvent = function(type) {
                return {
                    type: type || "",
                    bubbles: false,
                    cancelable: false,
                    defaultPrevented: false,
                    initEvent: function(t, b, c) {
                        this.type = t;
                        this.bubbles = !!b;
                        this.cancelable = !!c;
                    },
                    preventDefault: function() { this.defaultPrevented = true; }
                };
            };
            document.hasFocus = function() { return true; };
            document.write = function(str) {};
            document.writeln = function(str) {};
            document.getElementsByTagName = function(tag) { return __get_elements_by_tag_name(tag); };

            globalThis.__createClickEvent = function(target, currentTarget) {
                var ev = new CustomEvent("click", { bubbles: true, cancelable: true });
                ev.target = target;
                ev.currentTarget = currentTarget;
                ev.srcElement = target;
                return ev;
            };

            globalThis.__createCustomEvent = function(type, target, currentTarget) {
                var ev = new CustomEvent(type, { bubbles: true, cancelable: true });
                ev.target = target;
                ev.currentTarget = currentTarget;
                ev.srcElement = target;
                return ev;
            };
        "##;
        let _ = context.eval(Source::from_bytes(browser_preamble.as_bytes()));

        Self { context, inner }
    }

    /// Evaluates JavaScript source code within the runtime context.
    pub fn eval(&mut self, code: &str) -> Result<JsValue, JsError> {
        let _guard = ActiveRuntimeGuard::new(&self.inner);
        self.context.eval(Source::from_bytes(code.as_bytes()))
    }

    /// Dispatches a click event to `node_id`, bubbling up ancestors if necessary.
    /// Returns `Ok(true)` if the event was not cancelled (defaultPrevented is false),
    /// or `Ok(false)` if preventDefault() was called.
    pub fn dispatch_click(&mut self, node_id: NodeId) -> Result<bool, JsError> {
        let _guard = ActiveRuntimeGuard::new(&self.inner);
        dispatch_click_with_context(node_id, &mut self.context)
    }

    /// Dispatches an event of `event_type` to `target` (or `document`/`window` if None).
    /// Returns `Ok(true)` if the event was not cancelled (defaultPrevented is false),
    /// or `Ok(false)` if preventDefault() was called.
    pub fn dispatch_event(
        &mut self,
        target: Option<NodeId>,
        event_type: &str,
    ) -> Result<bool, JsError> {
        let _guard = ActiveRuntimeGuard::new(&self.inner);
        dispatch_event_with_context(target, event_type, None, &mut self.context)
    }

    /// Updates `window.location` with the given URL.
    pub fn set_location_url(&mut self, url: &str) {
        let search = url.find('?').map(|pos| &url[pos..]).unwrap_or("");
        let script = format!(
            r#"
            window.location.href = {:?};
            window.location.search = {:?};
            document.location = window.location;
            "#,
            url, search
        );
        let _ = self.eval(&script);
    }

    /// Returns whether any DOM mutation occurred during script execution.
    pub fn is_dirty(&self) -> bool {
        self.inner.borrow().dirty
    }

    /// Sets the runtime dirty flag.
    pub fn set_dirty(&mut self, dirty: bool) {
        self.inner.borrow_mut().dirty = dirty;
    }

    /// Clears and returns the dirty flag.
    pub fn take_dirty(&mut self) -> bool {
        let mut inner = self.inner.borrow_mut();
        let was_dirty = inner.dirty;
        inner.dirty = false;
        was_dirty
    }

    /// Resets the dirty flag to false.
    pub fn reset_dirty(&mut self) {
        self.inner.borrow_mut().dirty = false;
    }

    /// Borrows the internal `Document` immutably.
    pub fn doc(&self) -> Ref<'_, Document> {
        Ref::map(self.inner.borrow(), |i| &i.doc)
    }

    /// Borrows the internal `Document` mutably.
    pub fn doc_mut(&mut self) -> RefMut<'_, Document> {
        RefMut::map(self.inner.borrow_mut(), |i| &mut i.doc)
    }

    /// Finds a node by `id` in the current document.
    pub fn find_element_by_id(&self, id: &str) -> Option<NodeId> {
        find_element_by_id(&self.doc(), id)
    }

    /// Returns a mutable reference to the Boa `Context`.
    pub fn context(&mut self) -> &mut Context {
        &mut self.context
    }

    /// Runs all timers that have reached or passed their scheduled deadline.
    /// Reschedules repeating interval timers.
    /// Returns the number of callbacks executed.
    pub fn run_ready_timers(&mut self) -> Result<usize, JsError> {
        let _guard = ActiveRuntimeGuard::new(&self.inner);
        let now = Instant::now();
        let ready_timers: Vec<Timer> = {
            let mut inner = self.inner.borrow_mut();
            let mut ready = Vec::new();
            let mut remaining = Vec::new();
            for timer in inner.timers.drain(..) {
                if timer.target_time <= now {
                    ready.push(timer);
                } else {
                    remaining.push(timer);
                }
            }
            inner.timers = remaining;
            ready
        };

        let mut count = 0;
        let mut reschedule = Vec::new();
        for timer in ready_timers {
            count += 1;
            match &timer.callback {
                TimerCallback::Function(f) => {
                    let global = self.context.global_object();
                    let this_val = JsValue::new(global);
                    if let Err(e) = f.call(&this_val, &timer.args, &mut self.context) {
                        eprintln!("Timer callback error: {e}");
                    }
                }
                TimerCallback::Script(script) => {
                    if let Err(e) = self.eval(script) {
                        eprintln!("Timer script evaluation error: {e}");
                    }
                }
            }

            if let Some(interval) = timer.interval {
                let cancelled = self.inner.borrow().cancelled_timers.contains(&timer.id);
                if !cancelled {
                    reschedule.push(Timer {
                        id: timer.id,
                        callback: timer.callback,
                        target_time: Instant::now() + interval,
                        interval: Some(interval),
                        args: timer.args,
                    });
                }
            }
        }

        if !reschedule.is_empty() {
            let mut inner = self.inner.borrow_mut();
            inner.timers.extend(reschedule);
        }
        self.inner.borrow_mut().cancelled_timers.clear();

        Ok(count)
    }

    /// Runs all queued animation frame callbacks, passing the timestamp in milliseconds.
    /// Returns the number of callbacks executed.
    pub fn run_animation_frames(&mut self, timestamp_ms: f64) -> Result<usize, JsError> {
        let _guard = ActiveRuntimeGuard::new(&self.inner);
        let frames: Vec<AnimationFrame> = {
            let mut inner = self.inner.borrow_mut();
            inner.animation_frames.drain(..).collect()
        };

        let mut count = 0;
        let global = self.context.global_object();
        let this_val = JsValue::new(global);
        let ts_val = JsValue::new(timestamp_ms);

        for frame in frames {
            let cancelled = self.inner.borrow().cancelled_rafs.contains(&frame.id);
            if !cancelled {
                count += 1;
                if let Err(e) = frame.callback.call(&this_val, &[ts_val.clone()], &mut self.context) {
                    eprintln!("requestAnimationFrame callback error: {e}");
                }
            }
        }

        self.inner.borrow_mut().cancelled_rafs.clear();
        Ok(count)
    }

    /// Returns the time duration until the next timer is scheduled to fire,
    /// or None if there are no pending timers.
    pub fn next_timer_delay(&self) -> Option<Duration> {
        let inner = self.inner.borrow();
        let now = Instant::now();
        let mut min_delay: Option<Duration> = None;
        for t in &inner.timers {
            let delay = if t.target_time <= now {
                Duration::from_millis(0)
            } else {
                t.target_time.duration_since(now)
            };
            min_delay = match min_delay {
                Some(current) => Some(current.min(delay)),
                None => Some(delay),
            };
        }
        if !inner.animation_frames.is_empty() {
            min_delay = Some(Duration::from_millis(0));
        }
        min_delay
    }

    /// Returns the number of pending timers.
    pub fn pending_timers_count(&self) -> usize {
        self.inner.borrow().timers.len()
    }

    /// Returns the number of pending animation frames.
    pub fn pending_animation_frames_count(&self) -> usize {
        self.inner.borrow().animation_frames.len()
    }
}
