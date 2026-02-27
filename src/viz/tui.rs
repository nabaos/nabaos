#[cfg(feature = "tui")]
pub mod tui_app {
    use ratatui::{
        backend::CrosstermBackend,
        crossterm::{
            event::{self, Event, KeyCode, KeyEventKind},
            execute,
            terminal::{
                disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
            },
        },
        layout::{Constraint, Direction, Layout, Rect},
        style::{Color, Modifier, Style},
        text::{Line, Span},
        widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
        Frame, Terminal,
    };
    use std::io;
    use std::path::Path;
    use std::time::{Duration, Instant};

    use crate::chain::workflow::{WorkflowDef, WorkflowInstance, WorkflowNode, WorkflowStatus};
    use crate::chain::workflow_store::WorkflowStore;

    // -----------------------------------------------------------------------
    // Public helpers (used by tests and drawing)
    // -----------------------------------------------------------------------

    /// Return an ASCII/Unicode shape indicator for a workflow node type.
    pub fn node_symbol(node: &WorkflowNode) -> &'static str {
        match node {
            WorkflowNode::Action(_) => "\u{25aa}",     // ▪
            WorkflowNode::WaitEvent(_) => "\u{2b21}",  // ⬡
            WorkflowNode::Delay(_) => "\u{25f7}",      // ◷
            WorkflowNode::WaitPoll(_) => "\u{2b19}",   // ⬙
            WorkflowNode::Parallel(_) => "\u{25c6}",   // ◆
            WorkflowNode::Branch(_) => "\u{25c7}",     // ◇
            WorkflowNode::Compensate(_) => "\u{21b6}", // ↶
        }
    }

    /// Return the display name for a node type.
    pub fn node_type_label(node: &WorkflowNode) -> &'static str {
        match node {
            WorkflowNode::Action(_) => "Action",
            WorkflowNode::WaitEvent(_) => "WaitEvent",
            WorkflowNode::Delay(_) => "Delay",
            WorkflowNode::WaitPoll(_) => "WaitPoll",
            WorkflowNode::Parallel(_) => "Parallel",
            WorkflowNode::Branch(_) => "Branch",
            WorkflowNode::Compensate(_) => "Compensate",
        }
    }

    /// Map a WorkflowStatus to a terminal color.
    pub fn status_color(status: &WorkflowStatus) -> Color {
        match status {
            WorkflowStatus::Completed => Color::Green,
            WorkflowStatus::Running => Color::Blue,
            WorkflowStatus::Waiting => Color::Yellow,
            WorkflowStatus::Failed => Color::Red,
            WorkflowStatus::Compensated => Color::Red,
            WorkflowStatus::Created | WorkflowStatus::Cancelled | WorkflowStatus::TimedOut => {
                Color::Gray
            }
        }
    }

    // -----------------------------------------------------------------------
    // View state
    // -----------------------------------------------------------------------

    /// Which view the TUI is currently displaying.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum TuiView {
        WorkflowList,
        WorkflowDetail(String),
        InstanceTracker(String),
    }

    // -----------------------------------------------------------------------
    // Application state
    // -----------------------------------------------------------------------

    struct TuiApp {
        store: WorkflowStore,
        view: TuiView,
        table_state: TableState,
        /// Cached workflow definitions (refreshed periodically).
        cached_defs: Vec<WorkflowDef>,
        /// Cached instances for InstanceTracker view.
        cached_instances: Vec<WorkflowInstance>,
        last_refresh: Instant,
    }

    impl TuiApp {
        fn new(store: WorkflowStore) -> Self {
            let mut app = TuiApp {
                store,
                view: TuiView::WorkflowList,
                table_state: TableState::default(),
                cached_defs: Vec::new(),
                cached_instances: Vec::new(),
                last_refresh: Instant::now() - Duration::from_secs(10), // force initial refresh
            };
            app.refresh();
            if !app.cached_defs.is_empty() {
                app.table_state.select(Some(0));
            }
            app
        }

        fn refresh(&mut self) {
            self.last_refresh = Instant::now();
            // Always refresh defs
            if let Ok(defs_list) = self.store.list_defs() {
                let mut defs = Vec::new();
                for (id, _name) in &defs_list {
                    if let Ok(Some(def)) = self.store.get_def(id) {
                        defs.push(def);
                    }
                }
                self.cached_defs = defs;
            }

            // Refresh instances if in tracker view
            if let TuiView::InstanceTracker(ref wf_id) = self.view {
                let wf_id = wf_id.clone();
                if let Ok(instances) = self.store.list_instances_for_workflow(&wf_id) {
                    self.cached_instances = instances;
                }
            }
        }

        fn selected_index(&self) -> Option<usize> {
            self.table_state.selected()
        }

        fn row_count(&self) -> usize {
            match &self.view {
                TuiView::WorkflowList => self.cached_defs.len(),
                TuiView::WorkflowDetail(wf_id) => self
                    .cached_defs
                    .iter()
                    .find(|d| &d.id == wf_id)
                    .map(|d| d.nodes.len())
                    .unwrap_or(0),
                TuiView::InstanceTracker(_) => self.cached_instances.len(),
            }
        }

        fn move_down(&mut self) {
            let count = self.row_count();
            if count == 0 {
                return;
            }
            let i = match self.selected_index() {
                Some(i) => {
                    if i >= count - 1 {
                        0
                    } else {
                        i + 1
                    }
                }
                None => 0,
            };
            self.table_state.select(Some(i));
        }

        fn move_up(&mut self) {
            let count = self.row_count();
            if count == 0 {
                return;
            }
            let i = match self.selected_index() {
                Some(i) => {
                    if i == 0 {
                        count - 1
                    } else {
                        i - 1
                    }
                }
                None => 0,
            };
            self.table_state.select(Some(i));
        }

        /// Handle Enter key: drill into the selected item.
        fn enter(&mut self) {
            match &self.view {
                TuiView::WorkflowList => {
                    if let Some(i) = self.selected_index() {
                        if let Some(def) = self.cached_defs.get(i) {
                            let wf_id = def.id.clone();
                            self.view = TuiView::WorkflowDetail(wf_id);
                            self.table_state = TableState::default();
                            if self.row_count() > 0 {
                                self.table_state.select(Some(0));
                            }
                        }
                    }
                }
                TuiView::WorkflowDetail(wf_id) => {
                    let wf_id = wf_id.clone();
                    self.view = TuiView::InstanceTracker(wf_id.clone());
                    self.table_state = TableState::default();
                    // Refresh instances
                    if let Ok(instances) = self.store.list_instances_for_workflow(&wf_id) {
                        self.cached_instances = instances;
                    }
                    if self.row_count() > 0 {
                        self.table_state.select(Some(0));
                    }
                }
                TuiView::InstanceTracker(_) => {
                    // No deeper drill-in for now
                }
            }
        }

        /// Handle Escape/q: go back one level or quit.
        /// Returns true if the app should quit.
        fn back(&mut self) -> bool {
            match &self.view {
                TuiView::WorkflowList => true, // quit
                TuiView::WorkflowDetail(_) => {
                    self.view = TuiView::WorkflowList;
                    self.table_state = TableState::default();
                    if !self.cached_defs.is_empty() {
                        self.table_state.select(Some(0));
                    }
                    false
                }
                TuiView::InstanceTracker(wf_id) => {
                    let wf_id = wf_id.clone();
                    self.view = TuiView::WorkflowDetail(wf_id);
                    self.table_state = TableState::default();
                    if self.row_count() > 0 {
                        self.table_state.select(Some(0));
                    }
                    false
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // Entry point
    // -----------------------------------------------------------------------

    /// Run the TUI workflow viewer, reading from the given SQLite database.
    pub fn run_tui(db_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let store = WorkflowStore::open(db_path)?;
        let mut app = TuiApp::new(store);

        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        let refresh_interval = Duration::from_secs(2);

        loop {
            // Auto-refresh every 2 seconds
            if app.last_refresh.elapsed() >= refresh_interval {
                app.refresh();
            }

            terminal.draw(|f| draw(f, &mut app))?;

            // Poll for events with a small timeout so we can auto-refresh
            if event::poll(Duration::from_millis(200))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind != KeyEventKind::Press {
                        continue;
                    }
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            if app.back() {
                                break;
                            }
                        }
                        KeyCode::Char('j') | KeyCode::Down => app.move_down(),
                        KeyCode::Char('k') | KeyCode::Up => app.move_up(),
                        KeyCode::Enter => app.enter(),
                        KeyCode::Char('r') => app.refresh(),
                        _ => {}
                    }
                }
            }
        }

        // Restore terminal
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Drawing
    // -----------------------------------------------------------------------

    fn draw(f: &mut Frame, app: &mut TuiApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(f.area());

        match &app.view {
            TuiView::WorkflowList => draw_workflow_list(f, app, chunks[0]),
            TuiView::WorkflowDetail(wf_id) => {
                let wf_id = wf_id.clone();
                draw_workflow_detail(f, app, chunks[0], &wf_id);
            }
            TuiView::InstanceTracker(wf_id) => {
                let wf_id = wf_id.clone();
                draw_instance_tracker(f, app, chunks[0], &wf_id);
            }
        }

        // Status bar
        let help = match &app.view {
            TuiView::WorkflowList => " j/k: navigate | Enter: detail | r: refresh | q: quit ",
            TuiView::WorkflowDetail(_) => {
                " j/k: navigate | Enter: instances | r: refresh | q/Esc: back "
            }
            TuiView::InstanceTracker(_) => " j/k: navigate | r: refresh | q/Esc: back ",
        };
        let status_bar = Paragraph::new(Line::from(vec![Span::styled(
            help,
            Style::default().fg(Color::DarkGray),
        )]));
        f.render_widget(status_bar, chunks[1]);
    }

    fn draw_workflow_list(f: &mut Frame, app: &mut TuiApp, area: Rect) {
        let header = Row::new(vec![
            Cell::from("ID"),
            Cell::from("Name"),
            Cell::from("Nodes"),
            Cell::from("Max Inst"),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1);

        let rows: Vec<Row> = app
            .cached_defs
            .iter()
            .map(|def| {
                Row::new(vec![
                    Cell::from(def.id.clone()),
                    Cell::from(def.name.clone()),
                    Cell::from(def.nodes.len().to_string()),
                    Cell::from(if def.max_instances == 0 {
                        "unlimited".to_string()
                    } else {
                        def.max_instances.to_string()
                    }),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Percentage(30),
                Constraint::Percentage(35),
                Constraint::Percentage(15),
                Constraint::Percentage(20),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Workflow Definitions "),
        )
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::Cyan),
        );

        f.render_stateful_widget(table, area, &mut app.table_state);
    }

    fn draw_workflow_detail(f: &mut Frame, app: &mut TuiApp, area: Rect, wf_id: &str) {
        let def = app.cached_defs.iter().find(|d| d.id == wf_id);
        let def = match def {
            Some(d) => d.clone(),
            None => {
                let msg = Paragraph::new(format!("Workflow '{}' not found", wf_id))
                    .block(Block::default().borders(Borders::ALL).title(" Detail "));
                f.render_widget(msg, area);
                return;
            }
        };

        let header = Row::new(vec![
            Cell::from("#"),
            Cell::from("Symbol"),
            Cell::from("Type"),
            Cell::from("ID"),
            Cell::from("Detail"),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1);

        let rows: Vec<Row> = def
            .nodes
            .iter()
            .enumerate()
            .map(|(i, node)| {
                let sym = node_symbol(node);
                let type_label = node_type_label(node);
                let node_id = node.id().to_string();
                let detail = node_detail(node);

                let arrow = if i < def.nodes.len() - 1 {
                    format!("{} ->", i)
                } else {
                    format!("{}", i)
                };

                Row::new(vec![
                    Cell::from(arrow),
                    Cell::from(sym),
                    Cell::from(type_label),
                    Cell::from(node_id),
                    Cell::from(detail),
                ])
            })
            .collect();

        let title = format!(" {} - {} ({} nodes) ", def.id, def.name, def.nodes.len());
        let table = Table::new(
            rows,
            [
                Constraint::Length(6),
                Constraint::Length(4),
                Constraint::Length(12),
                Constraint::Percentage(25),
                Constraint::Percentage(45),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::Cyan),
        );

        f.render_stateful_widget(table, area, &mut app.table_state);
    }

    fn draw_instance_tracker(f: &mut Frame, app: &mut TuiApp, area: Rect, wf_id: &str) {
        let header = Row::new(vec![
            Cell::from("Instance ID"),
            Cell::from("Status"),
            Cell::from("Cursor"),
            Cell::from("Exec (ms)"),
            Cell::from("Error"),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD))
        .bottom_margin(1);

        let rows: Vec<Row> = app
            .cached_instances
            .iter()
            .map(|inst| {
                let color = status_color(&inst.status);
                let status_text = inst.status.to_string();
                let error_text = inst.error.as_deref().unwrap_or("-").to_string();
                let error_display = if error_text.len() > 40 {
                    format!("{}...", &error_text[..37])
                } else {
                    error_text
                };

                Row::new(vec![
                    Cell::from(truncate_id(&inst.instance_id)),
                    Cell::from(status_text).style(Style::default().fg(color)),
                    Cell::from(format!("node {}", inst.cursor.node_index)),
                    Cell::from(inst.execution_ms.to_string()),
                    Cell::from(error_display),
                ])
            })
            .collect();

        let title = format!(
            " Instances: {} ({} total) ",
            wf_id,
            app.cached_instances.len()
        );
        let table = Table::new(
            rows,
            [
                Constraint::Length(24),
                Constraint::Length(12),
                Constraint::Length(10),
                Constraint::Length(12),
                Constraint::Min(20),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(
            Style::default()
                .add_modifier(Modifier::REVERSED)
                .fg(Color::Cyan),
        );

        f.render_stateful_widget(table, area, &mut app.table_state);
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn node_detail(node: &WorkflowNode) -> String {
        match node {
            WorkflowNode::Action(n) => {
                let mut s = format!("ability: {}", n.ability);
                if let Some(ref of) = n.on_failure {
                    s.push_str(&format!(" | on_fail: {}", of));
                }
                s
            }
            WorkflowNode::WaitEvent(n) => {
                let mut s = format!("event: {}", n.event_type);
                if n.timeout_secs > 0 {
                    s.push_str(&format!(" | timeout: {}s", n.timeout_secs));
                }
                s
            }
            WorkflowNode::Delay(n) => format!("{}s", n.duration_secs),
            WorkflowNode::WaitPoll(n) => {
                format!("ability: {} | every {}s", n.ability, n.poll_interval_secs)
            }
            WorkflowNode::Parallel(n) => {
                format!("{} branches, join: {:?}", n.branches.len(), n.join)
            }
            WorkflowNode::Branch(n) => {
                format!(
                    "{} arms + {} otherwise",
                    n.conditions.len(),
                    n.otherwise.len()
                )
            }
            WorkflowNode::Compensate(n) => {
                format!(
                    "compensates: {} | ability: {}",
                    n.compensates_node, n.ability
                )
            }
        }
    }

    fn truncate_id(id: &str) -> String {
        if id.len() > 22 {
            format!("{}...", &id[..19])
        } else {
            id.to_string()
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, feature = "tui"))]
mod tests {
    use super::tui_app::*;
    use crate::chain::workflow::*;
    use std::collections::HashMap;

    // -- Node symbol tests --------------------------------------------------

    #[test]
    fn test_action_node_symbol() {
        let node = WorkflowNode::Action(ActionNode {
            id: "a1".into(),
            ability: "test.action".into(),
            args: HashMap::new(),
            output_key: None,
            condition: None,
            on_failure: None,
        });
        assert_eq!(node_symbol(&node), "\u{25aa}"); // ▪
    }

    #[test]
    fn test_wait_event_node_symbol() {
        let node = WorkflowNode::WaitEvent(WaitEventNode {
            id: "w1".into(),
            event_type: "order.shipped".into(),
            filter: None,
            output_key: None,
            timeout_secs: 0,
            on_timeout: "fail".into(),
        });
        assert_eq!(node_symbol(&node), "\u{2b21}"); // ⬡
    }

    #[test]
    fn test_delay_node_symbol() {
        let node = WorkflowNode::Delay(DelayNode {
            id: "d1".into(),
            duration_secs: 60,
        });
        assert_eq!(node_symbol(&node), "\u{25f7}"); // ◷
    }

    #[test]
    fn test_wait_poll_node_symbol() {
        use crate::chain::dsl::{ConditionOp, StepCondition};
        let node = WorkflowNode::WaitPoll(WaitPollNode {
            id: "wp1".into(),
            ability: "status.check".into(),
            args: HashMap::new(),
            output_key: None,
            until: StepCondition {
                ref_key: "status".into(),
                op: ConditionOp::Equals,
                value: "done".into(),
            },
            poll_interval_secs: 30,
            timeout_secs: 0,
            on_timeout: "fail".into(),
        });
        assert_eq!(node_symbol(&node), "\u{2b19}"); // ⬙
    }

    #[test]
    fn test_parallel_node_symbol() {
        let node = WorkflowNode::Parallel(ParallelNode {
            id: "p1".into(),
            branches: vec![],
            join: JoinStrategy::All,
        });
        assert_eq!(node_symbol(&node), "\u{25c6}"); // ◆
    }

    #[test]
    fn test_branch_node_symbol() {
        let node = WorkflowNode::Branch(BranchNode {
            id: "b1".into(),
            conditions: vec![],
            otherwise: vec![],
        });
        assert_eq!(node_symbol(&node), "\u{25c7}"); // ◇
    }

    // -- Status color tests -------------------------------------------------

    #[test]
    fn test_status_color_completed() {
        assert_eq!(
            status_color(&WorkflowStatus::Completed),
            ratatui::style::Color::Green
        );
    }

    #[test]
    fn test_status_color_running() {
        assert_eq!(
            status_color(&WorkflowStatus::Running),
            ratatui::style::Color::Blue
        );
    }

    #[test]
    fn test_status_color_waiting() {
        assert_eq!(
            status_color(&WorkflowStatus::Waiting),
            ratatui::style::Color::Yellow
        );
    }

    #[test]
    fn test_status_color_failed() {
        assert_eq!(
            status_color(&WorkflowStatus::Failed),
            ratatui::style::Color::Red
        );
    }

    #[test]
    fn test_status_color_created() {
        assert_eq!(
            status_color(&WorkflowStatus::Created),
            ratatui::style::Color::Gray
        );
    }

    #[test]
    fn test_status_color_cancelled() {
        assert_eq!(
            status_color(&WorkflowStatus::Cancelled),
            ratatui::style::Color::Gray
        );
    }

    #[test]
    fn test_status_color_timed_out() {
        assert_eq!(
            status_color(&WorkflowStatus::TimedOut),
            ratatui::style::Color::Gray
        );
    }

    // -- View navigation tests ----------------------------------------------

    #[test]
    fn test_view_initial_state() {
        let view = TuiView::WorkflowList;
        assert_eq!(view, TuiView::WorkflowList);
    }

    #[test]
    fn test_view_transitions() {
        // WorkflowList -> WorkflowDetail
        let v1 = TuiView::WorkflowList;
        assert_eq!(v1, TuiView::WorkflowList);

        let v2 = TuiView::WorkflowDetail("wf_test".to_string());
        assert_eq!(v2, TuiView::WorkflowDetail("wf_test".to_string()));

        // WorkflowDetail -> InstanceTracker
        let v3 = TuiView::InstanceTracker("wf_test".to_string());
        assert_eq!(v3, TuiView::InstanceTracker("wf_test".to_string()));
    }

    #[test]
    fn test_node_type_labels() {
        let action = WorkflowNode::Action(ActionNode {
            id: "a".into(),
            ability: "x".into(),
            args: HashMap::new(),
            output_key: None,
            condition: None,
            on_failure: None,
        });
        assert_eq!(node_type_label(&action), "Action");

        let delay = WorkflowNode::Delay(DelayNode {
            id: "d".into(),
            duration_secs: 10,
        });
        assert_eq!(node_type_label(&delay), "Delay");

        let parallel = WorkflowNode::Parallel(ParallelNode {
            id: "p".into(),
            branches: vec![],
            join: JoinStrategy::All,
        });
        assert_eq!(node_type_label(&parallel), "Parallel");

        let branch = WorkflowNode::Branch(BranchNode {
            id: "b".into(),
            conditions: vec![],
            otherwise: vec![],
        });
        assert_eq!(node_type_label(&branch), "Branch");
    }

    // -- All node symbols are unique ----------------------------------------

    #[test]
    fn test_all_symbols_unique() {
        use crate::chain::dsl::{ConditionOp, StepCondition};
        let nodes: Vec<WorkflowNode> = vec![
            WorkflowNode::Action(ActionNode {
                id: "a".into(),
                ability: "x".into(),
                args: HashMap::new(),
                output_key: None,
                condition: None,
                on_failure: None,
            }),
            WorkflowNode::WaitEvent(WaitEventNode {
                id: "w".into(),
                event_type: "e".into(),
                filter: None,
                output_key: None,
                timeout_secs: 0,
                on_timeout: "fail".into(),
            }),
            WorkflowNode::Delay(DelayNode {
                id: "d".into(),
                duration_secs: 1,
            }),
            WorkflowNode::WaitPoll(WaitPollNode {
                id: "wp".into(),
                ability: "x".into(),
                args: HashMap::new(),
                output_key: None,
                until: StepCondition {
                    ref_key: "k".into(),
                    op: ConditionOp::Equals,
                    value: "v".into(),
                },
                poll_interval_secs: 30,
                timeout_secs: 0,
                on_timeout: "fail".into(),
            }),
            WorkflowNode::Parallel(ParallelNode {
                id: "p".into(),
                branches: vec![],
                join: JoinStrategy::All,
            }),
            WorkflowNode::Branch(BranchNode {
                id: "b".into(),
                conditions: vec![],
                otherwise: vec![],
            }),
        ];

        let symbols: Vec<&str> = nodes.iter().map(|n| node_symbol(n)).collect();
        let unique: std::collections::HashSet<&&str> = symbols.iter().collect();
        assert_eq!(
            symbols.len(),
            unique.len(),
            "All node symbols should be unique"
        );
    }
}
