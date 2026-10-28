use std::fmt;

use chrono::{Local, NaiveDate, NaiveDateTime, TimeZone, Utc};
use tallytrail_core::CoreError;

pub type CliResult<T> = Result<T, CliError>;

#[derive(Debug)]
pub struct CliError(pub String);

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CliError {}

impl From<CoreError> for CliError {
    fn from(value: CoreError) -> Self {
        Self(value.to_string())
    }
}

impl From<String> for CliError {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for CliError {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

/// 解析人类输入的十进制金额为整数分。最多两位小数，禁止浮点。
pub fn parse_cents(input: &str) -> CliResult<i64> {
    let text = input.trim();
    if text.is_empty() {
        return Err("金额不能为空".into());
    }
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    if body.is_empty() {
        return Err(format!("非法金额: {input}").into());
    }
    let mut parts = body.split('.');
    let whole_part = parts.next().unwrap_or("");
    let fraction_part = parts.next();
    if parts.next().is_some() {
        return Err(format!("非法金额（小数点过多）: {input}").into());
    }
    if whole_part.is_empty() && fraction_part.is_none() {
        return Err(format!("非法金额: {input}").into());
    }
    if !whole_part.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("非法金额: {input}").into());
    }
    let fraction = fraction_part.unwrap_or("");
    if !fraction.chars().all(|c| c.is_ascii_digit()) {
        return Err(format!("非法金额: {input}").into());
    }
    if fraction.len() > 2 {
        return Err(format!("金额最多两位小数: {input}").into());
    }
    let whole: i128 = if whole_part.is_empty() {
        0
    } else {
        whole_part
            .parse::<i128>()
            .map_err(|_| format!("非法金额: {input}"))?
    };
    let fraction_value: i128 = if fraction.is_empty() {
        0
    } else {
        fraction
            .parse::<i128>()
            .map_err(|_| format!("非法金额: {input}"))?
            * if fraction.len() == 1 { 10 } else { 1 }
    };
    let total = whole
        .checked_mul(100)
        .and_then(|value| value.checked_add(fraction_value))
        .ok_or_else(|| format!("金额超出范围: {input}"))?;
    let signed = if negative { -total } else { total };
    i64::try_from(signed).map_err(|_| format!("金额超出 i64 分范围: {input}").into())
}

pub fn format_cents(cents: i64) -> String {
    let value = (cents as i128).abs();
    let sign = if cents < 0 { "-" } else { "" };
    format!("{sign}{}.{:02}", value / 100, value % 100)
}

#[derive(Debug, Clone, Copy)]
pub struct ParsedTime {
    pub at: i64,
    pub date_only: bool,
}

pub fn parse_input_time(input: &str) -> CliResult<ParsedTime> {
    let text = input.trim();
    if let Ok(value) = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M") {
        return Ok(ParsedTime {
            at: local_timestamp(value),
            date_only: false,
        });
    }
    if let Ok(date) = NaiveDate::parse_from_str(text, "%Y-%m-%d") {
        let value = date
            .and_hms_opt(0, 0, 0)
            .ok_or_else(|| format!("非法日期: {input}"))?;
        return Ok(ParsedTime {
            // 仅日期输入按 UTC 自然日解释，保证月/周时间桶与 Core 的 UTC 边界一致。
            at: Utc.from_utc_datetime(&value).timestamp(),
            date_only: true,
        });
    }
    Err(format!("时间格式必须为 `YYYY-MM-DD HH:MM` 或 `YYYY-MM-DD`: {input}").into())
}

/// 作为范围起点：日期型取当日 00:00。
pub fn parse_from(input: &str) -> CliResult<i64> {
    Ok(parse_input_time(input)?.at)
}

/// 作为范围终点（左闭右开）：日期型包含整日，取次日 00:00（UTC 自然日）。
pub fn parse_to(input: &str) -> CliResult<i64> {
    let parsed = parse_input_time(input)?;
    if !parsed.date_only {
        return Ok(parsed.at);
    }
    parsed
        .at
        .checked_add(86_400)
        .ok_or_else(|| format!("日期溢出: {input}").into())
}

/// 作为“截止时刻”：日期型包含整日最后一秒。
pub fn parse_as_of(input: &str) -> CliResult<i64> {
    let parsed = parse_input_time(input)?;
    if parsed.date_only {
        Ok(parse_to(input)? - 1)
    } else {
        Ok(parsed.at)
    }
}

fn local_timestamp(value: NaiveDateTime) -> i64 {
    Local
        .from_local_datetime(&value)
        .earliest()
        .map(|dt| dt.timestamp())
        .unwrap_or_else(|| Local.from_utc_datetime(&value).timestamp())
}

pub fn format_at(at: i64) -> String {
    // 仅日期输入被规范化为 UTC 00:00；本地时区查看时直接显示日期，避免出现 08:00 这类偏移。
    if at.rem_euclid(86_400) == 0 {
        return format_date(at);
    }
    Local
        .timestamp_opt(at, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| at.to_string())
}

/// 时间桶边界与“仅日期”值统一按 UTC 日期展示，避免本地时区造成跨日偏移。
pub fn format_date(at: i64) -> String {
    Utc.timestamp_opt(at, 0)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| at.to_string())
}

pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    if rows.is_empty() {
        println!("（无数据）");
        return;
    }
    let mut widths: Vec<usize> = headers.iter().map(|header| display_width(header)).collect();
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            if index >= widths.len() {
                widths.push(0);
            }
            widths[index] = widths[index].max(display_width(cell));
        }
    }
    println!(
        "{}",
        headers
            .iter()
            .enumerate()
            .map(|(index, header)| pad(header, widths[index]))
            .collect::<Vec<_>>()
            .join("  ")
    );
    println!(
        "{}",
        widths
            .iter()
            .map(|width| "-".repeat(*width))
            .collect::<Vec<_>>()
            .join("  ")
    );
    for row in rows {
        println!(
            "{}",
            row.iter()
                .enumerate()
                .map(|(index, cell)| pad(cell, widths.get(index).copied().unwrap_or(0)))
                .collect::<Vec<_>>()
                .join("  ")
        );
    }
}

fn pad(value: &str, width: usize) -> String {
    let current = display_width(value);
    if current >= width {
        value.to_string()
    } else {
        format!("{value}{}", " ".repeat(width - current))
    }
}

fn display_width(value: &str) -> usize {
    value
        .chars()
        .map(|character| if is_wide(character) { 2 } else { 1 })
        .sum()
}

fn is_wide(character: char) -> bool {
    matches!(
        character as u32,
        0x1100..=0x115F
            | 0x2E80..=0xA4CF
            | 0xAC00..=0xD7A3
            | 0xF900..=0xFAFF
            | 0xFE10..=0xFE19
            | 0xFE30..=0xFE6F
            | 0xFF00..=0xFF60
            | 0xFFE0..=0xFFE6
            | 0x1F300..=0x1FAFF
            | 0x20000..=0x3FFFD
    )
}
