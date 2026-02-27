# Trust Levels

> **What you'll learn**
>
> - The three trust levels: Supervised, Graduated, and Autonomous
> - How agents earn trust through tracked success metrics
> - Which operations are permanently pinned to Level 0
> - How to manually set trust levels
> - How and when trust is revoked

---

## Overview

Trust levels control how much human oversight an agent chain requires. New chains start fully supervised and can earn autonomy over time through demonstrated reliability. This is a progressive trust model: the system proves itself before being given more freedom.

```
Level 0: Supervised         Level 1: Graduated         Level 2: Autonomous
(default for new chains)    (earned through success)    (full self-governance)

Every step verified    -->  Only unproven steps    -->  No verification needed
by LLM gate                 verified                    within constitution

Requirements:               Requirements:               Requirements:
  None (default)              >50 runs                    ALL steps graduated
                              >95% success rate           (>50 runs, >95% each)
                              Per-step tracking           No pinned abilities
```

---

## Level 0: Supervised

**This is the default for all new agent chains.** Every action in the chain requires verification by the LLM gate before execution.

**What happens at Level 0:**

1. The chain submits each step to the LLM verifier
2. The LLM evaluates whether the step is safe and appropriate given the current context
3. The LLM returns APPROVE or REJECT for each step
4. Only approved steps execute

**Example scenario:**

```
Chain: "check_and_forward_email"
  Step 1: email.list (check inbox)         → LLM verifies → APPROVED
  Step 2: email.read (read message)        → LLM verifies → APPROVED
  Step 3: email.forward (forward to boss)  → LLM verifies → APPROVED
```

All three steps required LLM verification. This adds latency and cost but ensures maximum safety during the learning period.

**When you are at Level 0:**

- All new chains with no execution history
- Chains that use pinned abilities (financial, irreversible operations)
- Chains whose trust was revoked due to anomalies

---

## Level 1: Graduated

**Earned after individual steps within a chain prove reliable.** Steps that have been executed successfully more than 50 times with a success rate above 95% are auto-approved. Only unproven or low-success steps require LLM verification.

**Graduation criteria (per step):**

| Metric | Threshold |
|---|---|
| Total runs | > 50 |
| Success rate | > 95% |

**What happens at Level 1:**

1. The trust manager checks each step's execution history
2. Steps that meet both thresholds are marked as graduated and skip verification
3. Steps that have not yet graduated still go through LLM verification
4. The chain's overall trust level is "Graduated" if at least one step (but not all) has graduated

**Example scenario:**

```
Chain: "morning_briefing"
  Step 1: weather.check    (127 runs, 99% success)  → GRADUATED, skip verify
  Step 2: email.summary    (89 runs, 97% success)   → GRADUATED, skip verify
  Step 3: calendar.today   (112 runs, 98% success)  → GRADUATED, skip verify
  Step 4: news.summarize   (12 runs, 92% success)   → NOT graduated, LLM verifies
```

Steps 1-3 run instantly without verification. Step 4 still needs LLM approval because it has not yet reached 50 runs or 95% success.

**The benefit:** Graduated chains are faster and cheaper because proven steps skip the LLM verification call. The system only spends verification resources on steps that have not yet demonstrated reliability.

---

## Level 2: Autonomous

**The highest trust level.** Every step in the chain has graduated (>50 runs, >95% success), so no LLM verification is needed at all. The chain executes fully autonomously within the boundaries set by the constitution.

**What happens at Level 2:**

1. The trust manager confirms all steps are graduated
2. The chain executes without any LLM verification calls
3. Execution is logged for continued monitoring
4. The constitution still enforces hard boundaries (block/confirm rules still apply)

**Example scenario:**

```
Chain: "daily_portfolio_check"
  Step 1: market.get_prices   (234 runs, 99.6% success)  → GRADUATED
  Step 2: portfolio.summary   (234 runs, 98.7% success)  → GRADUATED
  Step 3: notify.user         (230 runs, 99.1% success)  → GRADUATED

  Trust level: AUTONOMOUS — no verification needed
  Constitution: still enforces (e.g., confirm before trading)
```

**Important:** Autonomous does NOT mean unrestricted. The constitution still applies. A Level 2 chain that tries to execute a `block`ed action will still be rejected. A `confirm` action will still prompt the user. Trust levels control LLM verification, not constitution enforcement.

---

## How Agents Earn Trust

Trust is earned through a statistical tracking system that records every step execution:

### Per-step tracking

The trust manager maintains a SQLite database with execution statistics for every (chain_id, step_id) pair:

```
+──────────+──────────+────────────+───────────────+───────────────+
| chain_id | step_id  | total_runs | success_count | failure_count |
+──────────+──────────+────────────+───────────────+───────────────+
| morning  | weather  | 127        | 126           | 1             |
| morning  | email    | 89         | 87            | 2             |
| morning  | calendar | 112        | 110           | 2             |
| morning  | news     | 12         | 11            | 1             |
| trade    | execute  | 340        | 338           | 2             |
+──────────+──────────+────────────+───────────────+───────────────+
```

After each step execution, the trust manager records whether it succeeded or failed. The success rate is computed as `success_count / total_runs`.

### Graduation check

Before each chain execution, the trust manager evaluates the trust level:

```
assess(chain_id, step_ids, abilities):

  1. Check for pinned abilities → if any, return Level 0

  2. For each step_id:
       Look up (chain_id, step_id) stats
       If total_runs >= 50 AND success_rate >= 0.95:
         Mark as graduated
       Else:
         Mark as unproven, add to verification list

  3. If ALL steps graduated → Level 2 (Autonomous)
     If SOME steps graduated → Level 1 (Graduated)
     If NO steps graduated  → Level 0 (Supervised)
```

### What counts as success or failure

- **Success:** The step executed without errors and produced the expected output type
- **Failure:** The step threw an error, timed out, produced invalid output, or was rejected by the user during a confirmation flow

---

## Pinned Operations

Certain operations are **permanently pinned to Level 0** regardless of their success history. These are operations where the cost of a single failure is too high to ever allow unsupervised execution:

```
Pinned abilities (always Level 0):
  trading.execute     Financial trade execution
  trading.sell        Sell orders
  trading.buy         Buy orders
  payment.send        Money transfers
  payment.transfer    Money transfers
  email.send          Outbound email (irreversible)
```

Even if `trading.execute` has been run 1,000 times with 100% success, it remains at Level 0. Every execution requires LLM verification.

**Rationale:** A 99.9% success rate over 1,000 runs still means one potential failure. For financial transactions and irreversible communications, that one failure could mean real monetary loss or reputational damage. The LLM verification cost ($0.001-0.01 per step) is negligible compared to the potential cost of an unsupervised failure.

---

## Manually Setting Trust Levels

In most cases, trust levels are managed automatically by the trust tracking system. However, administrators can override trust levels using the CLI:

### Promote a chain to a higher trust level

```bash
# Promote a chain to Level 2 (Autonomous)
# This bypasses the 50-run and 95% success requirements
nabaos trust set morning_briefing autonomous

# Promote to Level 1 (Graduated)
nabaos trust set morning_briefing graduated
```

**Warning:** Manual promotion skips the statistical verification. Use this only for chains you have thoroughly tested outside the production system.

### Demote a chain to a lower trust level

```bash
# Reset a chain to Level 0 (Supervised)
nabaos trust set morning_briefing supervised

# Reset all chains to Level 0
nabaos trust reset --all
```

### View current trust levels

```bash
# Show trust status for all chains
nabaos trust status

# Output:
# Chain              Level              Steps (graduated/total)
# morning_briefing   Level 1 (Graduated) 3/4
# daily_portfolio    Level 2 (Autonomous) 3/3
# trade_executor     Level 0 (Supervised) 0/2 [PINNED: trading.execute]
```

---

## Trust Revocation

Trust can be revoked (downgraded) when the system detects problems:

### Automatic revocation triggers

**Success rate degradation:** If a previously graduated step's success rate drops below 0.95 (calculated over a rolling window), the step loses its graduated status. This causes the chain's trust level to drop from Autonomous to Graduated, or from Graduated to Supervised.

**Anomaly detection:** The anomaly detector monitors for unusual patterns that may indicate a compromised or malfunctioning agent:

- **Frequency anomaly:** A chain that normally runs 5 times per day suddenly runs 50 times
- **Scope expansion:** A chain starts accessing targets it never accessed before
- **Pattern deviation:** Step execution order or parameter patterns differ from the historical norm

When an anomaly is detected:

1. The chain's trust level is immediately set to Level 0 (Supervised)
2. A security alert is sent to the security bot Telegram channel
3. The anomaly is logged with full context for investigation
4. The chain remains at Level 0 until an administrator reviews and manually clears the anomaly flag

### Manual revocation

```bash
# Revoke trust for a specific chain due to observed issues
nabaos trust revoke morning_briefing --reason "Unexpected email forwarding behavior"
```

### Recovery after revocation

After revocation, the chain starts accumulating fresh statistics from zero. It must re-earn graduation through the normal 50-run, 95% success path. Previous statistics are retained for audit purposes but are not used in the new graduation assessment.

This ensures that a chain which was revoked due to genuine problems cannot immediately re-graduate based on historical data that predates the problem.
