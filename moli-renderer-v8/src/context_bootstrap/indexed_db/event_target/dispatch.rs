use super::*;
use crate::context_bootstrap::dispatch_simple_event_target_event;

pub(in crate::context_bootstrap::indexed_db) fn dispatch_idb_named_event<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    event_type: &str,
    extras: impl FnOnce(&mut v8::PinScope<'s, '_>, v8::Local<'s, v8::Object>),
) -> bool {
    let global = scope.get_current_context().global(scope);
    let Some(event_ctor) = global
        .get(scope, v8str(scope, "Event").into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
    else {
        return true;
    };
    let Some(event_type) = v8_string(scope, event_type) else {
        return true;
    };
    let Some(event) = event_ctor.new_instance(scope, &[event_type.into()]) else {
        return true;
    };
    extras(scope, event);
    dispatch_idb_event_object(scope, target, event)
}

pub(in crate::context_bootstrap::indexed_db) fn dispatch_idb_event_object<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
    event: v8::Local<'s, v8::Object>,
) -> bool {
    let Some(event_type) = crate::context_bootstrap::event_attribute(scope, event, "type")
        .and_then(|value| value.to_string(scope))
        .map(|value| value.to_rust_string_lossy(scope))
    else {
        return true;
    };
    let owner_scope = idb_event_target_owner_scope(scope, target);
    let previous_owner = owner_scope.enter(scope);
    let uncanceled = dispatch_simple_event_target_event(scope, target, &event_type, event);

    owner_scope.defer_restore(scope, previous_owner);
    uncanceled
}

fn idb_event_target_owner_scope<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Object>,
) -> crate::native_bridge::OwnerDispatchScope {
    indexed_db_typed_owner_scope(scope, target)
        .expect("IDB event target should have typed owner state")
}
