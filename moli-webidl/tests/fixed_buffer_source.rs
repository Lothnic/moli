use moli_webidl::{Context, NonSharedBufferSource, convert};

fn eval<'s>(scope: &mut v8::PinScope<'s, '_>, source: &str) -> v8::Local<'s, v8::Value> {
    let source = v8::String::new(scope, source).unwrap();
    v8::Script::compile(scope, source, None)
        .unwrap()
        .run(scope)
        .unwrap()
}

#[test]
fn fixed_buffer_sources_use_native_ranges_and_retain_live_identity() {
    moli_v8_test_util::ensure_v8();
    let mut isolate = v8::Isolate::new(Default::default());
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    for (source, expected) in [
        ("new Uint8Array([7,1,2,9]).buffer", vec![7, 1, 2, 9]),
        ("new Uint8Array([7,1,2,9]).subarray(1,3)", vec![1, 2]),
        (
            "new DataView(new Uint8Array([7,1,2,9]).buffer,1,2)",
            vec![1, 2],
        ),
        (
            "Object.setPrototypeOf(new Uint8Array([1,2]),null)",
            vec![1, 2],
        ),
        (
            "(() => {const view = new DataView(new Uint8Array([7,1,2,9]).buffer,1,2); for (const key of ['buffer','byteOffset','byteLength']) Object.defineProperty(view,key,{get(){throw Error('author getter')}}); return view;})()",
            vec![1, 2],
        ),
    ] {
        let value = eval(scope, source);
        let native =
            convert::<NonSharedBufferSource>(scope, value, Context::argument("Buffer", 1)).unwrap();
        assert_eq!(native.to_vec(scope), expected, "{source}");
    }
    let value = eval(
        scope,
        "globalThis.bytes = new Uint8Array([1,2,3]); bytes.subarray(1)",
    );
    let native =
        convert::<NonSharedBufferSource>(scope, value, Context::argument("Buffer", 1)).unwrap();
    eval(scope, "bytes[1] = 8");
    assert_eq!(native.to_vec(scope), vec![8, 3]);
}

#[test]
fn fixed_buffer_sources_reject_shared_resizable_and_author_proxies_without_traps() {
    moli_v8_test_util::ensure_v8();
    let mut isolate = v8::Isolate::new(Default::default());
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    eval(scope, "globalThis.bufferTraps = 0");
    for source in [
        "null",
        "undefined",
        "({})",
        "([])",
        "1",
        "'buffer'",
        "Symbol()",
        "new SharedArrayBuffer(0)",
        "new SharedArrayBuffer(2)",
        "new Uint8Array(new SharedArrayBuffer(0))",
        "new DataView(new SharedArrayBuffer(2))",
        "new ArrayBuffer(2,{maxByteLength:4})",
        "new Uint8Array(new ArrayBuffer(2,{maxByteLength:4}))",
        "new DataView(new ArrayBuffer(2,{maxByteLength:4}))",
        "Object.create(new Uint8Array(2))",
        "new Proxy(new Uint8Array(2),{get(){bufferTraps++;throw Error('proxy trap')}})",
        "(() => {const pair = Proxy.revocable(new ArrayBuffer(2),{}); pair.revoke(); return pair.proxy;})()",
    ] {
        let value = eval(scope, source);
        assert!(
            convert::<NonSharedBufferSource>(scope, value, Context::argument("Buffer", 1)).is_err(),
            "{source}"
        );
    }
    assert_eq!(eval(scope, "bufferTraps").int32_value(scope), Some(0));
}

#[test]
fn fixed_buffer_detachment_preserves_native_validation_and_empty_byte_reads() {
    moli_v8_test_util::ensure_v8();
    let mut isolate = v8::Isolate::new(Default::default());
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    for resizable in [false, true] {
        let value = eval(
            scope,
            if resizable {
                "new ArrayBuffer(2,{maxByteLength:4})"
            } else {
                "new ArrayBuffer(2)"
            },
        );
        let buffer = v8::Local::<v8::ArrayBuffer>::try_from(value).unwrap();
        let view = v8::Uint8Array::new(scope, buffer, 0, 2).unwrap();
        let retained =
            convert::<NonSharedBufferSource>(scope, view.into(), Context::argument("Buffer", 1));
        assert_eq!(retained.is_err(), resizable);
        assert_eq!(buffer.detach(None), Some(true));
        if let Ok(retained) = retained {
            assert!(retained.to_vec(scope).is_empty());
        }
        for value in [buffer.into(), view.into()] {
            let result =
                convert::<NonSharedBufferSource>(scope, value, Context::argument("Buffer", 1));
            if resizable {
                assert!(result.is_err());
            } else {
                assert!(result.unwrap().to_vec(scope).is_empty());
            }
        }
    }
}
