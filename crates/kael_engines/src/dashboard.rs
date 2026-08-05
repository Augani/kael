//! Data dashboard workload engine.

use serde::{Deserialize, Serialize};

/// Supported chart visualization types.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChartType {
    /// Vertical bar chart.
    Bar,
    /// Line chart.
    Line,
    /// Scatter plot.
    Scatter,
    /// Pie chart.
    Pie,
    /// Area chart.
    Area,
    /// Histogram.
    Histogram,
}

/// A single data point with optional label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    /// X-axis value.
    pub x: f64,
    /// Y-axis value.
    pub y: f64,
    /// Optional display label.
    pub label: Option<String>,
}

/// A named series of data points for charting.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataSeries {
    /// Series display name.
    pub name: String,
    /// Chart type for this series.
    pub chart_type: ChartType,
    /// Data points in the series.
    pub points: Vec<DataPoint>,
}

impl DataSeries {
    /// Add a data point to the series.
    pub fn add_point(&mut self, point: DataPoint) {
        self.points.push(point);
    }

    /// Minimum y-value in the series, or `None` if empty.
    pub fn min(&self) -> Option<f64> {
        self.points
            .iter()
            .map(|p| p.y)
            .filter(|value| value.is_finite())
            .min_by(f64::total_cmp)
    }

    /// Maximum y-value in the series, or `None` if empty.
    pub fn max(&self) -> Option<f64> {
        self.points
            .iter()
            .map(|p| p.y)
            .filter(|value| value.is_finite())
            .max_by(f64::total_cmp)
    }

    /// Mean of all y-values, or `None` if empty.
    pub fn mean(&self) -> Option<f64> {
        let mut count = 0u64;
        let mut mean = 0.0f64;
        for value in self
            .points
            .iter()
            .map(|point| point.y)
            .filter(|y| y.is_finite())
        {
            count = count.saturating_add(1);
            let count_f64 = count as f64;
            mean = mean * ((count_f64 - 1.0) / count_f64) + value / count_f64;
        }
        if count == 0 {
            return None;
        }
        Some(mean)
    }

    /// Sum of all y-values.
    pub fn sum(&self) -> f64 {
        self.points
            .iter()
            .map(|p| p.y)
            .filter(|value| value.is_finite())
            .fold(0.0, |sum, value| {
                let next = sum + value;
                if next.is_finite() {
                    next
                } else if next.is_sign_negative() {
                    f64::MIN
                } else {
                    f64::MAX
                }
            })
    }

    /// Sort points by x-value in ascending order.
    pub fn sort_by_x(&mut self) {
        self.points
            .sort_by(|a, b| match (a.x.is_finite(), b.x.is_finite()) {
                (true, true) => a.x.total_cmp(&b.x),
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                (false, false) => a.x.total_cmp(&b.x),
            });
    }

    /// Number of data points in the series.
    pub fn len(&self) -> usize {
        self.points.len()
    }

    /// Whether the series contains no data points.
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Comparison operator for data filters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterOp {
    /// Equal.
    Eq,
    /// Not equal.
    Neq,
    /// Greater than.
    Gt,
    /// Greater than or equal.
    Gte,
    /// Less than.
    Lt,
    /// Less than or equal.
    Lte,
    /// Contains substring.
    Contains,
    /// Starts with prefix.
    StartsWith,
}

/// A filter condition applied to a data column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataFilter {
    /// Column name to filter on.
    pub column: String,
    /// Comparison operator.
    pub op: FilterOp,
    /// Value to compare against (as string).
    pub value: String,
}

/// Specification for grouping data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupBy {
    /// Column to group by.
    pub column: String,
    /// Aggregation to apply to each group.
    pub aggregation: Aggregation,
}

/// Aggregation functions for grouped data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Aggregation {
    /// Count of rows.
    Count,
    /// Sum of values.
    Sum,
    /// Average of values.
    Avg,
    /// Minimum value.
    Min,
    /// Maximum value.
    Max,
}

/// Status of a query job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum QueryStatus {
    /// Waiting to be executed.
    Queued,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Failed with an error message.
    Failed(String),
    /// Cancelled by user.
    Cancelled,
}

/// A scheduled or running query job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueryJob {
    /// Unique job identifier.
    pub id: String,
    /// Query expression.
    pub query: String,
    /// Filters to apply.
    pub filters: Vec<DataFilter>,
    /// Optional grouping specification.
    pub group_by: Option<GroupBy>,
    /// Current status.
    pub status: QueryStatus,
    /// Creation timestamp (epoch ms).
    pub created_at: u64,
    /// Completion timestamp (epoch ms).
    pub completed_at: Option<u64>,
}

/// Manages query job scheduling and lifecycle.
#[derive(Debug, Default)]
pub struct QueryScheduler {
    jobs: Vec<QueryJob>,
    next_id: u64,
}

impl QueryScheduler {
    /// Create a new empty scheduler.
    pub fn new() -> Self {
        Self::default()
    }

    /// Submit a new query job at the supplied epoch-millisecond timestamp.
    /// Returns the assigned job id.
    pub fn submit(
        &mut self,
        query: String,
        filters: Vec<DataFilter>,
        group_by: Option<GroupBy>,
        created_at: u64,
    ) -> String {
        let id = self.allocate_id();
        self.jobs.push(QueryJob {
            id: id.clone(),
            query,
            filters,
            group_by,
            status: QueryStatus::Queued,
            created_at,
            completed_at: None,
        });
        id
    }

    fn allocate_id(&mut self) -> String {
        let start = self.next_id;
        let mut candidate = start;
        loop {
            let id = format!("job-{candidate}");
            if self.jobs.iter().all(|job| job.id != id) {
                self.next_id = candidate.wrapping_add(1);
                return id;
            }
            candidate = candidate.wrapping_add(1);
            assert!(candidate != start, "query job id space exhausted");
        }
    }

    /// Cancel a queued or running job at the supplied epoch-millisecond
    /// timestamp. Returns an error if the job is not found or already terminal.
    pub fn cancel(&mut self, id: &str, completed_at: u64) -> anyhow::Result<()> {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("job '{id}' not found"))?;
        match job.status {
            QueryStatus::Queued | QueryStatus::Running => {
                job.status = QueryStatus::Cancelled;
                job.completed_at = Some(completed_at);
                Ok(())
            }
            _ => anyhow::bail!("job '{id}' is already in terminal state"),
        }
    }

    /// Move a queued job into the running state.
    pub fn start(&mut self, id: &str) -> anyhow::Result<()> {
        let job = self.job_mut(id)?;
        if job.status != QueryStatus::Queued {
            anyhow::bail!("job '{id}' is not queued");
        }
        job.status = QueryStatus::Running;
        Ok(())
    }

    /// Mark a running job completed at `completed_at` epoch milliseconds.
    pub fn complete(&mut self, id: &str, completed_at: u64) -> anyhow::Result<()> {
        let job = self.job_mut(id)?;
        if job.status != QueryStatus::Running {
            anyhow::bail!("job '{id}' is not running");
        }
        job.status = QueryStatus::Completed;
        job.completed_at = Some(completed_at);
        Ok(())
    }

    /// Mark a queued or running job failed at `completed_at` epoch milliseconds.
    pub fn fail(
        &mut self,
        id: &str,
        message: impl Into<String>,
        completed_at: u64,
    ) -> anyhow::Result<()> {
        let job = self.job_mut(id)?;
        if !matches!(job.status, QueryStatus::Queued | QueryStatus::Running) {
            anyhow::bail!("job '{id}' is already in terminal state");
        }
        job.status = QueryStatus::Failed(message.into());
        job.completed_at = Some(completed_at);
        Ok(())
    }

    fn job_mut(&mut self, id: &str) -> anyhow::Result<&mut QueryJob> {
        self.jobs
            .iter_mut()
            .find(|job| job.id == id)
            .ok_or_else(|| anyhow::anyhow!("job '{id}' not found"))
    }

    /// Get a reference to a job by id.
    pub fn get(&self, id: &str) -> Option<&QueryJob> {
        self.jobs.iter().find(|j| j.id == id)
    }

    /// List all jobs.
    pub fn list(&self) -> &[QueryJob] {
        &self.jobs
    }

    /// Return all completed jobs.
    pub fn completed_jobs(&self) -> Vec<&QueryJob> {
        self.jobs
            .iter()
            .filter(|j| j.status == QueryStatus::Completed)
            .collect()
    }

    /// Count of jobs in queued or running state.
    pub fn pending_count(&self) -> usize {
        self.jobs
            .iter()
            .filter(|j| matches!(j.status, QueryStatus::Queued | QueryStatus::Running))
            .count()
    }
}

/// Utility for parsing CSV data.
#[derive(Debug, Default)]
pub struct CsvImporter {
    headers: Vec<String>,
}

impl CsvImporter {
    /// Create a new CSV importer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse and store a CSV header, including RFC 4180-style quoted fields.
    pub fn parse_header(&mut self, line: &str) -> anyhow::Result<Vec<String>> {
        let headers = parse_csv_line(line)?;
        if headers.is_empty() || headers.iter().any(String::is_empty) {
            anyhow::bail!("CSV headers must be non-empty");
        }
        let mut unique = std::collections::HashSet::new();
        if headers.iter().any(|header| !unique.insert(header.as_str())) {
            anyhow::bail!("CSV headers must be unique");
        }
        self.headers = headers.clone();
        Ok(headers)
    }

    /// Parse a data row into key-value pairs using the stored headers.
    /// Returns an error if headers have not been parsed or column count mismatches.
    pub fn parse_row(&self, line: &str) -> anyhow::Result<Vec<(String, String)>> {
        if self.headers.is_empty() {
            anyhow::bail!("headers must be parsed before rows");
        }
        let values = parse_csv_line(line)?;
        if values.len() != self.headers.len() {
            anyhow::bail!(
                "column count mismatch: expected {}, got {}",
                self.headers.len(),
                values.len()
            );
        }
        Ok(self
            .headers
            .iter()
            .zip(values.iter())
            .map(|(h, v)| (h.clone(), v.clone()))
            .collect())
    }

    /// Validate that a row has the expected number of columns.
    pub fn validate_row(&self, line: &str) -> bool {
        if self.headers.is_empty() {
            return false;
        }
        parse_csv_line(line).is_ok_and(|values| values.len() == self.headers.len())
    }
}

fn parse_csv_line(line: &str) -> anyhow::Result<Vec<String>> {
    const MAX_RECORD_BYTES: usize = 16 * 1024 * 1024;
    const MAX_FIELDS: usize = 65_536;
    if line.len() > MAX_RECORD_BYTES {
        anyhow::bail!("CSV record exceeds {MAX_RECORD_BYTES} bytes");
    }

    #[derive(Clone, Copy)]
    enum State {
        FieldStart,
        Unquoted,
        Quoted,
        AfterQuote,
    }

    let mut values = Vec::new();
    let mut value = String::new();
    let mut state = State::FieldStart;
    for character in line.chars() {
        match (state, character) {
            (State::FieldStart, '"') => state = State::Quoted,
            (State::FieldStart, ',') => push_csv_value(&mut values, String::new(), MAX_FIELDS)?,
            (State::FieldStart, character) if character.is_ascii_whitespace() => {}
            (State::FieldStart, _) => {
                value.push(character);
                state = State::Unquoted;
            }
            (State::Unquoted, ',') => {
                push_csv_value(&mut values, value.trim().to_string(), MAX_FIELDS)?;
                value = String::new();
                state = State::FieldStart;
            }
            (State::Unquoted, '"') => anyhow::bail!("quote inside unquoted CSV field"),
            (State::Unquoted, _) => value.push(character),
            (State::Quoted, '"') => state = State::AfterQuote,
            (State::Quoted, _) => value.push(character),
            (State::AfterQuote, '"') => {
                value.push('"');
                state = State::Quoted;
            }
            (State::AfterQuote, ',') => {
                push_csv_value(&mut values, std::mem::take(&mut value), MAX_FIELDS)?;
                state = State::FieldStart;
            }
            (State::AfterQuote, character) if character.is_ascii_whitespace() => {}
            (State::AfterQuote, _) => {
                anyhow::bail!("unexpected character after closing CSV quote")
            }
        }
    }

    match state {
        State::Quoted => anyhow::bail!("unterminated quoted CSV field"),
        State::Unquoted => {
            push_csv_value(&mut values, value.trim().to_string(), MAX_FIELDS)?;
        }
        State::FieldStart => push_csv_value(&mut values, String::new(), MAX_FIELDS)?,
        State::AfterQuote => push_csv_value(&mut values, value, MAX_FIELDS)?,
    }
    Ok(values)
}

fn push_csv_value(
    values: &mut Vec<String>,
    value: String,
    max_fields: usize,
) -> anyhow::Result<()> {
    if values.len() >= max_fields {
        anyhow::bail!("CSV record exceeds {max_fields} fields");
    }
    values.push(value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_series() -> DataSeries {
        DataSeries {
            name: "test".into(),
            chart_type: ChartType::Line,
            points: vec![
                DataPoint {
                    x: 1.0,
                    y: 10.0,
                    label: None,
                },
                DataPoint {
                    x: 3.0,
                    y: 30.0,
                    label: None,
                },
                DataPoint {
                    x: 2.0,
                    y: 20.0,
                    label: Some("mid".into()),
                },
            ],
        }
    }

    #[test]
    fn data_series_add_point() {
        let mut ds = DataSeries {
            name: "s".into(),
            chart_type: ChartType::Bar,
            points: vec![],
        };
        ds.add_point(DataPoint {
            x: 0.0,
            y: 5.0,
            label: None,
        });
        assert_eq!(ds.len(), 1);
        assert!(!ds.is_empty());
    }

    #[test]
    fn data_series_min_max() {
        let ds = sample_series();
        assert!((ds.min().unwrap() - 10.0).abs() < f64::EPSILON);
        assert!((ds.max().unwrap() - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn data_series_mean() {
        let ds = sample_series();
        assert!((ds.mean().unwrap() - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn data_series_sum() {
        let ds = sample_series();
        assert!((ds.sum() - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn data_series_sort_by_x() {
        let mut ds = sample_series();
        ds.sort_by_x();
        let xs: Vec<f64> = ds.points.iter().map(|p| p.x).collect();
        assert_eq!(xs, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn data_series_empty_stats() {
        let ds = DataSeries {
            name: "empty".into(),
            chart_type: ChartType::Pie,
            points: vec![],
        };
        assert!(ds.min().is_none());
        assert!(ds.max().is_none());
        assert!(ds.mean().is_none());
        assert!(ds.is_empty());
    }

    #[test]
    fn data_series_stats_ignore_non_finite_values_and_sort_them_last() {
        let mut series = sample_series();
        series.points.push(DataPoint {
            x: f64::NAN,
            y: f64::NAN,
            label: None,
        });
        assert_eq!(series.min(), Some(10.0));
        assert_eq!(series.max(), Some(30.0));
        assert_eq!(series.mean(), Some(20.0));
        assert_eq!(series.sum(), 60.0);
        series.sort_by_x();
        assert!(series.points.last().unwrap().x.is_nan());
    }

    #[test]
    fn query_scheduler_submit_and_get() {
        let mut sched = QueryScheduler::new();
        let id = sched.submit("SELECT *".into(), vec![], None, 10);
        assert!(sched.get(&id).is_some());
        assert_eq!(sched.get(&id).unwrap().status, QueryStatus::Queued);
        assert_eq!(sched.pending_count(), 1);
    }

    #[test]
    fn query_scheduler_cancel() {
        let mut sched = QueryScheduler::new();
        let id = sched.submit("SELECT 1".into(), vec![], None, 10);
        assert!(sched.cancel(&id, 20).is_ok());
        assert_eq!(sched.get(&id).unwrap().status, QueryStatus::Cancelled);
        assert_eq!(sched.get(&id).unwrap().completed_at, Some(20));
        assert!(sched.cancel(&id, 21).is_err());
    }

    #[test]
    fn query_scheduler_cancel_unknown() {
        let mut sched = QueryScheduler::new();
        assert!(sched.cancel("nope", 0).is_err());
    }

    #[test]
    fn query_scheduler_list_and_completed() {
        let mut sched = QueryScheduler::new();
        sched.submit("q1".into(), vec![], None, 1);
        sched.submit("q2".into(), vec![], None, 2);
        assert_eq!(sched.list().len(), 2);
        assert!(sched.completed_jobs().is_empty());
    }

    #[test]
    fn query_scheduler_runs_jobs_to_terminal_states_and_wraps_ids() {
        let mut scheduler = QueryScheduler::new();
        scheduler.next_id = u64::MAX;
        let max = scheduler.submit("max".into(), vec![], None, 1);
        let zero = scheduler.submit("zero".into(), vec![], None, 2);
        assert_eq!(max, format!("job-{}", u64::MAX));
        assert_eq!(zero, "job-0");
        scheduler.start(&max).unwrap();
        scheduler.complete(&max, 123).unwrap();
        assert_eq!(scheduler.get(&max).unwrap().completed_at, Some(123));
        assert_eq!(scheduler.completed_jobs().len(), 1);
        scheduler.fail(&zero, "bad query", 124).unwrap();
        assert!(matches!(
            scheduler.get(&zero).unwrap().status,
            QueryStatus::Failed(_)
        ));
    }

    #[test]
    fn csv_importer_parse_header_and_row() {
        let mut csv = CsvImporter::new();
        let headers = csv.parse_header("name, age, city").unwrap();
        assert_eq!(headers, vec!["name", "age", "city"]);
        let row = csv.parse_row("Alice, 30, NYC").unwrap();
        assert_eq!(row.len(), 3);
        assert_eq!(row[0], ("name".into(), "Alice".into()));
        assert_eq!(row[1], ("age".into(), "30".into()));
    }

    #[test]
    fn csv_importer_handles_quotes_and_rejects_malformed_headers() {
        let mut csv = CsvImporter::new();
        assert_eq!(
            csv.parse_header("name,notes").unwrap(),
            vec!["name", "notes"]
        );
        let row = csv.parse_row("Alice,\"hello, \"\"world\"\"\"").unwrap();
        assert_eq!(row[1].1, "hello, \"world\"");
        let spaced = csv.parse_row(" Alice ,  \" keep me \"  ").unwrap();
        assert_eq!(spaced[0].1, "Alice");
        assert_eq!(spaced[1].1, " keep me ");
        assert!(!csv.validate_row("Alice,\"unterminated"));
        assert!(csv.parse_header("name,name").is_err());
        assert!(csv.parse_header("name,").is_err());
        assert!(csv.parse_row("Alice,un\"quoted").is_err());
        assert!(csv.parse_row("Alice,\"quoted\"suffix").is_err());
    }

    #[test]
    fn csv_importer_row_without_header() {
        let csv = CsvImporter::new();
        assert!(csv.parse_row("a,b,c").is_err());
    }

    #[test]
    fn csv_importer_column_mismatch() {
        let mut csv = CsvImporter::new();
        csv.parse_header("a,b").unwrap();
        assert!(csv.parse_row("1,2,3").is_err());
    }

    #[test]
    fn csv_importer_validate_row() {
        let mut csv = CsvImporter::new();
        assert!(!csv.validate_row("x,y"));
        csv.parse_header("a,b").unwrap();
        assert!(csv.validate_row("1,2"));
        assert!(!csv.validate_row("1,2,3"));
    }

    #[test]
    fn filter_op_serialization() {
        let filter = DataFilter {
            column: "price".into(),
            op: FilterOp::Gte,
            value: "100".into(),
        };
        let json = serde_json::to_string(&filter).unwrap();
        let deser: DataFilter = serde_json::from_str(&json).unwrap();
        assert_eq!(deser.op, FilterOp::Gte);
        assert_eq!(deser.column, "price");
    }

    #[test]
    fn query_status_variants() {
        let statuses = vec![
            QueryStatus::Queued,
            QueryStatus::Running,
            QueryStatus::Completed,
            QueryStatus::Failed("err".into()),
            QueryStatus::Cancelled,
        ];
        for s in &statuses {
            let json = serde_json::to_string(s).unwrap();
            let deser: QueryStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(&deser, s);
        }
    }
}
