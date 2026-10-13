(async () => {
  const policyError = new Error('policy-callback');
  const policyConversionError = new Error('policy-conversion');
  const streamError = new Error('stream-callback');
  const streamConversionError = new Error('stream-conversion');
  const notificationError = new Error('microtask-notification');
  const notifications = [];
  let resolveNotification;
  const notification = new Promise(resolve => { resolveNotification = resolve; });
  const report = event => {
    notifications.push(event.error === notificationError ? 'microtask' : 'unexpected');
    event.preventDefault();
    if (event.error === notificationError) resolveNotification();
  };
  addEventListener('error', report);
  try {
    const policy = trustedTypes.createPolicy('callback-execution-throw', {
      createHTML() { throw policyError; }
    });
    let policyThrow = false;
    try { policy.createHTML('input'); } catch (error) { policyThrow = error === policyError; }
    const convertingPolicy = trustedTypes.createPolicy('callback-execution-conversion', {
      createHTML() { return { toString() { throw policyConversionError; } }; }
    });
    let policyConversionThrow = false;
    try { convertingPolicy.createHTML('input'); }
    catch (error) { policyConversionThrow = error === policyConversionError; }

    const reader = new ReadableStream({ pull() { throw streamError; } }).getReader();
    reader.closed.catch(() => {});
    const read = reader.read().then(() => false, error => error === streamError);
    const writer = new WritableStream({
      write() { return { get then() { throw streamConversionError; } }; }
    }).getWriter();
    writer.closed.catch(() => {});
    const write = writer.write('chunk').then(() => false, error => error === streamConversionError);

    queueMicrotask(() => { throw notificationError; });
    const [streamThrow, streamConversionThrow] = await Promise.all([read, write, notification]);
    return { policyThrow, policyConversionThrow, streamThrow, streamConversionThrow, notifications };
  } finally {
    removeEventListener('error', report);
  }
})()
