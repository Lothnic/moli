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
