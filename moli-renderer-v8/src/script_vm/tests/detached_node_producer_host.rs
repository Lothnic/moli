use super::*;

#[test]
fn borrowed_node_accessors_resolve_windowless_nodes_in_the_producing_host() {
    let mut vm = new_storage_page_task_executor_test_vm("https://detached-producer-host.test/");
    let result = vm.eval(r#"(() => {
      document.body.innerHTML = '<iframe></iframe>';
      const other = document.querySelector('iframe').contentWindow;
      const docs = [other.document.implementation.createHTMLDocument(''),
        other.document.implementation.createDocument('urn:child', 'root')];
      for (const doc of docs) {
        const element = doc.createElementNS('urn:child', 'owned');
        element.setAttribute('data-owner', 'child');
        for (let i = 0; i < 40; ++i) document.createElement('unrelated');
        const getName = Object.getOwnPropertyDescriptor(Node.prototype, 'nodeName').get;
        if (getName.call(element) !== 'owned') throw Error('wrong native node');
        if (Element.prototype.getAttribute.call(element, 'data-owner') !== 'child') throw Error('wrong owner arena');
        Element.prototype.setAttribute.call(element, 'data-owner', 'borrowed');
        if (element.getAttribute('data-owner') !== 'borrowed') throw Error('wrong mutation owner');
      }
      return true;
    })()"#).unwrap();
    assert_eq!(result, "true");
}
