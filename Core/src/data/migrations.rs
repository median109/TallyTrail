use rusqlite::Connection;
use rusqlite_migration::{Migrations, M};

use crate::domain::error::CoreResult;

/// 第一版 Schema。严格按需求文档第四节的建表顺序，避免外键前向引用。
const INITIAL_SCHEMA: &str = r#"
CREATE TABLE account (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  type TEXT NOT NULL CHECK(type IN ('BANK','CASH','E_WALLET','CREDIT','REIMBURSE_SOURCE','OTHER')),
  currency TEXT NOT NULL DEFAULT 'CNY',
  initial_balance_cents INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL
);

CREATE TABLE category (
  id TEXT PRIMARY KEY,
  parent_id TEXT REFERENCES category(id),
  name TEXT NOT NULL,
  level INTEGER NOT NULL CHECK(level IN (1,2)),
  kind TEXT NOT NULL CHECK(kind IN ('EXPENSE','INCOME')),
  sort_order INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE tag (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL UNIQUE
);

CREATE TABLE loan (
  id TEXT PRIMARY KEY,
  lender TEXT NOT NULL,
  principal_cents INTEGER NOT NULL CHECK(principal_cents > 0),
  currency TEXT NOT NULL DEFAULT 'CNY',
  interest_rate_bp INTEGER NOT NULL DEFAULT 0,
  start_at INTEGER NOT NULL,
  term_months INTEGER,
  account_id TEXT REFERENCES account(id),
  status TEXT NOT NULL DEFAULT 'ACTIVE' CHECK(status IN ('ACTIVE','SETTLED')),
  note TEXT
);

CREATE TABLE txn (
  id TEXT PRIMARY KEY,
  type TEXT NOT NULL CHECK(type IN (
    'EXPENSE','INCOME','TRANSFER','REFUND','REIMBURSEMENT',
    'LOAN_DISBURSE','LOAN_REPAY'
  )),
  occurred_at INTEGER NOT NULL,
  category_id TEXT REFERENCES category(id),
  note TEXT,
  total_amount_cents INTEGER NOT NULL CHECK(total_amount_cents > 0),
  currency TEXT NOT NULL DEFAULT 'CNY',
  actual_amount_cents INTEGER NOT NULL CHECK(actual_amount_cents > 0),
  direction TEXT NOT NULL CHECK(direction IN ('INFLOW','OUTFLOW')),
  ref_txn_id TEXT REFERENCES txn(id),
  loan_id TEXT REFERENCES loan(id),
  principal_part_cents INTEGER,
  interest_part_cents INTEGER,
  status TEXT NOT NULL DEFAULT 'CONFIRMED' CHECK(status IN ('CONFIRMED','VOIDED')),
  voided_at INTEGER,
  void_reason TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE settlement_split (
  id TEXT PRIMARY KEY,
  txn_id TEXT NOT NULL REFERENCES txn(id) ON DELETE CASCADE,
  account_id TEXT NOT NULL REFERENCES account(id),
  amount_cents INTEGER NOT NULL CHECK(amount_cents > 0),
  role TEXT NOT NULL CHECK(role IN ('PAYMENT','RECEIPT','REIMBURSE_SOURCE'))
);

CREATE TABLE txn_tag (
  txn_id TEXT NOT NULL REFERENCES txn(id) ON DELETE CASCADE,
  tag_id TEXT NOT NULL REFERENCES tag(id) ON DELETE CASCADE,
  PRIMARY KEY (txn_id, tag_id)
);

CREATE TABLE budget (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  scope TEXT NOT NULL CHECK(scope IN ('TOTAL','CATEGORY_L1','CATEGORY_L2')),
  category_id TEXT REFERENCES category(id),
  period_type TEXT NOT NULL CHECK(period_type IN ('MONTHLY','YEARLY','CUSTOM')),
  amount_cents INTEGER NOT NULL CHECK(amount_cents > 0),
  currency TEXT NOT NULL DEFAULT 'CNY',
  start_date INTEGER,
  end_date INTEGER,
  rollover INTEGER NOT NULL DEFAULT 0,
  is_active INTEGER NOT NULL DEFAULT 1
);

CREATE INDEX idx_txn_occurred_at ON txn(occurred_at);
CREATE INDEX idx_txn_type ON txn(type);
CREATE INDEX idx_txn_ref ON txn(ref_txn_id);
CREATE INDEX idx_txn_loan ON txn(loan_id);
CREATE INDEX idx_txn_category ON txn(category_id);
CREATE INDEX idx_txn_currency ON txn(currency);
CREATE INDEX idx_split_txn ON settlement_split(txn_id);
CREATE INDEX idx_split_account ON settlement_split(account_id);
CREATE INDEX idx_category_parent ON category(parent_id);
CREATE INDEX idx_budget_scope ON budget(scope, category_id);
"#;

pub fn migrate(conn: &mut Connection) -> CoreResult<()> {
    let migrations = Migrations::new(vec![M::up(INITIAL_SCHEMA)]);
    migrations.to_latest(conn)?;
    Ok(())
}

/// 打开连接后的统一 PRAGMA 配置。
pub fn configure_connection(conn: &Connection) -> CoreResult<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

pub fn checkpoint(conn: &Connection) -> CoreResult<()> {
    conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")?;
    Ok(())
}
