#![deny(unsafe_code)]

use boa_engine::{
    js_string,
    object::{builtins::JsFunction, FunctionObjectBuilder, ObjectInitializer},
    property::Attribute,
    Context, JsArgs, JsData, JsError, JsNativeError, JsResult, JsValue, NativeFunction, Source,
};
use boa_gc::{Finalize, Trace};
use lilies_dom::{Document, NodeId};
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
                if !k.is_empty() {
                    entries.push((k, v));
                }
            }
        }
    }

    let target_key = prop.to_ascii_lowercase();
    let mut updated = false;
    for (k, v) in &mut entries {
        if k == &target_key {
            *v = val.to_string();
            updated = true;
            break;
        }
    }
    if !updated {
        entries.push((target_key, val.to_string()));
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

/// Finds an element node in the DOM arena matching the given `id` attribute.
pub fn find_element_by_id(doc: &Document, id: &str) -> Option<NodeId> {
    for node in doc.arena().iter() {
        if let Some(el) = node.as_element() {
            if el.id() == Some(id) {
                return Some(node.id);
            }
        }
    }
    None
}

/// Replaces all child nodes of `node_id` with a single new Text node containing `text`.
pub fn set_node_text_content(doc: &mut Document, node_id: NodeId, text: &str) {
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
        let el_obj = create_element_object(node_id, context)?;
        Ok(el_obj.into())
    } else {
        Ok(JsValue::null())
    }
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

fn internal_set_style(
    _this: &JsValue,
    args: &[JsValue],
    context: &mut Context,
) -> JsResult<JsValue> {
    let node_idx = args.get_or_undefined(0).to_u32(context)? as usize;
    let node_id = NodeId(node_idx);
    let prop_raw = args.get_or_undefined(1).to_string(context)?.to_std_string_escaped();
    let val_str = args.get_or_undefined(2).to_string(context)?.to_std_string_escaped();
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

/// Constructs a JavaScript Element object wrapping `node_id`.
pub fn create_element_object(
    node_id: NodeId,
    context: &mut Context,
) -> JsResult<boa_engine::object::JsObject> {
    let get_text = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(element_get_text_content),
    )
    .name(js_string!("get textContent"))
    .build();

    let set_text = FunctionObjectBuilder::new(
        context.realm(),
        NativeFunction::from_fn_ptr(element_set_text_content),
    )
    .name(js_string!("set textContent"))
    .length(1)
    .build();

    let mut obj_init = ObjectInitializer::with_native_data(ElementNode(node_id), context);
    obj_init
        .accessor(
            js_string!("textContent"),
            Some(get_text),
            Some(set_text),
            Attribute::all(),
        )
        .function(
            NativeFunction::from_fn_ptr(element_set_attribute),
            js_string!("setAttribute"),
            2,
        )
        .function(
            NativeFunction::from_fn_ptr(element_get_attribute),
            js_string!("getAttribute"),
            1,
        )
        .function(
            NativeFunction::from_fn_ptr(element_add_event_listener),
            js_string!("addEventListener"),
            2,
        );

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
        }));

        let _guard = ActiveRuntimeGuard::new(&inner);

        // 1. Register console
        let console = ObjectInitializer::new(&mut context)
            .function(NativeFunction::from_fn_ptr(console_log), js_string!("log"), 1)
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
            document,
            Attribute::all(),
        );

        // 3. Register internal style helpers
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

        // 4. Register style proxy factory
        let proxy_preamble = r#"
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
        "#;
        let _ = context.eval(Source::from_bytes(proxy_preamble.as_bytes()));

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

        let mut current = Some(node_id);
        let mut callbacks = Vec::new();
        while let Some(curr_id) = current {
            let listeners = with_current_runtime(|inner| {
                Ok(inner.click_listeners.get(&curr_id).cloned())
            })?;
            if let Some(list) = listeners {
                if !list.is_empty() {
                    callbacks = list;
                    break;
                }
            }
            current = with_current_runtime(|inner| Ok(inner.doc.parent(curr_id)))?;
        }

        for cb in callbacks {
            cb.call(&JsValue::undefined(), &[], &mut self.context)?;
        }

        Ok(())
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
