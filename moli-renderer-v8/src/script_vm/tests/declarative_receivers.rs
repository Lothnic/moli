use super::*;

fn receiver_test_vm() -> crate::runtime::PageVmTaskExecutorTestHarness {
    let mut vm = new_storage_page_task_executor_test_vm("https://declarative-receivers.test/");
    vm.eval("document.body.innerHTML = '<iframe></iframe>'")
        .unwrap();
    vm.eval(include_str!("declarative_receivers.js")).unwrap();
    vm
}

fn finish_receiver_checks(vm: &mut crate::runtime::PageVmTaskExecutorTestHarness) {
    vm.eval("receiverChecks.finish()").unwrap();
    vm.eval("0").unwrap();
    assert_eq!(vm.eval("receiverChecks.complete").unwrap(), "true");
    assert_eq!(
        vm.eval("JSON.stringify(receiverChecks.failures)").unwrap(),
        "[]"
    );
}

#[test]
fn webrtc_declarations_validate_foreign_receivers_before_converting_arguments() {
    let mut vm = receiver_test_vm();
    vm.eval(r#"(() => {
      const {child, assert, check, getters} = receiverChecks;
      for (const realm of [window, child]) {
        const other = realm === window ? child : window;
        const pc = new other.RTCPeerConnection();
        const channel = pc.createDataChannel('foreign');
        getters(realm, 'RTCPeerConnection', pc, ['signalingState', 'iceGatheringState', 'iceConnectionState',
          'connectionState', 'localDescription', 'currentLocalDescription', 'pendingLocalDescription']);
        for (const name of ['getConfiguration', 'setConfiguration', 'createDataChannel', 'createOffer', 'setLocalDescription', 'close']) {
          check(realm, pc, realm.RTCPeerConnection.prototype[name]);
        }
        assert(realm.RTCPeerConnection.prototype.createDataChannel.call(pc, 'borrowed').label === 'borrowed', 'foreign peer method');
        getters(realm, 'RTCDataChannel', channel, ['label', 'ordered', 'maxPacketLifeTime', 'maxRetransmits',
          'protocol', 'negotiated', 'id', 'readyState', 'bufferedAmount', 'binaryType']);
        for (const name of ['send', 'close']) check(realm, channel, realm.RTCDataChannel.prototype[name]);
        const candidate = new other.RTCIceCandidate({candidate: 'candidate:1 1 UDP 1 127.0.0.1 9 typ host', sdpMid: '0'});
        getters(realm, 'RTCIceCandidate', candidate, ['candidate', 'sdpMid', 'sdpMLineIndex', 'foundation',
          'component', 'priority', 'address', 'protocol', 'port', 'type', 'tcpType', 'relatedAddress',
          'relatedPort', 'usernameFragment', 'relayProtocol', 'url']);
        check(realm, candidate, realm.RTCIceCandidate.prototype.toJSON);
        assert(realm.RTCIceCandidate.prototype.toJSON.call(candidate).candidate === candidate.candidate, 'foreign ICE method');
        const description = new other.RTCSessionDescription({type: 'offer', sdp: 'foreign'});
        getters(realm, 'RTCSessionDescription', description, ['type', 'sdp']);
        check(realm, description, realm.RTCSessionDescription.prototype.toJSON);
        assert(realm.RTCSessionDescription.prototype.toJSON.call(description).sdp === 'foreign', 'foreign description method');
        realm.RTCPeerConnection.prototype.close.call(pc);
      }
    })()"#).unwrap();
    finish_receiver_checks(&mut vm);
}

#[test]
fn navigator_declarations_preserve_receiver_checks_and_promise_identity_across_realms() {
    let mut vm = receiver_test_vm();
    vm.eval(r#"(async () => {
      const {child, assert, check, getters} = receiverChecks;
      for (const realm of [window, child]) {
        const other = realm === window ? child : window;
        const nav = other.navigator;
        for (const name of ['getGamepads', 'javaEnabled', 'sendBeacon', 'vibrate', 'getAutoplayPolicy']) {
          check(realm, nav, realm.Navigator.prototype[name]);
        }
        check(realm, nav, realm.Navigator.prototype.getBattery, true);
        getters(realm, 'Navigator', nav, ['cookieEnabled']);
        assert(realm.Navigator.prototype.javaEnabled.call(nav) === false, 'foreign Navigator method');
        const localConnection = realm.navigator.connection;
        const saveData = Object.getOwnPropertyDescriptor(localConnection, 'saveData').get;
        check(realm, nav.connection, saveData);
        assert(saveData.call(nav.connection) === nav.connection.saveData, 'foreign connection getter');
        for (const name of ['addEventListener', 'removeEventListener']) check(realm, nav.connection, localConnection[name]);
        const battery = await nav.getBattery();
        const localBattery = await realm.navigator.getBattery();
        for (const name of ['addEventListener', 'removeEventListener', 'dispatchEvent']) check(realm, battery, localBattery[name]);
        check(realm, nav.permissions, realm.Permissions.prototype.query, true);
        check(realm, nav.userAgentData, realm.NavigatorUAData.prototype.toJSON);
        check(realm, nav.userAgentData, realm.NavigatorUAData.prototype.getHighEntropyValues, true);
        assert(realm.NavigatorUAData.prototype.toJSON.call(nav.userAgentData).platform === nav.userAgentData.platform, 'foreign UAData method');
        for (const name of ['getCurrentPosition', 'watchPosition', 'clearWatch']) check(realm, nav.geolocation, realm.Geolocation.prototype[name]);
        realm.Geolocation.prototype.clearWatch.call(nav.geolocation, 0);
        for (const name of ['decodingInfo', 'encodingInfo']) check(realm, nav.mediaCapabilities, realm.MediaCapabilities.prototype[name], true);
        for (const name of ['read', 'readText', 'write', 'writeText']) check(realm, nav.clipboard, realm.Clipboard.prototype[name], true);
        check(realm, nav.mediaDevices, realm.MediaDevices.prototype.getUserMedia, true);
        const item = new other.ClipboardItem({'text/plain': new other.Blob(['foreign'])});
        getters(realm, 'ClipboardItem', item, ['types', 'presentationStyle']);
        check(realm, item, realm.ClipboardItem.prototype.getType, true);
        const originalPromise = other.ClipboardItem.prototype.getType.call(item, 'text/plain');
        assert(realm.ClipboardItem.prototype.getType.call(item, 'text/plain') === originalPromise, 'getType preserves stored Promise identity');
        assert(realm.ClipboardItem.supports.call({}, 'text/plain'), 'static method has no receiver guard');
        const bucket = await nav.storageBuckets.open('receiver-bucket');
        getters(realm, 'StorageBucket', bucket, ['name', 'caches']);
        const factoryGetter = Object.getOwnPropertyDescriptor(realm.StorageBucket.prototype, 'indexedDB').get;
        check(realm, bucket, factoryGetter);
        assert(typeof factoryGetter.call(bucket).open === 'function', 'foreign bucket factory getter');
      }
      return 'ok';
    })().then(result => {globalThis.receiverSetup = result;}, error => {globalThis.receiverSetup = String(error);})"#).unwrap();
    assert_eq!(vm.eval("receiverSetup").unwrap(), "ok");
    finish_receiver_checks(&mut vm);
}
