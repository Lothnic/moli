use super::*;
#[cfg(test)]
mod tests;

const LOCATION: &str = "__moliCrossOriginLocationObject";
const TARGET_WINDOW: &str = "__moliCrossOriginLocationWindow";
const WINDOW_LOCATION_SLOT: &str = "__moliWindowLocation";

pub(super) fn set_slot(
    scope: &mut v8::PinScope<'_, '_>,
    object: v8::Local<'_, v8::Object>,
    slot: &str,
    value: v8::Local<'_, v8::Value>,
) -> Option<()> {
    let key = crate::util::private_key(scope, slot)?;
    object
        .set_private(scope, key, value)
        .filter(|set| *set)
        .map(|_| ())
}

fn ensure_shared_location<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    anchor: v8::Local<'s, v8::Object>,
    window: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
    if let Some(location) = get_private_value(scope, anchor, LOCATION)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    {
        return Some(location);
    }
    let target = new_cross_origin_location_proxy_target(scope)?;
    set_slot(scope, target, TARGET_WINDOW, window.into())?;
    let location = initialize_cross_origin_location_proxy(scope, target)?;
    set_slot(scope, anchor, LOCATION, location.into())?;
    Some(location)
}

fn target_window<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
    get_cross_origin_proxy_private_value(scope, receiver, TARGET_WINDOW)
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
}

pub(super) fn is_window_location<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
) -> bool {
    target_window(scope, receiver).is_some()
}

pub(super) fn navigate<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    receiver: v8::Local<'s, v8::Object>,
    value: v8::Local<'s, v8::Value>,
    kind: crate::context_bootstrap::LocationNavigationKind,
) -> bool {
    let Some(window) = target_window(scope, receiver) else {
        return false;
    };
    // The stable WindowProxy follows its successor, but a Location belongs to
    // the Window realm that created it. An old Location must not gain the new
    // Document's navigation authority through that proxy.
    if receiver.get_creation_context(scope) != window.get_creation_context(scope) {
        return false;
    }
    navigate_window(scope, window, value, kind, true)
}

fn navigate_window<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    value: v8::Local<'s, v8::Value>,
    kind: crate::context_bootstrap::LocationNavigationKind,
    block_javascript: bool,
) -> bool {
    let raw = match webidl::convert::<webidl::UsvString>(
        scope,
        value,
        webidl::Context::member("Location", "href"),
    ) {
        Ok(value) => value.0,
        Err(error) => {
            webidl::throw_error(scope, &error);
            return false;
        }
    };
    if block_javascript && url::Url::parse(&raw).is_ok_and(|url| url.scheme() == "javascript") {
        throw_cross_origin_location_security_error(scope);
        return false;
    }
    let raw = if kind != crate::context_bootstrap::LocationNavigationKind::Reload {
        crate::context_bootstrap::resolve_cross_window_location_target(scope, &raw).unwrap_or(raw)
    } else {
        raw
    };
    let Some(context) = window.get_creation_context(scope) else {
        return false;
    };
    let scope = &mut v8::ContextScope::new(scope, context);
    let Some(location) = window
        .get(scope, v8str(scope, "location").into())
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
    else {
        return false;
    };
    crate::context_bootstrap::navigate_location_object_with_child_navigate_event(
        scope,
        location,
        kind,
        Some(raw),
    );
    true
}

fn shared_cross_origin_location<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
    let context = window.get_creation_context(scope)?;
    let scope = &mut v8::ContextScope::new(scope, context);
    let anchor = context.get_extras_binding_object(scope);
    let window = context.global(scope);
    ensure_shared_location(scope, anchor, window)
}

pub(super) fn window_location<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Value>> {
    if caller_can_access_window(scope, window) {
        return get_private_value(scope, window, WINDOW_LOCATION_SLOT);
    }
    shared_cross_origin_location(scope, window).map(Into::into)
}

pub(super) fn set_window_location<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    window: v8::Local<'s, v8::Object>,
    value: v8::Local<'s, v8::Value>,
) -> Option<()> {
    let block_javascript = !caller_can_access_window(scope, window);
    navigate_window(
        scope,
        window,
        value,
        crate::context_bootstrap::LocationNavigationKind::Assign,
        block_javascript,
    );
    Some(())
}
