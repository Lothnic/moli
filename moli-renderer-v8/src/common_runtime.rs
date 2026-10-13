//! Shared owner-local Web API state and its Context entry point.
//!
//! A Context locates its owner through a weak binding. Window Realm tokens and
//! Worker run state remain the authoritative execution identities; retaining a
//! Context must not independently root its host or its callback graph.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

use crate::{
    native_bridge::JsContextHost,
    util::{ContextHostLifecycle, page_context_is_disconnected},
    v8_finalizer::V8FinalizerRegistry,
    worker::WorkerGlobalState,
};

#[derive(Default)]
pub(crate) struct CommonRuntimeState {
    pub(crate) finalizers: RefCell<V8FinalizerRegistry>,
}

impl CommonRuntimeState {
    pub(crate) fn clear_for_context_teardown(&self) {
        // Cleanup may reenter native components. Drop the component borrow
        // before executing any cleanup, just as registration releases owner
        // borrows before allocating a V8 weak handle.
        let mut finalizers = std::mem::take(&mut *self.finalizers.borrow_mut());
        finalizers.clear_for_context_teardown();
    }

    #[cfg(test)]
    pub(crate) fn finalizer_count(&self) -> usize {
        self.finalizers.borrow().len()
    }
}

pub(crate) struct CommonRuntimeBinding {
    common: Weak<CommonRuntimeState>,
    owner: RuntimeOwner,
}

enum RuntimeOwner {
    Window {
        host_ptr: *mut JsContextHost,
        lifecycle: Weak<Cell<ContextHostLifecycle>>,
    },
    Worker {
        state: Weak<RefCell<WorkerGlobalState>>,
    },
}

impl CommonRuntimeBinding {
    pub(crate) fn install_window(
        context: v8::Local<'_, v8::Context>,
        common: &Rc<CommonRuntimeState>,
        host_ptr: *mut JsContextHost,
        lifecycle: &Rc<Cell<ContextHostLifecycle>>,
    ) {
        if let Some(existing) = context.get_slot::<Self>() {
            assert!(
                Weak::ptr_eq(&existing.common, &Rc::downgrade(common))
                    && matches!(existing.owner, RuntimeOwner::Window { host_ptr: installed, .. } if installed == host_ptr),
                "a Context's common runtime owner must not be rebound"
            );
            return;
        }
        let previous = context.set_slot(Rc::new(Self {
            common: Rc::downgrade(common),
            owner: RuntimeOwner::Window {
                host_ptr,
                lifecycle: Rc::downgrade(lifecycle),
            },
        }));
        debug_assert!(previous.is_none());
    }

    pub(crate) fn install_worker(
        context: v8::Local<'_, v8::Context>,
        state: &Rc<RefCell<WorkerGlobalState>>,
    ) {
        let common = Rc::clone(&state.borrow().common);
        let previous = context.set_slot(Rc::new(Self {
            common: Rc::downgrade(&common),
            owner: RuntimeOwner::Worker {
                state: Rc::downgrade(state),
            },
        }));
        assert!(
            previous.is_none(),
            "a Worker Context must have one common runtime binding"
        );
    }

    pub(crate) fn from_scope(scope: &mut v8::PinScope<'_, '_>) -> Option<Rc<Self>> {
        let binding = scope.get_current_context().get_slot::<Self>()?;
        binding.validate_context(scope)?;
        Some(binding)
    }

    pub(crate) fn runtime(&self) -> Option<Rc<CommonRuntimeState>> {
        self.common.upgrade()
    }

    fn validate_context(&self, scope: &mut v8::PinScope<'_, '_>) -> Option<()> {
        let context = scope.get_current_context();
        let installed = context.get_slot::<Self>()?;
        if !std::ptr::eq(self, installed.as_ref()) {
            return None;
        }
        self.runtime()?;
        match &self.owner {
            RuntimeOwner::Window { .. } => {
                if page_context_is_disconnected(context) {
                    return None;
                }
                self.window_host_ptr()?;
            }
            RuntimeOwner::Worker { state, .. } => {
                state.upgrade()?;
            }
        }
        Some(())
    }

    pub(crate) fn window_host_ptr(&self) -> Option<*mut JsContextHost> {
        self.runtime()?;
        let RuntimeOwner::Window {
            host_ptr,
            lifecycle,
        } = &self.owner
        else {
            return None;
        };
        (lifecycle.upgrade()?.get() != ContextHostLifecycle::Destroyed).then_some(*host_ptr)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_bindings_share_components_without_rooting_or_confusing_owners() {
        crate::ensure_v8_for_test();
        let mut isolate = v8::Isolate::new(Default::default());
        let scope = std::pin::pin!(v8::HandleScope::new(&mut isolate));
        let scope = &mut scope.init();
        let first = v8::Context::new(scope, Default::default());
        let second = v8::Context::new(scope, Default::default());
        let other = v8::Context::new(scope, Default::default());
        let common = Rc::new(CommonRuntimeState::default());
        let other_common = Rc::new(CommonRuntimeState::default());
        let lifecycle = Rc::new(Cell::new(ContextHostLifecycle::Active));
        let other_lifecycle = Rc::new(Cell::new(ContextHostLifecycle::Active));
        // This test exercises component ownership and Context identity only.
        // The opaque host address is never dereferenced by a host API.
        let host = std::ptr::NonNull::<JsContextHost>::dangling().as_ptr();
        CommonRuntimeBinding::install_window(first, &common, host, &lifecycle);
        CommonRuntimeBinding::install_window(second, &common, host, &lifecycle);
        CommonRuntimeBinding::install_window(other, &other_common, host, &other_lifecycle);
        let first_binding = {
            let scope = &mut v8::ContextScope::new(scope, first);
            let binding = CommonRuntimeBinding::from_scope(scope).unwrap();
            assert!(Rc::ptr_eq(&binding.runtime().unwrap(), &common));
            binding
        };
        {
            let scope = &mut v8::ContextScope::new(scope, second);
            let binding = CommonRuntimeBinding::from_scope(scope).unwrap();
            assert!(Rc::ptr_eq(&binding.runtime().unwrap(), &common));
            assert!(!Rc::ptr_eq(&binding, &first_binding));
            assert!(first_binding.validate_context(scope).is_none());
        }
        {
            let scope = &mut v8::ContextScope::new(scope, other);
            let binding = CommonRuntimeBinding::from_scope(scope).unwrap();
            assert!(!Rc::ptr_eq(&binding.runtime().unwrap(), &common));
            other_lifecycle.set(ContextHostLifecycle::Destroyed);
            assert!(CommonRuntimeBinding::from_scope(scope).is_none());
        }
        drop(common);
        let scope = &mut v8::ContextScope::new(scope, first);
        assert!(CommonRuntimeBinding::from_scope(scope).is_none());
        assert!(first_binding.runtime().is_none());
    }
}
