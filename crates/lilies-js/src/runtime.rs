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
use lilies_dom::{Document, IndexMap, NodeId};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::Rc;

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

/// Internal state managed by the JavaScript runtime.
pub struct RuntimeInner {
    pub doc: Document,
    pub dirty: bool,
    pub click_listeners: HashMap<NodeId, Vec<JsFunction>>,
    pub node_cache: HashMap<NodeId, boa_engine::object::JsObject>,
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

    let array = JsArray::new(context);
    for node_id in node_ids {
        let obj = get_or_create_element_object(node_id, context)?;
        array.push(JsValue::new(obj), context)?;
    }
    Ok(JsValue::new(array))
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

    if event_type.eq_ignore_ascii_case("click") {
        with_current_runtime(|inner| {
            inner.click_listeners.entry(node_id).or_default().push(callback);
            Ok(())
        })?;
    }

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

    if event_type.eq_ignore_ascii_case("click") {
        if let Some(target_obj) = callback_val.as_object() {
            with_current_runtime(|inner| {
                if let Some(list) = inner.click_listeners.get_mut(&node_id) {
                    list.retain(|f| !boa_engine::object::JsObject::equals(f, target_obj));
                }
                Ok(())
            })?;
        }
    }
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

    let array = JsArray::new(context);
    for cid in child_ids {
        let obj = get_or_create_element_object(cid, context)?;
        array.push(JsValue::new(obj), context)?;
    }
    Ok(JsValue::new(array))
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
            .map(|e| e.tag_name().to_ascii_uppercase())
            .unwrap_or_default())
    })?;
    Ok(JsValue::new(js_string!(tag)))
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

fn dispatch_click_with_context(node_id: NodeId, context: &mut Context) -> JsResult<()> {
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

    if ancestor_chain.is_empty() {
        return Ok(());
    }

    let target_obj = get_or_create_element_object(target_id, context)?;
    let create_event_fn = context
        .global_object()
        .get(js_string!("__createClickEvent"), context)?;

    for (curr_id, cbs) in ancestor_chain {
        let current_target_obj = get_or_create_element_object(curr_id, context)?;
        let this_val = JsValue::new(current_target_obj.clone());

        let event_val = if let Some(create_fn) = create_event_fn.as_callable() {
            create_fn.call(
                &JsValue::undefined(),
                &[
                    JsValue::new(target_obj.clone()),
                    JsValue::new(current_target_obj),
                ],
                context,
            )?
        } else {
            JsValue::undefined()
        };

        let mut stopped = false;
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
            break;
        }
    }

    Ok(())
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

    let get_parent = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_parent_node))
        .name(js_string!("get parentNode")).build();
    let get_children = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_children))
        .name(js_string!("get children")).build();
    let get_first_child = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_first_child))
        .name(js_string!("get firstChild")).build();
    let get_last_child = FunctionObjectBuilder::new(context.realm(), NativeFunction::from_fn_ptr(element_get_last_child))
        .name(js_string!("get lastChild")).build();

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

    let dataset = ObjectInitializer::new(context).build();
    let mut obj_init = ObjectInitializer::with_native_data(ElementNode(node_id), context);
    obj_init
        .accessor(js_string!("textContent"), Some(get_text), Some(set_text), Attribute::all())
        .accessor(js_string!("parentNode"), Some(get_parent.clone()), None, Attribute::all())
        .accessor(js_string!("parentElement"), Some(get_parent), None, Attribute::all())
        .accessor(js_string!("children"), Some(get_children.clone()), None, Attribute::all())
        .accessor(js_string!("childNodes"), Some(get_children), None, Attribute::all())
        .accessor(js_string!("firstChild"), Some(get_first_child), None, Attribute::all())
        .accessor(js_string!("lastChild"), Some(get_last_child), None, Attribute::all())
        .accessor(js_string!("tagName"), Some(get_tag_name.clone()), None, Attribute::all())
        .accessor(js_string!("nodeName"), Some(get_tag_name), None, Attribute::all())
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
        .function(NativeFunction::from_fn_ptr(element_append_child), js_string!("appendChild"), 1)
        .function(NativeFunction::from_fn_ptr(element_remove_child), js_string!("removeChild"), 1)
        .function(NativeFunction::from_fn_ptr(element_insert_before), js_string!("insertBefore"), 2)
        .function(NativeFunction::from_fn_ptr(element_focus), js_string!("focus"), 0)
        .function(NativeFunction::from_fn_ptr(element_blur), js_string!("blur"), 0)
        .function(NativeFunction::from_fn_ptr(element_click), js_string!("click"), 0)
        .function(NativeFunction::from_fn_ptr(element_get_bounding_client_rect), js_string!("getBoundingClientRect"), 0)
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

        for (cid, name) in named_children {
            let child_obj = get_or_create_element_object(cid, context)?;
            obj.set(js_string!(name), child_obj, false, context)?;
        }
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
            .build();
        let _ = context.register_global_property(
            js_string!("document"),
            document.clone(),
            Attribute::all(),
        );

        // 3. Register internal DOM helpers
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
            js_string!("__get_elements_by_tag_name"),
            1,
            NativeFunction::from_fn_ptr(document_get_elements_by_tag_name),
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
        let browser_preamble = r#"
            globalThis.__createStyleProxy = function(nodeId) {
                return new Proxy({}, {
                    get(target, prop) {
                        if (typeof prop === 'string') {
                            if (prop === 'setProperty') {
                                return function(k, v) { __set_style_internal(nodeId, k, v); };
                            }
                            if (prop === 'getPropertyValue') {
                                return function(k) { return __get_style_internal(nodeId, k); };
                            }
                            return __get_style_internal(nodeId, prop);
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

            window.setTimeout = function(fn, delay) { return 1; };
            window.clearTimeout = function(id) {};
            window.setInterval = function(fn, delay) { return 1; };
            window.clearInterval = function(id) {};
            window.requestAnimationFrame = function(cb) { return 1; };
            window.cancelAnimationFrame = function(id) {};
            window.addEventListener = function(evt, handler) {};
            window.removeEventListener = function(evt, handler) {};

            window.Image = function() {
                this.src = "";
                this.width = 0;
                this.height = 0;
            };

            document.defaultView = window;
            document.compatMode = "CSS1Compat";
            document.readyState = "complete";
            document.cookie = "";
            document.title = "";
            document.addEventListener = function(evt, handler) {};
            document.removeEventListener = function(evt, handler) {};

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
            document.getElementsByTagName = function(tag) { return __get_elements_by_tag_name(tag); };

            globalThis.__createClickEvent = function(target, currentTarget) {
                return {
                    type: "click",
                    target: target,
                    currentTarget: currentTarget,
                    srcElement: target,
                    defaultPrevented: false,
                    cancelBubble: false,
                    _stopped: false,
                    preventDefault: function() { this.defaultPrevented = true; },
                    stopPropagation: function() { this._stopped = true; this.cancelBubble = true; }
                };
            };
        "#;
        let _ = context.eval(Source::from_bytes(browser_preamble.as_bytes()));

        Self { context, inner }
    }

    /// Evaluates JavaScript source code within the runtime context.
    pub fn eval(&mut self, code: &str) -> Result<JsValue, JsError> {
        let _guard = ActiveRuntimeGuard::new(&self.inner);
        self.context.eval(Source::from_bytes(code.as_bytes()))
    }

    /// Dispatches a click event to `node_id`, bubbling up ancestors if necessary.
    pub fn dispatch_click(&mut self, node_id: NodeId) -> Result<(), JsError> {
        let _guard = ActiveRuntimeGuard::new(&self.inner);
        dispatch_click_with_context(node_id, &mut self.context)
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
}
