# Anomaly Detection

> **What you'll learn**
>
> - How behavioral profiling tracks agent activity patterns
> - What learning mode is and why it lasts 24 hours
> - The two anomaly categories: frequency and scope
> - Alert severity levels and how thresholds work
> - How to check anomaly status and handle false positives

---

## Overview

The anomaly detector builds a behavioral profile for each agent and flags
deviations from established patterns. Unlike the credential scanner and pattern
matcher (which use static rules), the anomaly detector learns what "normal"
looks like for each agent and alerts when behavior changes.

This catches attacks that static rules miss: a compromised agent that suddenly
starts accessing new file paths, contacting new network domains, or calling
tools at unusual rates.

---

## Behavioral Profiling

Each agent has a `BehaviorProfile` that tracks:

| Data point | What it records | Storage |
|---|---|---|
| **Tool call frequency** | Rolling counters for last hour, last 24h, last 7 days, plus a rolling hourly average | `FrequencyCounters` struct |
| **Known file paths** | SHA-256 hashes of file paths the agent has accessed | `HashSet<String>` (max 10,000 entries) |
| **Known domains** | Network domains the agent has contacted | `HashSet<String>` (max 10,000 entries) |
| **Known tools** | Tool/ability names the agent has used | `HashSet<String>` (max 10,000 entries) |
| **Channel frequency** | Message counts per channel (Telegram, Discord, etc.) | `HashMap<String, u32>` |
| **Recent tool calls** | Timestamps of recent tool invocations (sliding 7-day window) | `Vec<i64>` (max 50,000 entries) |
| **Recent messages** | Timestamps of recent messages (sliding 1-hour window) | `Vec<i64>` (max 50,000 entries) |

**Privacy property:** File paths and domains are never stored in raw form.
Paths are SHA-256 hashed before storage. Anomaly descriptions use category
labels (like `SENSITIVE_CREDENTIALS` or `SYSTEM_CONFIG`) instead of actual
paths.

---

## Learning Mode

When an agent is first created, its profile enters **learning mode**. During
learning mode, the detector records all activity to build a baseline but does
**not** generate any alerts.

The default learning period is **24 hours**, configurable via the
`NABA_LEARNING_HOURS` environment variable:

```bash
# Default: 24 hours
export NABA_LEARNING_HOURS=24

# Shorter for testing
export NABA_LEARNING_HOURS=1

# Longer for complex agents with varied daily patterns
export NABA_LEARNING_HOURS=72
```

### Why 24 hours?

Most agent usage follows daily patterns. An agent that checks email at 7 AM,
monitors stocks during market hours, and runs a digest at 6 PM needs a full
day cycle to establish its normal tool call frequency and domain access
patterns. Starting alerts before the baseline is established would generate
a flood of false positives.

### How learning mode ends

The detector checks learning mode status on every event. When the elapsed time
since profile creation exceeds `learning_hours`, learning mode is disabled
automatically:

```text
Profile created:  2026-02-23 10:00 UTC
Learning hours:   24
Learning ends:    2026-02-24 10:00 UTC

Events before 10:00 Feb 24:  recorded, no alerts
Events after 10:00 Feb 24:   recorded AND evaluated for anomalies
```

---

## Two Anomaly Categories

### 1. Frequency Anomalies

Frequency anomalies detect unusual rates of activity.

**Tool call spike:** The detector compares the current hour's tool call count
against the rolling hourly average. If the ratio exceeds the configured
threshold (default 3.0x), an anomaly is raised.

```text
Average hourly rate: 5 tool calls/hour
Current hour:        18 tool calls
Ratio:               3.6x
Threshold:           3.0x
Result:              FREQUENCY anomaly, MEDIUM severity
```

The severity scales with the ratio:

| Ratio | Severity |
|---|---|
| 1x - 3x (threshold) | No alert |
| 3x - 6x | Medium |
| 6x - 9x | High |
| > 9x | Critical |

**Message burst:** More than 10 messages in a single minute triggers a `MEDIUM`
severity frequency anomaly. This pattern indicates possible automated probing
or a compromised channel adapter.

```text
Messages in last 60 seconds: 15
Threshold:                    10
Result:                       FREQUENCY anomaly, MEDIUM severity
Description:                  "15 messages in last minute - possible automated probing"
```

### 2. Scope Anomalies

Scope anomalies detect access to resources the agent has never used before.

**New file path:** When an agent accesses a file path whose SHA-256 hash is not
in the profile's `known_paths` set, a scope anomaly is raised. Severity depends
on path sensitivity:

| Path category | Severity | Examples |
|---|---|---|
| Sensitive credentials | High | `~/.ssh/id_rsa`, `~/.aws/credentials`, `.env` |
| System config | Low | `/etc/hostname` |
| User documents | Low | `~/Documents/report.pdf` |
| Temp files | Low | `/tmp/data.json` |

**New network domain:** First-ever contact with a domain that is not in the
profile's `known_domains` set triggers a `MEDIUM` scope anomaly. This catches
data exfiltration attempts that route through new endpoints.

**New tool:** First-ever use of a tool/ability not in the profile's
`known_tools` set triggers a `LOW` scope anomaly. While new tools are often
legitimate (the user installed a new plugin), the alert provides an audit trail.

---

## Alert Severity Levels

| Level | Meaning | Action |
|---|---|---|
| `LOW` | Noteworthy but likely benign | Logged, visible in dashboard |
| `MEDIUM` | Unusual pattern, warrants review | Logged, security bot notification |
| `HIGH` | Likely malicious or dangerous | Logged, security bot alert, may pause execution |
| `CRITICAL` | Extreme deviation, immediate action | Logged, security bot urgent alert, execution halted |

Anomaly assessments are summarized into an `AnomalyAssessment` struct:

```text
AnomalyAssessment {
    anomaly_count: 2,
    max_severity: Some(High),
    has_critical: false,
    categories: [
        "scope: First-ever access to path category: SENSITIVE_CREDENTIALS",
        "frequency: Tool call rate 18/hr is 3.6x above average 5.0/hr"
    ]
}
```

If `has_critical` is true (any `HIGH` or `CRITICAL` anomaly was detected), the
orchestrator can block the request before it reaches the pipeline.

---

## Alert Notification

When an anomaly is detected, the security bot sends a notification via the
configured alert channel (typically a dedicated Telegram chat):

```text
SECURITY ALERT [MEDIUM]
Agent: stock-watcher
Category: frequency
Description: Tool call rate 18/hr is 3.6x above average 5.0/hr
Session: telegram:user123
Time: 2026-02-24 14:32:07 UTC
```

Configure the security bot:

```bash
export NABA_SECURITY_BOT_TOKEN=your-security-bot-token
export NABA_ALERT_CHAT_ID=your-alert-chat-id
```

---

## Checking Anomaly Status

### View the current behavioral profile

```bash
nyaya anomaly profile stock-watcher
```

**Expected output:**

```text
Agent: stock-watcher
Created: 2026-02-23 10:00 UTC
Learning mode: OFF (ended 2026-02-24 10:00 UTC)

Tool calls:
  Last hour:    5
  Last 24h:     87
  Last 7d:      412
  Avg hourly:   2.5

Known resources:
  Paths:    23 (hashed)
  Domains:  8
  Tools:    6

Channel frequency:
  telegram: 412
  web:      31
```

### View recent anomalies

```bash
nyaya anomaly list --since 24h
```

**Expected output:**

```text
Recent anomalies (last 24h):

  [1] 2026-02-24 14:32:07  MEDIUM  frequency
      Tool call rate 18/hr is 3.6x above average 5.0/hr
      Session: telegram:user123

  [2] 2026-02-24 09:15:22  LOW     scope
      First-ever use of tool: calendar.list
      Session: telegram:user123
```

### Test anomaly detection

```bash
nyaya anomaly test stock-watcher --tool "shell.execute" --path "/etc/passwd"
```

**Expected output:**

```text
Simulating event for agent "stock-watcher":
  Tool: shell.execute
  Path: /etc/passwd

Anomalies detected: 2
  [1] LOW   scope  "First-ever use of tool: shell.execute"
  [2] HIGH  scope  "First-ever access to path category: SYSTEM_CONFIG"

Assessment: has_critical=true (HIGH severity detected)
```

---

## False Positive Handling

False positives are inevitable during the first few days after learning mode
ends, especially for agents with irregular usage patterns. Here are the
strategies for handling them:

### Extend learning mode

If your agent has weekly patterns (e.g., different behavior on weekdays vs.
weekends), extend the learning period to 7 days:

```bash
export NABA_LEARNING_HOURS=168
```

### Acknowledge known tools/paths

If a scope anomaly fires for a legitimate new tool or path, the act of using it
adds it to the profile's known set. Future uses of the same resource will not
trigger an alert.

### Adjust the threshold

The anomaly threshold (default 3.0x) controls how much deviation is tolerated
before a frequency anomaly fires. Increase it for agents with bursty patterns:

```bash
# Default: alert at 3x above average
# Higher: more tolerant of spikes
export NABA_ANOMALY_THRESHOLD=5.0
```

### Bounded growth

All profile data structures are bounded to prevent memory exhaustion:

| Data | Maximum entries |
|---|---|
| Known paths | 10,000 |
| Known domains | 10,000 |
| Known tools | 10,000 |
| Recent tool call timestamps | 50,000 |
| Recent message timestamps | 50,000 |

When a bound is reached, no new entries are added until existing entries age out
of the sliding window.

---

## How Anomaly Detection Complements Other Security Layers

| Security layer | Catches | Misses |
|---|---|---|
| Pattern matcher | Known injection patterns | Novel attacks, obfuscated payloads |
| Credential scanner | Secrets with known formats | Custom credential formats |
| BERT classifier | Broad attack categories | Subtle, in-distribution attacks |
| Constitution enforcer | Policy violations | Attacks within allowed scope |
| **Anomaly detector** | **Behavioral deviations** | **Attacks during learning mode** |

The anomaly detector's unique value is that it catches attacks that look
"normal" to static rules but are abnormal for the specific agent. A tool call
to `file.read` is perfectly safe in general, but if the `stock-watcher` agent
has never called `file.read` before, it warrants investigation.

---

## Next Steps

- [Threat Model](threat-model.md) -- see how anomaly detection fits in the defense-in-depth model
- [Circuit Breakers](circuit-breakers.md) -- add hard limits that complement behavioral monitoring
- [Debug Mode](../troubleshooting/debug-mode.md) -- inspect anomaly detection decisions at debug log level
