// Shared by the Rust lifecycle regressions and the Chromium differential runner.
(() => {
  const root = document.documentElement || document.appendChild(document.createElement('html'));
  if (!document.body) root.appendChild(document.createElement('body'));
  let events;
  let resolveDone;
  let remainingLoads;
  let frame;
  const parserDocument = () => new DOMParser().parseFromString(
    '<html><body><iframe></iframe></body></html>', 'text/html');
  const foreignFrame = () => parserDocument().querySelector('iframe');
  const watch = (target, owner, label = 'frame', afterLoad) => {
    target.onload = function(event) {
      if (this !== target || event.target !== target || event.currentTarget !== target
          || this.ownerDocument !== owner || this.contentDocument.readyState !== 'complete') {
        events.push('invalid-handler-target-or-document');
      }
      events.push(`${label}:handler`);
      if (afterLoad) afterLoad();
    };
    target.addEventListener('load', function(event) {
      events.push(this === target && event.target === target && event.currentTarget === target
        ? `${label}:listener` : 'invalid-listener-target');
      if (--remainingLoads === 0) resolveDone();
    });
  };
  const insert = (parent, target, method) => {
    if (method === 'append') parent.appendChild(target);
    else if (method === 'insert') parent.insertBefore(target, null);
    else if (method === 'replace') {
      const previous = parent.appendChild(parent.ownerDocument.createElement('p'));
      parent.replaceChild(target, previous);
    } else throw new Error(`Unknown insertion method: ${method}`);
  };
  globalThis.iframeLoadDelivery = {
    done: null,
    start(name) {
      events = [];
      this.done = new Promise(resolve => { resolveDone = resolve; });
      remainingLoads = name.startsWith('inert-') ? 0
        : (name === 'reinsert' || name === 'navigate-in-onload' ? 2 : 1);
      events.push('before');
      if (name.startsWith('inert-parser-') || name.startsWith('inert-html-')) {
        const [, kind, method] = name.split('-');
        const doc = kind === 'parser' ? parserDocument()
          : document.implementation.createHTMLDocument('inert');
        frame = doc.createElement('iframe');
        watch(frame, doc);
        insert(doc.body, frame, method);
      } else if (name === 'inert-adoption') {
        const doc = document.implementation.createHTMLDocument('target');
        frame = foreignFrame();
        watch(frame, doc);
        doc.body.appendChild(frame);
      } else if (name.startsWith('disconnected-')) {
        const parent = document.createElement('div');
        frame = name.endsWith('foreign') ? foreignFrame() : document.createElement('iframe');
        watch(frame, document);
        parent.appendChild(frame);
        events.push('staged');
        document.body.appendChild(parent);
      } else if (name.startsWith('active-parser-')) {
        frame = foreignFrame();
        watch(frame, document);
        insert(document.body, frame, name.split('-')[2]);
      } else if (name === 'active-fragment') {
        const doc = parserDocument();
        const fragment = doc.createDocumentFragment();
        const parent = doc.createElement('div');
        frame = doc.querySelector('iframe');
        watch(frame, document);
        parent.appendChild(frame);
        fragment.appendChild(parent);
        events.push('staged');
        document.body.appendChild(fragment);
      } else if (name === 'active-cross-window') {
        const host = document.body.appendChild(document.createElement('iframe'));
        frame = host.contentDocument.createElement('iframe');
        watch(frame, document);
        document.body.appendChild(frame);
      } else if (name === 'reinsert') {
        frame = foreignFrame();
        watch(frame, document);
        document.body.appendChild(frame);
        const previousDocument = frame.contentDocument;
        frame.remove();
        events.push('removed');
        document.body.appendChild(frame);
        if (frame.contentDocument === previousDocument) events.push('reused-detached-document');
      } else if (name === 'remove-in-onload') {
        frame = foreignFrame();
        watch(frame, document, 'frame', () => frame.remove());
        document.body.appendChild(frame);
      } else if (name === 'srcdoc' || name === 'pending-replacement') {
        frame = foreignFrame();
        frame.srcdoc = '<body>original</body>';
        watch(frame, document);
        document.body.appendChild(frame);
        if (name === 'pending-replacement') this.replace();
      } else if (name === 'navigate-in-onload') {
        frame = foreignFrame();
        let first = true;
        watch(frame, document, 'frame', () => {
          if (first) {
            first = false;
            frame.srcdoc = '<body>replacement</body>';
          }
        });
        document.body.appendChild(frame);
      } else if (name === 'url-navigation') {
        frame = foreignFrame();
        document.body.appendChild(frame);
        watch(frame, document);
        frame.src = 'about:blank?navigation';
      } else throw new Error(`Unknown case: ${name}`);
      events.push('after');
      if (remainingLoads === 0) resolveDone();
      return this.snapshot();
    },
    replace() { frame.srcdoc = '<body>replacement</body>'; },
    snapshot() { return events.slice(); },
    bodyText() { return frame.contentDocument.body.textContent; }
  };
})();
