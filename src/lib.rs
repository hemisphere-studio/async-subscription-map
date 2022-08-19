//! A subscription map is a self cleaning map of `Observable`s which tracks
//! tasks that want to subscribe to state updates on a certain key. This
//! becomes very useful if you have multiple tasks in your program and you want
//! to wait in one task until the other starts publishing state updates.
//!
//! It enables you to generically  communicate through your whole program by
//! just knowing an identifier, no need to pass observables around - they are
//! created on the fly and only if someone subcribes to them. This is ideal for
//! highly asynchronous and performance critical backend implementations which
//! process and serve data accross multiple protocols and want to cut down
//! latency through communicating in memory.
//!
//! <div>
//! <br/>
//! <img
//!     style="margin: 0 auto; display: block;"
//!     src="../../../docs/diagram.png"
//!     height="300"
//!     width="720"
//! />
//! <br/>
//! </div>
//!
//! ## Self Cleaning Nature
//!
//! The subscription map is selfcleaing in the sense that it removes every
//! subscription entry and its data as soon as no one subscribes to it and thus
//! actively preventing memory leaks!
use anyhow::Context;
use async_observable::Observable;
use async_std::sync::Mutex;
use async_std::task::block_on;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A concurrent and self cleaning map of observable values to easily
/// communicate dynamically across tasks.
///
/// ```
/// # use async_subscription_map::SubscriptionMap;
/// # use async_std::task;
/// # async {
/// let map = SubscriptionMap::<usize, usize>::default();
/// let mut subscription = map.get_or_insert(1, 0).await;
///
/// task::spawn(async move {
///     // somewhere else in your program
///     let mut subscription = map.get_or_insert(1, 0).await;
///     log::info!("received update throguh map: {}", subscription.next().await);
/// });
///
/// // wait for some event and publish the state
/// subscription.publish(1);
/// // just drop the ref as soon as you are done with it to trigger the cleanup
/// drop(subscription);
/// # };
/// ```
#[derive(Clone, Debug)]
pub struct SubscriptionMap<K, V>(Arc<Mutex<BTreeMap<K, SubscriptionEntry<V>>>>)
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug;

/// A single observable entry and its subscription count
#[derive(Clone, Debug)]
struct SubscriptionEntry<V>
where
    V: Clone + Debug,
{
    observable: Observable<V>,
    rc: usize,
}

impl<V> SubscriptionEntry<V>
where
    V: Clone + Debug,
{
    pub fn new(value: V) -> Self {
        Self {
            observable: Observable::new(value),
            rc: 0,
        }
    }
}

impl<K, V> SubscriptionMap<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug,
{
    /// Create an empty SubscriptionMap
    pub fn new() -> Self {
        Self(Arc::new(Mutex::new(BTreeMap::new())))
    }

    /// Either creates a ref to a existing subscription or initializes a new one.
    pub async fn get_or_insert(&self, key: K, value: V) -> SubscriptionRef<K, V> {
        let mut map = self.0.lock().await;
        let entry = {
            let entry = SubscriptionEntry::new(value);
            map.entry(key.clone()).or_insert(entry)
        };

        SubscriptionRef::new(key, self.clone(), entry)
    }

    #[cfg(test)]
    async fn snapshot(&self) -> BTreeMap<K, SubscriptionEntry<V>> {
        self.0.lock().await.deref().clone()
    }

    async fn remove(&self, key: &K) -> anyhow::Result<()> {
        let mut map = self.0.lock().await;

        let entry = map
            .get(key)
            .with_context(|| format!("unable remove not present key {:?} in {:#?}", key, self))?;

        assert!(
            entry.rc == 0,
            "invalid removal of referenced subscription at {:?}",
            key
        );

        map.remove(key);

        Ok(())
    }
}

impl<K, V> SubscriptionMap<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug + Eq,
{
    /// Check if the provided value differs from the observable and return the info if a publish
    /// was made.
    ///
    /// ```
    /// # use async_subscription_map::SubscriptionMap;
    /// # async {
    /// let map = SubscriptionMap::<usize, usize>::default();
    /// let mut subscription = map.get_or_insert(1, 0).await;
    ///
    /// assert_eq!(subscription.latest(), 0);
    /// map.publish_if_changed(&1, 1);
    /// assert_eq!(subscription.next().await, 1);
    /// map.publish_if_changed(&1, 1);
    ///
    /// // this will never resolve since we did not publish an update!
    /// subscription.next().await
    /// # };
    /// ```
    pub async fn publish_if_changed(&self, key: &K, value: V) -> anyhow::Result<bool> {
        let mut map = self.0.lock().await;
        let entry = map
            .get_mut(key)
            .with_context(|| format!("unable publish new version of not present key {:?}", key))?;

        Ok(entry.observable.publish_if_changed(value))
    }

    /// Modify the value contained in the subscription through a mutable reference and notify
    /// others.
    ///
    ///
    /// This is handy for expensive data structures such as vectors, trees or maps.
    ///
    /// ```
    /// # use async_subscription_map::SubscriptionMap;
    /// # async {
    /// let map = SubscriptionMap::<usize, usize>::default();
    /// let mut subscription = map.get_or_insert(1, 0).await;
    ///
    /// assert_eq!(subscription.latest(), 0);
    /// map.modify_and_publish(&1, |mut v| *v = 1);
    /// assert_eq!(subscription.latest(), 1);
    /// # };
    /// ```
    pub async fn modify_and_publish<F, R>(&self, key: &K, modify: F) -> anyhow::Result<()>
    where
        F: FnOnce(&mut V) -> R,
    {
        let mut map = self.0.lock().await;
        let entry = map
            .get_mut(key)
            .with_context(|| format!("unable modify not present key {:?}", key))?;

        entry.observable.modify(|v| {
            modify(v);
        });

        Ok(())
    }
}

impl<K, V> Default for SubscriptionMap<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug,
{
    fn default() -> Self {
        Self::new()
    }
}

/// A transparent wrapper for the underlying subscription in the map
/// which manages the subscription count and removes the observable if no one
/// holds a subscription to it.
#[derive(Debug)]
#[must_use = "entries are removed as soon as no one subscribes to them"]
pub struct SubscriptionRef<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug,
{
    key: K,
    owner: SubscriptionMap<K, V>,
    observable: Observable<V>,
}

impl<K, V> SubscriptionRef<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug,
{
    fn new(key: K, owner: SubscriptionMap<K, V>, entry: &mut SubscriptionEntry<V>) -> Self {
        entry.rc += 1;

        Self {
            key,
            owner,
            observable: entry.observable.clone(),
        }
    }
}

impl<K, V> Deref for SubscriptionRef<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug,
{
    type Target = Observable<V>;

    fn deref(&self) -> &Self::Target {
        &self.observable
    }
}

impl<K, V> DerefMut for SubscriptionRef<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.observable
    }
}

impl<K, V> Drop for SubscriptionRef<K, V>
where
    K: Clone + Debug + Eq + Hash + Ord,
    V: Clone + Debug,
{
    fn drop(&mut self) {
        log::trace!("drop for subscription ref for key {:?}", self.key);

        let mut map = block_on(self.owner.0.lock());
        let mut entry = match map.get_mut(&self.key) {
            Some(entry) => entry,
            None => {
                log::error!("could not obtain rc in subscription map {:#?}", map.deref());
                return;
            }
        };

        entry.rc -= 1;

        if entry.rc == 0 {
            drop(map);
            let res = block_on(self.owner.remove(&self.key));

            if let Err(e) = res {
                log::error!("error occurred while cleanup subscription ref {}", e);
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::SubscriptionMap;

    macro_rules! assert_map_len {
        ($map:ident, $len:expr) => {
            assert_eq!($map.snapshot().await.len(), $len);
        };
    }

    macro_rules! assert_ref_count {
        ($map:ident, $key:expr, $rc:expr) => {
            assert_eq!($map.snapshot().await.get($key).unwrap().rc, $rc);
        };
    }

    #[async_std::test]
    async fn should_immediately_remove_unused() {
        let map: SubscriptionMap<usize, usize> = SubscriptionMap::new();
        assert_map_len!(map, 0);

        let _ = map.get_or_insert(1, 1).await;
        assert_map_len!(map, 0);

        let _ = map.get_or_insert(2, 2).await;
        assert_map_len!(map, 0);
    }

    #[async_std::test]
    async fn should_remove_entries_on_ref_drop() {
        let map: SubscriptionMap<usize, usize> = SubscriptionMap::new();
        assert_map_len!(map, 0);

        let ref_one = map.get_or_insert(1, 1).await;
        assert_map_len!(map, 1);

        let ref_two = map.get_or_insert(2, 2).await;
        assert_map_len!(map, 2);

        drop(ref_one);
        assert_map_len!(map, 1);
        assert!(map.snapshot().await.get(&1).is_none());
        assert!(map.snapshot().await.get(&2).is_some());

        drop(ref_two);
        assert_map_len!(map, 0);
        assert!(map.snapshot().await.get(&1).is_none());
        assert!(map.snapshot().await.get(&2).is_none());
    }

    #[async_std::test]
    async fn should_keep_track_of_ref_count() {
        let map: SubscriptionMap<usize, usize> = SubscriptionMap::new();
        assert_map_len!(map, 0);

        let ref_one = map.get_or_insert(1, 1).await;
        assert_ref_count!(map, &1, 1);

        let ref_two = map.get_or_insert(1, 1).await;
        assert_ref_count!(map, &1, 2);

        drop(ref_one);
        assert_ref_count!(map, &1, 1);

        drop(ref_two);
        assert_map_len!(map, 0);
    }

    #[async_std::test]
    #[should_panic]
    async fn shouldnt_remove_if_rc_is_not_zero() {
        let map: SubscriptionMap<usize, usize> = SubscriptionMap::new();
        assert_map_len!(map, 0);

        let _ref = map.get_or_insert(1, 1).await;
        assert_ref_count!(map, &1, 1);

        map.remove(&1).await.unwrap();
    }
}
