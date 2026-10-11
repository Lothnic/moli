use moli_webidl::{Context, WebIdlDictionary};

#[derive(WebIdlDictionary)]
#[webidl(prefix = "LongLongInit")]
struct Init {
    #[webidl(default = 0)]
    value: i64,
    #[webidl(converter = "long_long", default = 0)]
    explicit: i64,
}

#[test]
fn long_long_derive_wraps_and_preserves_conversion_errors() {
    moli_v8_test_util::ensure_v8();
    let mut isolate = v8::Isolate::new(v8::CreateParams::default());
    v8::scope!(let scope, &mut isolate);
    let context = v8::Context::new(scope, Default::default());
    let scope = &mut v8::ContextScope::new(scope, context);
    for (expression, expected) in [
        ("-17.9", -17),
        ("NaN", 0),
        ("Infinity", 0),
        ("undefined", 0),
        ("2**63", i64::MIN),
        ("2**64", 0),
        ("-(2**63)-2048", i64::MAX - 2047),
    ] {
        let source = v8::String::new(
            scope,
            &format!("({{value:{expression},explicit:{expression}}})"),
        )
        .unwrap();
        let value = v8::Script::compile(scope, source, None)
            .unwrap()
            .run(scope)
            .unwrap();
        let init =
            moli_webidl::parse_dictionary::<Init>(scope, value, Context::argument("Test", 1))
                .unwrap()
                .unwrap();
        assert_eq!(init.value, expected, "{expression}");
        assert_eq!(init.explicit, expected, "{expression}");
    }
    for source in [
        "({value:Symbol(),explicit:0})",
        "({value:0,explicit:1n})",
        "({get explicit(){throw 42},value:0})",
    ] {
        v8::tc_scope!(let scope, scope);
        let source = v8::String::new(scope, source).unwrap();
        let value = v8::Script::compile(scope, source, None)
            .unwrap()
            .run(scope)
            .unwrap();
        assert!(
            moli_webidl::parse_dictionary::<Init>(scope, value, Context::argument("Test", 1))
                .is_err()
        );
    }
}
