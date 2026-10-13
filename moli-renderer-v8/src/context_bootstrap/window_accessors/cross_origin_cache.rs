use anyhow::{Result, anyhow};

use crate::util::{get_private_value, set_private_value, v8str};

const CACHE: &str = "__moliCrossOriginWindowSurfaceCache";

/// Capture the builtins before author code runs. The array is owned by V8's
/// realm graph, so neither the cache nor its functions root a retired realm.
/// Slot zero starts as the constructor and becomes a WeakMap on first use.
pub(in crate::context_bootstrap) fn initialize_cross_origin_window_cache(
    scope: &mut v8::PinScope<'_, '_>,
) -> Result<()> {
    let context = scope.get_current_context();
    let anchor = context.get_extras_binding_object(scope);
    if get_private_value(scope, anchor, CACHE).is_some() {
        return Ok(());
    }
    let constructor = context
        .global(scope)
        .get(scope, v8str(scope, "WeakMap").into())
        .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
        .ok_or_else(|| anyhow!("missing intrinsic WeakMap constructor"))?;
    let prototype = constructor
        .get(scope, v8str(scope, "prototype").into())
        .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
        .ok_or_else(|| anyhow!("missing intrinsic WeakMap prototype"))?;
    let [get, set] = ["get", "set"].map(|name| {
        prototype
            .get(scope, v8str(scope, name).into())
            .filter(|value| value.is_function())
            .ok_or_else(|| anyhow!("missing intrinsic WeakMap.{name}"))
    });
    let entries = [constructor.into(), get?, set?];
    let cache = v8::Array::new_with_elements(scope, &entries);
    set_private_value(scope, anchor, CACHE, cache.into());
    Ok(())
}

/// Each target owns an ephemeron cache keyed by the accessing realm. This
/// preserves per-observer function identity without retaining dead observers.
pub(crate) fn cached_cross_origin_window_surface<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    target: v8::Local<'s, v8::Context>,
    accessing: v8::Local<'s, v8::Context>,
    build: impl FnOnce(&mut v8::PinScope<'s, '_>) -> v8::Local<'s, v8::Object>,
) -> Option<v8::Local<'s, v8::Object>> {
    if scope.is_execution_terminating() {
        return None;
    }
    let (map, get, set) = {
        let scope = &mut v8::ContextScope::new(scope, target);
        let anchor = target.get_extras_binding_object(scope);
        let cache = get_private_value(scope, anchor, CACHE)
            .and_then(|value| v8::Local::<v8::Array>::try_from(value).ok())
            .expect("Window cross-origin cache initialized during bootstrap");
        let value = cache.get_index(scope, 0).expect("Window cache slot");
        let map = if let Ok(constructor) = v8::Local::<v8::Function>::try_from(value) {
            let map = intrinsic_or_termination(scope, "intrinsic WeakMap construction", |scope| {
                constructor.new_instance(scope, &[])
            })?;
            assert_eq!(cache.set_index(scope, 0, map.into()), Some(true));
            map
        } else {
            v8::Local::<v8::Object>::try_from(value).expect("Window surface WeakMap")
        };
        let method = |scope: &mut v8::PinScope<'s, '_>, index| {
            cache
                .get_index(scope, index)
                .and_then(|value| v8::Local::<v8::Function>::try_from(value).ok())
                .expect("captured intrinsic WeakMap method")
        };
        (map, method(scope, 1), method(scope, 2))
    };
    let scope = &mut v8::ContextScope::new(scope, accessing);
    let key = accessing.get_extras_binding_object(scope);
    let existing = intrinsic_or_termination(scope, "intrinsic WeakMap.get", |scope| {
        get.call(scope, map.into(), &[key.into()])
    })?;
    if let Ok(surface) = v8::Local::<v8::Object>::try_from(existing) {
        return Some(surface);
    }
    let surface = build(scope);
    intrinsic_or_termination(scope, "intrinsic WeakMap.set", |scope| {
        set.call(scope, map.into(), &[key.into(), surface.into()])
    })?;
    Some(surface)
}

fn intrinsic_or_termination<'s, T>(
    scope: &mut v8::PinScope<'s, '_>,
    operation: &str,
    invoke: impl FnOnce(&mut v8::PinScope<'s, '_>) -> Option<T>,
) -> Option<T> {
    let scope = std::pin::pin!(v8::TryCatch::new(scope));
    let scope = &mut scope.init();
    match invoke(scope) {
        Some(value) => Some(value),
        // The isolate flag can reset at the V8 call boundary; TryCatch retains
        // the termination reason. Keep the interruption visible to its caller.
        None if scope.has_terminated() || scope.is_execution_terminating() => {
            let _ = scope.rethrow();
            None
        }
        None => panic!("{operation}"),
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::Cell, pin::pin};

    use super::*;

    #[test]
    fn terminated_execution_stops_cross_origin_cache_lookup_without_building() {
        crate::ensure_v8_for_test();
        let mut isolate = v8::Isolate::new(Default::default());
        let isolate_handle = isolate.thread_safe_handle();
        let scope = pin!(v8::HandleScope::new(&mut isolate));
        let scope = &mut scope.init();
        let target = v8::Context::new(scope, Default::default());
        let accessing = v8::Context::new(scope, Default::default());
        {
            let scope = &mut v8::ContextScope::new(scope, target);
            initialize_cross_origin_window_cache(scope).unwrap();
        }

        let scope = &mut v8::ContextScope::new(scope, accessing);
        assert!(isolate_handle.terminate_execution());
        let built = Cell::new(false);
        let surface = cached_cross_origin_window_surface(scope, target, accessing, |scope| {
            built.set(true);
            v8::Object::new(scope)
        });
        assert!(surface.is_none());
        assert!(!built.get());
        assert!(scope.is_execution_terminating());
        assert!(isolate_handle.cancel_terminate_execution());
    }

    #[test]
    fn termination_during_surface_creation_does_not_cache_and_can_recover() {
        crate::ensure_v8_for_test();
        let mut isolate = v8::Isolate::new(Default::default());
        let handle = isolate.thread_safe_handle();
        let scope = pin!(v8::HandleScope::new(&mut isolate));
        let scope = &mut scope.init();
        let target = v8::Context::new(scope, Default::default());
        let accessing = v8::Context::new(scope, Default::default());
        let other = v8::Context::new(scope, Default::default());
        {
            let scope = &mut v8::ContextScope::new(scope, target);
            initialize_cross_origin_window_cache(scope).unwrap();
        }
        let scope = &mut v8::ContextScope::new(scope, accessing);
        assert!(
            cached_cross_origin_window_surface(scope, target, accessing, |scope| {
                let surface = v8::Object::new(scope);
                assert!(handle.terminate_execution());
                surface
            })
            .is_none()
        );
        assert!(handle.cancel_terminate_execution());
        let surface = cached_cross_origin_window_surface(scope, target, accessing, |scope| {
            v8::Object::new(scope)
        })
        .unwrap();
        let cached = cached_cross_origin_window_surface(scope, target, accessing, |_| {
            panic!("the same observer must reuse its surface")
        })
        .unwrap();
        assert!(surface.strict_equals(cached.into()));
        let distinct = cached_cross_origin_window_surface(scope, target, other, |scope| {
            v8::Object::new(scope)
        })
        .unwrap();
        assert!(!surface.strict_equals(distinct.into()));
    }

    #[test]
    #[should_panic(expected = "intrinsic WeakMap.get")]
    fn non_termination_cache_errors_are_not_silenced() {
        crate::ensure_v8_for_test();
        let mut isolate = v8::Isolate::new(Default::default());
        let scope = pin!(v8::HandleScope::new(&mut isolate));
        let scope = &mut scope.init();
        let target = v8::Context::new(scope, Default::default());
        let accessing = v8::Context::new(scope, Default::default());
        {
            let scope = &mut v8::ContextScope::new(scope, target);
            initialize_cross_origin_window_cache(scope).unwrap();
            let anchor = target.get_extras_binding_object(scope);
            let cache = get_private_value(scope, anchor, CACHE).unwrap();
            let cache = v8::Local::<v8::Array>::try_from(cache).unwrap();
            let source = v8str(scope, "(function () { throw new Error('broken cache'); })");
            let script = v8::Script::compile(scope, source, None).unwrap();
            let throwing = script.run(scope).unwrap();
            assert_eq!(cache.set_index(scope, 1, throwing), Some(true));
        }
        let scope = &mut v8::ContextScope::new(scope, accessing);
        let scope = pin!(v8::TryCatch::new(scope));
        let scope = &mut scope.init();
        let _ = cached_cross_origin_window_surface(scope, target, accessing, |scope| {
            v8::Object::new(scope)
        });
    }
}
