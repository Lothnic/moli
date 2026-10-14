use super::{NativeDom, NativeNodeId, Node, SvgUserTransform};

const SVG_NS: &str = "http://www.w3.org/2000/svg";

impl NativeDom {
    fn is_svg_root(&self, handle: NativeNodeId) -> bool {
        self.node(handle).is_some_and(|node| {
            node.namespace() == Some(SVG_NS) && node.local_name() == Some("svg")
        })
    }

    pub fn svg_root_is_outermost(&self, handle: NativeNodeId) -> bool {
        self.is_svg_root(handle)
            && self
                .parent_node(handle)
                .and_then(|parent| self.node(parent))
                .is_none_or(|parent| {
                    parent.namespace() != Some(SVG_NS)
                        || parent.local_name() == Some("foreignObject")
                })
    }

    pub(super) fn svg_user_transform_before_detach(&mut self, handle: NativeNodeId) {
        if self.is_svg_root(handle)
            && self
                .parent_node(handle)
                .and_then(|parent| self.node(parent))
                .is_some_and(Node::is_document)
        {
            self.node_mut(handle)
                .and_then(|node| node.data_mut().as_element_mut())
                .expect("SVG root has element data")
                .clear_svg_translation();
        }
    }

    pub(super) fn retarget_svg_user_transform(&mut self, handle: NativeNodeId) {
        if !self.is_svg_root(handle) {
            return;
        }
        if !self.svg_root_is_outermost(handle) {
            self.node_mut(handle)
                .and_then(|node| node.data_mut().as_element_mut())
                .expect("SVG root has element data")
                .clear_svg_translation();
            return;
        }
        let document_transform = self
            .parent_node(handle)
            .and_then(|parent| self.node(parent))
            .and_then(Node::as_document)
            .map(|document| document.svg_user_transform());
        if let Some(document_transform) = document_transform {
            let element = self
                .node_mut(handle)
                .and_then(|node| node.data_mut().as_element_mut())
                .expect("SVG root has element data");
            let mut value = element.svg_user_transform();
            value.scale = document_transform.scale;
            value.point[..2].copy_from_slice(&document_transform.point[..2]);
            element.set_svg_user_transform(value);
        }
    }

    pub(super) fn set_svg_root_user_transform(
        &mut self,
        handle: NativeNodeId,
        value: SvgUserTransform,
    ) {
        self.node_mut(handle)
            .and_then(|node| node.data_mut().as_element_mut())
            .expect("validated SVG root has element data")
            .set_svg_user_transform(value);
        if let Some(document) = self
            .parent_node(handle)
            .and_then(|parent| self.node_mut(parent))
            .and_then(|node| node.data_mut().as_document_mut())
        {
            document.set_svg_user_transform(value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::DomHost;

    fn host() -> DomHost {
        DomHost::from_dom(NativeDom::new_html(
            url::Url::parse("https://svg-user-transform.test/").unwrap(),
        ))
    }

    #[test]
    fn document_root_replacement_retains_scale_and_panning_but_new_documents_do_not() {
        let mut host = host();
        let document = host.document_handle();
        let first = host.create_element_ns(Some(SVG_NS), "svg").unwrap();
        assert!(host.append_child(document, first));
        host.set_svg_root_scale(first, 2.0);
        host.set_svg_root_translation(first, [10.0, -5.0, 3.0, 4.0]);
        assert!(host.remove_child(document, first));
        assert_eq!(
            host.svg_root_user_transform(first).point,
            [0.0, 0.0, 3.0, 4.0]
        );
        assert_eq!(host.svg_root_user_transform(first).scale, 2.0);
        let second = host.create_element_ns(Some(SVG_NS), "svg").unwrap();
        assert!(host.append_child(document, second));
        assert_eq!(
            host.svg_root_user_transform(second).point,
            [10.0, -5.0, 0.0, 1.0]
        );
        assert_eq!(host.svg_root_user_transform(second).scale, 2.0);
        let clone = host.clone_node(document, true).unwrap();
        let cloned_root = host.node(clone).unwrap().first_child().unwrap();
        assert_eq!(
            host.svg_root_user_transform(cloned_root),
            SvgUserTransform::default()
        );
        assert_eq!(host.svg_root_user_transform(second).scale, 2.0);
    }

    #[test]
    fn nested_roots_ignore_changes_and_clear_panning_when_their_tree_scope_changes() {
        let mut host = host();
        let outer = host.create_element_ns(Some(SVG_NS), "svg").unwrap();
        let group = host.create_element_ns(Some(SVG_NS), "g").unwrap();
        let inner = host.create_element_ns(Some(SVG_NS), "svg").unwrap();
        host.set_svg_root_scale(inner, 3.0);
        host.set_svg_root_translation(inner, [7.0, 8.0, 9.0, 1.0]);
        assert!(host.append_child(outer, group));
        assert!(host.append_child(group, inner));
        assert!(!host.svg_root_is_outermost(inner));
        assert_eq!(
            host.svg_root_user_transform(inner).point,
            [0.0, 0.0, 9.0, 1.0]
        );
        host.set_svg_root_scale(inner, 5.0);
        host.set_svg_root_translation(inner, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(host.svg_root_user_transform(inner).scale, 3.0);
        assert_eq!(
            host.svg_root_user_transform(inner).point,
            [0.0, 0.0, 9.0, 1.0]
        );
        let foreign = host
            .create_element_ns(Some(SVG_NS), "foreignObject")
            .unwrap();
        assert!(host.append_child(outer, foreign));
        assert!(host.append_child(foreign, inner));
        assert!(host.svg_root_is_outermost(inner));
        host.set_svg_root_translation(inner, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(
            host.svg_root_user_transform(inner).point,
            [1.0, 2.0, 3.0, 4.0]
        );
    }

    #[test]
    fn svg_user_transform_defaults_preserve_signed_zero() {
        let mut host = host();
        let root = host.create_element_ns(Some(SVG_NS), "svg").unwrap();
        host.set_svg_root_scale(root, -0.0);
        host.set_svg_root_translation(root, [-0.0, 0.0, 0.0, 1.0]);
        let value = host.svg_root_user_transform(root);
        assert_eq!(value.scale.to_bits(), (-0.0_f32).to_bits());
        assert_eq!(value.point[0].to_bits(), (-0.0_f64).to_bits());
        host.set_svg_root_scale(root, 1.0);
        host.set_svg_root_translation(root, [0.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            host.svg_root_user_transform(root),
            SvgUserTransform::default()
        );
    }
}
