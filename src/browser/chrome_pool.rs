// ChromePool — manages a pool of shared Chrome tabs for concurrent browser tasks.
//
// Wraps `modules::browser::CdpTransport` and `CdpTarget` to provide checkout/return
// semantics so multiple async tasks can share a single headless Chrome instance.

use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::core::error::{NyayaError, Result};
use crate::modules::browser::{BrowserConfig, CdpTarget, CdpTransport, CookieJar};

// ---------------------------------------------------------------------------
// TabHandle — a checked-out browser tab
// ---------------------------------------------------------------------------

/// A handle to a single browser tab managed by the pool.
#[derive(Debug)]
pub struct TabHandle {
    /// The CDP target backing this tab.
    pub target: CdpTarget,
    /// CDP transport for sending commands to this tab.
    pub transport: CdpTransport,
    /// Per-tab cookie jar for authenticated sessions.
    pub cookies: CookieJar,
}

impl TabHandle {
    /// Create a new `TabHandle` from a discovered CDP target.
    pub fn new(target: CdpTarget) -> Self {
        let ws_url = target
            .ws_url
            .clone()
            .unwrap_or_else(|| format!("ws://127.0.0.1:9222/devtools/page/{}", target.id));
        Self {
            transport: CdpTransport::new(&ws_url),
            target,
            cookies: CookieJar::new(),
        }
    }

    /// Restore a saved session (cookies) onto this tab.
    pub fn restore_session(&mut self, session: &crate::browser::session_store::StoredSession) {
        if let Ok(cookies) = serde_json::from_str(&session.cookies_json) {
            self.cookies = cookies;
        }
    }

    /// Capture the current session for persistence.
    pub fn capture_session(&self, domain: &str) -> crate::browser::session_store::StoredSession {
        crate::browser::session_store::StoredSession {
            domain: domain.to_string(),
            cookies_json: serde_json::to_string(&self.cookies).unwrap_or_default(),
            local_storage_json: "{}".to_string(),
            updated_at: chrono::Utc::now().timestamp(),
        }
    }
}

// ---------------------------------------------------------------------------
// ChromePool — checkout / return semantics for shared tabs
// ---------------------------------------------------------------------------

/// Pool of reusable Chrome tabs.
///
/// Consumers call [`checkout`] to get exclusive access to a tab and [`return_tab`]
/// when they are done. The pool does NOT launch Chrome itself — it is populated
/// via [`add_tab`] or [`populate_from_targets`].
#[derive(Debug)]
pub struct ChromePool {
    /// Available (idle) tabs ready for checkout.
    available: Arc<Mutex<VecDeque<TabHandle>>>,
    /// Maximum tabs the pool will track. Excess tabs are dropped on return.
    max_tabs: usize,
    /// Browser configuration (timeout, domain allowlist, etc.).
    config: BrowserConfig,
}

impl ChromePool {
    /// Create a new, empty pool with the given capacity and config.
    pub fn new(max_tabs: usize, config: BrowserConfig) -> Self {
        Self {
            available: Arc::new(Mutex::new(VecDeque::with_capacity(max_tabs))),
            max_tabs,
            config,
        }
    }

    /// Number of tabs currently available for checkout.
    pub async fn available_count(&self) -> usize {
        self.available.lock().await.len()
    }

    /// Add a tab to the pool. Returns `Err` if the pool is already at capacity.
    pub async fn add_tab(&self, tab: TabHandle) -> Result<()> {
        let mut tabs = self.available.lock().await;
        if tabs.len() >= self.max_tabs {
            return Err(NyayaError::Config(format!(
                "ChromePool at capacity ({} tabs)",
                self.max_tabs
            )));
        }
        tabs.push_back(tab);
        Ok(())
    }

    /// Populate the pool from a list of discovered CDP targets.
    ///
    /// Only targets of type `"page"` with a WebSocket URL are added.
    /// Stops once the pool reaches `max_tabs`.
    pub async fn populate_from_targets(&self, targets: Vec<CdpTarget>) -> usize {
        let mut tabs = self.available.lock().await;
        let mut added = 0usize;
        for target in targets {
            if tabs.len() >= self.max_tabs {
                break;
            }
            if target.target_type == "page" && target.ws_url.is_some() {
                tabs.push_back(TabHandle::new(target));
                added += 1;
            }
        }
        added
    }

    /// Check out an idle tab from the pool.
    ///
    /// Returns `Err(NyayaError::Config)` if no tabs are available.
    pub async fn checkout(&self) -> Result<TabHandle> {
        let mut tabs = self.available.lock().await;
        tabs.pop_front()
            .ok_or_else(|| NyayaError::Config("No tabs available in ChromePool".into()))
    }

    /// Return a tab to the pool after use.
    ///
    /// If the pool is at capacity the tab is silently dropped.
    pub async fn return_tab(&self, tab: TabHandle) {
        let mut tabs = self.available.lock().await;
        if tabs.len() < self.max_tabs {
            tabs.push_back(tab);
        }
        // else: tab is dropped (excess)
    }

    /// Reference to the browser config used by this pool.
    pub fn config(&self) -> &BrowserConfig {
        &self.config
    }

    /// Maximum number of tabs this pool will hold.
    pub fn max_tabs(&self) -> usize {
        self.max_tabs
    }
}

impl fmt::Display for ChromePool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ChromePool(max_tabs={})", self.max_tabs)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a CdpTarget suitable for testing (no real Chrome needed).
    fn mock_target(id: &str) -> CdpTarget {
        CdpTarget {
            id: id.to_string(),
            ws_url: Some(format!("ws://127.0.0.1:9222/devtools/page/{}", id)),
            title: format!("Tab {}", id),
            url: "about:blank".to_string(),
            target_type: "page".to_string(),
        }
    }

    #[tokio::test]
    async fn test_checkout_empty_pool_errors() {
        let pool = ChromePool::new(4, BrowserConfig::default());
        let result = pool.checkout().await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("No tabs available"),
            "Expected 'No tabs available' error, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_checkout_return_tab() {
        let pool = ChromePool::new(4, BrowserConfig::default());
        let target = mock_target("AAA");
        let tab = TabHandle::new(target);

        pool.add_tab(tab).await.unwrap();
        assert_eq!(pool.available_count().await, 1);

        // Checkout should succeed and remove from available.
        let handle = pool.checkout().await.unwrap();
        assert_eq!(handle.target.id, "AAA");
        assert_eq!(pool.available_count().await, 0);

        // Return it.
        pool.return_tab(handle).await;
        assert_eq!(pool.available_count().await, 1);
    }

    #[tokio::test]
    async fn test_available_count() {
        let pool = ChromePool::new(4, BrowserConfig::default());
        assert_eq!(pool.available_count().await, 0);

        pool.add_tab(TabHandle::new(mock_target("T1")))
            .await
            .unwrap();
        pool.add_tab(TabHandle::new(mock_target("T2")))
            .await
            .unwrap();
        assert_eq!(pool.available_count().await, 2);

        let _ = pool.checkout().await.unwrap();
        assert_eq!(pool.available_count().await, 1);
    }

    #[tokio::test]
    async fn test_add_tab_at_capacity_errors() {
        let pool = ChromePool::new(1, BrowserConfig::default());
        pool.add_tab(TabHandle::new(mock_target("T1")))
            .await
            .unwrap();

        let result = pool.add_tab(TabHandle::new(mock_target("T2"))).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("capacity"));
    }

    #[tokio::test]
    async fn test_return_tab_at_capacity_drops() {
        let pool = ChromePool::new(1, BrowserConfig::default());
        pool.add_tab(TabHandle::new(mock_target("T1")))
            .await
            .unwrap();

        // Return another tab when already at capacity — should be silently dropped.
        pool.return_tab(TabHandle::new(mock_target("T2"))).await;
        assert_eq!(pool.available_count().await, 1);
    }

    #[tokio::test]
    async fn test_populate_from_targets() {
        let pool = ChromePool::new(3, BrowserConfig::default());
        let targets = vec![
            mock_target("P1"),
            mock_target("P2"),
            CdpTarget {
                id: "worker1".into(),
                ws_url: Some("ws://127.0.0.1:9222/devtools/worker/W1".into()),
                title: "Worker".into(),
                url: "".into(),
                target_type: "service_worker".into(), // not "page" — should be skipped
            },
            mock_target("P3"),
            mock_target("P4"), // exceeds max_tabs=3, should be skipped
        ];

        let added = pool.populate_from_targets(targets).await;
        assert_eq!(added, 3);
        assert_eq!(pool.available_count().await, 3);
    }

    #[tokio::test]
    async fn test_checkout_order_is_fifo() {
        let pool = ChromePool::new(4, BrowserConfig::default());
        pool.add_tab(TabHandle::new(mock_target("FIRST")))
            .await
            .unwrap();
        pool.add_tab(TabHandle::new(mock_target("SECOND")))
            .await
            .unwrap();

        let first = pool.checkout().await.unwrap();
        assert_eq!(first.target.id, "FIRST");

        let second = pool.checkout().await.unwrap();
        assert_eq!(second.target.id, "SECOND");
    }

    #[tokio::test]
    async fn test_display_impl() {
        let pool = ChromePool::new(5, BrowserConfig::default());
        let display = format!("{}", pool);
        assert_eq!(display, "ChromePool(max_tabs=5)");
    }

    #[tokio::test]
    async fn test_config_accessor() {
        let config = BrowserConfig {
            timeout_secs: 60,
            ..Default::default()
        };
        let pool = ChromePool::new(2, config);
        assert_eq!(pool.config().timeout_secs, 60);
        assert_eq!(pool.max_tabs(), 2);
    }

    #[test]
    fn test_tab_handle_capture_and_restore_session() {
        let target = CdpTarget {
            id: "test-1".into(),
            title: "Test".into(),
            url: "https://example.com".into(),
            ws_url: Some("ws://127.0.0.1:9222/devtools/page/test-1".into()),
            target_type: "page".into(),
        };
        let mut handle = TabHandle::new(target);

        // Capture session
        let session = handle.capture_session("example.com");
        assert_eq!(session.domain, "example.com");
        assert!(!session.cookies_json.is_empty());

        // Restore session (roundtrip)
        handle.restore_session(&session);
        // After restore, cookies should still be valid
        let session2 = handle.capture_session("example.com");
        assert_eq!(session.cookies_json, session2.cookies_json);
    }
}
