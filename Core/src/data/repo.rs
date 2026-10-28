use std::str::FromStr;

use rusqlite::types::{FromSqlError, Value};
use rusqlite::{params, params_from_iter, Connection, OptionalExtension, Row};

use crate::domain::error::{CoreError, CoreResult};
use crate::domain::rules::{checked_add, checked_sub};
use crate::domain::types::{
    AccountType, BudgetScope, CategoryKind, Direction, LoanStatus, PeriodType, SplitRole,
    TransactionFilter, TxnStatus, TxnType,
};

#[derive(Debug, Clone)]
pub(crate) struct AccountRow {
    pub id: String,
    pub name: String,
    pub account_type: AccountType,
    pub currency: String,
    pub initial_balance_cents: i64,
    pub is_active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct CategoryRow {
    pub id: String,
    pub parent_id: Option<String>,
    pub name: String,
    pub level: i32,
    pub kind: CategoryKind,
    pub sort_order: i32,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct TagRow {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub(crate) struct TxnRow {
    pub id: String,
    pub txn_type: TxnType,
    pub occurred_at: i64,
    pub category_id: Option<String>,
    pub category_name: Option<String>,
    pub note: Option<String>,
    pub total_amount_cents: i64,
    pub currency: String,
    pub actual_amount_cents: i64,
    pub direction: Direction,
    pub ref_txn_id: Option<String>,
    pub loan_id: Option<String>,
    pub principal_part_cents: Option<i64>,
    pub interest_part_cents: Option<i64>,
    pub status: TxnStatus,
    pub voided_at: Option<i64>,
    pub void_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone)]
pub(crate) struct SettlementRow {
    pub id: String,
    pub account_id: String,
    pub account_name: String,
    pub currency: String,
    pub amount_cents: i64,
    pub role: SplitRole,
}

#[derive(Debug, Clone)]
pub(crate) struct LoanRow {
    pub id: String,
    pub lender: String,
    pub principal_cents: i64,
    pub currency: String,
    pub interest_rate_bp: i32,
    pub start_at: i64,
    pub term_months: Option<i32>,
    pub account_id: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct BudgetRow {
    pub id: String,
    pub name: String,
    pub scope: BudgetScope,
    pub category_id: Option<String>,
    pub period_type: PeriodType,
    pub amount_cents: i64,
    pub currency: String,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub rollover: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct FlowRow {
    pub txn_type: TxnType,
    pub category_id: Option<String>,
    pub occurred_at: i64,
    pub currency: String,
    pub total_amount_cents: i64,
    pub interest_part_cents: i64,
}

pub(crate) struct TxnInsert<'a> {
    pub id: &'a str,
    pub txn_type: TxnType,
    pub occurred_at: i64,
    pub category_id: Option<&'a str>,
    pub note: Option<&'a str>,
    pub total_amount_cents: i64,
    pub currency: &'a str,
    pub actual_amount_cents: i64,
    pub direction: Direction,
    pub ref_txn_id: Option<&'a str>,
    pub loan_id: Option<&'a str>,
    pub principal_part_cents: Option<i64>,
    pub interest_part_cents: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn enum_from_db<T>(value: String, column: usize) -> rusqlite::Result<T>
where
    T: FromStr<Err = CoreError>,
{
    T::from_str(&value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(FromSqlError::Other(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                err.to_string(),
            )))),
        )
    })
}

fn map_account(row: &Row<'_>) -> rusqlite::Result<AccountRow> {
    Ok(AccountRow {
        id: row.get(0)?,
        name: row.get(1)?,
        account_type: enum_from_db(row.get::<_, String>(2)?, 2)?,
        currency: row.get(3)?,
        initial_balance_cents: row.get(4)?,
        is_active: row.get::<_, i64>(5)? != 0,
        created_at: row.get(6)?,
    })
}

fn map_category(row: &Row<'_>) -> rusqlite::Result<CategoryRow> {
    Ok(CategoryRow {
        id: row.get(0)?,
        parent_id: row.get(1)?,
        name: row.get(2)?,
        level: row.get(3)?,
        kind: enum_from_db(row.get::<_, String>(4)?, 4)?,
        sort_order: row.get(5)?,
        is_active: row.get::<_, i64>(6)? != 0,
    })
}

fn map_tag(row: &Row<'_>) -> rusqlite::Result<TagRow> {
    Ok(TagRow {
        id: row.get(0)?,
        name: row.get(1)?,
    })
}

fn map_txn(row: &Row<'_>) -> rusqlite::Result<TxnRow> {
    Ok(TxnRow {
        id: row.get(0)?,
        txn_type: enum_from_db(row.get::<_, String>(1)?, 1)?,
        occurred_at: row.get(2)?,
        category_id: row.get(3)?,
        category_name: row.get(4)?,
        note: row.get(5)?,
        total_amount_cents: row.get(6)?,
        currency: row.get(7)?,
        actual_amount_cents: row.get(8)?,
        direction: enum_from_db(row.get::<_, String>(9)?, 9)?,
        ref_txn_id: row.get(10)?,
        loan_id: row.get(11)?,
        principal_part_cents: row.get(12)?,
        interest_part_cents: row.get(13)?,
        status: enum_from_db(row.get::<_, String>(14)?, 14)?,
        voided_at: row.get(15)?,
        void_reason: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

fn map_loan(row: &Row<'_>) -> rusqlite::Result<LoanRow> {
    Ok(LoanRow {
        id: row.get(0)?,
        lender: row.get(1)?,
        principal_cents: row.get(2)?,
        currency: row.get(3)?,
        interest_rate_bp: row.get(4)?,
        start_at: row.get(5)?,
        term_months: row.get(6)?,
        account_id: row.get(7)?,
        note: row.get(9)?,
    })
}

fn map_budget(row: &Row<'_>) -> rusqlite::Result<BudgetRow> {
    Ok(BudgetRow {
        id: row.get(0)?,
        name: row.get(1)?,
        scope: enum_from_db(row.get::<_, String>(2)?, 2)?,
        category_id: row.get(3)?,
        period_type: enum_from_db(row.get::<_, String>(4)?, 4)?,
        amount_cents: row.get(5)?,
        currency: row.get(6)?,
        start_at: row.get(7)?,
        end_at: row.get(8)?,
        rollover: row.get::<_, i64>(9)? != 0,
        is_active: row.get::<_, i64>(10)? != 0,
    })
}

// ---------------------------------------------------------------------------
// account
// ---------------------------------------------------------------------------

pub(crate) fn insert_account(
    conn: &Connection,
    id: &str,
    name: &str,
    account_type: AccountType,
    currency: &str,
    initial_balance_cents: i64,
    created_at: i64,
) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO account (id, name, type, currency, initial_balance_cents, is_active, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
        params![
            id,
            name,
            account_type.as_db_str(),
            currency,
            initial_balance_cents,
            created_at
        ],
    )?;
    Ok(())
}

pub(crate) fn get_account(conn: &Connection, id: &str) -> CoreResult<Option<AccountRow>> {
    Ok(conn
        .query_row(
            "SELECT id, name, type, currency, initial_balance_cents, is_active, created_at
             FROM account WHERE id = ?1",
            params![id],
            map_account,
        )
        .optional()?)
}

pub(crate) fn list_account_rows(
    conn: &Connection,
    include_inactive: bool,
) -> CoreResult<Vec<AccountRow>> {
    let mut sql =
        "SELECT id, name, type, currency, initial_balance_cents, is_active, created_at FROM account"
            .to_string();
    if !include_inactive {
        sql.push_str(" WHERE is_active = 1");
    }
    sql.push_str(" ORDER BY created_at, name");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([], map_account)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn rename_account(conn: &Connection, id: &str, name: &str) -> CoreResult<()> {
    conn.execute(
        "UPDATE account SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(())
}

pub(crate) fn set_account_active(conn: &Connection, id: &str, active: bool) -> CoreResult<()> {
    conn.execute(
        "UPDATE account SET is_active = ?1 WHERE id = ?2",
        params![i64::from(active), id],
    )?;
    Ok(())
}

pub(crate) fn settlement_balance(
    conn: &Connection,
    account_id: &str,
    as_of: i64,
) -> CoreResult<i64> {
    let mut stmt = conn.prepare(
        "SELECT s.role, s.amount_cents
         FROM settlement_split s
         JOIN txn t ON t.id = s.txn_id
         WHERE s.account_id = ?1 AND t.status = 'CONFIRMED' AND t.occurred_at <= ?2",
    )?;
    let rows = stmt.query_map(params![account_id, as_of], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut total = 0_i64;
    for row in rows {
        let (role, amount_cents) = row?;
        total = if role == SplitRole::Payment.as_db_str() {
            checked_sub(total, amount_cents)?
        } else {
            checked_add(total, amount_cents)?
        };
    }
    Ok(total)
}

// ---------------------------------------------------------------------------
// category
// ---------------------------------------------------------------------------

pub(crate) fn insert_category(
    conn: &Connection,
    id: &str,
    parent_id: Option<&str>,
    name: &str,
    level: i32,
    kind: CategoryKind,
    sort_order: i32,
) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO category (id, parent_id, name, level, kind, sort_order, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
        params![id, parent_id, name, level, kind.as_db_str(), sort_order],
    )?;
    Ok(())
}

pub(crate) fn get_category(conn: &Connection, id: &str) -> CoreResult<Option<CategoryRow>> {
    Ok(conn
        .query_row(
            "SELECT id, parent_id, name, level, kind, sort_order, is_active
             FROM category WHERE id = ?1",
            params![id],
            map_category,
        )
        .optional()?)
}

pub(crate) fn list_category_rows(
    conn: &Connection,
    kind: Option<CategoryKind>,
    include_inactive: bool,
) -> CoreResult<Vec<CategoryRow>> {
    let mut sql =
        "SELECT id, parent_id, name, level, kind, sort_order, is_active FROM category WHERE 1=1"
            .to_string();
    let mut values: Vec<Value> = Vec::new();
    if let Some(kind) = kind {
        sql.push_str(" AND kind = ?");
        values.push(Value::Text(kind.as_db_str().to_string()));
    }
    if !include_inactive {
        sql.push_str(" AND is_active = 1");
    }
    sql.push_str(" ORDER BY level, sort_order, name");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(values.iter()), map_category)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn category_name_exists(
    conn: &Connection,
    parent_id: Option<&str>,
    name: &str,
    exclude_id: Option<&str>,
) -> CoreResult<bool> {
    let count: i64 = match parent_id {
        Some(parent) => conn.query_row(
            "SELECT COUNT(*) FROM category
             WHERE parent_id = ?1 AND name = ?2 COLLATE NOCASE AND (?3 IS NULL OR id <> ?3)",
            params![parent, name, exclude_id],
            |row| row.get(0),
        )?,
        None => conn.query_row(
            "SELECT COUNT(*) FROM category
             WHERE parent_id IS NULL AND name = ?1 COLLATE NOCASE AND (?2 IS NULL OR id <> ?2)",
            params![name, exclude_id],
            |row| row.get(0),
        )?,
    };
    Ok(count > 0)
}

pub(crate) fn category_has_children(conn: &Connection, id: &str) -> CoreResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM category WHERE parent_id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub(crate) fn category_reference_count(conn: &Connection, id: &str) -> CoreResult<i64> {
    let txn_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM txn WHERE category_id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    let budget_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM budget WHERE category_id = ?1",
        params![id],
        |row| row.get(0),
    )?;
    Ok(txn_count + budget_count)
}

pub(crate) fn category_children_ids(conn: &Connection, id: &str) -> CoreResult<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT id FROM category WHERE parent_id = ?1 ORDER BY sort_order, name")?;
    let rows = stmt.query_map(params![id], |row| row.get::<_, String>(0))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn rename_category(conn: &Connection, id: &str, name: &str) -> CoreResult<()> {
    conn.execute(
        "UPDATE category SET name = ?1 WHERE id = ?2",
        params![name, id],
    )?;
    Ok(())
}

pub(crate) fn set_category_active(conn: &Connection, id: &str, active: bool) -> CoreResult<()> {
    conn.execute(
        "UPDATE category SET is_active = ?1 WHERE id = ?2",
        params![i64::from(active), id],
    )?;
    Ok(())
}

pub(crate) fn set_category_kind(conn: &Connection, id: &str, kind: CategoryKind) -> CoreResult<()> {
    conn.execute(
        "UPDATE category SET kind = ?1 WHERE id = ?2",
        params![kind.as_db_str(), id],
    )?;
    Ok(())
}

pub(crate) fn delete_category(conn: &Connection, id: &str) -> CoreResult<()> {
    conn.execute("DELETE FROM category WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// tag
// ---------------------------------------------------------------------------

pub(crate) fn insert_tag(conn: &Connection, id: &str, name: &str) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO tag (id, name) VALUES (?1, ?2)",
        params![id, name],
    )?;
    Ok(())
}

pub(crate) fn get_tag(conn: &Connection, id: &str) -> CoreResult<Option<TagRow>> {
    Ok(conn
        .query_row(
            "SELECT id, name FROM tag WHERE id = ?1",
            params![id],
            map_tag,
        )
        .optional()?)
}

pub(crate) fn tag_name_exists(
    conn: &Connection,
    name: &str,
    exclude_id: Option<&str>,
) -> CoreResult<bool> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tag WHERE name = ?1 COLLATE NOCASE AND (?2 IS NULL OR id <> ?2)",
        params![name, exclude_id],
        |row| row.get(0),
    )?;
    Ok(count > 0)
}

pub(crate) fn list_tag_rows(conn: &Connection) -> CoreResult<Vec<TagRow>> {
    let mut stmt = conn.prepare("SELECT id, name FROM tag ORDER BY name COLLATE NOCASE")?;
    let rows = stmt.query_map([], map_tag)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn rename_tag(conn: &Connection, id: &str, name: &str) -> CoreResult<()> {
    conn.execute("UPDATE tag SET name = ?1 WHERE id = ?2", params![name, id])?;
    Ok(())
}

pub(crate) fn delete_tag(conn: &Connection, id: &str) -> CoreResult<()> {
    conn.execute("DELETE FROM tag WHERE id = ?1", params![id])?;
    Ok(())
}

pub(crate) fn link_txn_tag(conn: &Connection, txn_id: &str, tag_id: &str) -> CoreResult<()> {
    conn.execute(
        "INSERT OR IGNORE INTO txn_tag (txn_id, tag_id) VALUES (?1, ?2)",
        params![txn_id, tag_id],
    )?;
    Ok(())
}

pub(crate) fn tags_for_txn(conn: &Connection, txn_id: &str) -> CoreResult<Vec<TagRow>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name FROM tag t
         JOIN txn_tag tt ON tt.tag_id = t.id
         WHERE tt.txn_id = ?1 ORDER BY t.name COLLATE NOCASE",
    )?;
    let rows = stmt.query_map(params![txn_id], map_tag)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------------------
// txn
// ---------------------------------------------------------------------------

const TXN_SELECT: &str = "SELECT t.id, t.type, t.occurred_at, t.category_id, c.name, t.note,
        t.total_amount_cents, t.currency, t.actual_amount_cents, t.direction,
        t.ref_txn_id, t.loan_id, t.principal_part_cents, t.interest_part_cents,
        t.status, t.voided_at, t.void_reason, t.created_at, t.updated_at
     FROM txn t LEFT JOIN category c ON c.id = t.category_id";

pub(crate) fn insert_transaction(conn: &Connection, input: &TxnInsert<'_>) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO txn (
            id, type, occurred_at, category_id, note, total_amount_cents, currency,
            actual_amount_cents, direction, ref_txn_id, loan_id, principal_part_cents,
            interest_part_cents, status, voided_at, void_reason, created_at, updated_at
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13,
            'CONFIRMED', NULL, NULL, ?14, ?15
         )",
        params![
            input.id,
            input.txn_type.as_db_str(),
            input.occurred_at,
            input.category_id,
            input.note,
            input.total_amount_cents,
            input.currency,
            input.actual_amount_cents,
            input.direction.as_db_str(),
            input.ref_txn_id,
            input.loan_id,
            input.principal_part_cents,
            input.interest_part_cents,
            input.created_at,
            input.updated_at
        ],
    )?;
    Ok(())
}

pub(crate) fn insert_settlement(
    conn: &Connection,
    id: &str,
    txn_id: &str,
    account_id: &str,
    amount_cents: i64,
    role: SplitRole,
) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO settlement_split (id, txn_id, account_id, amount_cents, role)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, txn_id, account_id, amount_cents, role.as_db_str()],
    )?;
    Ok(())
}

pub(crate) fn get_txn_row(conn: &Connection, id: &str) -> CoreResult<Option<TxnRow>> {
    let sql = format!("{TXN_SELECT} WHERE t.id = ?1");
    Ok(conn.query_row(&sql, params![id], map_txn).optional()?)
}

pub(crate) fn list_txn_rows(
    conn: &Connection,
    filter: &TransactionFilter,
) -> CoreResult<Vec<TxnRow>> {
    let mut sql = String::from(TXN_SELECT);
    sql.push_str(" WHERE 1=1");
    let mut values: Vec<Value> = Vec::new();

    if let Some(from_at) = filter.from_at {
        sql.push_str(" AND t.occurred_at >= ?");
        values.push(Value::Integer(from_at));
    }
    if let Some(to_at) = filter.to_at {
        sql.push_str(" AND t.occurred_at < ?");
        values.push(Value::Integer(to_at));
    }
    if let Some(types) = &filter.types {
        if types.is_empty() {
            sql.push_str(" AND 1=0");
        } else {
            sql.push_str(" AND t.type IN (");
            for (index, txn_type) in types.iter().enumerate() {
                if index > 0 {
                    sql.push(',');
                }
                sql.push('?');
                values.push(Value::Text(txn_type.as_db_str().to_string()));
            }
            sql.push(')');
        }
    }
    if let Some(category_id) = &filter.category_id {
        sql.push_str(
            " AND (t.category_id = ? OR t.category_id IN
               (SELECT id FROM category WHERE parent_id = ?))",
        );
        values.push(Value::Text(category_id.clone()));
        values.push(Value::Text(category_id.clone()));
    }
    if let Some(account_id) = &filter.account_id {
        sql.push_str(
            " AND EXISTS (SELECT 1 FROM settlement_split s
               WHERE s.txn_id = t.id AND s.account_id = ?)",
        );
        values.push(Value::Text(account_id.clone()));
    }
    if let Some(tag_ids) = &filter.tag_ids {
        if tag_ids.is_empty() {
            sql.push_str(" AND 1=0");
        } else {
            for tag_id in tag_ids {
                sql.push_str(
                    " AND EXISTS (SELECT 1 FROM txn_tag tt
                       WHERE tt.txn_id = t.id AND tt.tag_id = ?)",
                );
                values.push(Value::Text(tag_id.clone()));
            }
        }
    }
    if let Some(currency) = &filter.currency {
        sql.push_str(" AND t.currency = ?");
        values.push(Value::Text(currency.clone()));
    }
    if let Some(status) = filter.status {
        sql.push_str(" AND t.status = ?");
        values.push(Value::Text(status.as_db_str().to_string()));
    }

    sql.push_str(" ORDER BY t.occurred_at DESC, t.created_at DESC, t.id");

    if let Some(limit) = filter.limit {
        sql.push_str(" LIMIT ?");
        values.push(Value::Integer(i64::from(limit)));
        if let Some(offset) = filter.offset {
            sql.push_str(" OFFSET ?");
            values.push(Value::Integer(i64::from(offset)));
        }
    } else if let Some(offset) = filter.offset {
        sql.push_str(" LIMIT -1 OFFSET ?");
        values.push(Value::Integer(i64::from(offset)));
    }

    let rows = {
        let mut stmt = conn.prepare(&sql)?;
        let mapped = stmt.query_map(params_from_iter(values.iter()), map_txn)?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

pub(crate) fn settlements_for_txn(
    conn: &Connection,
    txn_id: &str,
) -> CoreResult<Vec<SettlementRow>> {
    let mut stmt = conn.prepare(
        "SELECT s.id, s.account_id, a.name, a.currency, s.amount_cents, s.role
         FROM settlement_split s JOIN account a ON a.id = s.account_id
         WHERE s.txn_id = ?1
         ORDER BY CASE s.role WHEN 'PAYMENT' THEN 0 WHEN 'RECEIPT' THEN 1 ELSE 2 END, s.id",
    )?;
    let rows = stmt.query_map(params![txn_id], |row| {
        Ok(SettlementRow {
            id: row.get(0)?,
            account_id: row.get(1)?,
            account_name: row.get(2)?,
            currency: row.get(3)?,
            amount_cents: row.get(4)?,
            role: enum_from_db(row.get::<_, String>(5)?, 5)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn mark_voided(
    conn: &Connection,
    id: &str,
    voided_at: i64,
    reason: &str,
) -> CoreResult<()> {
    conn.execute(
        "UPDATE txn SET status = 'VOIDED', voided_at = ?1, void_reason = ?2, updated_at = ?1
         WHERE id = ?3 AND status = 'CONFIRMED'",
        params![voided_at, reason, id],
    )?;
    Ok(())
}

pub(crate) fn active_dependent_count(conn: &Connection, txn_id: &str) -> CoreResult<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM txn
         WHERE ref_txn_id = ?1 AND status = 'CONFIRMED'
           AND type IN ('REFUND','REIMBURSEMENT')",
        params![txn_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub(crate) fn dependent_total_cents(
    conn: &Connection,
    ref_txn_id: &str,
    txn_type: TxnType,
) -> CoreResult<i64> {
    let mut stmt = conn.prepare(
        "SELECT total_amount_cents FROM txn
         WHERE ref_txn_id = ?1 AND type = ?2 AND status = 'CONFIRMED'",
    )?;
    let rows = stmt.query_map(params![ref_txn_id, txn_type.as_db_str()], |row| {
        row.get::<_, i64>(0)
    })?;
    let mut total = 0_i64;
    for row in rows {
        total = checked_add(total, row?)?;
    }
    Ok(total)
}

pub(crate) fn confirmed_repayment_count(conn: &Connection, loan_id: &str) -> CoreResult<i64> {
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM txn
         WHERE loan_id = ?1 AND status = 'CONFIRMED' AND type = 'LOAN_REPAY'",
        params![loan_id],
        |row| row.get(0),
    )?;
    Ok(count)
}

pub(crate) fn flow_rows(
    conn: &Connection,
    from_at: i64,
    to_at: i64,
    currency: Option<&str>,
) -> CoreResult<Vec<FlowRow>> {
    let mut sql = "SELECT type, category_id, occurred_at, currency, total_amount_cents,
                          COALESCE(interest_part_cents, 0)
                   FROM txn
                   WHERE status = 'CONFIRMED' AND occurred_at >= ?1 AND occurred_at < ?2
                     AND type IN ('EXPENSE','INCOME','REFUND','REIMBURSEMENT','LOAN_REPAY')"
        .to_string();
    if currency.is_some() {
        sql.push_str(" AND currency = ?3");
    }
    sql.push_str(" ORDER BY occurred_at");
    let mut stmt = conn.prepare(&sql)?;
    let map = |row: &Row<'_>| -> rusqlite::Result<FlowRow> {
        Ok(FlowRow {
            txn_type: enum_from_db(row.get::<_, String>(0)?, 0)?,
            category_id: row.get(1)?,
            occurred_at: row.get(2)?,
            currency: row.get(3)?,
            total_amount_cents: row.get(4)?,
            interest_part_cents: row.get(5)?,
        })
    };
    let rows = if let Some(currency) = currency {
        stmt.query_map(params![from_at, to_at, currency], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        stmt.query_map(params![from_at, to_at], map)?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(rows)
}

// ---------------------------------------------------------------------------
// loan
// ---------------------------------------------------------------------------

pub(crate) struct NewLoan<'a> {
    pub id: &'a str,
    pub lender: &'a str,
    pub principal_cents: i64,
    pub currency: &'a str,
    pub interest_rate_bp: i32,
    pub start_at: i64,
    pub term_months: Option<i32>,
    pub account_id: Option<&'a str>,
    pub note: Option<&'a str>,
}

pub(crate) fn insert_loan(conn: &Connection, loan: NewLoan<'_>) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO loan (id, lender, principal_cents, currency, interest_rate_bp,
                           start_at, term_months, account_id, status, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'ACTIVE', ?9)",
        params![
            loan.id,
            loan.lender,
            loan.principal_cents,
            loan.currency,
            loan.interest_rate_bp,
            loan.start_at,
            loan.term_months,
            loan.account_id,
            loan.note
        ],
    )?;
    Ok(())
}

pub(crate) fn get_loan(conn: &Connection, id: &str) -> CoreResult<Option<LoanRow>> {
    Ok(conn
        .query_row(
            "SELECT id, lender, principal_cents, currency, interest_rate_bp, start_at,
                    term_months, account_id, status, note
             FROM loan WHERE id = ?1",
            params![id],
            map_loan,
        )
        .optional()?)
}

pub(crate) fn list_loan_rows(conn: &Connection) -> CoreResult<Vec<LoanRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, lender, principal_cents, currency, interest_rate_bp, start_at,
                term_months, account_id, status, note
         FROM loan ORDER BY start_at DESC, id",
    )?;
    let rows = stmt.query_map([], map_loan)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn loan_repaid_principal(conn: &Connection, loan_id: &str) -> CoreResult<i64> {
    loan_repaid_principal_as_of(conn, loan_id, i64::MAX)
}

pub(crate) fn loan_repaid_principal_as_of(
    conn: &Connection,
    loan_id: &str,
    as_of: i64,
) -> CoreResult<i64> {
    let mut stmt = conn.prepare(
        "SELECT COALESCE(principal_part_cents, 0) FROM txn
         WHERE loan_id = ?1 AND type = 'LOAN_REPAY' AND status = 'CONFIRMED'
           AND occurred_at <= ?2",
    )?;
    let rows = stmt.query_map(params![loan_id, as_of], |row| row.get::<_, i64>(0))?;
    let mut total = 0_i64;
    for row in rows {
        total = checked_add(total, row?)?;
    }
    Ok(total)
}

pub(crate) fn update_loan_status(
    conn: &Connection,
    loan_id: &str,
    status: LoanStatus,
) -> CoreResult<()> {
    conn.execute(
        "UPDATE loan SET status = ?1 WHERE id = ?2",
        params![status.as_db_str(), loan_id],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// budget
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub(crate) fn insert_budget(
    conn: &Connection,
    id: &str,
    name: &str,
    scope: BudgetScope,
    category_id: Option<&str>,
    period_type: PeriodType,
    amount_cents: i64,
    currency: &str,
    start_at: Option<i64>,
    end_at: Option<i64>,
    rollover: bool,
) -> CoreResult<()> {
    conn.execute(
        "INSERT INTO budget (id, name, scope, category_id, period_type, amount_cents,
                             currency, start_date, end_date, rollover, is_active)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 1)",
        params![
            id,
            name,
            scope.as_db_str(),
            category_id,
            period_type.as_db_str(),
            amount_cents,
            currency,
            start_at,
            end_at,
            i64::from(rollover)
        ],
    )?;
    Ok(())
}

pub(crate) fn get_budget(conn: &Connection, id: &str) -> CoreResult<Option<BudgetRow>> {
    Ok(conn
        .query_row(
            "SELECT id, name, scope, category_id, period_type, amount_cents, currency,
                    start_date, end_date, rollover, is_active
             FROM budget WHERE id = ?1",
            params![id],
            map_budget,
        )
        .optional()?)
}

pub(crate) fn list_budget_rows(
    conn: &Connection,
    include_inactive: bool,
) -> CoreResult<Vec<BudgetRow>> {
    let sql = if include_inactive {
        "SELECT id, name, scope, category_id, period_type, amount_cents, currency,
                start_date, end_date, rollover, is_active
         FROM budget ORDER BY name"
    } else {
        "SELECT id, name, scope, category_id, period_type, amount_cents, currency,
                start_date, end_date, rollover, is_active
         FROM budget WHERE is_active = 1 ORDER BY name"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], map_budget)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

pub(crate) fn set_budget_active(conn: &Connection, id: &str, active: bool) -> CoreResult<()> {
    conn.execute(
        "UPDATE budget SET is_active = ?1 WHERE id = ?2",
        params![i64::from(active), id],
    )?;
    Ok(())
}

pub(crate) fn delete_budget(conn: &Connection, id: &str) -> CoreResult<()> {
    conn.execute("DELETE FROM budget WHERE id = ?1", params![id])?;
    Ok(())
}
