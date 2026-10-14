use super::*;
use crate::util::set_private_value;
use crate::web_api_interfaces;
use moli_webapi_declare::WebApiObject;

#[derive(WebApiObject)]
#[webapi(
    interface = web_api_interfaces::EventTarget,
    prototype = "Object",
    enumerable,
    receiver
)]
struct SimpleEventTargetMethodsDeclaration {
    #[webapi(method, length = 2, callback = simple_event_target_add_event_listener_callback)]
    add_event_listener: (),
    #[webapi(
        method,
        length = 2,
        callback = simple_event_target_remove_event_listener_callback
    )]
    remove_event_listener: (),
    #[webapi(method, length = 1, callback = simple_event_target_dispatch_event_callback)]
    dispatch_event: (),
}

pub(crate) fn install_simple_event_target_methods<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    slot_name: &'static str,
) {
    mark_simple_event_target_slot(scope, target, slot_name);
    SimpleEventTargetMethodsDeclaration::new()
        .initialize(scope, target)
        .expect("simple event target declaration should initialize");
}

pub(crate) fn mark_simple_event_target_slot<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    slot_name: &'static str,
) {
    set_private_value(
        scope,
        target,
        SIMPLE_EVENT_TARGET_SLOT,
        v8str(scope, slot_name).into(),
    );
}
