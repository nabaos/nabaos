# NabaOS Feature Parity Study: TUI vs Web UI vs Telegram

**Date**: 2026-03-08
**Version**: v0.3.2 (all gaps closed)

## Executive Summary

| Metric | TUI | Web UI | Telegram |
|--------|-----|--------|----------|
| **Total Features** | 80+ | 70+ | 55+ |
| **Interactive Elements** | 62 | 45+ | 35+ |
| **API Endpoints Used** | N/A (direct) | 50+ | N/A (direct) |
| **Pages/Tabs** | 9 tabs | 6 pages | N/A (flat) |
| **Critical Gaps** | **0** | **0** | **0** |

---

## Feature Comparison Matrix

### Legend
- Y = Fully implemented
- P = Partial / limited
- N = Not implemented
- N/A = Not applicable to this channel

---

### 1. Core Query & Chat

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| Natural language query | Y | Y | Y | All channels route through orchestrator |
| Streaming response | Y (live render) | Y (SSE delta) | P (edit msg) | Telegram sends "Thinking..." then edits |
| Chat history persistence | Y (in-memory) | Y (localStorage) | Y (session memory) | Web persists 100 msgs locally |
| Message timestamps | Y | Y | Y (Telegram native) | |
| Markdown rendering | Y (bold, code) | Y (bold, code, lists) | P (MarkdownV2) | Web has richest formatting |
| Copy message | N | Y (clipboard) | Y (Telegram native) | TUI lacks clipboard integration |
| Retry failed query | N | Y (retry button) | N | Web-only feature |
| Loading spinner | Y (animated ⠋⠙⠹) | Y (CSS spinner) | P ("Thinking...") | |
| Cost label per message | Y (tier + USD) | Y (metadata panel) | Y (footer line) | |
| Query history navigation | Y (Up/Down keys) | N | N | TUI-only feature |

### 2. Confirmation Modal (Interactive Permissions)

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| Confirmation modal | Y | Y | Y (inline keyboard) | All 3 channels implemented |
| Allow Once | Y | Y | N | |
| Allow for Session | Y | Y | N | |
| Always Allow for Agent | Y | Y | N | |
| Deny | Y | Y | N | |
| Keyboard navigation | Y (↑↓ Enter Esc) | Y (↑↓ Enter Esc 1-4) | N/A | |
| Mouse/click selection | N (terminal) | Y | N/A | |
| Quick-select (1-4 keys) | Y | Y | N/A | |
| PII redaction in modal | Y | Y | N/A | Both redact email/phone |

### 3. @Agent Routing

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| @agent-name prefix | Y | Y | Y | All 3 channels parse @agent prefix |
| Agent save/restore | Y | Y | N/A | Restores previous agent after query |
| Persona dropdown | N/A | Y (header) | N/A | |
| Persona switching | Y (command palette) | Y (dropdown) | Y (/persona, talk: callback) | Different UX per channel |
| Agent category filter | Y (8 categories) | N | N | TUI-only feature |
| Agent install/uninstall | Y (i/u keys) | Y (API) | Y (/agents) | All 3 channels |
| Agent start/stop | Y (s key) | Y (API) | Y (/agents) | All 3 channels |
| Agent search | Y (/ key) | N | Y (/agents lists) | |

### 4. Workflow Management

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| List workflows | Y | Y | Y (/chains, /workflow list) | |
| Start workflow | Y (n key + modal) | Y (API) | Y (/workflow start) | |
| Cancel workflow | Y (c key) | Y (API) | Y (/workflow cancel) | |
| Workflow status | Y (context panel) | Y (API) | Y (/workflow status) | |
| Workflow visualization | N | Y (API endpoint) | N | **Web advantage** |
| Build workflow from chat | N | Y (modal + stream) | N | **Web advantage** |
| Workflow parameters input | Y (modal fields) | P (API only) | N | TUI has rich modal |
| Trust badges | N | Y | Y (trusted/learning/new) | |
| Instance drill-down | Y (Enter key) | N | N | TUI shows instance list per workflow |

### 5. Scheduling

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| List scheduled jobs | Y (Schedule tab) | Y (API) | Y (/status shows count) | |
| Create schedule | Y (n key + modal) | Y (API) | Y (/watch command) | |
| Disable schedule | Y (d key) | Y (API) | Y (/stop disables all) | |
| Enable schedule | Y (d key toggle) | Y (API) | N | Scheduler::enable() added |
| Job history view | Y (Enter on job) | N | N | TUI-only feature |
| Search jobs | Y (/ key) | N | N | TUI-only feature |
| Emergency stop all | N | N | Y (/stop admin) | Telegram-only feature |

### 6. PEA (Persistent Execution Agents)

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| PEA mode toggle | Y | Y (header button) | N | |
| List objectives | Y (PEA tab) | Y (Pea page) | N | **Gap: Telegram has no PEA** |
| Create objective | Y (n key + modal) | Y (modal) | N | |
| Pause/resume objective | Y (p key) | P | N | |
| Cancel objective | Y (x key) | P | N | |
| Task tree view | Y (hierarchical) | N | N | TUI-only feature |
| Budget display | Y (context panel) | P | N | |
| Milestone tracking | Y (context panel) | P | N | |

### 7. Resources

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| List resources | Y | Y (API) | Y (/resource list) | |
| Resource status | Y (context panel) | Y (API) | Y (/resource status) | |
| Register resource | Y (r key + modal) | Y (API) | Y (/resource register) | All 3 channels |
| Delete resource | Y (d key) | Y (API) | Y (/resource delete) | All 3 channels |
| View leases | Y (Enter on resource) | Y (API) | Y (/resource leases) | |
| Quota usage bars | Y (context panel) | N | N | TUI-only visual |

### 8. Settings & Configuration

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| View settings | Y (Settings tab) | Y (Settings page) | Y (/settings) | |
| Edit settings | Y (Enter + modal) | Y (modals) | P (limited) | |
| Theme toggle | N (fixed dark) | Y (light/dark/system) | N/A | **Web advantage** |
| Vault/provider config | Y (settings) | Y (modals) | N | |
| Tool/MCP management | N | Y (discover, secrets) | Y (/mcp commands) | |
| Constitution rules view | N | Y (rules list) | N | **Web advantage** |
| Response style | Y (palette) | Y (6 options) | Y (/style commands) | All 3 channels |
| Reload config | Y (r key) | N | N | TUI-only feature |

### 9. Security & Safety

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| Security scan | Y (palette) | Y (settings) | Y (/scan) | All 3 channels |
| PII redaction | Y (in responses) | Y (in responses) | Y (in responses) | All channels |
| Injection detection | Y (orchestrator) | Y (orchestrator) | Y (orchestrator) | All channels |
| 2FA authentication | N | Y (weblink confirm) | Y (TOTP/password) | **Gap: TUI has no 2FA** |
| Rate limiting | N | Y (per-IP) | Y (60/min) | **Gap: TUI has no rate limit** |
| Input size limit | N | N | Y (4096 bytes) | Telegram-only |
| Privilege elevation | N | Y (API) | Y (TOTP challenge) | **Gap: TUI direct access** |
| Channel permissions | N | Y (API) | Y (/permissions) | |

### 10. Cost & Analytics

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| Total cost display | Y (status bar) | Y (dashboard) | Y (/costs) | |
| Cost dashboard | N | Y (daily/weekly/monthly) | Y (/costs dashboard) | **Gap: TUI only shows totals** |
| Cache hit rate | Y (status bar %) | Y (dashboard) | Y (/costs) | |
| Savings percentage | Y (status bar) | Y (pie chart) | Y (/costs) | |
| Period filtering | Y (palette cycle) | Y (multi-period) | Y (24h/7d/30d buttons) | All 3 channels |
| Token usage | N | Y (dashboard) | N | Web-only |

### 11. Navigation & UX

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| Tab/page navigation | Y (Tab/1-8 keys) | Y (sidebar click) | N/A (commands) | |
| Command palette | Y (Ctrl+K, 12 cmds) | N | N | TUI-only feature |
| Help overlay | Y (? key) | N | Y (/help) | |
| Keyboard shortcuts | Y (extensive) | P (confirm modal) | N/A | |
| Mouse support | Y (tab click, scroll) | Y (full) | N/A | |
| Toast notifications | Y (3s auto-dismiss) | Y (3s auto-dismiss) | N/A | |
| Logs panel | Y (L key toggle) | N | N | TUI-only feature |
| Quick action chips | N | Y (4 preset chips) | N | Web-only feature |
| Empty state suggestions | N | Y (5 suggestions) | N | Web-only feature |
| Responsive layout | N/A (terminal) | Y (mobile breakpoints) | N/A | |

### 12. External Integrations

| Feature | TUI | Web UI | Telegram | Notes |
|---------|-----|--------|----------|-------|
| Webhook receivers | N | Y (3 endpoint types) | N | Web-only |
| Slack events | N | Y (events endpoint) | N | Web-only |
| OAuth flow | N | Y (start/callback) | N | Web-only |
| WebApp integration | N | N/A | Y (Open Dashboard) | Telegram links to web UI |
| Browser automation | N | Y (API) | Y (/browser) | |

---

## Gap Analysis: Priority Fixes

### Critical Gaps — All Resolved

All 8 critical gaps identified on 2026-03-07 have been closed in v0.3.1.

| # | Feature | Missing From | Effort | Priority |
|---|---------|-------------|--------|----------|
| 1 | ~~**Confirmation modal**~~ | ~~Telegram~~ | ~~Medium~~ | **DONE** (v0.3.1) — inline keyboard |
| 2 | ~~**@agent inline routing**~~ | ~~Telegram~~ | ~~Low~~ | **DONE** (v0.3.1) — all 3 channels |
| 3 | ~~**Agent lifecycle (install/start/stop)**~~ | ~~Web, Telegram~~ | ~~Medium~~ | **DONE** (v0.3.1) — API + /agents |
| 4 | ~~**Resource register/delete**~~ | ~~Web, Telegram~~ | ~~Low~~ | **DONE** (v0.3.1) — API + /resource |
| 5 | ~~**Schedule enable**~~ | ~~Web, Telegram~~ | ~~Low~~ | **DONE** (v0.3.1) — TUI Schedule tab |
| 6 | ~~**Cost period filtering**~~ | ~~TUI~~ | ~~Low~~ | **DONE** (v0.3.1) — palette + status bar |
| 7 | ~~**Response style selector**~~ | ~~TUI~~ | ~~Low~~ | **DONE** (v0.3.1) — palette: 4 styles |
| 8 | ~~**Security scan**~~ | ~~TUI~~ | ~~Low~~ | **DONE** (v0.3.1) — palette: Run Security Scan |

### Remaining Asymmetries (by design, not gaps)

| Advantage | Channel | Features |
|-----------|---------|----------|
| **Richest keyboard UX** | TUI | Command palette (20 cmds), vim keys, help overlay, logs panel |
| **Richest visual UX** | Web UI | Theme toggle, charts, responsive, workflow builder, quick actions |
| **Most portable** | Telegram | Mobile-first, inline keyboards, WebApp bridge, 2FA, rate limiting |
| **Best security** | Telegram | 2FA, rate limiting, chat ID auth, admin-only commands |
| **Best agent management** | TUI | 8 category filters, install/uninstall, start/stop, search (all channels now support lifecycle) |
| **Best cost analytics** | Web UI | Multi-period dashboard, pie chart, token breakdown (TUI now has period filtering) |

### Completion Log

| Date | Commit | Change |
|------|--------|--------|
| 2026-03-07 | `06eadcc` | Web UI: confirmation modal + @agent routing |
| 2026-03-07 | `0552f67` | Telegram: confirmation modal + @agent routing + Schedule tab |
| 2026-03-08 | `16a35f3` | Web + Telegram: agent lifecycle (install/start/stop) |
| 2026-03-08 | `99373a3` | Web + Telegram: resource register/delete |
| 2026-03-08 | `3fa6a30` | TUI: cost period filtering, style selector, security scan |

---

## Architecture Notes

### Confirmation Flow Comparison

```
TUI:    orchestrator → mpsc::channel → AppMessage::ConfirmationNeeded → modal → mpsc response
Web:    orchestrator → SSE confirm_required event → browser modal → POST /api/v1/confirm/{id}
Telegram: orchestrator → tokio::mpsc → inline keyboard → confirm:{id}:{action} callback → mpsc response
```

### Query Routing Comparison

```
TUI:    input → parse_agent_mention → set_active_agent → process_query → restore
Web:    input → parse_agent_mention (Rust) → set_active_agent → process_query → restore
Telegram: input → parse_agent_mention → set_active_agent → process_query → restore
```

### Data Flow

```
TUI:     Direct Rust calls (no serialization overhead)
Web:     HTTP/SSE → JSON serialization → Axum handlers → Orchestrator mutex
Telegram: Telegram API → polling → message handler → Orchestrator mutex
```
