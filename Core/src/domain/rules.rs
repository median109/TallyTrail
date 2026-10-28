use chrono::{DateTime, Datelike, Days, Months, TimeZone, Utc};

use crate::domain::error::{CoreError, CoreResult};
use crate::domain::types::{Direction, Granularity, SplitRole, TimeBucket, TxnType};

pub const DEFAULT_CURRENCY: &str = "CNY";

pub fn validate_name(label: &str, value: &str) -> CoreResult<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CoreError::validation(format!("{label}不能为空")));
    }
    Ok(trimmed.to_string())
}

pub fn validate_currency(value: impl Into<String>) -> CoreResult<String> {
    let value = value.into();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(CoreError::validation("币种不能为空"));
    }
    Ok(trimmed.to_string())
}

pub fn default_currency(value: Option<String>) -> CoreResult<String> {
    match value {
        Some(currency) => validate_currency(currency),
        None => Ok(DEFAULT_CURRENCY.to_string()),
    }
}

pub fn ensure_positive(amount_cents: i64, field: &str) -> CoreResult<()> {
    if amount_cents <= 0 {
        return Err(CoreError::validation(format!("{field}必须大于 0")));
    }
    Ok(())
}

pub fn ensure_non_negative(amount_cents: i64, field: &str) -> CoreResult<()> {
    if amount_cents < 0 {
        return Err(CoreError::validation(format!("{field}不能为负数")));
    }
    Ok(())
}

pub fn checked_add(left: i64, right: i64) -> CoreResult<i64> {
    left.checked_add(right).ok_or(CoreError::AmountOverflow)
}

pub fn checked_sub(left: i64, right: i64) -> CoreResult<i64> {
    left.checked_sub(right).ok_or(CoreError::AmountOverflow)
}

pub fn checked_sum(values: impl IntoIterator<Item = i64>) -> CoreResult<i64> {
    values.into_iter().try_fold(0_i64, checked_add)
}

pub fn expected_direction(txn_type: TxnType) -> Direction {
    match txn_type {
        TxnType::Expense | TxnType::Transfer | TxnType::LoanRepay => Direction::Outflow,
        TxnType::Income | TxnType::Refund | TxnType::Reimbursement | TxnType::LoanDisburse => {
            Direction::Inflow
        }
    }
}

pub fn validate_direction(txn_type: TxnType, direction: Direction) -> CoreResult<()> {
    if expected_direction(txn_type) != direction {
        return Err(CoreError::InvalidDirection);
    }
    Ok(())
}

/// 校验结算行角色、金额与 actual_amount 的对应关系，以及转账的两条等额结算行规则。
pub fn validate_settlement_shape(
    txn_type: TxnType,
    roles: &[SplitRole],
    amounts: &[i64],
    actual_amount_cents: i64,
) -> CoreResult<()> {
    if roles.is_empty() {
        return Err(CoreError::validation("至少需要一条结算行"));
    }
    if roles.len() != amounts.len() {
        return Err(CoreError::validation("结算行角色与金额数量不一致"));
    }
    for amount in amounts {
        ensure_positive(*amount, "结算行金额")?;
    }
    ensure_positive(actual_amount_cents, "实际交易金额")?;

    let total = checked_sum(amounts.iter().copied())?;
    if txn_type == TxnType::Transfer {
        if roles.len() != 2 {
            return Err(CoreError::validation("转账必须恰好有两条结算行"));
        }
        let payment_count = roles
            .iter()
            .filter(|role| **role == SplitRole::Payment)
            .count();
        let receipt_count = roles
            .iter()
            .filter(|role| **role == SplitRole::Receipt)
            .count();
        if payment_count != 1 || receipt_count != 1 {
            return Err(CoreError::validation(
                "转账必须恰好包含一条 PAYMENT 和一条 RECEIPT",
            ));
        }
        let payment = roles
            .iter()
            .zip(amounts.iter())
            .find(|(role, _)| **role == SplitRole::Payment)
            .map(|(_, amount)| *amount)
            .ok_or_else(|| CoreError::validation("转账缺少 PAYMENT 结算行"))?;
        let receipt = roles
            .iter()
            .zip(amounts.iter())
            .find(|(role, _)| **role == SplitRole::Receipt)
            .map(|(_, amount)| *amount)
            .ok_or_else(|| CoreError::validation("转账缺少 RECEIPT 结算行"))?;
        if payment != receipt {
            return Err(CoreError::validation("转账的转出与转入金额必须相等"));
        }
        if payment != actual_amount_cents {
            return Err(CoreError::validation(
                "转账金额必须等于 actual_amount_cents",
            ));
        }
        return Ok(());
    }

    if total != actual_amount_cents {
        return Err(CoreError::validation(format!(
            "结算行金额之和 {total} 不等于实际交易金额 {actual_amount_cents}"
        )));
    }

    match txn_type {
        TxnType::Expense | TxnType::LoanRepay => {
            if roles.iter().any(|role| *role != SplitRole::Payment) {
                return Err(CoreError::validation(
                    "支出/还款交易的结算行角色必须全部为 PAYMENT",
                ));
            }
        }
        TxnType::Income | TxnType::Refund | TxnType::LoanDisburse => {
            if roles.iter().any(|role| *role != SplitRole::Receipt) {
                return Err(CoreError::validation(
                    "收入/退款/放款交易的结算行角色必须全部为 RECEIPT",
                ));
            }
        }
        TxnType::Reimbursement => {
            if roles.iter().any(|role| *role != SplitRole::ReimburseSource) {
                return Err(CoreError::validation(
                    "报销交易的结算行角色必须全部为 REIMBURSE_SOURCE",
                ));
            }
        }
        TxnType::Transfer => unreachable!(),
    }

    Ok(())
}

pub fn validate_time_range(from_at: i64, to_at: i64) -> CoreResult<()> {
    if from_at >= to_at {
        return Err(CoreError::validation("时间范围必须满足 from_at < to_at"));
    }
    Ok(())
}

fn utc_datetime(at: i64) -> CoreResult<DateTime<Utc>> {
    Utc.timestamp_opt(at, 0)
        .single()
        .ok_or_else(|| CoreError::validation(format!("非法 UTC 时间戳: {at}")))
}

pub fn bucket_start(at: i64, granularity: Granularity) -> CoreResult<i64> {
    let dt = utc_datetime(at)?;
    let date = dt.date_naive();
    let start_date = match granularity {
        Granularity::Daily => date,
        Granularity::Weekly => date
            .checked_sub_days(Days::new(date.weekday().num_days_from_monday() as u64))
            .ok_or_else(|| CoreError::validation("周起始日期计算溢出"))?,
        Granularity::Monthly => date
            .with_day(1)
            .ok_or_else(|| CoreError::validation("月起始日期计算失败"))?,
    };
    let midnight = start_date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| CoreError::validation("日期转时间失败"))?;
    Ok(Utc.from_utc_datetime(&midnight).timestamp())
}

pub fn next_bucket_start(start_at: i64, granularity: Granularity) -> CoreResult<i64> {
    let dt = utc_datetime(start_at)?;
    let next = match granularity {
        Granularity::Daily => dt.checked_add_days(Days::new(1)),
        Granularity::Weekly => dt.checked_add_days(Days::new(7)),
        Granularity::Monthly => dt.checked_add_months(Months::new(1)),
    }
    .ok_or_else(|| CoreError::validation("时间桶计算溢出"))?;
    Ok(next.timestamp())
}

pub fn time_buckets(
    from_at: i64,
    to_at: i64,
    granularity: Granularity,
) -> CoreResult<Vec<TimeBucket>> {
    validate_time_range(from_at, to_at)?;
    let mut start = bucket_start(from_at, granularity)?;
    let mut buckets = Vec::new();
    while start < to_at {
        let end = next_bucket_start(start, granularity)?;
        if end <= start {
            return Err(CoreError::validation("时间桶推进失败"));
        }
        buckets.push(TimeBucket {
            start_at: start,
            end_at: end,
        });
        if buckets.len() > 100_000 {
            return Err(CoreError::validation(
                "时间范围过大，请缩小范围或使用更粗粒度",
            ));
        }
        start = end;
    }
    Ok(buckets)
}

pub fn bucket_containing(at: i64, buckets: &[TimeBucket]) -> CoreResult<Option<usize>> {
    // 桶是连续且有序的，二分查找即可。
    let mut low = 0_usize;
    let mut high = buckets.len();
    while low < high {
        let mid = (low + high) / 2;
        let bucket = buckets[mid];
        if at < bucket.start_at {
            high = mid;
        } else if at >= bucket.end_at {
            low = mid + 1;
        } else {
            return Ok(Some(mid));
        }
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daily_bucket_is_utc_midnight() {
        let at = Utc
            .with_ymd_and_hms(2026, 10, 28, 13, 45, 0)
            .unwrap()
            .timestamp();
        let start = bucket_start(at, Granularity::Daily).unwrap();
        assert_eq!(
            start,
            Utc.with_ymd_and_hms(2026, 10, 28, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
        assert_eq!(
            next_bucket_start(start, Granularity::Daily).unwrap(),
            Utc.with_ymd_and_hms(2026, 10, 29, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
    }

    #[test]
    fn weekly_bucket_starts_monday() {
        let at = Utc
            .with_ymd_and_hms(2026, 10, 28, 13, 45, 0)
            .unwrap()
            .timestamp();
        let start = bucket_start(at, Granularity::Weekly).unwrap();
        assert_eq!(
            start,
            Utc.with_ymd_and_hms(2026, 10, 26, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
    }

    #[test]
    fn monthly_buckets_cover_range() {
        let from = Utc
            .with_ymd_and_hms(2026, 1, 15, 0, 0, 0)
            .unwrap()
            .timestamp();
        let to = Utc
            .with_ymd_and_hms(2026, 4, 2, 0, 0, 0)
            .unwrap()
            .timestamp();
        let buckets = time_buckets(from, to, Granularity::Monthly).unwrap();
        assert_eq!(buckets.len(), 4);
        assert_eq!(
            buckets[0].start_at,
            Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
        assert_eq!(
            buckets[3].start_at,
            Utc.with_ymd_and_hms(2026, 4, 1, 0, 0, 0)
                .unwrap()
                .timestamp()
        );
        assert_eq!(bucket_containing(from, &buckets).unwrap(), Some(0));
        assert_eq!(bucket_containing(to - 1, &buckets).unwrap(), Some(3));
        assert_eq!(
            bucket_containing(
                Utc.with_ymd_and_hms(2026, 5, 1, 0, 0, 0)
                    .unwrap()
                    .timestamp(),
                &buckets
            )
            .unwrap(),
            None
        );
    }
}
