use std::path::Path;

use crate::errors::AppError;
use crate::models::config::DataFormat;

/// Result of CSV validation and format detection.
pub struct ValidationResult {
    /// Whether the data is tick or bar (OHLCV) format.
    pub format: DataFormat,
    /// Whether the CSV has a header row.
    pub has_header: bool,
    /// The field delimiter byte (usually b',').
    pub delimiter: u8,
    /// Number of data rows sampled during validation.
    pub row_count_sample: usize,
    /// Number of columns detected.
    pub column_count: usize,
}

/// Validate a CSV file and detect its format (Tick or Bar).
///
/// Reads up to the first 100 rows to determine delimiter, header presence,
/// column count, and data format.
pub fn validate_csv(path: &Path) -> Result<ValidationResult, AppError> {
    if !path.exists() {
        return Err(AppError::FileNotFound(path.to_string_lossy().to_string()));
    }

    let content = std::fs::read_to_string(path)
        .map_err(|e| AppError::FileRead(format!("{}: {}", path.display(), e)))?;

    if content.trim().is_empty() {
        return Err(AppError::InvalidCsvFormat("File is empty".to_string()));
    }

    // Detect delimiter
    let delimiter = detect_delimiter(&content);

    // Split lines and find header
    let mut lines = content.lines().filter(|l| !l.trim().is_empty());

    let first_line = lines
        .next()
        .ok_or_else(|| AppError::InvalidCsvFormat("No lines found in CSV".to_string()))?;

    let headers: Vec<&str> = first_line
        .split(delimiter as char)
        .map(|s| s.trim())
        .collect();

    let column_count = headers.len();
    if column_count < 3 {
        return Err(AppError::CsvValidation(format!(
            "Expected at least 3 columns, found {}",
            column_count
        )));
    }

    // Determine if the first row is a header by checking if first cell parses as a number
    let has_header = is_header_row(&headers);

    // Detect format from column names (if header) or column count
    let format = if has_header {
        detect_format_from_headers(&headers)?
    } else {
        detect_format_from_column_count(column_count)?
    };

    // Count sample rows (skip header if present)
    let row_count_sample = lines.take(100).count();

    Ok(ValidationResult {
        format,
        has_header,
        delimiter,
        row_count_sample,
        column_count,
    })
}

/// Detect the field delimiter by checking comma vs semicolon vs tab counts in the first line.
fn detect_delimiter(content: &str) -> u8 {
    let first_line = content.lines().next().unwrap_or("");
    let comma_count = first_line.chars().filter(|&c| c == ',').count();
    let semicolon_count = first_line.chars().filter(|&c| c == ';').count();
    let tab_count = first_line.chars().filter(|&c| c == '\t').count();

    if tab_count > comma_count && tab_count > semicolon_count {
        b'\t'
    } else if semicolon_count > comma_count {
        b';'
    } else {
        b','
    }
}

/// Heuristic: if the first cell of the first row cannot be parsed as a float,
/// it's likely a header row (e.g. "DateTime", "Date", "Open").
fn is_header_row(cells: &[&str]) -> bool {
    if cells.is_empty() {
        return false;
    }
    let first = cells[0].trim().trim_matches('"');
    // If the first cell contains letters it's a header
    first.chars().any(|c| c.is_alphabetic())
}

/// Detect format from header column names.
fn detect_format_from_headers(headers: &[&str]) -> Result<DataFormat, AppError> {
    let normalized: Vec<String> = headers
        .iter()
        .map(|h| h.trim().trim_matches('"').to_lowercase())
        .collect();

    let has_bid = normalized.iter().any(|h| h == "bid");
    let has_ask = normalized.iter().any(|h| h == "ask");
    let has_open = normalized.iter().any(|h| h == "open");
    let has_high = normalized.iter().any(|h| h == "high");
    let has_low = normalized.iter().any(|h| h == "low");
    let has_close = normalized.iter().any(|h| h == "close");

    if has_bid && has_ask {
        Ok(DataFormat::Tick)
    } else if has_open && has_high && has_low && has_close {
        Ok(DataFormat::Bar)
    } else {
        Err(AppError::UnsupportedFormat(format!(
            "Cannot detect format from headers: {:?}",
            headers
        )))
    }
}

/// Detect format from column count when there's no header.
fn detect_format_from_column_count(count: usize) -> Result<DataFormat, AppError> {
    match count {
        // Tick: DateTime, Bid, Ask [, Volume] = 3 or 4 cols
        3 | 4 => Ok(DataFormat::Tick),
        // Bar formats:
        //   6 cols: DateTime, Open, High, Low, Close, Volume
        //   7 cols: Date, Time, Open, High, Low, Close, Volume
        //   8 cols: Date, Time, Open, High, Low, Close, TickVol, Volume
        //   9 cols: Date, Time, Open, High, Low, Close, TickVol, Volume, Spread (MT4/MT5)
        6 | 7 | 8 | 9 => Ok(DataFormat::Bar),
        _ => Err(AppError::UnsupportedFormat(format!(
            "Cannot detect format from {} columns (no header present)",
            count
        ))),
    }
}
