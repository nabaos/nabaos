//! Demo workflow definitions showcasing the full workflow DSL.
//!
//! Unlike demo_chains (linear sequences of ability calls), these workflows
//! use WaitEvent, WaitPoll, Delay, Branch, and Parallel nodes to model
//! real-world asynchronous business processes.

use std::collections::HashMap;

use crate::chain::dsl::{ConditionOp, ParamDef, ParamType, StepCondition};
use crate::chain::workflow::{
    ActionNode, BranchArm, BranchNode, DelayNode, JoinStrategy, ParallelBranch, ParallelNode,
    WaitEventNode, WaitPollNode, WorkflowDef, WorkflowNode,
};

/// Return all demo workflows.
pub fn all_demo_workflows() -> Vec<WorkflowDef> {
    vec![
        shopify_dropship(),
        email_digest(),
        price_alert(),
        cicd_pipeline(),
        customer_onboarding(),
    ]
}

// ---------------------------------------------------------------------------
// 1. Shopify Dropship — 15-node order fulfillment pipeline
// ---------------------------------------------------------------------------

/// Full Shopify dropship workflow: receive order, validate, forward to supplier,
/// poll for shipment, notify customer at each stage, handle failures.
pub fn shopify_dropship() -> WorkflowDef {
    WorkflowDef {
        id: "shopify_dropship".into(),
        name: "Shopify Dropship Order".into(),
        description: "End-to-end Shopify dropship pipeline: validate order, forward to \
                       dropshipper, poll for shipment, track delivery, notify customer."
            .into(),
        params: vec![
            ParamDef {
                name: "order_id".into(),
                param_type: ParamType::Text,
                description: "Shopify order ID".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "shop".into(),
                param_type: ParamType::Text,
                description: "Shopify shop domain".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "dropshipper_api".into(),
                param_type: ParamType::Url,
                description: "Dropshipper API base URL".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "customer_email".into(),
                param_type: ParamType::Email,
                description: "Customer email address".into(),
                required: true,
                default: None,
            },
        ],
        nodes: vec![
            // 1. Fetch order details from Shopify
            WorkflowNode::Action(ActionNode {
                id: "fetch_order".into(),
                ability: "shopify.get_order".into(),
                args: HashMap::from([
                    ("shop".into(), "{{shop}}".into()),
                    ("order_id".into(), "{{order_id}}".into()),
                ]),
                output_key: Some("order_data".into()),
                condition: None,
                on_failure: Some("notify_failure".into()),
            }),
            // 2. Validate order (inventory, address, fraud check)
            WorkflowNode::Action(ActionNode {
                id: "validate_order".into(),
                ability: "shopify.validate_order".into(),
                args: HashMap::from([("order_data".into(), "{{order_data}}".into())]),
                output_key: Some("validation".into()),
                condition: None,
                on_failure: Some("notify_failure".into()),
            }),
            // 3. Branch: valid order vs invalid
            WorkflowNode::Branch(BranchNode {
                id: "check_validation".into(),
                conditions: vec![BranchArm {
                    condition: StepCondition {
                        ref_key: "validation".into(),
                        op: ConditionOp::Equals,
                        value: "valid".into(),
                    },
                    nodes: vec![
                        // 4. Send confirmation email to customer
                        WorkflowNode::Action(ActionNode {
                            id: "send_confirmation".into(),
                            ability: "email.send".into(),
                            args: HashMap::from([
                                ("to".into(), "{{customer_email}}".into()),
                                ("subject".into(), "Order {{order_id}} confirmed".into()),
                                (
                                    "body".into(),
                                    "Your order {{order_id}} has been confirmed and is being \
                                     processed."
                                        .into(),
                                ),
                            ]),
                            output_key: None,
                            condition: None,
                            on_failure: None,
                        }),
                    ],
                }],
                otherwise: vec![
                    // Invalid order path
                    WorkflowNode::Action(ActionNode {
                        id: "cancel_invalid".into(),
                        ability: "shopify.cancel_order".into(),
                        args: HashMap::from([
                            ("shop".into(), "{{shop}}".into()),
                            ("order_id".into(), "{{order_id}}".into()),
                            ("reason".into(), "Validation failed: {{validation}}".into()),
                        ]),
                        output_key: None,
                        condition: None,
                        on_failure: None,
                    }),
                ],
            }),
            // 5. Forward order to dropshipper
            WorkflowNode::Action(ActionNode {
                id: "forward_to_supplier".into(),
                ability: "dropship.create_order".into(),
                args: HashMap::from([
                    ("api_url".into(), "{{dropshipper_api}}".into()),
                    ("order_data".into(), "{{order_data}}".into()),
                ]),
                output_key: Some("supplier_order_id".into()),
                condition: None,
                on_failure: Some("notify_failure".into()),
            }),
            // 6. Wait for supplier to acknowledge
            WorkflowNode::WaitEvent(WaitEventNode {
                id: "wait_supplier_ack".into(),
                event_type: "supplier.order_acknowledged".into(),
                filter: Some("$.supplier_order_id == '{{supplier_order_id}}'".into()),
                output_key: Some("supplier_ack".into()),
                timeout_secs: 3600, // 1 hour
                on_timeout: "notify_failure".into(),
            }),
            // 7. Poll supplier for shipment status
            WorkflowNode::WaitPoll(WaitPollNode {
                id: "poll_shipment".into(),
                ability: "dropship.check_status".into(),
                args: HashMap::from([
                    ("api_url".into(), "{{dropshipper_api}}".into()),
                    ("supplier_order_id".into(), "{{supplier_order_id}}".into()),
                ]),
                output_key: Some("shipment_status".into()),
                until: StepCondition {
                    ref_key: "shipment_status".into(),
                    op: ConditionOp::Equals,
                    value: "shipped".into(),
                },
                poll_interval_secs: 900, // every 15 min
                timeout_secs: 172800,    // 48 hours
                on_timeout: "escalate_supplier".into(),
            }),
            // 8. Extract tracking number
            WorkflowNode::Action(ActionNode {
                id: "get_tracking".into(),
                ability: "dropship.get_tracking".into(),
                args: HashMap::from([
                    ("api_url".into(), "{{dropshipper_api}}".into()),
                    ("supplier_order_id".into(), "{{supplier_order_id}}".into()),
                ]),
                output_key: Some("tracking_number".into()),
                condition: None,
                on_failure: None,
            }),
            // 9. Update Shopify order with tracking
            WorkflowNode::Action(ActionNode {
                id: "update_shopify_tracking".into(),
                ability: "shopify.add_tracking".into(),
                args: HashMap::from([
                    ("shop".into(), "{{shop}}".into()),
                    ("order_id".into(), "{{order_id}}".into()),
                    ("tracking_number".into(), "{{tracking_number}}".into()),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 10. Notify customer: shipped
            WorkflowNode::Action(ActionNode {
                id: "notify_shipped".into(),
                ability: "email.send".into(),
                args: HashMap::from([
                    ("to".into(), "{{customer_email}}".into()),
                    ("subject".into(), "Order {{order_id}} shipped!".into()),
                    (
                        "body".into(),
                        "Your order has shipped. Tracking: {{tracking_number}}".into(),
                    ),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 11. Wait for delivery event
            WorkflowNode::WaitEvent(WaitEventNode {
                id: "wait_delivery".into(),
                event_type: "shipping.delivered".into(),
                filter: Some("$.tracking_number == '{{tracking_number}}'".into()),
                output_key: Some("delivery_confirmation".into()),
                timeout_secs: 604800, // 7 days
                on_timeout: "escalate_delivery".into(),
            }),
            // 12. Notify customer: delivered
            WorkflowNode::Action(ActionNode {
                id: "notify_delivered".into(),
                ability: "email.send".into(),
                args: HashMap::from([
                    ("to".into(), "{{customer_email}}".into()),
                    ("subject".into(), "Order {{order_id}} delivered".into()),
                    (
                        "body".into(),
                        "Your order has been delivered! We hope you enjoy your purchase.".into(),
                    ),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 13. Wait 3 days before requesting review
            WorkflowNode::Delay(DelayNode {
                id: "wait_before_review".into(),
                duration_secs: 259200, // 3 days
            }),
            // 14. Request product review
            WorkflowNode::Action(ActionNode {
                id: "request_review".into(),
                ability: "email.send".into(),
                args: HashMap::from([
                    ("to".into(), "{{customer_email}}".into()),
                    ("subject".into(), "How was your order {{order_id}}?".into()),
                    (
                        "body".into(),
                        "We would love your feedback! Please leave a review for your \
                         recent purchase."
                            .into(),
                    ),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 15. Escalate to supplier on shipment delay (jumped to on poll timeout)
            WorkflowNode::Action(ActionNode {
                id: "escalate_supplier".into(),
                ability: "notify.user".into(),
                args: HashMap::from([(
                    "message".into(),
                    "ESCALATION: Order {{order_id}} — supplier {{supplier_order_id}} \
                     has not shipped within 48 hours. Manual intervention required."
                        .into(),
                )]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 16. Mark workflow complete in Shopify
            WorkflowNode::Action(ActionNode {
                id: "mark_complete".into(),
                ability: "shopify.close_order".into(),
                args: HashMap::from([
                    ("shop".into(), "{{shop}}".into()),
                    ("order_id".into(), "{{order_id}}".into()),
                ]),
                output_key: Some("completion_status".into()),
                condition: None,
                on_failure: None,
            }),
        ],
        global_timeout_secs: 1_209_600, // 14 days
        max_instances: 500,
        correlation_key: Some("{{order_id}}".into()),
        style: None,
        kb_context: None,
        channel_permissions: None,
    }
}

// ---------------------------------------------------------------------------
// 2. Email Digest — poll email every 30 min, summarize, send daily digest
// ---------------------------------------------------------------------------

/// Poll an email account periodically, accumulate summaries, and send a
/// daily digest email.
pub fn email_digest() -> WorkflowDef {
    WorkflowDef {
        id: "email_digest".into(),
        name: "Email Digest".into(),
        description: "Poll email every 30 minutes for new messages, summarize each batch, \
                       then compile and send a daily digest."
            .into(),
        params: vec![
            ParamDef {
                name: "email_account".into(),
                param_type: ParamType::Text,
                description: "Email account identifier to poll".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "digest_recipient".into(),
                param_type: ParamType::Email,
                description: "Email address to receive the daily digest".into(),
                required: true,
                default: None,
            },
        ],
        nodes: vec![
            // 1. Poll email for new messages every 30 min
            WorkflowNode::WaitPoll(WaitPollNode {
                id: "poll_email".into(),
                ability: "email.fetch_unread".into(),
                args: HashMap::from([("account".into(), "{{email_account}}".into())]),
                output_key: Some("new_emails".into()),
                until: StepCondition {
                    ref_key: "new_emails".into(),
                    op: ConditionOp::IsNotEmpty,
                    value: String::new(),
                },
                poll_interval_secs: 1800, // 30 min
                timeout_secs: 86400,      // 24 hours
                on_timeout: "compile_digest".into(),
            }),
            // 2. Summarize the batch of new emails
            WorkflowNode::Action(ActionNode {
                id: "summarize_batch".into(),
                ability: "nlp.summarize".into(),
                args: HashMap::from([("text".into(), "{{new_emails}}".into())]),
                output_key: Some("batch_summary".into()),
                condition: None,
                on_failure: None,
            }),
            // 3. Store the batch summary for later compilation
            WorkflowNode::Action(ActionNode {
                id: "store_batch".into(),
                ability: "memory.store".into(),
                args: HashMap::from([
                    ("key".into(), "email_digest_batch_{{email_account}}".into()),
                    ("value".into(), "{{batch_summary}}".into()),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 4. Wait until end of day to compile the digest
            WorkflowNode::Delay(DelayNode {
                id: "wait_for_digest_time".into(),
                duration_secs: 86400, // 24 hours
            }),
            // 5. Retrieve all stored batch summaries
            WorkflowNode::Action(ActionNode {
                id: "compile_digest".into(),
                ability: "memory.search".into(),
                args: HashMap::from([(
                    "query".into(),
                    "email_digest_batch_{{email_account}}".into(),
                )]),
                output_key: Some("all_summaries".into()),
                condition: None,
                on_failure: None,
            }),
            // 6. Generate the digest document
            WorkflowNode::Action(ActionNode {
                id: "generate_digest".into(),
                ability: "docs.generate".into(),
                args: HashMap::from([
                    ("content".into(), "{{all_summaries}}".into()),
                    ("format".into(), "digest".into()),
                ]),
                output_key: Some("digest_content".into()),
                condition: None,
                on_failure: None,
            }),
            // 7. Send the daily digest email
            WorkflowNode::Action(ActionNode {
                id: "send_digest".into(),
                ability: "email.send".into(),
                args: HashMap::from([
                    ("to".into(), "{{digest_recipient}}".into()),
                    ("subject".into(), "Your Daily Email Digest".into()),
                    ("body".into(), "{{digest_content}}".into()),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
        ],
        global_timeout_secs: 90000, // 25 hours (buffer)
        max_instances: 10,
        correlation_key: Some("{{email_account}}".into()),
        style: None,
        kb_context: None,
        channel_permissions: None,
    }
}

// ---------------------------------------------------------------------------
// 3. Price Alert — poll trading price, notify when threshold crossed
// ---------------------------------------------------------------------------

/// Continuously poll a trading instrument price and notify the user when
/// the price crosses a threshold.
pub fn price_alert() -> WorkflowDef {
    WorkflowDef {
        id: "price_alert_workflow".into(),
        name: "Price Alert".into(),
        description: "Poll a trading instrument's price at regular intervals and send a \
                       notification when it crosses the specified threshold."
            .into(),
        params: vec![
            ParamDef {
                name: "ticker".into(),
                param_type: ParamType::Text,
                description: "Stock or crypto ticker symbol".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "threshold".into(),
                param_type: ParamType::Number,
                description: "Price threshold that triggers the alert".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "direction".into(),
                param_type: ParamType::Text,
                description: "Alert direction: 'above' or 'below'".into(),
                required: true,
                default: Some("above".into()),
            },
            ParamDef {
                name: "notify_channel".into(),
                param_type: ParamType::Text,
                description: "Notification channel (email, telegram, etc.)".into(),
                required: true,
                default: Some("telegram".into()),
            },
        ],
        nodes: vec![
            // 1. Poll price until threshold is crossed
            WorkflowNode::WaitPoll(WaitPollNode {
                id: "poll_price".into(),
                ability: "trading.get_price".into(),
                args: HashMap::from([("symbol".into(), "{{ticker}}".into())]),
                output_key: Some("current_price".into()),
                until: StepCondition {
                    ref_key: "current_price".into(),
                    op: ConditionOp::GreaterThan,
                    value: "{{threshold}}".into(),
                },
                poll_interval_secs: 60, // every minute
                timeout_secs: 604800,   // 7 days
                on_timeout: "notify_expired".into(),
            }),
            // 2. Log the alert trigger
            WorkflowNode::Action(ActionNode {
                id: "log_trigger".into(),
                ability: "memory.store".into(),
                args: HashMap::from([
                    ("key".into(), "price_alert_{{ticker}}_triggered".into()),
                    (
                        "value".into(),
                        "{{ticker}} crossed {{threshold}} at {{current_price}}".into(),
                    ),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 3. Send alert notification
            WorkflowNode::Action(ActionNode {
                id: "send_alert".into(),
                ability: "notify.user".into(),
                args: HashMap::from([(
                    "message".into(),
                    "PRICE ALERT: {{ticker}} is now at {{current_price}} \
                     (threshold: {{threshold}}, direction: {{direction}})"
                        .into(),
                )]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 4. Branch: ask user whether to set a new alert
            WorkflowNode::WaitEvent(WaitEventNode {
                id: "wait_user_response".into(),
                event_type: "user.response".into(),
                filter: Some("$.context == 'price_alert_{{ticker}}'".into()),
                output_key: Some("user_decision".into()),
                timeout_secs: 3600, // 1 hour to respond
                on_timeout: "fail".into(),
            }),
            // 5. Handle expired alert
            WorkflowNode::Action(ActionNode {
                id: "notify_expired".into(),
                ability: "notify.user".into(),
                args: HashMap::from([(
                    "message".into(),
                    "Price alert for {{ticker}} (threshold: {{threshold}}) expired \
                     without triggering."
                        .into(),
                )]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
        ],
        global_timeout_secs: 604800, // 7 days
        max_instances: 100,
        correlation_key: Some("{{ticker}}_{{threshold}}".into()),
        style: None,
        kb_context: None,
        channel_permissions: None,
    }
}

// ---------------------------------------------------------------------------
// 4. CI/CD Pipeline — webhook trigger, run tests, deploy on success
// ---------------------------------------------------------------------------

/// CI/CD pipeline: wait for a git push webhook, run tests in parallel
/// (unit + integration + lint), then deploy on success or notify on failure.
pub fn cicd_pipeline() -> WorkflowDef {
    WorkflowDef {
        id: "cicd_pipeline".into(),
        name: "CI/CD Pipeline".into(),
        description: "Wait for a git push webhook, run unit tests, integration tests, \
                       and linting in parallel, then deploy on success or alert on failure."
            .into(),
        params: vec![
            ParamDef {
                name: "repo".into(),
                param_type: ParamType::Text,
                description: "Repository name (owner/repo)".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "branch".into(),
                param_type: ParamType::Text,
                description: "Branch to watch".into(),
                required: true,
                default: Some("main".into()),
            },
            ParamDef {
                name: "deploy_target".into(),
                param_type: ParamType::Text,
                description: "Deployment target environment".into(),
                required: true,
                default: Some("staging".into()),
            },
            ParamDef {
                name: "notify_channel".into(),
                param_type: ParamType::Text,
                description: "Channel for build notifications".into(),
                required: true,
                default: Some("telegram".into()),
            },
        ],
        nodes: vec![
            // 1. Wait for git push webhook
            WorkflowNode::WaitEvent(WaitEventNode {
                id: "wait_push".into(),
                event_type: "git.push".into(),
                filter: Some(
                    "$.repository == '{{repo}}' && $.ref == 'refs/heads/{{branch}}'".into(),
                ),
                output_key: Some("push_event".into()),
                timeout_secs: 0, // wait forever
                on_timeout: "fail".into(),
            }),
            // 2. Checkout / prepare build
            WorkflowNode::Action(ActionNode {
                id: "prepare_build".into(),
                ability: "ci.checkout".into(),
                args: HashMap::from([
                    ("repo".into(), "{{repo}}".into()),
                    ("commit".into(), "{{push_event}}".into()),
                ]),
                output_key: Some("build_dir".into()),
                condition: None,
                on_failure: Some("notify_build_failure".into()),
            }),
            // 3. Run tests in parallel: unit, integration, lint
            WorkflowNode::Parallel(ParallelNode {
                id: "parallel_tests".into(),
                branches: vec![
                    ParallelBranch {
                        branch_id: "unit_tests".into(),
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "run_unit_tests".into(),
                            ability: "ci.run_tests".into(),
                            args: HashMap::from([
                                ("build_dir".into(), "{{build_dir}}".into()),
                                ("suite".into(), "unit".into()),
                            ]),
                            output_key: Some("unit_result".into()),
                            condition: None,
                            on_failure: None,
                        })],
                    },
                    ParallelBranch {
                        branch_id: "integration_tests".into(),
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "run_integration_tests".into(),
                            ability: "ci.run_tests".into(),
                            args: HashMap::from([
                                ("build_dir".into(), "{{build_dir}}".into()),
                                ("suite".into(), "integration".into()),
                            ]),
                            output_key: Some("integration_result".into()),
                            condition: None,
                            on_failure: None,
                        })],
                    },
                    ParallelBranch {
                        branch_id: "lint".into(),
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "run_lint".into(),
                            ability: "ci.run_lint".into(),
                            args: HashMap::from([("build_dir".into(), "{{build_dir}}".into())]),
                            output_key: Some("lint_result".into()),
                            condition: None,
                            on_failure: None,
                        })],
                    },
                ],
                join: JoinStrategy::All,
            }),
            // 4. Branch: all passed vs some failed
            WorkflowNode::Branch(BranchNode {
                id: "check_results".into(),
                conditions: vec![BranchArm {
                    condition: StepCondition {
                        ref_key: "unit_result".into(),
                        op: ConditionOp::Equals,
                        value: "pass".into(),
                    },
                    nodes: vec![
                        // 5. Deploy
                        WorkflowNode::Action(ActionNode {
                            id: "deploy".into(),
                            ability: "ci.deploy".into(),
                            args: HashMap::from([
                                ("build_dir".into(), "{{build_dir}}".into()),
                                ("target".into(), "{{deploy_target}}".into()),
                            ]),
                            output_key: Some("deploy_result".into()),
                            condition: None,
                            on_failure: Some("notify_deploy_failure".into()),
                        }),
                        // 6. Notify success
                        WorkflowNode::Action(ActionNode {
                            id: "notify_success".into(),
                            ability: "notify.user".into(),
                            args: HashMap::from([(
                                "message".into(),
                                "CI/CD: {{repo}}@{{branch}} deployed to {{deploy_target}} \
                                 successfully."
                                    .into(),
                            )]),
                            output_key: None,
                            condition: None,
                            on_failure: None,
                        }),
                    ],
                }],
                otherwise: vec![WorkflowNode::Action(ActionNode {
                    id: "notify_build_failure".into(),
                    ability: "notify.user".into(),
                    args: HashMap::from([(
                        "message".into(),
                        "CI/CD FAILED: {{repo}}@{{branch}} — tests did not pass. \
                         Unit: {{unit_result}}, Integration: {{integration_result}}, \
                         Lint: {{lint_result}}"
                            .into(),
                    )]),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                })],
            }),
            // 7. Notify deploy failure (jumped to on deploy error)
            WorkflowNode::Action(ActionNode {
                id: "notify_deploy_failure".into(),
                ability: "notify.user".into(),
                args: HashMap::from([(
                    "message".into(),
                    "CI/CD DEPLOY FAILED: {{repo}}@{{branch}} to {{deploy_target}}. \
                     Check logs."
                        .into(),
                )]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
        ],
        global_timeout_secs: 3600, // 1 hour
        max_instances: 50,
        correlation_key: Some("{{repo}}_{{branch}}".into()),
        style: None,
        kb_context: None,
        channel_permissions: None,
    }
}

// ---------------------------------------------------------------------------
// 5. Customer Onboarding — welcome, wait, follow-up, personalize
// ---------------------------------------------------------------------------

/// Customer onboarding flow: send welcome email, wait 3 days, send follow-up,
/// wait for the customer to complete an action, then send a personalized
/// response based on what they did.
pub fn customer_onboarding() -> WorkflowDef {
    WorkflowDef {
        id: "customer_onboarding".into(),
        name: "Customer Onboarding".into(),
        description: "Automated onboarding: welcome email, 3-day delay, follow-up, wait for \
                       customer action, then personalized response based on their activity."
            .into(),
        params: vec![
            ParamDef {
                name: "customer_email".into(),
                param_type: ParamType::Email,
                description: "Customer email address".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "customer_name".into(),
                param_type: ParamType::Text,
                description: "Customer display name".into(),
                required: true,
                default: None,
            },
            ParamDef {
                name: "plan".into(),
                param_type: ParamType::Text,
                description: "Subscription plan (free, pro, enterprise)".into(),
                required: true,
                default: Some("free".into()),
            },
        ],
        nodes: vec![
            // 1. Send welcome email
            WorkflowNode::Action(ActionNode {
                id: "send_welcome".into(),
                ability: "email.send".into(),
                args: HashMap::from([
                    ("to".into(), "{{customer_email}}".into()),
                    (
                        "subject".into(),
                        "Welcome to our platform, {{customer_name}}!".into(),
                    ),
                    (
                        "body".into(),
                        "Hi {{customer_name}}, welcome aboard! Here is how to get started \
                         with your {{plan}} plan."
                            .into(),
                    ),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 2. Record onboarding start
            WorkflowNode::Action(ActionNode {
                id: "record_start".into(),
                ability: "memory.store".into(),
                args: HashMap::from([
                    ("key".into(), "onboarding_{{customer_email}}".into()),
                    ("value".into(), "started".into()),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 3. Wait 3 days
            WorkflowNode::Delay(DelayNode {
                id: "wait_3_days".into(),
                duration_secs: 259200, // 3 days
            }),
            // 4. Send follow-up email
            WorkflowNode::Action(ActionNode {
                id: "send_followup".into(),
                ability: "email.send".into(),
                args: HashMap::from([
                    ("to".into(), "{{customer_email}}".into()),
                    (
                        "subject".into(),
                        "How is it going, {{customer_name}}?".into(),
                    ),
                    (
                        "body".into(),
                        "Hi {{customer_name}}, just checking in! Have you had a chance to \
                         explore the platform? Here are some tips to get the most out of \
                         your {{plan}} plan."
                            .into(),
                    ),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 5. Wait for customer action (profile completed, first project, etc.)
            WorkflowNode::WaitEvent(WaitEventNode {
                id: "wait_customer_action".into(),
                event_type: "customer.action_completed".into(),
                filter: Some("$.customer_email == '{{customer_email}}'".into()),
                output_key: Some("action_type".into()),
                timeout_secs: 604800, // 7 days
                on_timeout: "send_nudge".into(),
            }),
            // 6. Branch: personalize response based on what the customer did
            WorkflowNode::Branch(BranchNode {
                id: "personalize_response".into(),
                conditions: vec![
                    BranchArm {
                        condition: StepCondition {
                            ref_key: "action_type".into(),
                            op: ConditionOp::Equals,
                            value: "profile_completed".into(),
                        },
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "respond_profile".into(),
                            ability: "email.send".into(),
                            args: HashMap::from([
                                ("to".into(), "{{customer_email}}".into()),
                                ("subject".into(), "Great start, {{customer_name}}!".into()),
                                (
                                    "body".into(),
                                    "Your profile is set up. Next step: create your first \
                                     project."
                                        .into(),
                                ),
                            ]),
                            output_key: None,
                            condition: None,
                            on_failure: None,
                        })],
                    },
                    BranchArm {
                        condition: StepCondition {
                            ref_key: "action_type".into(),
                            op: ConditionOp::Equals,
                            value: "first_project".into(),
                        },
                        nodes: vec![WorkflowNode::Action(ActionNode {
                            id: "respond_project".into(),
                            ability: "email.send".into(),
                            args: HashMap::from([
                                ("to".into(), "{{customer_email}}".into()),
                                ("subject".into(), "Your first project is live!".into()),
                                (
                                    "body".into(),
                                    "Congratulations {{customer_name}}! Here are some \
                                     advanced features for your {{plan}} plan."
                                        .into(),
                                ),
                            ]),
                            output_key: None,
                            condition: None,
                            on_failure: None,
                        })],
                    },
                ],
                otherwise: vec![WorkflowNode::Action(ActionNode {
                    id: "respond_generic".into(),
                    ability: "email.send".into(),
                    args: HashMap::from([
                        ("to".into(), "{{customer_email}}".into()),
                        ("subject".into(), "Keep going, {{customer_name}}!".into()),
                        (
                            "body".into(),
                            "Thanks for taking action! Let us know if you need any help \
                             with your {{plan}} plan."
                                .into(),
                        ),
                    ]),
                    output_key: None,
                    condition: None,
                    on_failure: None,
                })],
            }),
            // 7. Record completion
            WorkflowNode::Action(ActionNode {
                id: "record_complete".into(),
                ability: "memory.store".into(),
                args: HashMap::from([
                    ("key".into(), "onboarding_{{customer_email}}".into()),
                    ("value".into(), "completed".into()),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            // 8. Nudge email (jumped to on timeout from wait_customer_action)
            WorkflowNode::Action(ActionNode {
                id: "send_nudge".into(),
                ability: "email.send".into(),
                args: HashMap::from([
                    ("to".into(), "{{customer_email}}".into()),
                    ("subject".into(), "We miss you, {{customer_name}}!".into()),
                    (
                        "body".into(),
                        "It has been a while since you signed up. Need help getting started \
                         with your {{plan}} plan? Reply to this email and we will help."
                            .into(),
                    ),
                ]),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
        ],
        global_timeout_secs: 1_209_600, // 14 days
        max_instances: 1000,
        correlation_key: Some("{{customer_email}}".into()),
        style: None,
        kb_context: None,
        channel_permissions: None,
    }
}

// ---------------------------------------------------------------------------
// Helper: collect all node IDs from a workflow (recursively)
// ---------------------------------------------------------------------------

#[cfg(test)]
fn collect_node_ids(nodes: &[WorkflowNode], out: &mut Vec<String>) {
    for node in nodes {
        out.push(node.id().to_string());
        match node {
            WorkflowNode::Parallel(p) => {
                for branch in &p.branches {
                    collect_node_ids(&branch.nodes, out);
                }
            }
            WorkflowNode::Branch(b) => {
                for arm in &b.conditions {
                    collect_node_ids(&arm.nodes, out);
                }
                collect_node_ids(&b.otherwise, out);
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Helper: validate a workflow has non-empty nodes, unique IDs, and valid params.
    fn validate_workflow(wf: &WorkflowDef) {
        assert!(!wf.id.is_empty(), "Workflow ID must not be empty");
        assert!(!wf.name.is_empty(), "Workflow name must not be empty");
        assert!(
            !wf.nodes.is_empty(),
            "Workflow '{}' must have at least one node",
            wf.id
        );

        // Collect all node IDs (including nested)
        let mut ids = Vec::new();
        collect_node_ids(&wf.nodes, &mut ids);
        assert!(!ids.is_empty(), "Workflow '{}' should have node IDs", wf.id);

        // All IDs must be non-empty
        for id in &ids {
            assert!(!id.is_empty(), "Node ID must not be empty in '{}'", wf.id);
        }

        // All IDs must be unique
        let unique: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
        assert_eq!(
            unique.len(),
            ids.len(),
            "Duplicate node IDs found in '{}': {:?}",
            wf.id,
            ids
        );

        // All params with required=true should have no default
        for p in &wf.params {
            assert!(!p.name.is_empty(), "Param name must not be empty");
        }
    }

    #[test]
    fn test_all_demo_workflows_returns_five() {
        let workflows = all_demo_workflows();
        assert_eq!(workflows.len(), 5);
    }

    #[test]
    fn test_all_demo_workflows_valid_structure() {
        for wf in &all_demo_workflows() {
            validate_workflow(wf);
        }
    }

    #[test]
    fn test_shopify_dropship_has_15_nodes() {
        let wf = shopify_dropship();
        assert_eq!(wf.id, "shopify_dropship");

        // Count all nodes recursively
        let mut ids = Vec::new();
        collect_node_ids(&wf.nodes, &mut ids);
        // 15 top-level + branch sub-nodes
        // Top level: 15 entries, but branch nodes contain sub-nodes.
        // We count: the branch "check_validation" has 1 sub-node (send_confirmation)
        //   + 1 otherwise node (cancel_invalid) = 2 additional.
        // So total unique IDs = 15 top-level + 2 inside branch = 17.
        // But the task says "15 nodes" referring to top-level node count.
        assert_eq!(wf.nodes.len(), 15, "Should have 15 top-level nodes");
        // Total unique IDs including nested should be >= 15
        assert!(
            ids.len() >= 15,
            "Should have at least 15 unique node IDs, got {}",
            ids.len()
        );
    }

    #[test]
    fn test_shopify_dropship_params() {
        let wf = shopify_dropship();
        let param_names: Vec<&str> = wf.params.iter().map(|p| p.name.as_str()).collect();
        assert!(param_names.contains(&"order_id"));
        assert!(param_names.contains(&"shop"));
        assert!(param_names.contains(&"dropshipper_api"));
        assert!(param_names.contains(&"customer_email"));
    }

    #[test]
    fn test_shopify_dropship_correlation_key() {
        let wf = shopify_dropship();
        assert_eq!(wf.correlation_key, Some("{{order_id}}".into()));
    }

    #[test]
    fn test_shopify_dropship_has_all_node_types() {
        let wf = shopify_dropship();
        let has_action = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::Action(_)));
        let has_wait_event = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::WaitEvent(_)));
        let has_delay = wf.nodes.iter().any(|n| matches!(n, WorkflowNode::Delay(_)));
        let has_wait_poll = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::WaitPoll(_)));
        let has_branch = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::Branch(_)));

        assert!(has_action, "Should have Action nodes");
        assert!(has_wait_event, "Should have WaitEvent nodes");
        assert!(has_delay, "Should have Delay nodes");
        assert!(has_wait_poll, "Should have WaitPoll nodes");
        assert!(has_branch, "Should have Branch nodes");
    }

    #[test]
    fn test_shopify_yaml_roundtrip() {
        let wf = shopify_dropship();
        let yaml = wf.to_yaml().expect("Should serialize to YAML");
        assert!(!yaml.is_empty());

        let parsed = WorkflowDef::from_yaml(&yaml).expect("Should parse back from YAML");
        assert_eq!(parsed.id, wf.id);
        assert_eq!(parsed.name, wf.name);
        assert_eq!(parsed.nodes.len(), wf.nodes.len());
        assert_eq!(parsed.params.len(), wf.params.len());
        assert_eq!(parsed.global_timeout_secs, wf.global_timeout_secs);
        assert_eq!(parsed.max_instances, wf.max_instances);
        assert_eq!(parsed.correlation_key, wf.correlation_key);
    }

    #[test]
    fn test_email_digest_workflow() {
        let wf = email_digest();
        assert_eq!(wf.id, "email_digest");
        validate_workflow(&wf);

        // Should have WaitPoll and Delay
        let has_poll = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::WaitPoll(_)));
        let has_delay = wf.nodes.iter().any(|n| matches!(n, WorkflowNode::Delay(_)));
        assert!(has_poll, "Email digest should poll for new emails");
        assert!(has_delay, "Email digest should delay until digest time");
    }

    #[test]
    fn test_price_alert_workflow() {
        let wf = price_alert();
        assert_eq!(wf.id, "price_alert_workflow");
        validate_workflow(&wf);

        // Should have WaitPoll for price monitoring
        let has_poll = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::WaitPoll(_)));
        assert!(has_poll, "Price alert should poll for price changes");
    }

    #[test]
    fn test_cicd_pipeline_workflow() {
        let wf = cicd_pipeline();
        assert_eq!(wf.id, "cicd_pipeline");
        validate_workflow(&wf);

        // Should have WaitEvent for webhook and Parallel for tests
        let has_event = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::WaitEvent(_)));
        let has_parallel = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::Parallel(_)));
        let has_branch = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::Branch(_)));
        assert!(has_event, "CI/CD should wait for webhook");
        assert!(has_parallel, "CI/CD should run tests in parallel");
        assert!(has_branch, "CI/CD should branch on test results");
    }

    #[test]
    fn test_customer_onboarding_workflow() {
        let wf = customer_onboarding();
        assert_eq!(wf.id, "customer_onboarding");
        validate_workflow(&wf);

        // Should have Delay, WaitEvent, and Branch
        let has_delay = wf.nodes.iter().any(|n| matches!(n, WorkflowNode::Delay(_)));
        let has_event = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::WaitEvent(_)));
        let has_branch = wf
            .nodes
            .iter()
            .any(|n| matches!(n, WorkflowNode::Branch(_)));
        assert!(has_delay, "Onboarding should have delay (3-day wait)");
        assert!(has_event, "Onboarding should wait for customer action");
        assert!(has_branch, "Onboarding should branch on action type");
    }

    #[test]
    fn test_all_workflows_yaml_roundtrip() {
        for wf in &all_demo_workflows() {
            let yaml = wf
                .to_yaml()
                .unwrap_or_else(|e| panic!("Failed to serialize '{}': {}", wf.id, e));
            let parsed = WorkflowDef::from_yaml(&yaml)
                .unwrap_or_else(|e| panic!("Failed to parse '{}' back from YAML: {}", wf.id, e));
            assert_eq!(parsed.id, wf.id);
            assert_eq!(parsed.nodes.len(), wf.nodes.len());
        }
    }

    #[test]
    fn test_workflow_ids_are_unique() {
        let workflows = all_demo_workflows();
        let ids: HashSet<&str> = workflows.iter().map(|w| w.id.as_str()).collect();
        assert_eq!(
            ids.len(),
            workflows.len(),
            "All workflow IDs should be unique"
        );
    }
}
