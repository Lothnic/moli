use super::*;

#[tokio::test]
async fn worker_performance_handlers_keep_their_listener_position_across_replacement() {
    ensure_v8();
    for kind in [WorkerScriptKind::Classic, WorkerScriptKind::Module] {
        let source = r#"
            const rows = [], target = performance, type = 'resourcetimingbufferfull';
            target.addEventListener(type, () => rows.push('first'));
            target.onresourcetimingbufferfull = () => rows.push('handler-one');
            target.addEventListener(type, () => rows.push('last'));
            const fire = () => target.dispatchEvent(new Event(type));
            fire();
            target.onresourcetimingbufferfull = function(event) {
                rows.push(this === target && event.currentTarget === target ? 'handler-two' : 'bad receiver');
            };
            fire();
            target.onresourcetimingbufferfull = null;
            fire();
            target.onresourcetimingbufferfull = () => rows.push('readded');
            fire();
            postMessage(rows);
            close();
        "#;
        let mut handle = spawn_test_worker_with_options(
            WorkerSpawnOptions::new(
                source.to_owned(),
                "https://performance-handler.test/worker.js".to_owned(),
            )
            .with_script_kind(kind),
        );
        assert_eq!(
            recv_post_json(&mut handle).await,
            r#"["first","handler-one","last","first","handler-two","last","first","last","first","last","readded"]"#
        );
    }
}
