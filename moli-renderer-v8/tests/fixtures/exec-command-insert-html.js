// Expectations verified in Chromium 145.0.7632.116. This expression can also
// run directly in a browser to compare its editing behavior with Moli.
(() => {
  const root = document.documentElement || document.appendChild(document.createElement('html'));
  const body = document.body || root.appendChild(document.createElement('body'));
  const failures = [];
  const cases = [
    ['blocks', '<div>A</div><div>B</div>', 'AB', 'A\nB'],
    ['paragraphs', '<p>A</p><p>B</p>', 'AB', 'A\n\nB'],
    ['nested-blocks', '<div>A<div>B</div>C</div>D', 'ABCD', 'A\nB\nC\nD'],
    ['empty-block', 'A<div></div>B', 'AB', 'A\nB'],
    ['leading-empty-block', '<div></div><div>B</div>', 'B', 'B'],
    ['trailing-empty-block', '<div>A</div><div></div>', 'A', 'A'],
    ['br', 'A<br>B', 'A B', 'A\nB'],
    ['leading-br', '<br>A', ' A', '\nA'],
    ['trailing-br', 'A<br>', 'A', 'A'],
    ['two-brs', 'A<br><br>B', 'A  B', 'A\n\nB'],
    ['two-trailing-brs', 'A<br><br>', 'A', 'A\n'],
    ['block-br', '<div>A<br></div><div>B</div>', 'A B', 'A\nB'],
    ['empty-line', 'A<div><br></div>B', 'A B', 'A\n\nB'],
    ['hidden', 'A<span hidden>H</span>B', 'AHB', 'AB'],
    ['hidden-subtree', 'A<div hidden><div>H</div><br>X</div>B', 'AH XB', 'AB'],
    ['display-none', 'A<span style="display:none">H</span>B', 'AHB', 'AB'],
    ['hidden-br', 'A<br hidden>B', 'A B', 'AB'],
    ['visibility-hidden', 'A<span style="visibility:hidden">H</span>B', 'AHB', 'AHB'],
    ['script', 'A<script>globalThis.__insertHtmlScriptRan = true;</script>B',
      'AglobalThis.__insertHtmlScriptRan = true;B', 'AB'],
    ['style', 'A<style>p{color:red}</style>B', 'Ap{color:red}B', 'AB'],
    ['inline', '<b>A</b><i>B</i>&amp;', 'AB&', 'AB&'],
    ['whitespace', '  A\n B\t  C  ', '  A  B\t  C  ', '  A\n B\t  C  '],
    ['nested-whitespace', '<span>  A\n B\t  C  </span>', '  A  B\t  C  ', '  A\n B\t  C  '],
    ['pre', '<pre>A\n B</pre>', 'A  B', 'A\n B'],
    ['inline-block-tags', '<div style="display:inline">A</div><div style="display:inline">B</div>', 'AB', 'AB'],
    ['block-inline-tags', '<span style="display:block">A</span><span style="display:block">B</span>', 'AB', 'A\nB'],
    ['hidden-override', 'A<span hidden style="display:inline">H</span>B', 'AHB', 'AHB'],
    ['rendered-script', 'A<script style="display:inline">H</script>B', 'AHB', 'AHB'],
    ['fragment-stylesheet', '<style>span{display:none}</style>A<span>H</span>B', 'span{display:none}AHB', 'AB'],
    ['table', '<table><tr><td>A</td><td>B</td></tr><tr><td>C</td><td>D</td></tr></table>', 'ABCD', 'A\tB\nC\tD'],
    ['empty', '', '', ''],
    ['only-hidden', '<span hidden>H</span>', 'H', ''],
    ['only-br', '<br>', '', ''],
  ];

  function probe(name, tag, cancel, markup, initial, selection, maxlength, expectedValue, caret, inserted) {
    const control = body.appendChild(document.createElement(tag));
    control.value = initial;
    if (maxlength !== null) control.maxLength = maxlength;
    control.focus();
    control.setSelectionRange(selection[0], selection[1]);
    const events = [];
    for (const type of ['beforeinput', 'input']) {
      control.addEventListener(type, event => {
        events.push([event.type, event.inputType, event.data, event.bubbles, event.cancelable, event.composed]);
        if (cancel && type === 'beforeinput') event.preventDefault();
      });
    }
    const returned = document.execCommand('insertHTML', false, markup);
    const actual = [returned, control.value, control.selectionStart, control.selectionEnd, events];
    const emitsInput = tag === 'textarea' || expectedValue !== initial || caret !== selection[0] || caret !== selection[1];
    const expectedEvents = emitsInput
      ? [['input', tag === 'input' ? 'insertText' : '', tag === 'input' ? inserted : null, true, false, true]]
      : [];
    const expected = [true, expectedValue, caret, caret, expectedEvents];
    if (JSON.stringify(actual) !== JSON.stringify(expected)) {
      failures.push({ name, tag, cancel, expected, actual });
    }
    control.remove();
  }

  for (const tag of ['input', 'textarea']) {
    for (const cancel of [false, true]) {
      for (const [name, markup, inputValue, textareaValue] of cases) {
        const value = tag === 'input' ? inputValue : textareaValue;
        probe(name, tag, cancel, markup, '', [0, 0], null, value, value.length, value);
      }
      const inserted = tag === 'input' ? '😀X' : '😀\n';
      probe('utf16-selection-maxlength', tag, cancel, '<div>😀</div><div>XY</div>',
        'L😀R', [1, 3], 5, 'L' + inserted + 'R', 4, inserted);
      probe('maxlength-surrogate-boundary', tag, cancel, '<b>😀</b>',
        'L😀R', [1, 3], 3, 'LR', 1, '');
      probe('maxlength-noop', tag, cancel, '<b>😀</b>',
        'LR', [1, 1], 3, 'LR', 1, '');
      probe('empty-at-caret', tag, cancel, '', 'XY', [1, 1], null, 'XY', 1, '');
      probe('replace-same-value', tag, cancel, '<b>X</b>', 'X', [0, 1], null, 'X', 1, 'X');
    }
  }
  if (globalThis.__insertHtmlScriptRan !== undefined) {
    failures.push('InsertHTML must not execute script elements');
  }
  return JSON.stringify(failures);
})()
