//! Registration and callback residence adapters for PerformanceObserver.
//!
//! Window retains exact Realm bindings and active roots in ObserverStore;
//! Worker retains active observers on its global. Neither backend changes the
//! timeline's batching algorithm or owns a second callback invocation path.

use moli_webidl_callback::{PreparedWebIdlCallbackFunction, WebIdlCallbackFunction};

use super::*;
use crate::common_runtime::{CommonRuntimeBinding, RuntimeExecutionContext};
use crate::observer_runtime::{ObserverCallbackId, ObserverCallbackResidence};
use crate::util::{get_private_value, set_private_value};
use crate::webidl_callback_execution::PreparedWebIdlCallbackExecution;

enum RegistrationEnvironment {
    Window {
        host_ptr: *mut JsContextHost,
        callback_id: ObserverCallbackId,
    },
    Worker,
}

pub(super) struct PerformanceObserverRegistration<'s> {
    observer: v8::Local<'s, v8::Object>,
    environment: RegistrationEnvironment,
    binding: std::rc::Rc<CommonRuntimeBinding>,
}

pub(super) fn register_performance_observer_callback<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    observer: v8::Local<'s, v8::Object>,
    callback: WebIdlCallbackFunction,
) -> Option<(
    u32,
    v8::Local<'s, v8::Object>,
    v8::Local<'s, v8::Object>,
    v8::Local<'s, v8::Object>,
)> {
    let (_, owner) = CommonRuntimeBinding::from_scope(scope)?;
    if let RuntimeExecutionContext::Window(host_ptr) = owner {
        let (id, callback, relevant, incumbent) =
            crate::observer_runtime::register_callback(scope, host_ptr, observer, callback)
                .into_parts();
        return Some((id.as_u32(), callback, relevant, incumbent));
    }
    let value = v8::Local::<v8::Object>::try_from(callback.value(scope)).ok()?;
    let relevant = callback.relevant_context(scope).global(scope);
    let incumbent = callback.incumbent_context(scope).global(scope);
    // Zero is outside ObserverCallbackId's valid range. Worker registration is
    // tied to the observer's creation context rather than a Window binding.
    Some((0, value, relevant, incumbent))
}

impl<'s> PerformanceObserverRegistration<'s> {
    pub(super) fn from_observer(
        scope: &mut v8::PinScope<'s, '_>,
        observer: v8::Local<'s, v8::Object>,
    ) -> Option<Self> {
        let id = get_private_value(scope, observer, PERFORMANCE_OBSERVER_CALLBACK_ID_SLOT)?
            .number_value(scope)?;
        let (binding, owner) = CommonRuntimeBinding::from_scope(scope)?;
        let environment = match owner {
            RuntimeExecutionContext::Window(host_ptr) => RegistrationEnvironment::Window {
                host_ptr,
                callback_id: ObserverCallbackId::from_number(id)?,
            },
            RuntimeExecutionContext::Worker => {
                if id != 0.0 || observer.get_creation_context(scope)? != scope.get_current_context()
                {
                    return None;
                }
                RegistrationEnvironment::Worker
            }
        };
        Some(Self {
            observer,
            environment,
            binding,
        })
    }

    pub(super) fn is_current(&self, scope: &mut v8::PinScope<'s, '_>) -> bool {
        match self.environment {
            RegistrationEnvironment::Window {
                host_ptr,
                callback_id,
            } => crate::observer_runtime::callback_is_current(host_ptr, callback_id),
            RegistrationEnvironment::Worker => {
                self.binding.worker_can_deliver_task() && !scope.is_execution_terminating()
            }
        }
    }

    pub(super) fn activate(&self, scope: &mut v8::PinScope<'s, '_>) -> bool {
        match self.environment {
            RegistrationEnvironment::Window {
                host_ptr,
                callback_id,
            } => {
                // ObserverStore owns the active root so disconnect and exact
                // Realm retirement release it along with the binding.
                crate::observer_runtime::activate_performance_observer_callback(
                    scope,
                    host_ptr,
                    callback_id,
                    self.observer,
                )
            }
            RegistrationEnvironment::Worker => {
                if !self.is_current(scope) {
                    return false;
                }
                push_object_to_global_registry(
                    scope,
                    PERFORMANCE_OBSERVER_REGISTRY_SLOT,
                    self.observer,
                );
                true
            }
        }
    }

    pub(super) fn deactivate(&self, scope: &mut v8::PinScope<'s, '_>) {
        match self.environment {
            RegistrationEnvironment::Window {
                host_ptr,
                callback_id,
            } => {
                crate::observer_runtime::deactivate_performance_observer_callback(
                    host_ptr,
                    callback_id,
                );
            }
            RegistrationEnvironment::Worker => {
                let Some(registry) = global_queue_array(scope, PERFORMANCE_OBSERVER_REGISTRY_SLOT)
                else {
                    return;
                };
                let retained: Vec<_> = (0..registry.length())
                    .filter_map(|index| registry.get_index(scope, index))
                    .filter(|value| !value.strict_equals(self.observer.into()))
                    .collect();
                let registry = v8::Array::new_with_elements(scope, &retained);
                let global = scope.get_current_context().global(scope);
                set_private_value(
                    scope,
                    global,
                    PERFORMANCE_OBSERVER_REGISTRY_SLOT,
                    registry.into(),
                );
            }
        }
    }

    pub(super) fn prepare_delivery(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
    ) -> Option<PreparedWebIdlCallbackExecution> {
        if !self.is_current(scope) {
            return None;
        }
        // Read callback and both conversion-time context anchors once for
        // either environment. These private values remain traced by V8.
        let object = |scope: &mut v8::PinScope<'s, '_>, slot| {
            get_private_value(scope, self.observer, slot)
                .and_then(|value| v8::Local::<v8::Object>::try_from(value).ok())
        };
        let callback = object(scope, PERFORMANCE_OBSERVER_CALLBACK_VALUE_SLOT)?;
        let relevant = object(scope, PERFORMANCE_OBSERVER_CALLBACK_RELEVANT_GLOBAL_SLOT)?;
        let incumbent = object(scope, PERFORMANCE_OBSERVER_CALLBACK_INCUMBENT_GLOBAL_SLOT)?;
        match self.environment {
            RegistrationEnvironment::Window {
                host_ptr,
                callback_id,
            } => {
                let residence = ObserverCallbackResidence::from_parts(
                    callback_id,
                    callback,
                    relevant,
                    incumbent,
                );
                crate::observer_runtime::prepare_callback(scope, host_ptr, residence)
            }
            RegistrationEnvironment::Worker => {
                let context = self.observer.get_creation_context(scope)?;
                let relevant = relevant.get_creation_context(scope)?;
                let incumbent = incumbent.get_creation_context(scope)?;
                let callback =
                    PreparedWebIdlCallbackFunction::try_new(scope, callback, relevant, incumbent)?;
                PreparedWebIdlCallbackExecution::worker(scope, callback, context)
            }
        }
    }
}

pub(super) fn performance_observer_callback_is_current<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    observer: v8::Local<'s, v8::Object>,
) -> bool {
    PerformanceObserverRegistration::from_observer(scope, observer)
        .is_some_and(|registration| registration.is_current(scope))
}

pub(super) fn active_performance_observers<'s>(
    scope: &mut v8::PinScope<'s, '_>,
) -> Vec<v8::Local<'s, v8::Object>> {
    let Some((_, owner)) = CommonRuntimeBinding::from_scope(scope) else {
        return Vec::new();
    };
    if let RuntimeExecutionContext::Window(host_ptr) = owner {
        return crate::observer_runtime::active_performance_observer_callbacks(scope, host_ptr);
    }
    let Some(registry) = global_queue_array(scope, PERFORMANCE_OBSERVER_REGISTRY_SLOT) else {
        return Vec::new();
    };
    (0..registry.length())
        .filter_map(|index| registry.get_index(scope, index))
        .filter_map(|value| v8::Local::<v8::Object>::try_from(value).ok())
        .collect()
}
