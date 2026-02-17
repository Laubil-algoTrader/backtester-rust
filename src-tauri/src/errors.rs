use serde::Serialize;

/// All application errors, categorized by domain.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    // ── Data / Import ──
    #[error("Invalid CSV format: {0}")]
    InvalidCsvFormat(String),

    #[error("CSV validation failed: {0}")]
    CsvValidation(String),

    #[error("Unsupported data format: {0}")]
    UnsupportedFormat(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Failed to read file: {0}")]
    FileRead(String),

    #[error("Failed to write file: {0}")]
    FileWrite(String),

    #[error("Parquet conversion failed: {0}")]
    ParquetConversion(String),

    #[error("Timeframe conversion failed: {0}")]
    TimeframeConversion(String),

    #[error("CSV parse error at row {row}: {message}")]
    CsvParseError { row: usize, message: String },

    // ── Database ──
    #[error("Database error: {0}")]
    Database(String),

    #[error("Record not found: {0}")]
    NotFound(String),

    // ── Strategy ──
    #[error("Invalid strategy: {0}")]
    InvalidStrategy(String),

    #[error("Invalid rule: {0}")]
    InvalidRule(String),

    #[error("Strategy not found: {0}")]
    StrategyNotFound(String),

    // ── Backtest ──
    #[error("Backtest execution error: {0}")]
    BacktestExecution(String),

    #[error("Backtest cancelled")]
    BacktestCancelled,

    #[error("No data available for the specified date range")]
    NoDataInRange,

    #[error("Insufficient data for indicator calculation: need {needed} bars, got {available}")]
    InsufficientData { needed: usize, available: usize },

    // ── Indicator ──
    #[error("Invalid indicator parameters: {0}")]
    InvalidIndicatorParams(String),

    // ── Optimization ──
    #[error("Optimization error: {0}")]
    OptimizationError(String),

    #[error("Optimization cancelled")]
    OptimizationCancelled,

    // ── Download ──
    #[error("Download failed: {0}")]
    DownloadError(String),

    #[error("Download cancelled")]
    DownloadCancelled,

    #[error("Too many combinations: {count} exceeds limit of {limit}")]
    TooManyCombinations { count: usize, limit: usize },

    // ── Configuration ──
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    // ── Serialization ──
    #[error("Serialization error: {0}")]
    Serialization(String),

    // ── General ──
    #[error("Internal error: {0}")]
    Internal(String),
}

/// Serializable error response for the frontend.
#[derive(Debug, Serialize, Clone)]
pub struct ErrorResponse {
    pub code: String,
    pub message: String,
}

impl From<&AppError> for ErrorResponse {
    fn from(err: &AppError) -> Self {
        let code = match err {
            AppError::InvalidCsvFormat(_) => "INVALID_CSV_FORMAT",
            AppError::CsvValidation(_) => "CSV_VALIDATION",
            AppError::UnsupportedFormat(_) => "UNSUPPORTED_FORMAT",
            AppError::FileNotFound(_) => "FILE_NOT_FOUND",
            AppError::FileRead(_) => "FILE_READ",
            AppError::FileWrite(_) => "FILE_WRITE",
            AppError::ParquetConversion(_) => "PARQUET_CONVERSION",
            AppError::TimeframeConversion(_) => "TIMEFRAME_CONVERSION",
            AppError::CsvParseError { .. } => "CSV_PARSE_ERROR",
            AppError::Database(_) => "DATABASE",
            AppError::NotFound(_) => "NOT_FOUND",
            AppError::InvalidStrategy(_) => "INVALID_STRATEGY",
            AppError::InvalidRule(_) => "INVALID_RULE",
            AppError::StrategyNotFound(_) => "STRATEGY_NOT_FOUND",
            AppError::BacktestExecution(_) => "BACKTEST_EXECUTION",
            AppError::BacktestCancelled => "BACKTEST_CANCELLED",
            AppError::NoDataInRange => "NO_DATA_IN_RANGE",
            AppError::InsufficientData { .. } => "INSUFFICIENT_DATA",
            AppError::InvalidIndicatorParams(_) => "INVALID_INDICATOR_PARAMS",
            AppError::OptimizationError(_) => "OPTIMIZATION_ERROR",
            AppError::OptimizationCancelled => "OPTIMIZATION_CANCELLED",
            AppError::TooManyCombinations { .. } => "TOO_MANY_COMBINATIONS",
            AppError::DownloadError(_) => "DOWNLOAD_ERROR",
            AppError::DownloadCancelled => "DOWNLOAD_CANCELLED",
            AppError::InvalidConfig(_) => "INVALID_CONFIG",
            AppError::Serialization(_) => "SERIALIZATION",
            AppError::Internal(_) => "INTERNAL",
        };
        ErrorResponse {
            code: code.to_string(),
            message: err.to_string(),
        }
    }
}

// Allow AppError to be returned from Tauri commands.
impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let response = ErrorResponse::from(self);
        response.serialize(serializer)
    }
}

// ── Conversions from external errors ──

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        AppError::Database(err.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        AppError::Serialization(err.to_string())
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        AppError::FileRead(err.to_string())
    }
}

impl From<polars::error::PolarsError> for AppError {
    fn from(err: polars::error::PolarsError) -> Self {
        AppError::ParquetConversion(err.to_string())
    }
}
