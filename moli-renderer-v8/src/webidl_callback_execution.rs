//! Renderer execution policy for Web IDL callback functions shared by Window
//! and Worker APIs. Callback values and Realm entry remain in the neutral
//! callback crate; API owners retain scheduling, registration, return-value
//! conversion, and the choice to propagate or report exceptions.

use moli_webidl_callback::{PreparedWebIdlCallbackFunction, invoke_webidl_callback_function};

use crate::{
    exception_reporting::{
        CallbackExceptionLogLevel, V8ExceptionReport, invoke_callback_with_report,
    },
    host::report_event_callback_exception,
    native_bridge::{JsContextHost, WindowExecutionContextIdentity},
    util::context_host_ptr_from_global_bridge,
    v8_traced_webidl_callback::V8TracedWebIdlCallbackFunction,
    worker::{
        dispatch_current_worker_callback_exception, get_worker_state, worker_termination_requested,
    },
};

/// A short-lived invocation snapshot. It must not be retained as an async
/// task binding: the API's registration or task owner supplies that lifetime.
pub(crate) struct PreparedWebIdlCallbackExecution {
    callback: PreparedWebIdlCallbackFunction,
    environment: CallbackExecutionEnvironment,
}

pub(crate) enum WebIdlCallbackExecutionOutcome<R, E> {
    Returned(R),
    Failed(E),
    Unavailable,
}

enum CallbackExecutionEnvironment {
    Window {
        host_ptr: *mut JsContextHost,
        // Some API registrations, including observers, authorize both the
        // API object's owner and the callback's potentially different Realm.
        owner_identity: Option<WindowExecutionContextIdentity>,
        callback_identity: WindowExecutionContextIdentity,
    },
    Worker {
        context: v8::Global<v8::Context>,
    },
}

impl PreparedWebIdlCallbackExecution {
    pub(crate) fn window(
        callback: PreparedWebIdlCallbackFunction,
        host_ptr: *mut JsContextHost,
        owner_identity: Option<WindowExecutionContextIdentity>,
        callback_identity: WindowExecutionContextIdentity,
    ) -> Self {
        Self {
            callback,
            environment: CallbackExecutionEnvironment::Window {
                host_ptr,
                owner_identity,
                callback_identity,
            },
        }
    }

    pub(crate) fn worker<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        callback: PreparedWebIdlCallbackFunction,
        context: v8::Local<'s, v8::Context>,
    ) -> Self {
        Self {
            callback,
            environment: CallbackExecutionEnvironment::Worker {
                context: v8::Global::new(scope, context),
            },
        }
    }

    /// Resolve the environment once while restoring the traced callback.
    /// APIs with additional registration-time identities use `window` or
    /// `worker` after their owner-specific authorization instead.
    pub(crate) fn from_traced<'s>(
        scope: &mut v8::PinScope<'s, '_>,
        carrier: v8::Local<'s, v8::Object>,
    ) -> Option<Self> {
        let callback = V8TracedWebIdlCallbackFunction::from_object(carrier).prepare(scope);
        if let Some(host_ptr) = context_host_ptr_from_global_bridge(scope) {
            let host = unsafe { &*host_ptr };
            let context = callback.relevant_context(scope);
            let callback_identity =
                host.window_execution_context_identity_for_v8_context(scope, context)?;
            return Some(Self::window(callback, host_ptr, None, callback_identity));
        }
        get_worker_state(scope)?;
        let context = scope.get_current_context();
        Some(Self::worker(scope, callback, context))
    }

    /// Execute the caller's invocation and return conversion while the Web
    /// IDL relevant/incumbent contexts are entered. This entry point neither
    /// captures nor reports exceptions on behalf of the API algorithm.
    pub(crate) fn invoke_with<'s, 'a, R, E>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        receiver: v8::Local<'s, v8::Value>,
        arguments: &'a [v8::Local<'s, v8::Value>],
        invoke_function: impl FnOnce(
            &mut v8::PinScope<'s, '_>,
            v8::Local<'s, v8::Function>,
            v8::Local<'s, v8::Value>,
            &'a [v8::Local<'s, v8::Value>],
        ) -> Result<R, E>,
    ) -> WebIdlCallbackExecutionOutcome<R, E> {
        if !self.environment.can_invoke(scope) {
            return WebIdlCallbackExecutionOutcome::Unavailable;
        }
        match invoke_webidl_callback_function(
            scope,
            &self.callback,
            receiver,
            arguments,
            invoke_function,
        ) {
            Ok(result) => WebIdlCallbackExecutionOutcome::Returned(result),
            Err(error) => WebIdlCallbackExecutionOutcome::Failed(error),
        }
    }

    pub(crate) fn invoke_and_report<'s>(
        &self,
        scope: &mut v8::PinScope<'s, '_>,
        event_type: &str,
        callback_name: &str,
        receiver: v8::Local<'s, v8::Value>,
        arguments: &[v8::Local<'s, v8::Value>],
    ) {
        // No registry borrow survives preparation. Author code may reenter
        // the API or register another callback synchronously.
        let result = self.invoke_with(
            scope,
            receiver,
            arguments,
            |scope, callback, receiver, arguments| {
                invoke_callback_with_report(
                    scope,
                    "callback",
                    "Web IDL callback threw",
                    CallbackExceptionLogLevel::Debug,
                    callback_name,
                    callback,
                    receiver,
                    arguments,
                )
            },
        );
        if let WebIdlCallbackExecutionOutcome::Failed(report) = result {
            self.environment.report_exception(scope, event_type, report);
        }
    }
}

impl CallbackExecutionEnvironment {
    fn can_invoke(&self, scope: &mut v8::PinScope<'_, '_>) -> bool {
        match self {
            Self::Window {
                host_ptr,
                owner_identity,
                callback_identity,
            } => {
                let host = unsafe { &**host_ptr };
                let current = owner_identity.is_none_or(|identity| {
                    host.window_execution_context_identity_is_current(identity)
                }) && host
                    .window_execution_context_identity_is_current(*callback_identity);
                if current {
                    host.debug_assert_not_in_structural_mutation("callback-function invocation");
                }
                current
            }
            Self::Worker { context } => {
                scope.get_current_context() == v8::Local::new(scope, context)
                    // `close()` stops future task delivery, but synchronous
                    // APIs and already queued V8 microtasks may still run.
                    // API/task owners enforce their own closing policy.
                    && get_worker_state(scope).is_some()
                    && !worker_termination_requested(scope)
                    && !scope.is_execution_terminating()
            }
        }
    }

    fn report_exception(
        &self,
        scope: &mut v8::PinScope<'_, '_>,
        event_type: &str,
        report: Box<V8ExceptionReport>,
    ) {
        match self {
            Self::Window {
                host_ptr,
                callback_identity,
                ..
            } => {
                report_event_callback_exception(
                    scope,
                    *host_ptr,
                    event_type,
                    Some(*callback_identity),
                    None,
                    &report,
                );
            }
            Self::Worker { context } => {
                let context = v8::Local::new(scope, context);
                let scope = &mut v8::ContextScope::new(scope, context);
                dispatch_current_worker_callback_exception(scope, *report);
            }
        }
    }
}
