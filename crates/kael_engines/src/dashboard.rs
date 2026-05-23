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
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Maximum y-value in the series, or `None` if empty.
    pub fn max(&self) -> Option<f64> {
        self.points
            .iter()
            .map(|p| p.y)
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Mean of all y-values, or `None` if empty.
    pub fn mean(&self) -> Option<f64> {
        if self.points.is_empty() {
            return None;
        }
        let total: f64 = self.points.iter().map(|p| p.y).sum();
        Some(total / self.points.len() as f64)
    }

    /// Sum of all y-values.
    pub fn sum(&self) -> f64 {
        self.points.iter().map(|p| p.y).sum()
    }

    /// Sort points by x-value in ascending order.
    pub fn sort_by_x(&mut self) {
        self.points
            .sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
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

    /// Submit a new query job. Returns the assigned job id.
    pub fn submit(
        &mut self,
        query: String,
        filters: Vec<DataFilter>,
        group_by: Option<GroupBy>,
    ) -> String {
        let id = format!("job-{}", self.next_id);
        self.next_id += 1;
        self.jobs.push(QueryJob {
            id: id.clone(),
            query,
            filters,
            group_by,
            status: QueryStatus::Queued,
            created_at: 0,
            completed_at: None,
        });
        id
    }

    /// Cancel a queued or running job. Returns an error if the job is not found or already terminal.
    pub fn cancel(&mut self, id: &str) -> anyhow::Result<()> {
        let job = self
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("job '{id}' not found"))?;
        match job.status {
            QueryStatus::Queued | QueryStatus::Running => {
                job.status = QueryStatus::Cancelled;
                Ok(())
            }
            _ => anyhow::bail!("job '{id}' is already in terminal state"),
        }
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

    /// Parse a header row and store the column names.
    pub fn parse_header(&mut self, line: &str) -> Vec<String> {
        self.headers = line.split(',').map(|s| s.trim().to_string()).collect();
        self.headers.clone()
    }

    /// Parse a data row into key-value pairs using the stored headers.
    /// Returns an error if headers have not been parsed or column count mismatches.
    pub fn parse_row(&self, line: &str) -> anyhow::Result<Vec<(String, String)>> {
        if self.headers.is_empty() {
            anyhow::bail!("headers must be parsed before rows");
        }
        let values: Vec<&str> = line.split(',').collect();
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
            .map(|(h, v)| (h.clone(), v.trim().to_string()))
            .collect())
    }

    /// Validate that a row has the expected number of columns.
    pub fn validate_row(&self, line: &str) -> bool {
        if self.headers.is_empty() {
            return false;
        }
        line.split(',').count() == self.headers.len()
    }
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
    fn query_scheduler_submit_and_get() {
        let mut sched = QueryScheduler::new();
        let id = sched.submit("SELECT *".into(), vec![], None);
        assert!(sched.get(&id).is_some());
        assert_eq!(sched.get(&id).unwrap().status, QueryStatus::Queued);
        assert_eq!(sched.pending_count(), 1);
    }

    #[test]
    fn query_scheduler_cancel() {
        let mut sched = QueryScheduler::new();
        let id = sched.submit("SELECT 1".into(), vec![], None);
        assert!(sched.cancel(&id).is_ok());
        assert_eq!(sched.get(&id).unwrap().status, QueryStatus::Cancelled);
        assert!(sched.cancel(&id).is_err());
    }

    #[test]
    fn query_scheduler_cancel_unknown() {
        let mut sched = QueryScheduler::new();
        assert!(sched.cancel("nope").is_err());
    }

    #[test]
    fn query_scheduler_list_and_completed() {
        let mut sched = QueryScheduler::new();
        sched.submit("q1".into(), vec![], None);
        sched.submit("q2".into(), vec![], None);
        assert_eq!(sched.list().len(), 2);
        assert!(sched.completed_jobs().is_empty());
    }

    #[test]
    fn csv_importer_parse_header_and_row() {
        let mut csv = CsvImporter::new();
        let headers = csv.parse_header("name, age, city");
        assert_eq!(headers, vec!["name", "age", "city"]);
        let row = csv.parse_row("Alice, 30, NYC").unwrap();
        assert_eq!(row.len(), 3);
        assert_eq!(row[0], ("name".into(), "Alice".into()));
        assert_eq!(row[1], ("age".into(), "30".into()));
    }

    #[test]
    fn csv_importer_row_without_header() {
        let csv = CsvImporter::new();
        assert!(csv.parse_row("a,b,c").is_err());
    }

    #[test]
    fn csv_importer_column_mismatch() {
        let mut csv = CsvImporter::new();
        csv.parse_header("a,b");
        assert!(csv.parse_row("1,2,3").is_err());
    }

    #[test]
    fn csv_importer_validate_row() {
        let mut csv = CsvImporter::new();
        assert!(!csv.validate_row("x,y"));
        csv.parse_header("a,b");
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
