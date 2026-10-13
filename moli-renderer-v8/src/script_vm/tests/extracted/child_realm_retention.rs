use super::*;
use moli_page_types::LayoutPolicy;

fn collected_native_context_count(vm: &mut StandaloneScriptVmHarness) -> usize {
    // Two collections also process weak handles cleared by the first pass.
    // No JS WeakRef is dereferenced in the collecting task.
    for _ in 0..2 {
        vm.renderer_document_isolate
            .with_renderer_document_isolate_mut(|isolate| isolate.low_memory_notification());
    }
    vm.renderer_document_isolate_heap_usage()
        .expect("native context inventory")
        .number_of_native_contexts
}

#[test]
fn retired_child_content_surfaces_do_not_root_unreachable_realms() {
    for policy in [LayoutPolicy::Mock, LayoutPolicy::OnDemand] {
        let mut vm = new_parsed_test_vm(
            "https://child-content-retention.test/",
            "<!doctype html><body></body>",
        );
        vm.set_layout_policy(policy);
        let baseline = collected_native_context_count(&mut vm);
        for _ in 0..4 {
            vm.eval(
                r#"
                (() => {
                  const frame = document.createElement('iframe');
                  document.body.append(frame);
                  const child = frame.contentDocument;
                  child.body.textContent = 'retire this child';
                  if (frame.contentWindow.document !== child) throw new Error('identity');
                  frame.remove();
                  return 'removed';
                })()
                "#,
            )
            .expect("create, expose and remove child content surfaces");
            vm.prune_stale_child_default_execution_contexts();
        }
        assert_eq!(vm.child_frame_realm_store.len(), 0, "{policy:?}");
        assert_eq!(
            collected_native_context_count(&mut vm),
            baseline,
            "{policy:?}: retiring the frame must release caches that root otherwise unreachable child realms"
        );
    }
}

#[test]
fn retained_child_content_surfaces_preserve_author_state_until_released() {
    for policy in [LayoutPolicy::Mock, LayoutPolicy::OnDemand] {
        let mut vm = new_parsed_test_vm(
            "https://retained-child-content.test/",
            "<!doctype html><body></body>",
        );
        vm.set_layout_policy(policy);
        let baseline = collected_native_context_count(&mut vm);
        vm.eval(
            r#"
            (() => {
              const frame = document.createElement('iframe');
              document.body.append(frame);
              const child = frame.contentDocument;
              child.body.innerHTML = '<p id="kept">before</p>';
              child.authorMarker = 42;
              globalThis.savedChildDocument = child;
              globalThis.savedChildWindow = frame.contentWindow;
              savedChildWindow.authorMarker = 43;
              globalThis.savedChildNode = child.getElementById('kept');
              frame.remove();
              return 'retained';
            })()
            "#,
        )
        .expect("retain author references across child detachment");
        vm.prune_stale_child_default_execution_contexts();
        assert!(
            collected_native_context_count(&mut vm) > baseline,
            "{policy:?}: an author reference must retain its realm"
        );
        assert_eq!(
            vm.eval(
                r#"
                savedChildNode.textContent = 'after';
                JSON.stringify([
                  savedChildDocument.authorMarker,
                  savedChildWindow.authorMarker,
                  savedChildDocument.getElementById('kept') === savedChildNode,
                  savedChildNode.ownerDocument === savedChildDocument,
                  savedChildDocument.body.textContent
                ])
                "#,
            )
            .expect("retained author state remains observable"),
            r#"[42,43,true,true,"after"]"#,
            "{policy:?}"
        );
        vm.eval("savedChildDocument = savedChildWindow = savedChildNode = undefined; 'released'")
            .expect("release the last author references");
        assert_eq!(
            collected_native_context_count(&mut vm),
            baseline,
            "{policy:?}: caches must not outlive the last author reference"
        );
    }
}

#[test]
fn promoted_child_content_surfaces_follow_author_reachability() {
    for policy in [LayoutPolicy::Mock, LayoutPolicy::OnDemand] {
        let mut vm = new_parsed_test_vm(
            "https://promoted-child-content.test/",
            "<!doctype html><body></body>",
        );
        vm.set_layout_policy(policy);
        let baseline = collected_native_context_count(&mut vm);
        vm.eval(
            r#"
            (() => {
              globalThis.savedFrame = document.createElement('iframe');
              document.body.append(savedFrame);
              const child = savedFrame.contentDocument;
              child.authorMarker = 42;
              child.body.innerHTML = '<p id="kept">before</p>';
              globalThis.savedChildNode = child.getElementById('kept');
              savedChildNode.authorMarker = 43;
              return 'live';
            })()
            "#,
        )
        .expect("expose live child surfaces");
        assert!(vm.run_child_realm_materialization_body_for_test().unwrap());
        assert_eq!(vm.child_frame_realm_store.len(), 1);
        assert!(collected_native_context_count(&mut vm) > baseline);
        assert_eq!(
            vm.eval(
                "savedFrame.contentDocument === savedChildNode.ownerDocument && \
                 savedFrame.contentWindow.document === savedChildNode.ownerDocument",
            )
            .unwrap(),
            "true",
            "{policy:?}: live registry owns the surfaces across GC"
        );
        vm.eval("savedFrame.remove(); savedFrame = undefined; 'removed'")
            .unwrap();
        vm.prune_stale_child_default_execution_contexts();
        assert_eq!(vm.child_frame_realm_store.len(), 0);
        assert!(collected_native_context_count(&mut vm) > baseline);
        assert_eq!(
            vm.eval(
                r#"
                savedChildNode.textContent = 'after';
                JSON.stringify([
                  savedChildNode.authorMarker,
                  savedChildNode.ownerDocument.authorMarker,
                  savedChildNode.ownerDocument.getElementById('kept') === savedChildNode,
                  savedChildNode.ownerDocument.body.textContent
                ])
                "#,
            )
            .unwrap(),
            r#"[43,42,true,"after"]"#,
            "{policy:?}: retaining only a Node preserves its Document and identity"
        );
        vm.eval("savedChildNode = undefined; 'released'").unwrap();
        assert_eq!(
            collected_native_context_count(&mut vm),
            baseline,
            "{policy:?}"
        );
    }
}

#[test]
fn detached_iframe_projection_keeps_its_cached_surfaces_across_gc() {
    let mut vm = new_storage_test_vm("https://detached-content-cache.test/");
    vm.eval(
        r#"
        (() => {
          const detached = document.implementation.createHTMLDocument('');
          globalThis.savedDetachedFrame = detached.createElement('iframe');
          detached.body.append(savedDetachedFrame);
          const child = savedDetachedFrame.contentDocument;
          const win = savedDetachedFrame.contentWindow;
          child.authorMarker = 42;
          win.authorMarker = 43;
          child.body.innerHTML = '<p>kept</p>';
          return 'cached';
        })()
        "#,
    )
    .unwrap();
    collected_native_context_count(&mut vm);
    assert_eq!(
        vm.eval(
            r#"
            JSON.stringify([
              savedDetachedFrame.contentDocument.authorMarker,
              savedDetachedFrame.contentWindow.authorMarker,
              savedDetachedFrame.contentWindow.document === savedDetachedFrame.contentDocument,
              savedDetachedFrame.contentDocument.body.textContent
            ])
            "#,
        )
        .unwrap(),
        r#"[42,43,true,"kept"]"#
    );
}
