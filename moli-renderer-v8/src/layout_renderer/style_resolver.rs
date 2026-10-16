use moli_layout::{
    LayoutAnonymousStyleContext, LayoutDisplay, LayoutError, LayoutStyleOrigin,
    LayoutStyleResolver, LayoutViewport, ResolvedLayoutElementStyles, ResolvedLayoutPseudoStyle,
    ResolvedLayoutStyle,
};
use std::{
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use crate::{
    document_runtime::DomHandle,
    native_bridge::{
        JsContextHost,
        element::{PreparedAnonymousStyleContext, StyleObservation},
    },
    style_engine::{StyleViewport, StyloAnonymousBoxKind},
};

static NEXT_STYLE_PASS: AtomicU64 = AtomicU64::new(1);

pub(super) struct NativeLayoutStyleResolver<'a> {
    runtime: &'a JsContextHost,
    reads: StyleObservation<'a>,
    anonymous_context: Option<PreparedAnonymousStyleContext<'a>>,
    pass_id: u64,
    scripting_enabled: bool,
    profile: Option<NativeLayoutStyleResolverProfile>,
}

#[derive(Default)]
struct NativeLayoutStyleResolverProfile {
    primary_count: u64,
    primary_read_ns: u128,
    primary_projection_ns: u128,
    primary_policy_ns: u128,
    eager_pseudo_count: u64,
    eager_pseudo_projection_ns: u128,
    marker_count: u64,
    marker_read_ns: u128,
    marker_projection_ns: u128,
    anonymous_count: u64,
    anonymous_read_ns: u128,
    anonymous_projection_ns: u128,
}

impl<'a> NativeLayoutStyleResolver<'a> {
    pub(super) fn new(
        runtime: &'a JsContextHost,
        root: DomHandle,
        document: DomHandle,
        viewport: LayoutViewport,
    ) -> Self {
        let mut reads = layout_style_read_scope(runtime, document, viewport);
        // Drain pending invalidations and update the retained world before
        // freezing the anonymous-style context for this synchronous pass.
        let anonymous_context = reads.read(root).prepare_anonymous_style_context();
        Self {
            runtime,
            reads,
            anonymous_context,
            pass_id: NEXT_STYLE_PASS
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |next| {
                    next.checked_add(1)
                })
                .expect("layout style pass identity exhausted"),
            scripting_enabled: runtime.document_scripting_enabled(document),
            profile: moli_trace::cpu_profile_enabled()
                .then(NativeLayoutStyleResolverProfile::default),
        }
    }

    fn style_origin(&self, owner: DomHandle) -> LayoutStyleOrigin {
        LayoutStyleOrigin::new(self.pass_id, u64::from(owner.index_u32()))
    }

    pub(super) fn trace_profile(&self, document: crate::document_runtime::DomHandle) {
        let Some(profile) = self.profile.as_ref() else {
            return;
        };
        tracing::info!(
            target: "moli_cpu_profile",
            stage = "layout_style_resolver",
            document = document.index_u32(),
            primary_count = profile.primary_count,
            primary_read_us = profile.primary_read_ns / 1_000,
            primary_projection_us = profile.primary_projection_ns / 1_000,
            primary_policy_us = profile.primary_policy_ns / 1_000,
            eager_pseudo_count = profile.eager_pseudo_count,
            eager_pseudo_projection_us = profile.eager_pseudo_projection_ns / 1_000,
            marker_count = profile.marker_count,
            marker_read_us = profile.marker_read_ns / 1_000,
            marker_projection_us = profile.marker_projection_ns / 1_000,
            anonymous_count = profile.anonymous_count,
            anonymous_read_us = profile.anonymous_read_ns / 1_000,
            anonymous_projection_us = profile.anonymous_projection_ns / 1_000,
        );
    }
}

fn layout_style_read_scope<'a>(
    runtime: &'a JsContextHost,
    document: DomHandle,
    viewport: LayoutViewport,
) -> StyleObservation<'a> {
    let viewport = layout_style_viewport(runtime, viewport);
    if document == runtime.document_handle() && viewport == runtime.style_viewport() {
        // Keep the normal main-document observation environment so layout and
        // rendered-text collection read the same persistent style world. Only
        // an override viewport or embedded Document needs an explicit context.
        StyleObservation::new(runtime)
    } else {
        StyleObservation::new_for_document_viewport(runtime, document, viewport)
    }
}

fn layout_style_viewport(runtime: &JsContextHost, viewport: LayoutViewport) -> StyleViewport {
    let screen = runtime.style_viewport();
    StyleViewport::new(
        Some(f64::from(viewport.css_width)),
        Some(f64::from(viewport.css_height)),
    )
    .with_screen_size(screen.screen_width, screen.screen_height)
    .with_device_pixel_ratio(screen.device_pixel_ratio)
}

impl LayoutStyleResolver<DomHandle> for NativeLayoutStyleResolver<'_> {
    fn element_styles(
        &mut self,
        node: DomHandle,
    ) -> Result<Option<ResolvedLayoutElementStyles>, LayoutError> {
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        let read = self.reads.read(node);
        let Some((computed, before, after)) = read.into_element_computed_values() else {
            if let Some(profile) = self.profile.as_mut() {
                profile.primary_count = profile.primary_count.saturating_add(1);
                profile.primary_read_ns = profile.primary_read_ns.saturating_add(
                    phase_started
                        .map(|started| started.elapsed().as_nanos())
                        .unwrap_or_default(),
                );
            }
            return Ok(None);
        };
        let eager_pseudo_count = u64::from(before.is_some()) + u64::from(after.is_some());
        if let Some(profile) = self.profile.as_mut() {
            profile.primary_count = profile.primary_count.saturating_add(1);
            profile.primary_read_ns = profile.primary_read_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        let origin = self.style_origin(node);
        let mut resolved = ResolvedLayoutStyle::from_stylo(computed).with_source_origin(origin);
        if let Some(profile) = self.profile.as_mut() {
            profile.primary_projection_ns = profile.primary_projection_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        let before = before
            .map(|computed| ResolvedLayoutStyle::from_stylo(computed).with_source_origin(origin));
        let after = after
            .map(|computed| ResolvedLayoutStyle::from_stylo(computed).with_source_origin(origin));
        if let Some(profile) = self.profile.as_mut() {
            profile.eager_pseudo_count = profile
                .eager_pseudo_count
                .saturating_add(eager_pseudo_count);
            profile.eager_pseudo_projection_ns = profile.eager_pseudo_projection_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        if self.scripting_enabled
            && self
                .runtime
                .dom_host()
                .is_html_element_named(node, "noscript")
        {
            // Match Blink's HTMLNoScriptElement::LayoutObjectIsNeeded(): the
            // computed `display` remains observable, including author
            // overrides, but no layout object is generated while this exact
            // Document can execute scripts.
            resolved.force_display_none();
        }
        if let Some(profile) = self.profile.as_mut() {
            profile.primary_policy_ns = profile.primary_policy_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        Ok(Some(ResolvedLayoutElementStyles::new(
            resolved, before, after,
        )))
    }

    fn marker_style(
        &mut self,
        node: DomHandle,
    ) -> Result<Option<ResolvedLayoutPseudoStyle>, LayoutError> {
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        let read = self.reads.read(node);
        let computed = read.pseudo_computed_values("marker");
        if let Some(profile) = self.profile.as_mut() {
            profile.marker_count = profile.marker_count.saturating_add(1);
            profile.marker_read_ns = profile.marker_read_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        let origin = self.style_origin(node);
        let resolved = computed.map(|computed| {
            ResolvedLayoutPseudoStyle::new(
                ResolvedLayoutStyle::from_stylo(computed).with_source_origin(origin),
            )
        });
        if let Some(profile) = self.profile.as_mut() {
            profile.marker_projection_ns = profile.marker_projection_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        Ok(resolved)
    }

    fn placeholder_style(
        &mut self,
        node: DomHandle,
    ) -> Result<Option<ResolvedLayoutPseudoStyle>, LayoutError> {
        let origin = self.style_origin(node);
        Ok(self
            .reads
            .read(node)
            .pseudo_computed_values("placeholder")
            .map(|computed| {
                ResolvedLayoutPseudoStyle::new(
                    ResolvedLayoutStyle::from_stylo(computed).with_source_origin(origin),
                )
            }))
    }

    fn anonymous_style(
        &mut self,
        context: LayoutAnonymousStyleContext<'_, DomHandle>,
        display: LayoutDisplay,
    ) -> Result<ResolvedLayoutStyle, LayoutError> {
        let owner = context.owner;
        let parent = context.parent_style;
        let parent_computed = parent.stylo_computed_values().ok_or_else(|| {
            LayoutError::style_resolution(
                format!("{owner:?}"),
                "native anonymous box parent has no retained Stylo ComputedValues",
            )
        })?;
        let anonymous_kind = anonymous_box_kind(display);
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        let prepared = self.anonymous_context.as_ref().ok_or_else(|| {
            LayoutError::style_resolution(
                format!("{owner:?}"),
                "anonymous style pass has no prepared document world",
            )
        })?;
        if self.runtime.dom_host().owner_document_handle(owner) != Some(prepared.document()) {
            return Err(LayoutError::style_resolution(
                format!("{owner:?}"),
                "anonymous owner document does not match the prepared style context",
            ));
        }
        let origin = self.style_origin(owner);
        if parent.source_origin() != Some(origin) {
            return Err(LayoutError::style_resolution(
                format!("{owner:?}"),
                "anonymous parent style does not match its owner and layout pass",
            ));
        }
        let computed = prepared
            .computed_values(owner, parent_computed.as_ref(), anonymous_kind)
            .ok_or_else(|| {
                LayoutError::style_resolution(
                    format!("{owner:?}"),
                    format!("Stylo could not resolve {anonymous_kind:?} anonymous style"),
                )
            })?;
        if let Some(profile) = self.profile.as_mut() {
            profile.anonymous_count = profile.anonymous_count.saturating_add(1);
            profile.anonymous_read_ns = profile.anonymous_read_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        let phase_started = self.profile.as_ref().map(|_| Instant::now());
        let mut resolved = ResolvedLayoutStyle::from_stylo(computed).with_source_origin(origin);
        resolved.force_layout_display(display);
        if let Some(profile) = self.profile.as_mut() {
            profile.anonymous_projection_ns = profile.anonymous_projection_ns.saturating_add(
                phase_started
                    .map(|started| started.elapsed().as_nanos())
                    .unwrap_or_default(),
            );
        }
        Ok(resolved)
    }
}

const fn anonymous_box_kind(display: LayoutDisplay) -> StyloAnonymousBoxKind {
    match display {
        LayoutDisplay::Table | LayoutDisplay::InlineTable => StyloAnonymousBoxKind::Table,
        LayoutDisplay::TableRow => StyloAnonymousBoxKind::TableRow,
        LayoutDisplay::TableCell => StyloAnonymousBoxKind::TableCell,
        LayoutDisplay::None
        | LayoutDisplay::Contents
        | LayoutDisplay::Block
        | LayoutDisplay::FlowRoot
        | LayoutDisplay::Inline
        | LayoutDisplay::InlineBlock
        | LayoutDisplay::Flex
        | LayoutDisplay::InlineFlex
        | LayoutDisplay::Grid
        | LayoutDisplay::InlineGrid
        | LayoutDisplay::BlockListItem
        | LayoutDisplay::InlineListItem
        | LayoutDisplay::TableCaption
        | LayoutDisplay::TableRowGroup
        | LayoutDisplay::TableHeaderGroup
        | LayoutDisplay::TableFooterGroup
        | LayoutDisplay::TableColumnGroup
        | LayoutDisplay::TableColumn => StyloAnonymousBoxKind::Generic,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style_engine::StyloAnonymousBoxKind;
    use moli_layout::LayoutDisplay;

    fn fixture() -> crate::script_vm::StandaloneScriptVmHarness {
        use crate::{
            dom::native::DomHost, parser::HtmlParser, script_vm::ScriptVmDefaultWorldBootstrap,
        };
        let _runtime = crate::JsRuntime::initialize();
        let document = HtmlParser::SCRIPTING_ENABLED.parse(
            url::Url::parse("https://anonymous-style.test/").unwrap(),
            "<!doctype html><style>html,body{margin:0}#owner{font-size:23px;color:red}#other{font-size:41px;color:blue}</style><div id=owner>one</div><div id=other>two</div><iframe id=frame></iframe>".into(),
        );
        let queue = crate::page_task_queue::PageTaskQueueTestHarness::new();
        let mut vm = ScriptVmDefaultWorldBootstrap::standalone_from_dom_host_for_test(
            DomHost::from_dom(document),
            queue.owner_attached_runtime_page_task_sender_for_test(),
            queue.parser_boundary_sender(),
        )
        .unwrap()
        .finish()
        .unwrap();
        vm.install_page_task_residence_for_executor_test(queue.residence());
        vm.set_layout_policy(crate::real_layout_test_policy());
        vm
    }

    fn parent_style(
        resolver: &mut NativeLayoutStyleResolver<'_>,
        owner: DomHandle,
    ) -> ResolvedLayoutStyle {
        ResolvedLayoutStyle::from_stylo(resolver.reads.read(owner).computed_values().unwrap())
            .with_source_origin(resolver.style_origin(owner))
    }

    #[test]
    fn anonymous_styles_reuse_one_prepared_observation() {
        let vm = fixture();
        let host = vm.context_host_weak_for_test().upgrade().unwrap();
        let host = host.borrow();
        let document = host.document_handle();
        let root = host.dom_host().document_element_handle().unwrap();
        let owner = host.dom_host().element_handle_by_id("owner").unwrap();
        let viewport = host.layout_viewport_for_document(document);
        let mut resolver = NativeLayoutStyleResolver::new(&host, root, document, viewport);
        let parent = parent_style(&mut resolver, owner);
        let reads = resolver.reads.read_count_for_test();
        for _ in 0..3 {
            resolver.profile = Some(NativeLayoutStyleResolverProfile::default());
            for _ in 0..10_000 {
                let style = resolver
                    .anonymous_style(
                        LayoutAnonymousStyleContext::new(owner, &parent),
                        LayoutDisplay::Block,
                    )
                    .unwrap();
                assert_eq!(style.display(), LayoutDisplay::Block);
                std::hint::black_box(style);
            }
            let profile = resolver.profile.as_ref().unwrap();
            assert_eq!(profile.anonymous_count, 10_000);
            assert_eq!(
                resolver.reads.read_count_for_test(),
                reads,
                "anonymous boxes must not observe element styles again"
            );
            eprintln!(
                "anonymous_count={} anonymous_read_ns={} anonymous_projection_ns={}",
                profile.anonymous_count, profile.anonymous_read_ns, profile.anonymous_projection_ns
            );
        }
    }

    #[test]
    fn anonymous_styles_reject_mismatched_owners_documents_and_passes() {
        let mut vm = fixture();
        vm.eval("document.getElementById('frame').contentDocument.body.textContent = 'child'")
            .unwrap();
        let host = vm.context_host_weak_for_test().upgrade().unwrap();
        let host = host.borrow();
        let document = host.document_handle();
        let root = host.dom_host().document_element_handle().unwrap();
        let owner = host.dom_host().element_handle_by_id("owner").unwrap();
        let other = host.dom_host().element_handle_by_id("other").unwrap();
        let viewport = host.layout_viewport_for_document(document);
        let mut resolver = NativeLayoutStyleResolver::new(&host, root, document, viewport);
        let parent = parent_style(&mut resolver, owner);
        let other_parent = parent_style(&mut resolver, other);
        for (owner, parent) in [(owner, &other_parent), (other, &parent)] {
            let error = resolver
                .anonymous_style(
                    LayoutAnonymousStyleContext::new(owner, parent),
                    LayoutDisplay::Block,
                )
                .unwrap_err();
            assert!(
                error.to_string().contains("owner and layout pass"),
                "{error}"
            );
        }

        let mut inherited = parent.clone();
        let reads = resolver.reads.read_count_for_test();
        for display in [
            LayoutDisplay::Table,
            LayoutDisplay::TableRow,
            LayoutDisplay::TableCell,
        ] {
            inherited = resolver
                .anonymous_style(LayoutAnonymousStyleContext::new(owner, &inherited), display)
                .unwrap();
            assert_eq!(inherited.font_size(), parent.font_size());
            assert_eq!(inherited.source_origin(), parent.source_origin());
        }
        assert_eq!(resolver.reads.read_count_for_test(), reads);

        let frame = host.dom_host().element_handle_by_id("frame").unwrap();
        let foreign_document = host.child_browsing_context_document_handle(frame).unwrap();
        let foreign_root = host
            .dom_host()
            .dom()
            .document_element_handle_for_document(foreign_document)
            .unwrap();
        let mut foreign =
            NativeLayoutStyleResolver::new(&host, foreign_root, foreign_document, viewport);
        let foreign_parent = parent_style(&mut foreign, foreign_root);
        assert!(
            foreign
                .anonymous_style(
                    LayoutAnonymousStyleContext::new(foreign_root, &foreign_parent),
                    LayoutDisplay::Block
                )
                .is_ok()
        );
        let error = resolver
            .anonymous_style(
                LayoutAnonymousStyleContext::new(foreign_root, &foreign_parent),
                LayoutDisplay::Block,
            )
            .unwrap_err();
        assert!(error.to_string().contains("owner document"), "{error}");
        assert!(
            resolver
                .anonymous_style(
                    LayoutAnonymousStyleContext::new(owner, &foreign_parent),
                    LayoutDisplay::Block
                )
                .is_err()
        );
        assert!(
            resolver
                .anonymous_context
                .as_ref()
                .unwrap()
                .computed_values(
                    foreign_root,
                    parent.stylo_computed_values().unwrap(),
                    StyloAnonymousBoxKind::Generic
                )
                .is_none()
        );

        let mut next = NativeLayoutStyleResolver::new(&host, root, document, viewport);
        assert!(
            next.anonymous_style(
                LayoutAnonymousStyleContext::new(owner, &parent),
                LayoutDisplay::Block
            )
            .is_err()
        );
    }

    #[test]
    fn anonymous_styles_prepare_pending_invalidations_for_each_pass() {
        let mut vm = fixture();
        vm.sync_live_document_style_sources();
        let initial = {
            let host = vm.context_host_weak_for_test().upgrade().unwrap();
            let host = host.borrow();
            let document = host.document_handle();
            let root = host.dom_host().document_element_handle().unwrap();
            let owner = host.dom_host().element_handle_by_id("owner").unwrap();
            let mut resolver = NativeLayoutStyleResolver::new(
                &host,
                root,
                document,
                host.layout_viewport_for_document(document),
            );
            let parent = parent_style(&mut resolver, owner);
            assert_eq!(parent.font_size(), 23.0);
            assert_eq!(
                resolver
                    .anonymous_style(
                        LayoutAnonymousStyleContext::new(owner, &parent),
                        LayoutDisplay::Block
                    )
                    .unwrap()
                    .font_size(),
                23.0
            );
            parent
        };
        for (mutation, expected) in [
            (
                "document.getElementById('owner').style.fontSize = '37px'",
                37.0,
            ),
            (
                "document.querySelector('style').textContent += '#owner {font-size:51px!important}'",
                51.0,
            ),
        ] {
            vm.eval(mutation).unwrap();
            vm.sync_live_document_style_sources();
            let host = vm.context_host_weak_for_test().upgrade().unwrap();
            let host = host.borrow();
            let document = host.document_handle();
            let root = host.dom_host().document_element_handle().unwrap();
            let owner = host.dom_host().element_handle_by_id("owner").unwrap();
            let mut resolver = NativeLayoutStyleResolver::new(
                &host,
                root,
                document,
                host.layout_viewport_for_document(document),
            );
            let parent = parent_style(&mut resolver, owner);
            assert_eq!(parent.font_size(), expected);
            let reads = resolver.reads.read_count_for_test();
            let anonymous = resolver
                .anonymous_style(
                    LayoutAnonymousStyleContext::new(owner, &parent),
                    LayoutDisplay::Block,
                )
                .unwrap();
            assert_eq!(anonymous.font_size(), expected);
            assert_eq!(resolver.reads.read_count_for_test(), reads);
            assert!(
                resolver
                    .anonymous_style(
                        LayoutAnonymousStyleContext::new(owner, &initial),
                        LayoutDisplay::Block
                    )
                    .is_err()
            );
        }
    }

    #[test]
    fn anonymous_table_roles_use_the_matching_servo_precomputed_pseudo() {
        assert_eq!(
            anonymous_box_kind(LayoutDisplay::Table),
            StyloAnonymousBoxKind::Table
        );
        assert_eq!(
            anonymous_box_kind(LayoutDisplay::TableRow),
            StyloAnonymousBoxKind::TableRow
        );
        assert_eq!(
            anonymous_box_kind(LayoutDisplay::TableCell),
            StyloAnonymousBoxKind::TableCell
        );
        assert_eq!(
            anonymous_box_kind(LayoutDisplay::TableRowGroup),
            StyloAnonymousBoxKind::Generic
        );
        assert_eq!(
            anonymous_box_kind(LayoutDisplay::Block),
            StyloAnonymousBoxKind::Generic
        );
    }
}
