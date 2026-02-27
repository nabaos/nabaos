//! Demo chain definitions for testing and out-of-the-box functionality.
//! These chains demonstrate the chain DSL and can be loaded into the chain store.

/// Weather check chain — fetches weather data and notifies user.
pub const WEATHER_CHECK_YAML: &str = r#"
id: weather_check
name: Weather Check
description: Fetch weather for a city and notify the user
params:
  - name: city
    param_type: text
    description: City name
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/v1/{{city}}"
    output_key: weather_data
  - id: notify
    ability: notify.user
    args:
      message: "Weather in {{city}}: {{weather_data}}"
"#;

/// Price check chain — fetches current price of a trading instrument.
pub const PRICE_CHECK_YAML: &str = r#"
id: price_check
name: Price Check
description: Get current price of a trading instrument
params:
  - name: ticker
    param_type: text
    description: Stock/crypto ticker symbol
    required: true
steps:
  - id: fetch_price
    ability: trading.get_price
    args:
      symbol: "{{ticker}}"
    output_key: price
  - id: notify
    ability: notify.user
    args:
      message: "{{ticker}} price: {{price}}"
"#;

/// Sentiment monitor chain — fetch social post and analyze sentiment.
pub const SENTIMENT_MONITOR_YAML: &str = r#"
id: sentiment_monitor
name: Sentiment Monitor
description: Monitor social media for sentiment on a topic
params:
  - name: source
    param_type: text
    description: Social media URL or handle
    required: true
  - name: topic
    param_type: text
    description: Topic to analyze
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "{{source}}"
    output_key: raw_text
  - id: sentiment
    ability: nlp.sentiment
    args:
      text: "{{raw_text}}"
    output_key: sentiment_result
  - id: notify
    ability: notify.user
    args:
      message: "Sentiment for {{topic}}: {{sentiment_result}}"
"#;

/// Morning briefing chain — fetch weather, check calendar, check emails, summarize.
pub const MORNING_BRIEFING_YAML: &str = r#"
id: morning_briefing
name: Morning Briefing
description: Multi-step morning briefing that fetches weather, checks calendar and email, then summarizes
params:
  - name: city
    param_type: text
    description: City for weather forecast
    required: true
  - name: email_account
    param_type: text
    description: Email account identifier
    required: true
steps:
  - id: fetch_weather
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/v1/{{city}}"
    output_key: weather_data
  - id: check_calendar
    ability: calendar.list
    args:
      range: "today"
    output_key: calendar_events
  - id: check_email
    ability: data.fetch_url
    args:
      url: "https://mail.api/count/{{email_account}}"
    output_key: email_count
  - id: summarize
    ability: notify.user
    args:
      message: "Good morning! Weather in {{city}}: {{weather_data}}. Calendar: {{calendar_events}}. Unread emails: {{email_count}}."
"#;

/// Email triage chain — fetch emails, analyze sentiment, notify high priority.
pub const EMAIL_TRIAGE_YAML: &str = r#"
id: email_triage
name: Email Triage
description: Fetch emails, analyze sentiment, and notify about high priority messages
params:
  - name: email_account
    param_type: text
    description: Email account identifier
    required: true
steps:
  - id: fetch_emails
    ability: data.fetch_url
    args:
      url: "https://mail.api/inbox/{{email_account}}"
    output_key: raw_emails
  - id: analyze_sentiment
    ability: nlp.sentiment
    args:
      text: "{{raw_emails}}"
    output_key: email_sentiment
  - id: notify_priority
    ability: notify.user
    args:
      message: "Email triage for {{email_account}}: {{email_sentiment}}"
"#;

/// Research topic chain — fetch URL, summarize, store in memory.
pub const RESEARCH_TOPIC_YAML: &str = r#"
id: research_topic
name: Research Topic
description: Fetch a web page, summarize its content, and store the summary in memory
params:
  - name: url
    param_type: url
    description: URL to research
    required: true
  - name: topic
    param_type: text
    description: Topic label for memory storage
    required: true
steps:
  - id: fetch_page
    ability: browser.fetch
    args:
      url: "{{url}}"
    output_key: page_content
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{page_content}}"
    output_key: summary
  - id: store_result
    ability: memory.store
    args:
      key: "research_{{topic}}"
      value: "{{summary}}"
"#;

/// Content repurpose chain — read file, generate docs, write output.
pub const CONTENT_REPURPOSE_YAML: &str = r#"
id: content_repurpose
name: Content Repurpose
description: Read a file, generate a summary document, and write the output to a new file
params:
  - name: input_path
    param_type: text
    description: Path to the input file
    required: true
  - name: output_path
    param_type: text
    description: Path for the output file
    required: true
steps:
  - id: read_source
    ability: files.read
    args:
      path: "{{input_path}}"
    output_key: file_content
  - id: generate_summary
    ability: docs.generate
    args:
      content: "{{file_content}}"
      format: "summary"
    output_key: generated_doc
  - id: write_output
    ability: files.write
    args:
      path: "{{output_path}}"
      content: "{{generated_doc}}"
"#;

/// Price alert chain — get price, branch on threshold, notify if triggered.
pub const PRICE_ALERT_YAML: &str = r#"
id: price_alert
name: Price Alert
description: Get a trading price and notify the user if it crosses a threshold
params:
  - name: ticker
    param_type: text
    description: Stock or crypto ticker symbol
    required: true
  - name: threshold
    param_type: number
    description: Price threshold for the alert
    required: true
steps:
  - id: fetch_price
    ability: trading.get_price
    args:
      symbol: "{{ticker}}"
    output_key: current_price
  - id: check_threshold
    ability: flow.branch
    args:
      ref_key: "current_price"
      op: "greater_than"
      value: "{{threshold}}"
    output_key: threshold_exceeded
  - id: notify_alert
    ability: notify.user
    args:
      message: "ALERT: {{ticker}} is at {{current_price}} (threshold: {{threshold}})"
    condition:
      ref_key: threshold_exceeded
      op: equals
      value: "true"
"#;

/// Code review chain — read file, delegate to deep agent, notify result.
pub const CODE_REVIEW_YAML: &str = r#"
id: code_review
name: Code Review
description: Read a source file, delegate code review to a deep agent, and notify the result
params:
  - name: file_path
    param_type: text
    description: Path to the source file to review
    required: true
steps:
  - id: read_file
    ability: files.read
    args:
      path: "{{file_path}}"
    output_key: source_code
  - id: review
    ability: deep.delegate
    args:
      task: "Review this code for bugs, style issues, and improvements"
      content: "{{source_code}}"
      type: "code"
    output_key: review_result
  - id: notify_review
    ability: notify.user
    args:
      message: "Code review for {{file_path}}: {{review_result}}"
"#;

/// Travel planner chain — fetch destination info, analyze, generate report.
pub const TRAVEL_PLANNER_YAML: &str = r#"
id: travel_planner
name: Travel Planner
description: Fetch destination information, analyze travel data, and generate a travel report
params:
  - name: destination
    param_type: text
    description: Travel destination city or region
    required: true
steps:
  - id: fetch_info
    ability: browser.fetch
    args:
      url: "https://travel.api/info/{{destination}}"
    output_key: destination_info
  - id: analyze
    ability: data.analyze
    args:
      data: "{{destination_info}}"
      focus: "travel logistics, costs, and attractions"
    output_key: analysis
  - id: generate_report
    ability: docs.generate
    args:
      content: "{{analysis}}"
      format: "report"
    output_key: travel_report
"#;

/// Resume screen chain — read resume, delegate analysis, store result.
pub const RESUME_SCREEN_YAML: &str = r#"
id: resume_screen
name: Resume Screen
description: Read a resume file, delegate screening to a deep agent, and store the result
params:
  - name: resume_path
    param_type: text
    description: Path to the resume file
    required: true
  - name: job_title
    param_type: text
    description: Job title to screen against
    required: true
steps:
  - id: read_resume
    ability: files.read
    args:
      path: "{{resume_path}}"
    output_key: resume_text
  - id: screen
    ability: deep.delegate
    args:
      task: "Screen this resume for the role of {{job_title}}. Assess qualifications, experience, and fit."
      content: "{{resume_text}}"
      type: "analysis"
    output_key: screening_result
  - id: store_result
    ability: memory.store
    args:
      key: "resume_screen_{{job_title}}"
      value: "{{screening_result}}"
"#;

/// Data report chain — analyze data, generate report, notify completion.
pub const DATA_REPORT_YAML: &str = r#"
id: data_report
name: Data Report
description: Analyze a dataset, generate a formatted report, and notify upon completion
params:
  - name: data_source
    param_type: text
    description: Data source identifier or path
    required: true
steps:
  - id: analyze
    ability: data.analyze
    args:
      data: "{{data_source}}"
      focus: "trends, outliers, and key metrics"
    output_key: analysis_result
  - id: generate_report
    ability: docs.generate
    args:
      content: "{{analysis_result}}"
      format: "report"
    output_key: report_doc
  - id: notify_done
    ability: notify.user
    args:
      message: "Data report complete for {{data_source}}: {{report_doc}}"
"#;

/// Home routine chain — list calendar, notify schedule, send summary to channel.
pub const HOME_ROUTINE_YAML: &str = r#"
id: home_routine
name: Home Routine
description: List calendar events, notify the user of their schedule, and send a summary to a channel
params:
  - name: channel
    param_type: text
    description: Channel to send the summary to
    required: true
steps:
  - id: list_events
    ability: calendar.list
    args:
      range: "today"
    output_key: events
  - id: notify_schedule
    ability: notify.user
    args:
      message: "Today's schedule: {{events}}"
  - id: send_summary
    ability: channel.send
    args:
      channel: "{{channel}}"
      message: "Daily schedule summary: {{events}}"
"#;

// ─── Business chains ───

/// Invoice draft chain — generate a draft invoice for a client.
pub const INVOICE_DRAFT_YAML: &str = r#"
id: invoice_draft
name: Invoice Draft
description: Generate a draft invoice document for a client
params:
  - name: client_name
    param_type: text
    description: Client name
    required: true
  - name: amount
    param_type: number
    description: Invoice amount
    required: true
  - name: description
    param_type: text
    description: Service description
    required: true
steps:
  - id: generate
    ability: nlp.summarize
    args:
      text: "Invoice for {{client_name}}: {{description}} — Amount: ${{amount}}"
    output_key: invoice_doc
  - id: notify
    ability: notify.user
    args:
      message: "Invoice drafted for {{client_name}}: ${{amount}}"
"#;

/// Client report chain — analyze data and generate a client-facing report.
pub const CLIENT_REPORT_YAML: &str = r#"
id: client_report
name: Client Report
description: Analyze data and generate a client-facing report
params:
  - name: client_name
    param_type: text
    description: Client name
    required: true
  - name: data_url
    param_type: url
    description: URL of data source
    required: true
steps:
  - id: fetch_data
    ability: data.fetch_url
    args:
      url: "{{data_url}}"
    output_key: raw_data
  - id: analyze
    ability: data.analyze
    args:
      data: "{{raw_data}}"
    output_key: analysis
  - id: generate_report
    ability: docs.generate
    args:
      content: "Report for {{client_name}}: {{analysis}}"
      format: "report"
    output_key: report_doc
  - id: notify
    ability: notify.user
    args:
      message: "Report ready for {{client_name}}"
"#;

/// Meeting prep chain — check calendar and prepare an agenda.
pub const MEETING_PREP_YAML: &str = r#"
id: meeting_prep
name: Meeting Prep
description: Check calendar for upcoming meeting and prepare an agenda
params:
  - name: meeting_topic
    param_type: text
    description: Topic of the meeting
    required: true
steps:
  - id: check_calendar
    ability: calendar.list
    args:
      range: "today"
    output_key: events
  - id: search_notes
    ability: memory.search
    args:
      query: "{{meeting_topic}}"
    output_key: past_notes
  - id: generate_agenda
    ability: docs.generate
    args:
      content: "Meeting: {{meeting_topic}}. Calendar: {{events}}. Notes: {{past_notes}}"
      format: "summary"
    output_key: agenda
  - id: notify
    ability: notify.user
    args:
      message: "Meeting agenda for {{meeting_topic}}: {{agenda}}"
"#;

/// Lead qualify chain — research a sales prospect and generate a qualification summary.
pub const LEAD_QUALIFY_YAML: &str = r#"
id: lead_qualify
name: Lead Qualify
description: Research a sales prospect and generate a qualification summary
params:
  - name: company_name
    param_type: text
    description: Company to research
    required: true
  - name: contact_name
    param_type: text
    description: Contact person name
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://search.api/q={{company_name}}"
    output_key: company_info
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{company_info}}"
    output_key: summary
  - id: store
    ability: memory.store
    args:
      key: "lead_{{company_name}}"
      value: "Contact: {{contact_name}}. {{summary}}"
  - id: notify
    ability: notify.user
    args:
      message: "Lead qualified: {{company_name}} ({{contact_name}}). {{summary}}"
"#;

// ─── Content chains ───

/// SEO audit chain — fetch a web page and analyze it for SEO issues.
pub const SEO_AUDIT_YAML: &str = r#"
id: seo_audit
name: SEO Audit
description: Fetch a web page and analyze it for SEO issues
params:
  - name: url
    param_type: url
    description: URL to audit
    required: true
steps:
  - id: fetch
    ability: browser.fetch
    args:
      url: "{{url}}"
    output_key: page_content
  - id: analyze
    ability: deep.delegate
    args:
      task: "Analyze this page for SEO issues: title tags, meta descriptions, headings, content length, keyword density. Page content: {{page_content}}"
    output_key: seo_analysis
  - id: notify
    ability: notify.user
    args:
      message: "SEO Audit for {{url}}: {{seo_analysis}}"
"#;

/// Social post draft chain — generate a social media post about a topic.
pub const SOCIAL_POST_DRAFT_YAML: &str = r#"
id: social_post_draft
name: Social Post Draft
description: Generate a social media post about a topic
params:
  - name: topic
    param_type: text
    description: Topic to write about
    required: true
  - name: platform
    param_type: text
    description: Target platform (twitter, linkedin, instagram)
    required: true
steps:
  - id: research
    ability: memory.search
    args:
      query: "{{topic}}"
    output_key: context
  - id: generate
    ability: docs.generate
    args:
      content: "Write a {{platform}} post about: {{topic}}. Context: {{context}}"
      format: "summary"
    output_key: post_draft
  - id: notify
    ability: notify.user
    args:
      message: "Draft {{platform}} post: {{post_draft}}"
"#;

/// Course outline chain — generate a structured course outline for a topic.
pub const COURSE_OUTLINE_YAML: &str = r#"
id: course_outline
name: Course Outline
description: Generate a structured course outline for a topic
params:
  - name: topic
    param_type: text
    description: Course topic
    required: true
  - name: level
    param_type: text
    description: Difficulty level (beginner, intermediate, advanced)
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://search.api/q={{topic}}+course+outline"
    output_key: research_data
  - id: generate
    ability: deep.delegate
    args:
      task: "Create a {{level}} course outline for: {{topic}}. Reference: {{research_data}}"
    output_key: outline
  - id: store
    ability: memory.store
    args:
      key: "course_{{topic}}"
      value: "{{outline}}"
  - id: notify
    ability: notify.user
    args:
      message: "Course outline for {{topic}} ({{level}}): {{outline}}"
"#;

// ─── Finance chains ───

/// Portfolio summary chain — check prices for a portfolio and generate a summary.
pub const PORTFOLIO_SUMMARY_YAML: &str = r#"
id: portfolio_summary
name: Portfolio Summary
description: Check prices for a portfolio and generate a summary
params:
  - name: tickers
    param_type: text
    description: Comma-separated list of ticker symbols
    required: true
steps:
  - id: fetch_prices
    ability: deep.delegate
    args:
      task: "Get current prices for these tickers: {{tickers}}. Return a summary table."
    output_key: price_data
  - id: notify
    ability: notify.user
    args:
      message: "Portfolio summary: {{price_data}}"
"#;

/// Expense report chain — read expense data and generate a categorized report.
pub const EXPENSE_REPORT_YAML: &str = r#"
id: expense_report
name: Expense Report
description: Read expense data and generate a categorized report
params:
  - name: data_path
    param_type: text
    description: Path to expense data file
    required: true
steps:
  - id: read_data
    ability: files.read
    args:
      path: "{{data_path}}"
    output_key: expense_data
  - id: analyze
    ability: data.analyze
    args:
      data: "{{expense_data}}"
    output_key: analysis
  - id: generate
    ability: docs.generate
    args:
      content: "Expense Report: {{analysis}}"
      format: "report"
    output_key: report
  - id: notify
    ability: notify.user
    args:
      message: "Expense report ready: {{analysis}}"
"#;

/// Market brief chain — fetch financial news and generate a market brief.
pub const MARKET_BRIEF_YAML: &str = r#"
id: market_brief
name: Market Brief
description: Fetch financial news and generate a market brief
params:
  - name: sector
    param_type: text
    description: Market sector to focus on
    required: true
steps:
  - id: fetch_news
    ability: browser.fetch
    args:
      url: "https://news.api/finance/{{sector}}"
    output_key: news_data
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{news_data}}"
    output_key: summary
  - id: sentiment
    ability: nlp.sentiment
    args:
      text: "{{news_data}}"
    output_key: market_sentiment
  - id: notify
    ability: notify.user
    args:
      message: "Market brief ({{sector}}): {{summary}}. Sentiment: {{market_sentiment}}"
"#;

// ─── Research chains ───

/// Paper summary chain — fetch an academic paper URL and generate a structured summary.
pub const PAPER_SUMMARY_YAML: &str = r#"
id: paper_summary
name: Paper Summary
description: Fetch an academic paper URL and generate a structured summary
params:
  - name: url
    param_type: url
    description: URL of the paper
    required: true
steps:
  - id: fetch
    ability: browser.fetch
    args:
      url: "{{url}}"
    output_key: paper_text
  - id: summarize
    ability: deep.delegate
    args:
      task: "Summarize this academic paper. Include: title, authors, key findings, methodology, limitations. Paper: {{paper_text}}"
    output_key: summary
  - id: store
    ability: memory.store
    args:
      key: "paper_{{url}}"
      value: "{{summary}}"
  - id: notify
    ability: notify.user
    args:
      message: "Paper summary: {{summary}}"
"#;

/// Literature review chain — search for research on a topic and compile findings.
pub const LITERATURE_REVIEW_YAML: &str = r#"
id: literature_review
name: Literature Review
description: Search for research on a topic and compile findings
params:
  - name: topic
    param_type: text
    description: Research topic
    required: true
steps:
  - id: search
    ability: browser.fetch
    args:
      url: "https://scholar.api/search?q={{topic}}"
    output_key: search_results
  - id: analyze
    ability: deep.delegate
    args:
      task: "Review these research results for {{topic}}. Identify key themes, consensus, and open questions. Results: {{search_results}}"
    output_key: review
  - id: store
    ability: memory.store
    args:
      key: "lit_review_{{topic}}"
      value: "{{review}}"
  - id: notify
    ability: notify.user
    args:
      message: "Literature review for {{topic}}: {{review}}"
"#;

/// Study plan chain — generate a structured study plan for a topic.
pub const STUDY_PLAN_YAML: &str = r#"
id: study_plan
name: Study Plan
description: Generate a structured study plan for a topic
params:
  - name: topic
    param_type: text
    description: Subject to study
    required: true
  - name: duration_days
    param_type: number
    description: Number of days for the study plan
    required: true
steps:
  - id: research
    ability: memory.search
    args:
      query: "{{topic}}"
    output_key: existing_notes
  - id: generate
    ability: nlp.summarize
    args:
      text: "Study plan for {{topic}} over {{duration_days}} days. Existing knowledge: {{existing_notes}}"
    output_key: plan
  - id: notify
    ability: notify.user
    args:
      message: "Study plan ({{duration_days}} days): {{plan}}"
"#;

// ─── Operations chains ───

/// Sprint plan chain — read backlog and generate a sprint plan.
pub const SPRINT_PLAN_YAML: &str = r#"
id: sprint_plan
name: Sprint Plan
description: Read backlog and generate a sprint plan
params:
  - name: backlog_path
    param_type: text
    description: Path to backlog file
    required: true
  - name: sprint_days
    param_type: number
    description: Sprint duration in days
    required: true
steps:
  - id: read_backlog
    ability: files.read
    args:
      path: "{{backlog_path}}"
    output_key: backlog
  - id: plan
    ability: deep.delegate
    args:
      task: "Create a {{sprint_days}}-day sprint plan from this backlog. Prioritize by impact. Backlog: {{backlog}}"
    output_key: sprint
  - id: notify
    ability: notify.user
    args:
      message: "Sprint plan ({{sprint_days}} days): {{sprint}}"
"#;

/// Incident response chain — analyze system incident data and generate a response report.
pub const INCIDENT_RESPONSE_YAML: &str = r#"
id: incident_response
name: Incident Response
description: Analyze system incident data and generate a response report
params:
  - name: incident_desc
    param_type: text
    description: Incident description
    required: true
steps:
  - id: search_history
    ability: memory.search
    args:
      query: "incident {{incident_desc}}"
    output_key: past_incidents
  - id: analyze
    ability: deep.delegate
    args:
      task: "Analyze this incident: {{incident_desc}}. Past similar incidents: {{past_incidents}}. Provide root cause analysis and remediation steps."
    output_key: analysis
  - id: store
    ability: memory.store
    args:
      key: "incident_{{incident_desc}}"
      value: "{{analysis}}"
  - id: notify
    ability: notify.user
    args:
      message: "Incident analysis: {{analysis}}"
"#;

/// Deployment checklist chain — generate a deployment checklist for a service.
pub const DEPLOYMENT_CHECKLIST_YAML: &str = r#"
id: deployment_checklist
name: Deployment Checklist
description: Generate a deployment checklist for a service
params:
  - name: service_name
    param_type: text
    description: Service being deployed
    required: true
  - name: version
    param_type: text
    description: Version being deployed
    required: true
steps:
  - id: search_config
    ability: memory.search
    args:
      query: "deployment {{service_name}}"
    output_key: deploy_history
  - id: generate
    ability: docs.generate
    args:
      content: "Deployment checklist for {{service_name}} v{{version}}. History: {{deploy_history}}"
      format: "summary"
    output_key: checklist
  - id: notify
    ability: notify.user
    args:
      message: "Deploy checklist for {{service_name}} v{{version}}: {{checklist}}"
"#;

// ─── Domain chains ───

/// Property comparison chain — research and compare real estate properties.
pub const PROPERTY_COMP_YAML: &str = r#"
id: property_comp
name: Property Comparison
description: Research and compare real estate properties
params:
  - name: location
    param_type: text
    description: Location to search
    required: true
  - name: budget
    param_type: number
    description: Maximum budget
    required: true
steps:
  - id: search
    ability: browser.fetch
    args:
      url: "https://realestate.api/search?location={{location}}&max_price={{budget}}"
    output_key: listings
  - id: analyze
    ability: deep.delegate
    args:
      task: "Compare these property listings for {{location}} under ${{budget}}. Rank by value. Listings: {{listings}}"
    output_key: comparison
  - id: notify
    ability: notify.user
    args:
      message: "Property comparison ({{location}}, <${{budget}}): {{comparison}}"
"#;

/// Case brief chain — research legal precedents and generate a case brief.
pub const CASE_BRIEF_YAML: &str = r#"
id: case_brief
name: Case Brief
description: Research legal precedents and generate a case brief
params:
  - name: case_topic
    param_type: text
    description: Legal topic or case reference
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://legal.api/search?q={{case_topic}}"
    output_key: case_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Generate a case brief for: {{case_topic}}. Include relevant precedents, key arguments, and potential outcomes. Research: {{case_data}}"
    output_key: brief
  - id: store
    ability: memory.store
    args:
      key: "case_{{case_topic}}"
      value: "{{brief}}"
  - id: notify
    ability: notify.user
    args:
      message: "Case brief for {{case_topic}}: {{brief}}"
"#;

/// Inventory alert chain — check inventory levels and alert on low stock items.
pub const INVENTORY_ALERT_YAML: &str = r#"
id: inventory_alert
name: Inventory Alert
description: Check inventory levels and alert on low stock items
params:
  - name: data_path
    param_type: text
    description: Path to inventory data file
    required: true
  - name: threshold
    param_type: number
    description: Low stock threshold
    required: true
steps:
  - id: read_inventory
    ability: files.read
    args:
      path: "{{data_path}}"
    output_key: inventory_data
  - id: analyze
    ability: data.analyze
    args:
      data: "{{inventory_data}}"
    output_key: analysis
  - id: notify
    ability: notify.user
    args:
      message: "Inventory check (threshold: {{threshold}}): {{analysis}}"
"#;

/// Ticket triage chain — analyze incoming support tickets and prioritize by urgency.
pub const TICKET_TRIAGE_YAML: &str = r#"
id: ticket_triage
name: Ticket Triage
description: Analyze incoming support tickets and prioritize by urgency
params:
  - name: ticket_source
    param_type: url
    description: URL of ticket feed
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "{{ticket_source}}"
    output_key: tickets
  - id: sentiment
    ability: nlp.sentiment
    args:
      text: "{{tickets}}"
    output_key: urgency
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{tickets}}"
    output_key: summary
  - id: notify
    ability: notify.user
    args:
      message: "Ticket triage: {{summary}}. Urgency: {{urgency}}"
"#;

// ── HR & Talent chains ──────────────────────────────────────

pub const ONBOARDING_CHECKLIST_YAML: &str = r#"
id: onboarding_checklist
name: Onboarding Checklist
description: Generate an onboarding checklist for a new hire
params:
  - name: employee_name
    param_type: text
    description: New hire name
    required: true
  - name: role
    param_type: text
    description: Job role
    required: true
steps:
  - id: search_templates
    ability: memory.search
    args:
      query: "onboarding {{role}}"
    output_key: templates
  - id: generate
    ability: deep.delegate
    args:
      task: "Generate onboarding checklist for {{employee_name}} ({{role}}). Reference templates: {{templates}}"
    output_key: checklist
  - id: notify
    ability: notify.user
    args:
      message: "Onboarding checklist for {{employee_name}}: {{checklist}}"
"#;

pub const ENGAGEMENT_SURVEY_YAML: &str = r#"
id: engagement_survey
name: Engagement Survey Analysis
description: Analyze employee engagement survey results
params:
  - name: data_path
    param_type: text
    description: Path to survey data file
    required: true
steps:
  - id: read_data
    ability: files.read
    args:
      path: "{{data_path}}"
    output_key: survey_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Analyze engagement survey results. Identify top concerns, department trends, and actionable recommendations. Data: {{survey_data}}"
    output_key: analysis
  - id: notify
    ability: notify.user
    args:
      message: "Engagement survey analysis: {{analysis}}"
"#;

pub const PERFORMANCE_REVIEW_YAML: &str = r#"
id: performance_review
name: Performance Review Prep
description: Prepare performance review materials from notes
params:
  - name: employee_name
    param_type: text
    description: Employee name
    required: true
steps:
  - id: search_notes
    ability: memory.search
    args:
      query: "performance {{employee_name}}"
    output_key: notes
  - id: generate
    ability: deep.delegate
    args:
      task: "Draft a performance review summary for {{employee_name}} based on these notes: {{notes}}"
    output_key: review
  - id: notify
    ability: notify.user
    args:
      message: "Performance review draft for {{employee_name}}: {{review}}"
"#;

// ── Finance & Insurance chains ──────────────────────────────

pub const TAX_FILING_PREP_YAML: &str = r#"
id: tax_filing_prep
name: Tax Filing Preparation
description: Prepare tax filing summary from financial data
params:
  - name: data_path
    param_type: text
    description: Path to financial records
    required: true
  - name: jurisdiction
    param_type: text
    description: Tax jurisdiction
    required: true
steps:
  - id: read_data
    ability: files.read
    args:
      path: "{{data_path}}"
    output_key: financial_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Prepare tax filing summary for {{jurisdiction}}. Categorize income, deductions, and credits. Data: {{financial_data}}"
    output_key: tax_summary
  - id: notify
    ability: notify.user
    args:
      message: "Tax filing prep ({{jurisdiction}}): {{tax_summary}}"
"#;

pub const AUDIT_TRAIL_YAML: &str = r#"
id: audit_trail
name: Audit Trail Report
description: Generate an audit trail report from transaction data
params:
  - name: data_path
    param_type: text
    description: Path to transaction log
    required: true
steps:
  - id: read_log
    ability: files.read
    args:
      path: "{{data_path}}"
    output_key: transactions
  - id: analyze
    ability: data.analyze
    args:
      data: "{{transactions}}"
    output_key: analysis
  - id: notify
    ability: notify.user
    args:
      message: "Audit trail report: {{analysis}}"
"#;

pub const BID_COMPARISON_YAML: &str = r#"
id: bid_comparison
name: Bid Comparison
description: Compare vendor bids against evaluation criteria
params:
  - name: bid_data_path
    param_type: text
    description: Path to bid data file
    required: true
  - name: criteria
    param_type: text
    description: Evaluation criteria
    required: true
steps:
  - id: read_bids
    ability: files.read
    args:
      path: "{{bid_data_path}}"
    output_key: bids
  - id: evaluate
    ability: deep.delegate
    args:
      task: "Compare these vendor bids against criteria: {{criteria}}. Rank by value and flag risks. Bids: {{bids}}"
    output_key: comparison
  - id: notify
    ability: notify.user
    args:
      message: "Bid comparison: {{comparison}}"
"#;

pub const LOAN_COMPARISON_YAML: &str = r#"
id: loan_comparison
name: Loan Product Comparison
description: Compare loan products across lenders
params:
  - name: loan_amount
    param_type: number
    description: Loan amount requested
    required: true
  - name: loan_type
    param_type: text
    description: Type of loan (mortgage, business, personal)
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://rates.api/compare?type={{loan_type}}&amount={{loan_amount}}"
    output_key: rates_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Compare {{loan_type}} loan products for ${{loan_amount}}. Include rates, terms, fees. Data: {{rates_data}}"
    output_key: comparison
  - id: notify
    ability: notify.user
    args:
      message: "Loan comparison ({{loan_type}}, ${{loan_amount}}): {{comparison}}"
"#;

// ── Healthcare chains ──────────────────────────────────────

pub const PATIENT_TRIAGE_YAML: &str = r#"
id: patient_triage
name: Patient Intake Triage
description: Triage patient intake forms and flag high-risk cases
params:
  - name: intake_data
    param_type: text
    description: Patient intake form data
    required: true
steps:
  - id: analyze
    ability: deep.delegate
    args:
      task: "Triage this patient intake. Flag high-risk indicators and suggest priority level. Data: {{intake_data}}"
    output_key: triage_result
  - id: notify
    ability: notify.user
    args:
      message: "Patient triage result: {{triage_result}}"
"#;

pub const CLINICAL_SUMMARY_YAML: &str = r#"
id: clinical_summary
name: Clinical Summary
description: Generate a clinical summary from session notes
params:
  - name: session_notes
    param_type: text
    description: Session or visit notes
    required: true
  - name: patient_id
    param_type: text
    description: Patient identifier
    required: true
steps:
  - id: search_history
    ability: memory.search
    args:
      query: "patient {{patient_id}}"
    output_key: history
  - id: generate
    ability: deep.delegate
    args:
      task: "Generate clinical summary for patient {{patient_id}}. Current notes: {{session_notes}}. History: {{history}}"
    output_key: summary
  - id: store
    ability: memory.store
    args:
      key: "patient_{{patient_id}}"
      value: "{{summary}}"
  - id: notify
    ability: notify.user
    args:
      message: "Clinical summary ({{patient_id}}): {{summary}}"
"#;

pub const DRUG_INTERACTION_YAML: &str = r#"
id: drug_interaction
name: Drug Interaction Check
description: Check for drug interactions in a medication list
params:
  - name: medications
    param_type: text
    description: Comma-separated list of medications
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://rxnav.api/interaction?drugs={{medications}}"
    output_key: interaction_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Analyze potential drug interactions for: {{medications}}. Flag severity levels. Data: {{interaction_data}}"
    output_key: analysis
  - id: notify
    ability: notify.user
    args:
      message: "Drug interaction check: {{analysis}}"
"#;

// ── Engineering & Manufacturing chains ──────────────────────

pub const SITE_INSPECTION_YAML: &str = r#"
id: site_inspection
name: Site Inspection Report
description: Generate a site inspection report from field notes
params:
  - name: site_name
    param_type: text
    description: Site or project name
    required: true
  - name: notes
    param_type: text
    description: Field inspection notes
    required: true
steps:
  - id: search_history
    ability: memory.search
    args:
      query: "inspection {{site_name}}"
    output_key: past_inspections
  - id: generate
    ability: deep.delegate
    args:
      task: "Generate site inspection report for {{site_name}}. Notes: {{notes}}. Past inspections: {{past_inspections}}"
    output_key: report
  - id: store
    ability: memory.store
    args:
      key: "inspection_{{site_name}}"
      value: "{{report}}"
  - id: notify
    ability: notify.user
    args:
      message: "Site inspection ({{site_name}}): {{report}}"
"#;

pub const MAINTENANCE_SCHEDULE_YAML: &str = r#"
id: maintenance_schedule
name: Maintenance Schedule
description: Generate equipment maintenance schedule
params:
  - name: equipment_list_path
    param_type: text
    description: Path to equipment inventory file
    required: true
steps:
  - id: read_equipment
    ability: files.read
    args:
      path: "{{equipment_list_path}}"
    output_key: equipment_data
  - id: generate
    ability: deep.delegate
    args:
      task: "Generate preventive maintenance schedule from this equipment list. Flag overdue items. Data: {{equipment_data}}"
    output_key: schedule
  - id: notify
    ability: notify.user
    args:
      message: "Maintenance schedule: {{schedule}}"
"#;

pub const EQUIPMENT_STATUS_YAML: &str = r#"
id: equipment_status
name: Equipment Status Check
description: Check equipment status and generate OEE report
params:
  - name: data_url
    param_type: url
    description: URL of equipment sensor data
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "{{data_url}}"
    output_key: sensor_data
  - id: analyze
    ability: data.analyze
    args:
      data: "{{sensor_data}}"
    output_key: analysis
  - id: notify
    ability: notify.user
    args:
      message: "Equipment status: {{analysis}}"
"#;

// ── Media & PR chains ──────────────────────────────────────

pub const PRESS_RELEASE_YAML: &str = r#"
id: press_release
name: Press Release Draft
description: Draft a press release from key facts
params:
  - name: headline
    param_type: text
    description: Press release headline
    required: true
  - name: key_facts
    param_type: text
    description: Key facts and quotes
    required: true
steps:
  - id: generate
    ability: deep.delegate
    args:
      task: "Draft a professional press release. Headline: {{headline}}. Key facts: {{key_facts}}"
    output_key: draft
  - id: notify
    ability: notify.user
    args:
      message: "Press release draft: {{draft}}"
"#;

pub const MEDIA_MONITOR_YAML: &str = r#"
id: media_monitor
name: Media Monitor
description: Monitor news mentions and compile a clip report
params:
  - name: search_term
    param_type: text
    description: Brand or topic to monitor
    required: true
steps:
  - id: fetch
    ability: browser.fetch
    args:
      url: "https://news.api/search?q={{search_term}}"
    output_key: articles
  - id: sentiment
    ability: nlp.sentiment
    args:
      text: "{{articles}}"
    output_key: sentiment
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{articles}}"
    output_key: summary
  - id: notify
    ability: notify.user
    args:
      message: "Media monitor ({{search_term}}): {{summary}}. Sentiment: {{sentiment}}"
"#;

pub const PODCAST_NOTES_YAML: &str = r#"
id: podcast_notes
name: Podcast Show Notes
description: Generate show notes from a podcast transcript
params:
  - name: transcript_path
    param_type: text
    description: Path to transcript file
    required: true
  - name: episode_title
    param_type: text
    description: Episode title
    required: true
steps:
  - id: read_transcript
    ability: files.read
    args:
      path: "{{transcript_path}}"
    output_key: transcript
  - id: generate
    ability: deep.delegate
    args:
      task: "Generate podcast show notes for '{{episode_title}}'. Include chapter markers, key topics, guest mentions, and timestamps. Transcript: {{transcript}}"
    output_key: notes
  - id: notify
    ability: notify.user
    args:
      message: "Show notes for {{episode_title}}: {{notes}}"
"#;

// ── Government & Compliance chains ──────────────────────────

pub const REGULATION_MONITOR_YAML: &str = r#"
id: regulation_monitor
name: Regulation Monitor
description: Monitor regulatory changes and summarize updates
params:
  - name: jurisdiction
    param_type: text
    description: Regulatory jurisdiction
    required: true
  - name: sector
    param_type: text
    description: Industry sector
    required: true
steps:
  - id: fetch
    ability: browser.fetch
    args:
      url: "https://regulatory.api/updates?jurisdiction={{jurisdiction}}&sector={{sector}}"
    output_key: updates
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{updates}}"
    output_key: summary
  - id: store
    ability: memory.store
    args:
      key: "reg_{{jurisdiction}}_{{sector}}"
      value: "{{summary}}"
  - id: notify
    ability: notify.user
    args:
      message: "Regulatory update ({{jurisdiction}}, {{sector}}): {{summary}}"
"#;

pub const POLICY_BRIEF_YAML: &str = r#"
id: policy_brief
name: Policy Brief
description: Generate a policy briefing note with pros and cons
params:
  - name: topic
    param_type: text
    description: Policy topic
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://policy.api/research?topic={{topic}}"
    output_key: research_data
  - id: generate
    ability: deep.delegate
    args:
      task: "Draft a policy brief on: {{topic}}. Include background, stakeholder analysis, pros/cons, and recommendation. Research: {{research_data}}"
    output_key: brief
  - id: notify
    ability: notify.user
    args:
      message: "Policy brief ({{topic}}): {{brief}}"
"#;

pub const COMPLIANCE_AUDIT_YAML: &str = r#"
id: compliance_audit
name: Compliance Audit
description: Run a compliance check against regulatory requirements
params:
  - name: framework
    param_type: text
    description: Compliance framework (GDPR, SOX, HIPAA, etc.)
    required: true
  - name: data_path
    param_type: text
    description: Path to data inventory or controls document
    required: true
steps:
  - id: read_data
    ability: files.read
    args:
      path: "{{data_path}}"
    output_key: controls_data
  - id: audit
    ability: deep.delegate
    args:
      task: "Audit these controls against {{framework}} requirements. Flag gaps and suggest remediations. Controls: {{controls_data}}"
    output_key: audit_result
  - id: notify
    ability: notify.user
    args:
      message: "Compliance audit ({{framework}}): {{audit_result}}"
"#;

// ── NGO & Development chains ──────────────────────────────

pub const DONOR_REPORT_YAML: &str = r#"
id: donor_report
name: Donor Impact Report
description: Generate a donor impact report from program data
params:
  - name: program_name
    param_type: text
    description: Program or project name
    required: true
  - name: data_path
    param_type: text
    description: Path to program data file
    required: true
steps:
  - id: read_data
    ability: files.read
    args:
      path: "{{data_path}}"
    output_key: program_data
  - id: generate
    ability: deep.delegate
    args:
      task: "Generate a donor impact report for {{program_name}}. Include beneficiary numbers, outcomes, and stories. Data: {{program_data}}"
    output_key: report
  - id: notify
    ability: notify.user
    args:
      message: "Donor report ({{program_name}}): {{report}}"
"#;

pub const GRANT_PREP_YAML: &str = r#"
id: grant_prep
name: Grant Application Prep
description: Research grant opportunities and prepare application materials
params:
  - name: organization
    param_type: text
    description: Organization name
    required: true
  - name: focus_area
    param_type: text
    description: Program focus area
    required: true
steps:
  - id: search
    ability: browser.fetch
    args:
      url: "https://grants.api/search?area={{focus_area}}"
    output_key: opportunities
  - id: analyze
    ability: deep.delegate
    args:
      task: "Review these grant opportunities for {{organization}} (focus: {{focus_area}}). Match eligibility and rank by fit. Opportunities: {{opportunities}}"
    output_key: analysis
  - id: store
    ability: memory.store
    args:
      key: "grants_{{focus_area}}"
      value: "{{analysis}}"
  - id: notify
    ability: notify.user
    args:
      message: "Grant opportunities ({{focus_area}}): {{analysis}}"
"#;

// ── Logistics & Supply Chain chains ──────────────────────────

pub const SHIPMENT_TRACKER_YAML: &str = r#"
id: shipment_tracker
name: Shipment Tracker
description: Track shipments and generate status reports
params:
  - name: tracking_url
    param_type: url
    description: Shipment tracking API URL
    required: true
steps:
  - id: fetch
    ability: data.fetch_url
    args:
      url: "{{tracking_url}}"
    output_key: tracking_data
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{tracking_data}}"
    output_key: summary
  - id: notify
    ability: notify.user
    args:
      message: "Shipment status: {{summary}}"
"#;

pub const ROUTE_OPTIMIZER_YAML: &str = r#"
id: route_optimizer
name: Route Optimizer
description: Optimize delivery routes based on stops and constraints
params:
  - name: stops
    param_type: text
    description: Comma-separated list of delivery stops
    required: true
  - name: vehicle_capacity
    param_type: number
    description: Vehicle capacity in units
    required: true
steps:
  - id: optimize
    ability: deep.delegate
    args:
      task: "Optimize delivery route for these stops with vehicle capacity {{vehicle_capacity}}: {{stops}}. Minimize distance and respect capacity."
    output_key: optimized_route
  - id: notify
    ability: notify.user
    args:
      message: "Optimized route (capacity: {{vehicle_capacity}}): {{optimized_route}}"
"#;

pub const CUSTOMS_CLEARANCE_YAML: &str = r#"
id: customs_clearance
name: Customs Clearance Check
description: Research customs requirements for an import/export shipment
params:
  - name: product
    param_type: text
    description: Product description
    required: true
  - name: destination
    param_type: text
    description: Destination country
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://customs.api/lookup?product={{product}}&country={{destination}}"
    output_key: customs_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Summarize customs requirements for {{product}} to {{destination}}. Include HS codes, duties, and required documents. Data: {{customs_data}}"
    output_key: requirements
  - id: notify
    ability: notify.user
    args:
      message: "Customs clearance ({{product}} → {{destination}}): {{requirements}}"
"#;

// ── Agriculture chains ──────────────────────────────────────

pub const CROP_MONITOR_YAML: &str = r#"
id: crop_monitor
name: Crop Monitor
description: Monitor crop conditions and generate field report
params:
  - name: crop_type
    param_type: text
    description: Type of crop
    required: true
  - name: location
    param_type: text
    description: Field location
    required: true
steps:
  - id: fetch_weather
    ability: data.fetch_url
    args:
      url: "https://api.weather.com/v1/{{location}}"
    output_key: weather_data
  - id: fetch_soil
    ability: browser.fetch
    args:
      url: "https://soil.api/conditions?location={{location}}"
    output_key: soil_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Generate crop report for {{crop_type}} at {{location}}. Weather: {{weather_data}}. Soil: {{soil_data}}. Include irrigation and pest risk recommendations."
    output_key: report
  - id: notify
    ability: notify.user
    args:
      message: "Crop monitor ({{crop_type}}, {{location}}): {{report}}"
"#;

pub const MARKET_PRICE_COMPARE_YAML: &str = r#"
id: market_price_compare
name: Market Price Comparison
description: Compare commodity prices across markets
params:
  - name: commodity
    param_type: text
    description: Commodity name
    required: true
steps:
  - id: fetch
    ability: browser.fetch
    args:
      url: "https://commodity.api/prices?q={{commodity}}"
    output_key: price_data
  - id: analyze
    ability: data.analyze
    args:
      data: "{{price_data}}"
    output_key: analysis
  - id: notify
    ability: notify.user
    args:
      message: "Market prices ({{commodity}}): {{analysis}}"
"#;

// ── Creative & Design chains ──────────────────────────────

pub const TREND_RESEARCH_YAML: &str = r#"
id: trend_research
name: Trend Research
description: Research current trends in a creative field
params:
  - name: field
    param_type: text
    description: Creative field (fashion, design, music, etc.)
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://trends.api/search?field={{field}}"
    output_key: trend_data
  - id: summarize
    ability: nlp.summarize
    args:
      text: "{{trend_data}}"
    output_key: summary
  - id: store
    ability: memory.store
    args:
      key: "trends_{{field}}"
      value: "{{summary}}"
  - id: notify
    ability: notify.user
    args:
      message: "Trend report ({{field}}): {{summary}}"
"#;

pub const SPEC_SHEET_YAML: &str = r#"
id: spec_sheet
name: Technical Spec Sheet
description: Generate a technical specification sheet from design notes
params:
  - name: product_name
    param_type: text
    description: Product or design name
    required: true
  - name: notes
    param_type: text
    description: Design notes and requirements
    required: true
steps:
  - id: generate
    ability: deep.delegate
    args:
      task: "Generate a technical spec sheet for {{product_name}}. Include dimensions, materials, tolerances, and manufacturing notes. Input: {{notes}}"
    output_key: spec
  - id: notify
    ability: notify.user
    args:
      message: "Spec sheet ({{product_name}}): {{spec}}"
"#;

// ── Consulting chains ──────────────────────────────────────

pub const COMPETITIVE_ANALYSIS_YAML: &str = r#"
id: competitive_analysis
name: Competitive Analysis
description: Research competitors and generate a battlecard
params:
  - name: company
    param_type: text
    description: Company to analyze against
    required: true
  - name: sector
    param_type: text
    description: Industry sector
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://search.api/q={{company}}+{{sector}}"
    output_key: research_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Generate competitive analysis battlecard for {{company}} in {{sector}}. Include strengths, weaknesses, pricing, and market position. Research: {{research_data}}"
    output_key: battlecard
  - id: store
    ability: memory.store
    args:
      key: "competitor_{{company}}"
      value: "{{battlecard}}"
  - id: notify
    ability: notify.user
    args:
      message: "Competitive analysis ({{company}}): {{battlecard}}"
"#;

pub const DUE_DILIGENCE_YAML: &str = r#"
id: due_diligence
name: Due Diligence Summary
description: Compile due diligence research on a target company
params:
  - name: target_company
    param_type: text
    description: Target company name
    required: true
steps:
  - id: research
    ability: browser.fetch
    args:
      url: "https://search.api/q={{target_company}}+financials+news"
    output_key: public_data
  - id: analyze
    ability: deep.delegate
    args:
      task: "Compile due diligence summary for {{target_company}}. Cover financials, management, market position, risks, and recent news. Data: {{public_data}}"
    output_key: dd_summary
  - id: store
    ability: memory.store
    args:
      key: "dd_{{target_company}}"
      value: "{{dd_summary}}"
  - id: notify
    ability: notify.user
    args:
      message: "Due diligence ({{target_company}}): {{dd_summary}}"
"#;

/// Load all demo chains into a chain store.
pub fn load_demo_chains(store: &super::store::ChainStore) -> crate::core::error::Result<usize> {
    let yamls = [
        // Original 13
        WEATHER_CHECK_YAML,
        PRICE_CHECK_YAML,
        SENTIMENT_MONITOR_YAML,
        MORNING_BRIEFING_YAML,
        EMAIL_TRIAGE_YAML,
        RESEARCH_TOPIC_YAML,
        CONTENT_REPURPOSE_YAML,
        PRICE_ALERT_YAML,
        CODE_REVIEW_YAML,
        TRAVEL_PLANNER_YAML,
        RESUME_SCREEN_YAML,
        DATA_REPORT_YAML,
        HOME_ROUTINE_YAML,
        // New 20
        INVOICE_DRAFT_YAML,
        CLIENT_REPORT_YAML,
        MEETING_PREP_YAML,
        LEAD_QUALIFY_YAML,
        SEO_AUDIT_YAML,
        SOCIAL_POST_DRAFT_YAML,
        COURSE_OUTLINE_YAML,
        PORTFOLIO_SUMMARY_YAML,
        EXPENSE_REPORT_YAML,
        MARKET_BRIEF_YAML,
        PAPER_SUMMARY_YAML,
        LITERATURE_REVIEW_YAML,
        STUDY_PLAN_YAML,
        SPRINT_PLAN_YAML,
        INCIDENT_RESPONSE_YAML,
        DEPLOYMENT_CHECKLIST_YAML,
        PROPERTY_COMP_YAML,
        CASE_BRIEF_YAML,
        INVENTORY_ALERT_YAML,
        TICKET_TRIAGE_YAML,
        // New 30 — Round 2: 100 user type coverage
        ONBOARDING_CHECKLIST_YAML,
        ENGAGEMENT_SURVEY_YAML,
        PERFORMANCE_REVIEW_YAML,
        TAX_FILING_PREP_YAML,
        AUDIT_TRAIL_YAML,
        BID_COMPARISON_YAML,
        LOAN_COMPARISON_YAML,
        PATIENT_TRIAGE_YAML,
        CLINICAL_SUMMARY_YAML,
        DRUG_INTERACTION_YAML,
        SITE_INSPECTION_YAML,
        MAINTENANCE_SCHEDULE_YAML,
        EQUIPMENT_STATUS_YAML,
        PRESS_RELEASE_YAML,
        MEDIA_MONITOR_YAML,
        PODCAST_NOTES_YAML,
        REGULATION_MONITOR_YAML,
        POLICY_BRIEF_YAML,
        COMPLIANCE_AUDIT_YAML,
        DONOR_REPORT_YAML,
        GRANT_PREP_YAML,
        SHIPMENT_TRACKER_YAML,
        ROUTE_OPTIMIZER_YAML,
        CUSTOMS_CLEARANCE_YAML,
        CROP_MONITOR_YAML,
        MARKET_PRICE_COMPARE_YAML,
        TREND_RESEARCH_YAML,
        SPEC_SHEET_YAML,
        COMPETITIVE_ANALYSIS_YAML,
        DUE_DILIGENCE_YAML,
    ];
    let mut loaded = 0;

    for yaml in &yamls {
        let chain = super::dsl::ChainDef::from_yaml(yaml)?;
        chain.check()?;
        store.store(&chain)?;
        loaded += 1;
    }

    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::dsl::ChainDef;
    use crate::chain::executor::ChainExecutor;
    use crate::chain::store::ChainStore;
    use crate::runtime::host_functions::AbilityRegistry;
    use crate::runtime::manifest::AgentManifest;
    use crate::runtime::receipt::ReceiptSigner;
    use std::collections::HashMap;

    fn full_manifest() -> AgentManifest {
        AgentManifest {
            name: "test-agent".into(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec![
                "data.fetch_url".into(),
                "notify.user".into(),
                "trading.get_price".into(),
                "nlp.sentiment".into(),
                "flow.stop".into(),
            ],
            memory_limit_mb: 64,
            fuel_limit: 1_000_000,
            kv_namespace: None,
            author: None,
            intent_filters: vec![],
            resources: None,
            background: false,
            subscriptions: vec![],
            data_namespace: None,
            signature: None,
        }
    }

    #[test]
    fn test_all_demo_chains_parse() {
        let yamls = [
            // Original 13
            WEATHER_CHECK_YAML,
            PRICE_CHECK_YAML,
            SENTIMENT_MONITOR_YAML,
            MORNING_BRIEFING_YAML,
            EMAIL_TRIAGE_YAML,
            RESEARCH_TOPIC_YAML,
            CONTENT_REPURPOSE_YAML,
            PRICE_ALERT_YAML,
            CODE_REVIEW_YAML,
            TRAVEL_PLANNER_YAML,
            RESUME_SCREEN_YAML,
            DATA_REPORT_YAML,
            HOME_ROUTINE_YAML,
            // New 20
            INVOICE_DRAFT_YAML,
            CLIENT_REPORT_YAML,
            MEETING_PREP_YAML,
            LEAD_QUALIFY_YAML,
            SEO_AUDIT_YAML,
            SOCIAL_POST_DRAFT_YAML,
            COURSE_OUTLINE_YAML,
            PORTFOLIO_SUMMARY_YAML,
            EXPENSE_REPORT_YAML,
            MARKET_BRIEF_YAML,
            PAPER_SUMMARY_YAML,
            LITERATURE_REVIEW_YAML,
            STUDY_PLAN_YAML,
            SPRINT_PLAN_YAML,
            INCIDENT_RESPONSE_YAML,
            DEPLOYMENT_CHECKLIST_YAML,
            PROPERTY_COMP_YAML,
            CASE_BRIEF_YAML,
            INVENTORY_ALERT_YAML,
            TICKET_TRIAGE_YAML,
            // New 30 — Round 2: 100 user type coverage
            ONBOARDING_CHECKLIST_YAML,
            ENGAGEMENT_SURVEY_YAML,
            PERFORMANCE_REVIEW_YAML,
            TAX_FILING_PREP_YAML,
            AUDIT_TRAIL_YAML,
            BID_COMPARISON_YAML,
            LOAN_COMPARISON_YAML,
            PATIENT_TRIAGE_YAML,
            CLINICAL_SUMMARY_YAML,
            DRUG_INTERACTION_YAML,
            SITE_INSPECTION_YAML,
            MAINTENANCE_SCHEDULE_YAML,
            EQUIPMENT_STATUS_YAML,
            PRESS_RELEASE_YAML,
            MEDIA_MONITOR_YAML,
            PODCAST_NOTES_YAML,
            REGULATION_MONITOR_YAML,
            POLICY_BRIEF_YAML,
            COMPLIANCE_AUDIT_YAML,
            DONOR_REPORT_YAML,
            GRANT_PREP_YAML,
            SHIPMENT_TRACKER_YAML,
            ROUTE_OPTIMIZER_YAML,
            CUSTOMS_CLEARANCE_YAML,
            CROP_MONITOR_YAML,
            MARKET_PRICE_COMPARE_YAML,
            TREND_RESEARCH_YAML,
            SPEC_SHEET_YAML,
            COMPETITIVE_ANALYSIS_YAML,
            DUE_DILIGENCE_YAML,
        ];
        for yaml in &yamls {
            let chain = ChainDef::from_yaml(yaml).unwrap();
            chain.check().unwrap();
        }
    }

    #[test]
    fn test_load_demo_chains_into_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChainStore::open(&dir.path().join("chains.db")).unwrap();

        let loaded = load_demo_chains(&store).unwrap();
        assert_eq!(loaded, 63);

        // Verify all chains are stored
        let chains = store.list(100).unwrap();
        assert_eq!(chains.len(), 63);

        // Verify specific chain lookup
        let weather = store.lookup("weather_check").unwrap().unwrap();
        assert_eq!(weather.name, "Weather Check");
    }

    #[test]
    fn test_weather_chain_execution() {
        let chain = ChainDef::from_yaml(WEATHER_CHECK_YAML).unwrap();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = full_manifest();
        let executor = ChainExecutor::new(&registry, &manifest);

        let params = HashMap::from([("city".into(), "NYC".into())]);
        let result = executor.run(&chain, &params).unwrap();

        assert!(result.success);
        assert_eq!(result.receipts.len(), 2); // fetch + notify
        assert!(result.outputs.contains_key("weather_data"));
    }

    #[test]
    fn test_price_chain_execution() {
        let chain = ChainDef::from_yaml(PRICE_CHECK_YAML).unwrap();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = full_manifest();
        let executor = ChainExecutor::new(&registry, &manifest);

        let params = HashMap::from([("ticker".into(), "NVDA".into())]);
        let result = executor.run(&chain, &params).unwrap();

        assert!(result.success);
        assert_eq!(result.receipts.len(), 2);
        assert!(result.outputs.contains_key("price"));
    }

    #[test]
    fn test_sentiment_chain_execution() {
        let chain = ChainDef::from_yaml(SENTIMENT_MONITOR_YAML).unwrap();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = full_manifest();
        let executor = ChainExecutor::new(&registry, &manifest);

        let params = HashMap::from([
            ("source".into(), "https://x.com/elonmusk".into()),
            ("topic".into(), "TSLA".into()),
        ]);
        let result = executor.run(&chain, &params).unwrap();

        assert!(result.success);
        assert_eq!(result.receipts.len(), 3); // fetch + sentiment + notify
    }

    #[test]
    fn test_chain_store_roundtrip_with_execution() {
        // Store → lookup → execute → record success
        let dir = tempfile::tempdir().unwrap();
        let store = ChainStore::open(&dir.path().join("chains.db")).unwrap();

        load_demo_chains(&store).unwrap();

        // Look up weather chain
        let record = store.lookup("weather_check").unwrap().unwrap();
        let chain = ChainDef::from_yaml(&record.yaml).unwrap();

        // Execute it
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = full_manifest();
        let executor = ChainExecutor::new(&registry, &manifest);
        let result = executor
            .run(&chain, &HashMap::from([("city".into(), "Delhi".into())]))
            .unwrap();

        assert!(result.success);

        // Record success
        store.record_success("weather_check").unwrap();

        // Verify counts
        let updated = store.lookup("weather_check").unwrap().unwrap();
        assert_eq!(updated.hit_count, 1);
        assert_eq!(updated.success_count, 1);
    }

    #[test]
    fn test_full_pipeline_simulation() {
        // Simulate: LLM emits MODE 2 → chain compiled → stored → looked up → executed
        let dir = tempfile::tempdir().unwrap();
        let store = ChainStore::open(&dir.path().join("chains.db")).unwrap();

        // Simulate LLM emitting a new chain (MODE 2)
        let nyaya_response = r#"I'll check the weather for you.
<nyaya>
NEW:weather_check
P:city:str:NYC
S:data.fetch_url:https://api.weather.com/$city>weather_data
S:notify.user:Weather: $weather_data
L:weather_query
R:weather in {city}|forecast for {city}|temperature in {city}
</nyaya>"#;

        let parsed = crate::llm_router::nyaya_block::parse_response(nyaya_response);
        assert_eq!(parsed.user_text, "I'll check the weather for you.");

        let block = parsed.nyaya.unwrap();
        assert_eq!(block.mode_name(), "NEW");

        // Compile the chain from the nyaya block
        let yaml = block.to_chain_yaml().unwrap();
        let chain = ChainDef::from_yaml(&yaml).unwrap();
        store.store(&chain).unwrap();

        // Verify it's stored
        let record = store.lookup("weather_check").unwrap().unwrap();
        assert_eq!(record.name, "weather_check");

        // Next request: template lookup (MODE 1)
        let template_ref = r#"Setting up weather check.
<nyaya>C:weather_check|Delhi</nyaya>"#;
        let parsed2 = crate::llm_router::nyaya_block::parse_response(template_ref);
        match parsed2.nyaya.unwrap() {
            crate::llm_router::nyaya_block::NyayaBlock::TemplateRef {
                template_name,
                params,
            } => {
                assert_eq!(template_name, "weather_check");
                assert_eq!(params[0], "Delhi");

                // Look up the stored chain
                let record = store.lookup(&template_name).unwrap().unwrap();
                let _chain = ChainDef::from_yaml(&record.yaml).unwrap();
                // Would execute here with params
            }
            _ => panic!("Expected TemplateRef"),
        }
    }

    /// Extended manifest with all permissions for testing new chains
    fn full_manifest_extended() -> AgentManifest {
        AgentManifest {
            name: "test-agent".into(),
            version: "0.1.0".into(),
            description: "test".into(),
            permissions: vec![
                "data.fetch_url".into(),
                "notify.user".into(),
                "trading.get_price".into(),
                "nlp.sentiment".into(),
                "nlp.summarize".into(),
                "flow.stop".into(),
                "flow.branch".into(),
                "docs.generate".into(),
                "files.read".into(),
                "files.write".into(),
                "memory.store".into(),
                "memory.search".into(),
                "calendar.list".into(),
                "data.analyze".into(),
                "browser.fetch".into(),
                "deep.delegate".into(),
                "channel.send".into(),
            ],
            memory_limit_mb: 64,
            fuel_limit: 1_000_000,
            kv_namespace: None,
            author: None,
            intent_filters: vec![],
            resources: None,
            background: false,
            subscriptions: vec![],
            data_namespace: None,
            signature: None,
        }
    }

    #[test]
    fn test_invoice_chain_execution() {
        let chain = ChainDef::from_yaml(INVOICE_DRAFT_YAML).unwrap();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = full_manifest_extended();
        let executor = ChainExecutor::new(&registry, &manifest);

        let params = HashMap::from([
            ("client_name".into(), "Acme Corp".into()),
            ("amount".into(), "5000".into()),
            ("description".into(), "Consulting services".into()),
        ]);
        let result = executor.run(&chain, &params).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_ticket_triage_chain_execution() {
        let chain = ChainDef::from_yaml(TICKET_TRIAGE_YAML).unwrap();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = full_manifest_extended();
        let executor = ChainExecutor::new(&registry, &manifest);

        let params = HashMap::from([("ticket_source".into(), "https://tickets.api/feed".into())]);
        let result = executor.run(&chain, &params).unwrap();
        assert!(result.success);
    }

    #[test]
    fn test_study_plan_chain_execution() {
        let chain = ChainDef::from_yaml(STUDY_PLAN_YAML).unwrap();
        let registry = AbilityRegistry::new(ReceiptSigner::generate());
        let manifest = full_manifest_extended();
        let executor = ChainExecutor::new(&registry, &manifest);

        let params = HashMap::from([
            ("topic".into(), "Linear Algebra".into()),
            ("duration_days".into(), "14".into()),
        ]);
        let result = executor.run(&chain, &params).unwrap();
        assert!(result.success);
    }
}
