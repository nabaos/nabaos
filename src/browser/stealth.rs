// Anti-bot stealth layer — CDP scripts, Chrome launch args, and human-like delays
// to reduce detection by bot-detection systems (Cloudflare, DataDome, etc.).

use std::fmt;
use std::time::Duration;

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// StealthConfig
// ---------------------------------------------------------------------------

/// Stealth configuration applied to Chrome launch and per-tab navigation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StealthConfig {
    /// User-Agent header sent by the browser.
    pub user_agent: String,
    /// Browser viewport width in pixels.
    pub viewport_width: u32,
    /// Browser viewport height in pixels.
    pub viewport_height: u32,
    /// Minimum delay between automated actions (ms).
    pub min_action_delay_ms: u64,
    /// Maximum delay between automated actions (ms).
    pub max_action_delay_ms: u64,
    /// Inject canvas fingerprint noise to prevent canvas-based tracking.
    pub canvas_noise: bool,
    /// Spoof WebGL vendor/renderer strings.
    pub webgl_spoof: bool,
}

impl Default for StealthConfig {
    fn default() -> Self {
        Self {
            user_agent: random_user_agent(),
            viewport_width: 1920,
            viewport_height: 1080,
            min_action_delay_ms: 50,
            max_action_delay_ms: 500,
            canvas_noise: true,
            webgl_spoof: true,
        }
    }
}

impl fmt::Display for StealthConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "StealthConfig(viewport={}x{}, delay={}..{}ms, canvas_noise={}, webgl_spoof={})",
            self.viewport_width,
            self.viewport_height,
            self.min_action_delay_ms,
            self.max_action_delay_ms,
            self.canvas_noise,
            self.webgl_spoof,
        )
    }
}

// ---------------------------------------------------------------------------
// CDP stealth scripts — injected via Page.addScriptToEvaluateOnNewDocument
// ---------------------------------------------------------------------------

/// CDP JavaScript snippets to inject on every new page to mask automation signals.
///
/// Returns a `Vec<String>` where each entry is a self-contained script. They are
/// designed to be injected independently so a failure in one does not block the rest.
pub fn stealth_scripts(config: &StealthConfig) -> Vec<String> {
    let mut scripts: Vec<String> = Vec::with_capacity(6);

    // 1. Remove navigator.webdriver flag
    scripts.push(
        r#"Object.defineProperty(navigator, 'webdriver', {
  get: () => undefined,
});"#
            .to_string(),
    );

    // 2. Fake plugins array (empty = bot signal)
    scripts.push(
        r#"Object.defineProperty(navigator, 'plugins', {
  get: () => {
    const plugins = [
      { name: 'Chrome PDF Plugin', filename: 'internal-pdf-viewer', description: 'Portable Document Format' },
      { name: 'Chrome PDF Viewer', filename: 'mhjfbmdgcfjbbpaeojofohoefgiehjai', description: '' },
      { name: 'Native Client', filename: 'internal-nacl-plugin', description: '' },
    ];
    plugins.length = 3;
    return plugins;
  },
});"#
            .to_string(),
    );

    // 3. Fake languages
    scripts.push(
        r#"Object.defineProperty(navigator, 'languages', {
  get: () => ['en-US', 'en', 'es'],
});
Object.defineProperty(navigator, 'language', {
  get: () => 'en-US',
});"#
            .to_string(),
    );

    // 4. Override Permissions API
    scripts.push(
        r#"if (navigator.permissions) {
  const originalQuery = navigator.permissions.query;
  navigator.permissions.query = (parameters) => {
    if (parameters.name === 'notifications') {
      return Promise.resolve({ state: Notification.permission });
    }
    return originalQuery.call(navigator.permissions, parameters);
  };
}"#
        .to_string(),
    );

    // 5. Canvas fingerprint noise (if enabled)
    if config.canvas_noise {
        scripts.push(
            r#"(function() {
  const origToDataURL = HTMLCanvasElement.prototype.toDataURL;
  const origToBlob = HTMLCanvasElement.prototype.toBlob;
  const origGetImageData = CanvasRenderingContext2D.prototype.getImageData;

  HTMLCanvasElement.prototype.toDataURL = function() {
    const ctx = this.getContext('2d');
    if (ctx) {
      const imageData = origGetImageData.call(ctx, 0, 0, this.width, this.height);
      for (let i = 0; i < imageData.data.length; i += 4) {
        imageData.data[i] = imageData.data[i] ^ 1;
      }
      ctx.putImageData(imageData, 0, 0);
    }
    return origToDataURL.apply(this, arguments);
  };

  HTMLCanvasElement.prototype.toBlob = function() {
    const ctx = this.getContext('2d');
    if (ctx) {
      const imageData = origGetImageData.call(ctx, 0, 0, this.width, this.height);
      for (let i = 0; i < imageData.data.length; i += 4) {
        imageData.data[i] = imageData.data[i] ^ 1;
      }
      ctx.putImageData(imageData, 0, 0);
    }
    return origToBlob.apply(this, arguments);
  };
})();"#
                .to_string(),
        );
    }

    // 6. WebGL vendor/renderer spoof (if enabled)
    if config.webgl_spoof {
        scripts.push(
            r#"(function() {
  const getParameterProto = WebGLRenderingContext.prototype.getParameter;
  WebGLRenderingContext.prototype.getParameter = function(param) {
    if (param === 37445) { return 'Google Inc. (Intel)'; }
    if (param === 37446) { return 'ANGLE (Intel, Intel(R) UHD Graphics 630, OpenGL 4.5)'; }
    return getParameterProto.call(this, param);
  };

  if (typeof WebGL2RenderingContext !== 'undefined') {
    const getParameter2Proto = WebGL2RenderingContext.prototype.getParameter;
    WebGL2RenderingContext.prototype.getParameter = function(param) {
      if (param === 37445) { return 'Google Inc. (Intel)'; }
      if (param === 37446) { return 'ANGLE (Intel, Intel(R) UHD Graphics 630, OpenGL 4.5)'; }
      return getParameter2Proto.call(this, param);
    };
  }
})();"#
                .to_string(),
        );
    }

    scripts
}

// ---------------------------------------------------------------------------
// Chrome launch arguments
// ---------------------------------------------------------------------------

/// Chrome launch arguments for stealth mode.
///
/// These flags disable automation signals, set the user-agent, and configure
/// the viewport size. They should be passed to the Chrome process at startup.
pub fn stealth_chrome_args(config: &StealthConfig) -> Vec<String> {
    vec![
        "--disable-blink-features=AutomationControlled".into(),
        format!("--user-agent={}", config.user_agent),
        format!(
            "--window-size={},{}",
            config.viewport_width, config.viewport_height
        ),
        "--disable-extensions".into(),
        "--disable-default-apps".into(),
        "--disable-component-extensions-with-background-pages".into(),
        "--no-first-run".into(),
        "--no-default-browser-check".into(),
    ]
}

// ---------------------------------------------------------------------------
// Human-like delay
// ---------------------------------------------------------------------------

/// Random delay between actions to simulate human behaviour.
///
/// The delay is uniformly distributed between `min_action_delay_ms` and
/// `max_action_delay_ms` in the given config.
pub fn human_delay(config: &StealthConfig) -> Duration {
    let range = config
        .max_action_delay_ms
        .saturating_sub(config.min_action_delay_ms);
    let delay = config.min_action_delay_ms + (rand_u64() % range.max(1));
    Duration::from_millis(delay)
}

// ---------------------------------------------------------------------------
// Simple PRNG — avoids pulling in the `rand` crate just for delay jitter
// ---------------------------------------------------------------------------

/// Simple PRNG seeded from the system clock. Good enough for delay randomisation;
/// NOT suitable for cryptographic use.
fn rand_u64() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::SystemTime;
    static STATE: AtomicU64 = AtomicU64::new(0);

    // Seed from clock on first call, then advance the LCG on every call
    let prev = STATE.load(Ordering::Relaxed);
    let seed = if prev == 0 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos() as u64
    } else {
        prev
    };
    let next = seed
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    STATE.store(next, Ordering::Relaxed);
    next
}

/// Rotate through realistic Chrome user-agents.
fn random_user_agent() -> String {
    let agents = [
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
         (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36",
    ];
    let idx = (rand_u64() as usize) % agents.len();
    agents[idx].to_string()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_stealth_scripts_count() {
        let config = StealthConfig::default();
        let scripts = stealth_scripts(&config);
        // webdriver removal + plugins + languages + permissions + canvas + webgl = 6
        assert_eq!(
            scripts.len(),
            6,
            "Default config should produce 6 stealth scripts, got {}",
            scripts.len()
        );
    }

    #[test]
    fn test_stealth_chrome_args() {
        let config = StealthConfig::default();
        let args = stealth_chrome_args(&config);

        let has_automation = args.iter().any(|a| a.contains("AutomationControlled"));
        assert!(
            has_automation,
            "Chrome args must include AutomationControlled disable flag"
        );

        let has_user_agent = args.iter().any(|a| a.starts_with("--user-agent="));
        assert!(has_user_agent, "Chrome args must include --user-agent");

        let has_window_size = args.iter().any(|a| a.starts_with("--window-size="));
        assert!(has_window_size, "Chrome args must include --window-size");
    }

    #[test]
    fn test_human_delay_in_range() {
        let config = StealthConfig {
            min_action_delay_ms: 100,
            max_action_delay_ms: 200,
            ..Default::default()
        };

        // Run multiple times to increase confidence.
        for _ in 0..50 {
            let delay = human_delay(&config);
            let ms = delay.as_millis() as u64;
            assert!(
                (100..200).contains(&ms),
                "Delay {} ms should be in [100, 200)",
                ms
            );
        }
    }

    #[test]
    fn test_random_user_agent_valid() {
        let ua = random_user_agent();
        assert!(
            ua.contains("Chrome/"),
            "User-agent should be a Chrome UA string, got: {}",
            ua
        );
        assert!(
            ua.starts_with("Mozilla/5.0"),
            "User-agent should start with Mozilla/5.0, got: {}",
            ua
        );
        assert!(
            ua.contains("AppleWebKit/537.36"),
            "User-agent should contain AppleWebKit, got: {}",
            ua
        );
    }
}
