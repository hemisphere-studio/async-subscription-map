use std::time::Duration;

use async_std::task;
use async_subscription_map::SubscriptionMap;
use simple_logger::SimpleLogger;

type Id = &'static str;
type State = u8;

#[async_std::main]
async fn main() -> anyhow::Result<()> {
    SimpleLogger::new()
        .with_level(log::LevelFilter::Info)
        .init()
        .unwrap();

    let map = SubscriptionMap::<Id, State>::default();

    // Spawn a task that observes some map entry
    task::spawn(reader(map.clone(), "example"));

    // Write to the entry periodically for example
    writer(map, "example").await
}

async fn writer(map: SubscriptionMap<Id, State>, observed_id: Id) -> anyhow::Result<()> {
    let mut entry = map.get_or_insert(observed_id, 0).await;
    let mut state = 0;

    loop {
        log::info!("writer: state change: {}", state);
        entry.publish_if_changed(state);
        state = state.wrapping_add(1);
        task::sleep(Duration::from_millis(10)).await;
    }
}

async fn reader(map: SubscriptionMap<Id, State>, observed_id: Id) -> anyhow::Result<()> {
    let mut entry = map.get_or_insert(observed_id, 0).await;

    loop {
        let update = entry.next().await;
        log::info!("reader: state change: {}", update);
    }
}
