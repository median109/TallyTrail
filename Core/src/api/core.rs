use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{Datelike, Months, TimeZone, Utc};
use rusqlite::Connection;
use uuid::Uuid;

use crate::data::repo::{
    AccountRow, BudgetRow, CategoryRow, LoanRow, SettlementRow, TagRow, TxnInsert, TxnRow,
};
use crate::data::{migrations, repo};
use crate::domain::error::{CoreError, CoreResult};
use crate::domain::rules::{
    bucket_containing, checked_add, checked_sub, default_currency, ensure_non_negative,
    ensure_positive, expected_direction, time_buckets, validate_currency, validate_direction,
    validate_name, validate_settlement_shape, validate_time_range,
};
use crate::domain::types::*;

/// 第一阶段 Core 的唯一入口。内部持有 `Arc<Mutex<Connection>>`，
/// 可安全地克隆到多个线程；所有写操作都在单个 SQLite 事务内完成。
#[derive(Clone)]
pub struct CoreHandle {
    inner: Arc<Mutex<Connection>>,
}

impl std::fmt::Debug for CoreHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoreHandle").finish_non_exhaustive()
    }
}

impl CoreHandle {
    /// 打开（或创建）单文件数据库，执行 PRAGMA 与迁移。
    pub fn open(path: impl AsRef<Path>) -> CoreResult<Self> {
        let mut conn = Connection::open(path)?;
        migrations::configure_connection(&conn)?;
        migrations::migrate(&mut conn)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    /// 执行 WAL checkpoint。直接拷贝 `.db` 文件前应先调用此方法或正常关闭应用。
    pub fn checkpoint(&self) -> CoreResult<()> {
        let conn = self.lock()?;
        migrations::checkpoint(&conn)
    }

    fn lock(&self) -> CoreResult<MutexGuard<'_, Connection>> {
        self.inner
            .lock()
            .map_err(|_| CoreError::DbError("数据库连接锁已损坏".to_string()))
    }

    fn read<T>(&self, action: impl FnOnce(&Connection) -> CoreResult<T>) -> CoreResult<T> {
        let conn = self.lock()?;
        action(&conn)
    }

    fn write<T>(&self, action: impl FnOnce(&Connection) -> CoreResult<T>) -> CoreResult<T> {
        let mut conn = self.lock()?;
        let tx = conn.transaction()?;
        let result = action(&tx)?;
        tx.commit()?;
        Ok(result)
    }

    // -----------------------------------------------------------------------
    // 分类
    // -----------------------------------------------------------------------

    pub fn create_category(&self, input: CreateCategoryInput) -> CoreResult<CategoryId> {
        let name = validate_name("分类名称", &input.name)?;
        let id = new_id();
        self.write(|conn| {
            let (parent_id, level) = match input.parent_id.as_deref() {
                Some(parent_id) => {
                    let parent = repo::get_category(conn, parent_id)?.ok_or(CoreError::NotFound)?;
                    if parent.level != 1 {
                        return Err(CoreError::validation("二级分类不能再有子分类"));
                    }
                    if parent.kind != input.kind {
                        return Err(CoreError::validation("子分类 kind 必须与父分类一致"));
                    }
                    if !parent.is_active {
                        return Err(CoreError::validation("父分类已停用，不能新建子分类"));
                    }
                    (Some(parent_id.to_string()), 2)
                }
                None => (None, 1),
            };
            if repo::category_name_exists(conn, parent_id.as_deref(), &name, None)? {
                return Err(CoreError::validation("同一父级下分类名称必须唯一"));
            }
            repo::insert_category(
                conn,
                &id,
                parent_id.as_deref(),
                &name,
                level,
                input.kind,
                input.sort_order,
            )?;
            Ok(id)
        })
    }

    pub fn rename_category(&self, id: CategoryId, new_name: String) -> CoreResult<()> {
        let name = validate_name("分类名称", &new_name)?;
        self.write(|conn| {
            let category = repo::get_category(conn, &id)?.ok_or(CoreError::NotFound)?;
            if repo::category_name_exists(conn, category.parent_id.as_deref(), &name, Some(&id))? {
                return Err(CoreError::validation("同一父级下分类名称必须唯一"));
            }
            repo::rename_category(conn, &id, &name)
        })
    }

    pub fn set_category_active(&self, id: CategoryId, active: bool) -> CoreResult<()> {
        self.write(|conn| {
            repo::get_category(conn, &id)?.ok_or(CoreError::NotFound)?;
            repo::set_category_active(conn, &id, active)
        })
    }

    /// 修改分类 kind。已有子分类或交易/预算引用时返回 `CategoryInUse`。
    pub fn change_category_kind(&self, id: CategoryId, kind: CategoryKind) -> CoreResult<()> {
        self.write(|conn| {
            let category = repo::get_category(conn, &id)?.ok_or(CoreError::NotFound)?;
            if category.kind == kind {
                return Ok(());
            }
            if repo::category_has_children(conn, &id)? {
                return Err(CoreError::CategoryInUse);
            }
            if repo::category_reference_count(conn, &id)? > 0 {
                return Err(CoreError::CategoryInUse);
            }
            repo::set_category_kind(conn, &id, kind)
        })
    }

    pub fn delete_category(&self, id: CategoryId) -> CoreResult<()> {
        self.write(|conn| {
            repo::get_category(conn, &id)?.ok_or(CoreError::NotFound)?;
            if repo::category_has_children(conn, &id)?
                || repo::category_reference_count(conn, &id)? > 0
            {
                return Err(CoreError::CategoryInUse);
            }
            repo::delete_category(conn, &id)
        })
    }

    pub fn list_categories(
        &self,
        kind: Option<CategoryKind>,
        include_inactive: bool,
    ) -> CoreResult<Vec<CategoryView>> {
        self.read(|conn| {
            Ok(repo::list_category_rows(conn, kind, include_inactive)?
                .into_iter()
                .map(category_view)
                .collect())
        })
    }

    // -----------------------------------------------------------------------
    // 标签
    // -----------------------------------------------------------------------

    pub fn create_tag(&self, name: String) -> CoreResult<TagId> {
        let name = validate_name("标签名称", &name)?;
        let id = new_id();
        self.write(|conn| {
            if repo::tag_name_exists(conn, &name, None)? {
                return Err(CoreError::validation("标签名称必须全局唯一（忽略大小写）"));
            }
            repo::insert_tag(conn, &id, &name)?;
            Ok(id)
        })
    }

    pub fn rename_tag(&self, id: TagId, new_name: String) -> CoreResult<()> {
        let name = validate_name("标签名称", &new_name)?;
        self.write(|conn| {
            repo::get_tag(conn, &id)?.ok_or(CoreError::NotFound)?;
            if repo::tag_name_exists(conn, &name, Some(&id))? {
                return Err(CoreError::validation("标签名称必须全局唯一（忽略大小写）"));
            }
            repo::rename_tag(conn, &id, &name)
        })
    }

    pub fn delete_tag(&self, id: TagId) -> CoreResult<()> {
        self.write(|conn| {
            repo::get_tag(conn, &id)?.ok_or(CoreError::NotFound)?;
            repo::delete_tag(conn, &id)
        })
    }

    pub fn list_tags(&self) -> CoreResult<Vec<TagView>> {
        self.read(|conn| {
            Ok(repo::list_tag_rows(conn)?
                .into_iter()
                .map(tag_view)
                .collect())
        })
    }

    // -----------------------------------------------------------------------
    // 账户
    // -----------------------------------------------------------------------

    pub fn create_account(&self, input: CreateAccountInput) -> CoreResult<AccountId> {
        let name = validate_name("账户名称", &input.name)?;
        let currency = default_currency(input.currency)?;
        let id = new_id();
        let created_at = now();
        self.write(|conn| {
            repo::insert_account(
                conn,
                &id,
                &name,
                input.account_type,
                &currency,
                input.initial_balance_cents,
                created_at,
            )?;
            Ok(id)
        })
    }

    pub fn rename_account(&self, id: AccountId, new_name: String) -> CoreResult<()> {
        let name = validate_name("账户名称", &new_name)?;
        self.write(|conn| {
            repo::get_account(conn, &id)?.ok_or(CoreError::NotFound)?;
            repo::rename_account(conn, &id, &name)
        })
    }

    pub fn set_account_active(&self, id: AccountId, active: bool) -> CoreResult<()> {
        self.write(|conn| {
            repo::get_account(conn, &id)?.ok_or(CoreError::NotFound)?;
            repo::set_account_active(conn, &id, active)
        })
    }

    pub fn list_accounts(&self, include_inactive: bool) -> CoreResult<Vec<AccountView>> {
        self.read(|conn| {
            Ok(repo::list_account_rows(conn, include_inactive)?
                .into_iter()
                .map(account_view)
                .collect())
        })
    }

    // -----------------------------------------------------------------------
    // 写入交易
    // -----------------------------------------------------------------------

    pub fn create_expense(&self, input: CreateExpenseInput) -> CoreResult<TransactionId> {
        let currency = default_currency(input.currency)?;
        self.write(|conn| {
            create_standard_flow(
                conn,
                TxnType::Expense,
                input.occurred_at,
                &input.category_id,
                CategoryKind::Expense,
                input.note.as_deref(),
                input.total_amount_cents,
                &currency,
                input.actual_amount_cents,
                &input.settlements,
                &input.tag_ids,
                SplitRole::Payment,
            )
        })
    }

    pub fn create_income(&self, input: CreateIncomeInput) -> CoreResult<TransactionId> {
        let currency = default_currency(input.currency)?;
        self.write(|conn| {
            create_standard_flow(
                conn,
                TxnType::Income,
                input.occurred_at,
                &input.category_id,
                CategoryKind::Income,
                input.note.as_deref(),
                input.total_amount_cents,
                &currency,
                input.actual_amount_cents,
                &input.settlements,
                &input.tag_ids,
                SplitRole::Receipt,
            )
        })
    }

    pub fn create_transfer(&self, input: CreateTransferInput) -> CoreResult<TransactionId> {
        ensure_positive(input.amount_cents, "转账金额")?;
        if input.from_account_id == input.to_account_id {
            return Err(CoreError::validation("转出账户与转入账户不能相同"));
        }
        self.write(|conn| {
            let from = require_active_account(conn, &input.from_account_id)?;
            let to = require_active_account(conn, &input.to_account_id)?;
            if from.currency != to.currency {
                return Err(CoreError::MixedCurrencySettlement);
            }
            let tag_ids = resolve_tag_ids(conn, &input.tag_ids)?;
            let currency = from.currency.clone();
            let roles = [SplitRole::Payment, SplitRole::Receipt];
            let amounts = [input.amount_cents, input.amount_cents];
            validate_settlement_shape(TxnType::Transfer, &roles, &amounts, input.amount_cents)?;
            let settlements = vec![
                ResolvedSettlement {
                    account_id: input.from_account_id.clone(),
                    amount_cents: input.amount_cents,
                    role: SplitRole::Payment,
                },
                ResolvedSettlement {
                    account_id: input.to_account_id.clone(),
                    amount_cents: input.amount_cents,
                    role: SplitRole::Receipt,
                },
            ];
            insert_flow(
                conn,
                TxnType::Transfer,
                input.occurred_at,
                None,
                input.note.as_deref(),
                input.amount_cents,
                &currency,
                input.amount_cents,
                &settlements,
                &tag_ids,
                None,
                None,
                None,
                None,
            )
        })
    }

    pub fn create_refund(&self, input: CreateRefundInput) -> CoreResult<TransactionId> {
        self.write(|conn| {
            create_counterparty_flow(
                conn,
                TxnType::Refund,
                input.occurred_at,
                &input.ref_txn_id,
                input.note.as_deref(),
                input.total_amount_cents,
                input.actual_amount_cents,
                &input.settlements,
                &input.tag_ids,
                SplitRole::Receipt,
            )
        })
    }

    pub fn create_reimbursement(
        &self,
        input: CreateReimbursementInput,
    ) -> CoreResult<TransactionId> {
        self.write(|conn| {
            create_counterparty_flow(
                conn,
                TxnType::Reimbursement,
                input.occurred_at,
                &input.ref_txn_id,
                input.note.as_deref(),
                input.total_amount_cents,
                input.actual_amount_cents,
                &input.settlements,
                &input.tag_ids,
                SplitRole::ReimburseSource,
            )
        })
    }

    // -----------------------------------------------------------------------
    // 贷款
    // -----------------------------------------------------------------------

    pub fn create_loan(&self, input: CreateLoanInput) -> CoreResult<LoanId> {
        let lender = validate_name("贷款方", &input.lender)?;
        ensure_positive(input.principal_cents, "贷款本金")?;
        ensure_non_negative(i64::from(input.interest_rate_bp), "年化基点")?;
        if let Some(term) = input.term_months {
            if term <= 0 {
                return Err(CoreError::validation("贷款期限必须大于 0 个月"));
            }
        }
        let currency = default_currency(input.currency)?;
        let id = new_id();
        self.write(|conn| {
            if let Some(account_id) = input.account_id.as_deref() {
                let account = require_active_account(conn, account_id)?;
                if account.currency != currency {
                    return Err(CoreError::validation("贷款关联账户币种必须与贷款币种一致"));
                }
            }
            repo::insert_loan(
                conn,
                repo::NewLoan {
                    id: &id,
                    lender: &lender,
                    principal_cents: input.principal_cents,
                    currency: &currency,
                    interest_rate_bp: input.interest_rate_bp,
                    start_at: input.start_at,
                    term_months: input.term_months,
                    account_id: input.account_id.as_deref(),
                    note: input.note.as_deref(),
                },
            )?;
            Ok(id)
        })
    }

    pub fn create_loan_disbursement(
        &self,
        input: CreateLoanDisbursementInput,
    ) -> CoreResult<TransactionId> {
        ensure_positive(input.amount_cents, "放款金额")?;
        self.write(|conn| {
            let loan = repo::get_loan(conn, &input.loan_id)?.ok_or(CoreError::NotFound)?;
            let account = require_active_account(conn, &input.account_id)?;
            if account.currency != loan.currency {
                return Err(CoreError::validation("放款账户币种必须与贷款币种一致"));
            }
            let tag_ids = resolve_tag_ids(conn, &input.tag_ids)?;
            let roles = [SplitRole::Receipt];
            let amounts = [input.amount_cents];
            validate_settlement_shape(TxnType::LoanDisburse, &roles, &amounts, input.amount_cents)?;
            let settlements = vec![ResolvedSettlement {
                account_id: input.account_id.clone(),
                amount_cents: input.amount_cents,
                role: SplitRole::Receipt,
            }];
            insert_flow(
                conn,
                TxnType::LoanDisburse,
                input.occurred_at,
                None,
                input.note.as_deref(),
                input.amount_cents,
                &loan.currency,
                input.amount_cents,
                &settlements,
                &tag_ids,
                None,
                None,
                None,
                Some(&input.loan_id),
            )
        })
    }

    pub fn create_loan_repayment(
        &self,
        input: CreateLoanRepaymentInput,
    ) -> CoreResult<TransactionId> {
        ensure_positive(input.actual_amount_cents, "实际还款金额")?;
        ensure_non_negative(input.principal_part_cents, "本金部分")?;
        ensure_non_negative(input.interest_part_cents, "利息部分")?;
        if checked_add(input.principal_part_cents, input.interest_part_cents)?
            != input.actual_amount_cents
        {
            return Err(CoreError::validation(
                "还款本金部分 + 利息部分必须等于实际还款金额",
            ));
        }
        self.write(|conn| {
            let loan = repo::get_loan(conn, &input.loan_id)?.ok_or(CoreError::NotFound)?;
            let repaid = repo::loan_repaid_principal(conn, &input.loan_id)?;
            let outstanding = checked_sub(loan.principal_cents, repaid)?;
            if outstanding <= 0 {
                return Err(CoreError::validation("该贷款已结清，不能继续还款"));
            }
            if input.principal_part_cents > outstanding {
                return Err(CoreError::validation("还款本金部分不能超过剩余未还本金"));
            }
            let account = require_active_account(conn, &input.account_id)?;
            if account.currency != loan.currency {
                return Err(CoreError::validation("还款账户币种必须与贷款币种一致"));
            }
            if let Some(category_id) = input.category_id.as_deref() {
                require_category(conn, category_id, CategoryKind::Expense, true)?;
            }
            let tag_ids = resolve_tag_ids(conn, &input.tag_ids)?;
            let roles = [SplitRole::Payment];
            let amounts = [input.actual_amount_cents];
            validate_settlement_shape(
                TxnType::LoanRepay,
                &roles,
                &amounts,
                input.actual_amount_cents,
            )?;
            let settlements = vec![ResolvedSettlement {
                account_id: input.account_id.clone(),
                amount_cents: input.actual_amount_cents,
                role: SplitRole::Payment,
            }];
            let txn_id = insert_flow(
                conn,
                TxnType::LoanRepay,
                input.occurred_at,
                input.category_id.as_deref(),
                input.note.as_deref(),
                input.actual_amount_cents,
                &loan.currency,
                input.actual_amount_cents,
                &settlements,
                &tag_ids,
                Some(input.principal_part_cents),
                Some(input.interest_part_cents),
                None,
                Some(&input.loan_id),
            )?;
            refresh_loan_status(conn, &input.loan_id)?;
            Ok(txn_id)
        })
    }

    pub fn void_transaction(&self, id: TransactionId, reason: String) -> CoreResult<()> {
        let reason = validate_name("作废原因", &reason)?;
        let voided_at = now();
        self.write(|conn| {
            let txn = repo::get_txn_row(conn, &id)?.ok_or(CoreError::NotFound)?;
            if txn.status != TxnStatus::Confirmed {
                return Err(CoreError::validation("交易已作废，不能重复作废"));
            }
            if txn.txn_type == TxnType::Expense && repo::active_dependent_count(conn, &id)? > 0 {
                return Err(CoreError::HasActiveDependents);
            }
            if txn.txn_type == TxnType::LoanDisburse {
                let loan_id = txn
                    .loan_id
                    .as_deref()
                    .ok_or_else(|| CoreError::validation("放款交易缺少 loan_id"))?;
                if repo::confirmed_repayment_count(conn, loan_id)? > 0 {
                    return Err(CoreError::HasActiveDependents);
                }
            }
            repo::mark_voided(conn, &id, voided_at, &reason)?;
            if txn.txn_type == TxnType::LoanRepay {
                if let Some(loan_id) = txn.loan_id.as_deref() {
                    refresh_loan_status(conn, loan_id)?;
                }
            }
            Ok(())
        })
    }

    // -----------------------------------------------------------------------
    // 流水查询
    // -----------------------------------------------------------------------

    pub fn list_transactions(&self, filter: TransactionFilter) -> CoreResult<Vec<TransactionView>> {
        self.read(|conn| {
            if let Some(category_id) = filter.category_id.as_deref() {
                repo::get_category(conn, category_id)?.ok_or(CoreError::NotFound)?;
            }
            if let Some(account_id) = filter.account_id.as_deref() {
                repo::get_account(conn, account_id)?.ok_or(CoreError::NotFound)?;
            }
            if let Some(tag_ids) = filter.tag_ids.as_deref() {
                for tag_id in tag_ids {
                    repo::get_tag(conn, tag_id)?.ok_or(CoreError::NotFound)?;
                }
            }
            let rows = repo::list_txn_rows(conn, &filter)?;
            rows.iter().map(|row| transaction_view(conn, row)).collect()
        })
    }

    pub fn get_transaction(&self, id: TransactionId) -> CoreResult<TransactionDetail> {
        self.read(|conn| {
            let row = repo::get_txn_row(conn, &id)?.ok_or(CoreError::NotFound)?;
            transaction_view(conn, &row)
        })
    }

    // -----------------------------------------------------------------------
    // 收支统计
    // -----------------------------------------------------------------------

    pub fn income_expense_summary(&self, from_at: i64, to_at: i64) -> CoreResult<Summary> {
        validate_time_range(from_at, to_at)?;
        self.read(|conn| {
            let rows = repo::flow_rows(conn, from_at, to_at, None)?;
            let mut by_currency: BTreeMap<String, CurrencySummary> = BTreeMap::new();
            for row in rows {
                match row.txn_type {
                    TxnType::Income => {
                        let entry = summary_entry(&mut by_currency, &row.currency);
                        entry.total_income_cents =
                            checked_add(entry.total_income_cents, row.total_amount_cents)?;
                    }
                    TxnType::Expense => {
                        let entry = summary_entry(&mut by_currency, &row.currency);
                        entry.total_expense_cents =
                            checked_add(entry.total_expense_cents, row.total_amount_cents)?;
                    }
                    TxnType::Refund => {
                        let entry = summary_entry(&mut by_currency, &row.currency);
                        entry.total_refund_cents =
                            checked_add(entry.total_refund_cents, row.total_amount_cents)?;
                    }
                    TxnType::Reimbursement => {
                        let entry = summary_entry(&mut by_currency, &row.currency);
                        entry.total_reimbursement_cents =
                            checked_add(entry.total_reimbursement_cents, row.total_amount_cents)?;
                    }
                    TxnType::LoanRepay => {
                        if row.interest_part_cents != 0 {
                            let entry = summary_entry(&mut by_currency, &row.currency);
                            entry.total_expense_cents =
                                checked_add(entry.total_expense_cents, row.interest_part_cents)?;
                        }
                    }
                    TxnType::Transfer | TxnType::LoanDisburse => {}
                }
            }
            Ok(Summary {
                by_currency: by_currency.into_values().collect(),
            })
        })
    }

    pub fn category_spending_series(
        &self,
        category_id: String,
        from_at: i64,
        to_at: i64,
        granularity: Granularity,
    ) -> CoreResult<Vec<SpendingPoint>> {
        let buckets = time_buckets(from_at, to_at, granularity)?;
        self.read(|conn| {
            let category = repo::get_category(conn, &category_id)?.ok_or(CoreError::NotFound)?;
            if category.kind != CategoryKind::Expense {
                return Err(CoreError::validation("分类支出统计只支持 EXPENSE 分类"));
            }
            let category_ids = scope_category_ids(conn, &category)?;
            let rows = repo::flow_rows(conn, from_at, to_at, None)?;
            let mut amounts: BTreeMap<String, Vec<i64>> = BTreeMap::new();

            for row in rows {
                if !category_matches(&category_ids, row.category_id.as_deref()) {
                    continue;
                }
                let delta = match row.txn_type {
                    TxnType::Expense => row.total_amount_cents,
                    TxnType::Refund | TxnType::Reimbursement => -row.total_amount_cents,
                    TxnType::LoanRepay => row.interest_part_cents,
                    _ => continue,
                };
                if delta == 0 {
                    continue;
                }
                let Some(index) = bucket_containing(row.occurred_at, &buckets)? else {
                    continue;
                };
                let series = amounts
                    .entry(row.currency)
                    .or_insert_with(|| vec![0_i64; buckets.len()]);
                series[index] = checked_add(series[index], delta)?;
            }

            let mut points = Vec::new();
            for (index, bucket) in buckets.iter().enumerate() {
                for (currency, series) in &amounts {
                    points.push(SpendingPoint {
                        period_start_at: bucket.start_at,
                        period_end_at: bucket.end_at,
                        currency: currency.clone(),
                        amount_cents: series[index],
                    });
                }
            }
            Ok(points)
        })
    }

    // -----------------------------------------------------------------------
    // 资产负债
    // -----------------------------------------------------------------------

    pub fn account_balance(&self, account_id: &str, as_of: i64) -> CoreResult<i64> {
        self.read(|conn| account_balance_inner(conn, account_id, as_of))
    }

    pub fn list_account_balances(&self, as_of: i64) -> CoreResult<Vec<AccountBalance>> {
        self.read(|conn| {
            let accounts = repo::list_account_rows(conn, true)?;
            accounts
                .into_iter()
                .map(|account| {
                    let balance = checked_add(
                        account.initial_balance_cents,
                        repo::settlement_balance(conn, &account.id, as_of)?,
                    )?;
                    Ok(AccountBalance {
                        account_id: account.id,
                        name: account.name,
                        account_type: account.account_type,
                        currency: account.currency,
                        balance_cents: balance,
                    })
                })
                .collect()
        })
    }

    pub fn loan_outstanding(&self, loan_id: &str) -> CoreResult<LoanBalance> {
        self.read(|conn| {
            let loan = repo::get_loan(conn, loan_id)?.ok_or(CoreError::NotFound)?;
            let repaid = repo::loan_repaid_principal(conn, loan_id)?;
            let outstanding = checked_sub(loan.principal_cents, repaid)?;
            Ok(LoanBalance {
                loan_id: loan.id,
                lender: loan.lender,
                currency: loan.currency,
                principal_cents: loan.principal_cents,
                repaid_principal_cents: repaid,
                outstanding_cents: outstanding,
                status: if outstanding <= 0 {
                    LoanStatus::Settled
                } else {
                    LoanStatus::Active
                },
            })
        })
    }

    pub fn list_loans(&self) -> CoreResult<Vec<LoanView>> {
        self.read(|conn| {
            repo::list_loan_rows(conn)?
                .iter()
                .map(|loan| loan_view(conn, loan))
                .collect()
        })
    }

    pub fn net_worth_series(
        &self,
        from_at: i64,
        to_at: i64,
        granularity: Granularity,
    ) -> CoreResult<Vec<NetWorthPoint>> {
        validate_time_range(from_at, to_at)?;
        let buckets = time_buckets(from_at, to_at, granularity)?;
        self.read(|conn| {
            let accounts = repo::list_account_rows(conn, true)?;
            let loans = repo::list_loan_rows(conn)?;
            let mut currencies = BTreeSet::new();
            for account in &accounts {
                currencies.insert(account.currency.clone());
            }
            for loan in &loans {
                currencies.insert(loan.currency.clone());
            }

            let mut points = Vec::with_capacity(buckets.len());
            for bucket in buckets {
                let as_of = bucket.end_at.min(to_at) - 1;
                let mut by_currency = Vec::new();
                for currency in &currencies {
                    let mut assets = 0_i64;
                    for account in accounts
                        .iter()
                        .filter(|account| &account.currency == currency)
                    {
                        assets =
                            checked_add(assets, account_balance_inner(conn, &account.id, as_of)?)?;
                    }
                    let mut liabilities = 0_i64;
                    for loan in loans
                        .iter()
                        .filter(|loan| &loan.currency == currency && loan.start_at <= as_of)
                    {
                        let repaid = repo::loan_repaid_principal_as_of(conn, &loan.id, as_of)?;
                        let outstanding = checked_sub(loan.principal_cents, repaid)?;
                        if outstanding > 0 {
                            liabilities = checked_add(liabilities, outstanding)?;
                        }
                    }
                    by_currency.push(CurrencyNetWorth {
                        currency: currency.clone(),
                        total_assets_cents: assets,
                        total_liabilities_cents: liabilities,
                        net_worth_cents: checked_sub(assets, liabilities)?,
                    });
                }
                points.push(NetWorthPoint {
                    at: bucket.start_at,
                    by_currency,
                });
            }
            Ok(points)
        })
    }

    // -----------------------------------------------------------------------
    // 预算
    // -----------------------------------------------------------------------

    pub fn create_budget(&self, input: CreateBudgetInput) -> CoreResult<BudgetId> {
        let name = validate_name("预算名称", &input.name)?;
        ensure_positive(input.amount_cents, "预算金额")?;
        let currency = default_currency(input.currency)?;
        if input.period_type == PeriodType::Custom {
            let start = input
                .start_at
                .ok_or_else(|| CoreError::validation("CUSTOM 预算必须填写 start_at"))?;
            let end = input
                .end_at
                .ok_or_else(|| CoreError::validation("CUSTOM 预算必须填写 end_at"))?;
            validate_time_range(start, end)?;
        }
        let id = new_id();
        self.write(|conn| {
            match input.scope {
                BudgetScope::Total => {
                    if input.category_id.is_some() {
                        return Err(CoreError::validation("TOTAL 预算不能指定分类"));
                    }
                    Ok(())
                }
                BudgetScope::CategoryL1 => {
                    validate_budget_category(conn, input.category_id.as_deref(), Some(1))
                }
                BudgetScope::CategoryL2 => {
                    validate_budget_category(conn, input.category_id.as_deref(), Some(2))
                }
            }?;
            repo::insert_budget(
                conn,
                &id,
                &name,
                input.scope,
                input.category_id.as_deref(),
                input.period_type,
                input.amount_cents,
                &currency,
                input.start_at,
                input.end_at,
                // rollover 第一版固定按 0 处理。
                false,
            )?;
            Ok(id)
        })
    }

    pub fn list_budgets(&self, include_inactive: bool) -> CoreResult<Vec<BudgetView>> {
        self.read(|conn| {
            Ok(repo::list_budget_rows(conn, include_inactive)?
                .into_iter()
                .map(budget_view)
                .collect())
        })
    }

    pub fn set_budget_active(&self, id: BudgetId, active: bool) -> CoreResult<()> {
        self.write(|conn| {
            repo::get_budget(conn, &id)?.ok_or(CoreError::NotFound)?;
            repo::set_budget_active(conn, &id, active)
        })
    }

    pub fn delete_budget(&self, id: BudgetId) -> CoreResult<()> {
        self.write(|conn| {
            repo::get_budget(conn, &id)?.ok_or(CoreError::NotFound)?;
            repo::delete_budget(conn, &id)
        })
    }

    pub fn budget_usage(&self, budget_id: &str, as_of: i64) -> CoreResult<BudgetUsage> {
        self.read(|conn| {
            let budget = repo::get_budget(conn, budget_id)?.ok_or(CoreError::NotFound)?;
            budget_usage_inner(conn, &budget, as_of)
        })
    }

    pub fn list_budget_usage(&self, as_of: i64) -> CoreResult<Vec<BudgetUsage>> {
        self.read(|conn| {
            let budgets = repo::list_budget_rows(conn, false)?;
            let mut usages = Vec::new();
            for budget in budgets {
                let (period, in_period) = budget_period(&budget, as_of)?;
                if !in_period {
                    continue;
                }
                usages.push(compute_budget_usage(conn, &budget, period)?);
            }
            Ok(usages)
        })
    }
}

impl Drop for CoreHandle {
    fn drop(&mut self) {
        if Arc::strong_count(&self.inner) == 1 {
            if let Ok(conn) = self.inner.lock() {
                let _ = migrations::checkpoint(&conn);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 内部辅助
// ---------------------------------------------------------------------------

struct ResolvedSettlement {
    account_id: String,
    amount_cents: i64,
    role: SplitRole,
}

fn new_id() -> String {
    Uuid::new_v4().to_string()
}

fn now() -> i64 {
    Utc::now().timestamp()
}

fn account_view(row: AccountRow) -> AccountView {
    AccountView {
        id: row.id,
        name: row.name,
        account_type: row.account_type,
        currency: row.currency,
        initial_balance_cents: row.initial_balance_cents,
        is_active: row.is_active,
        created_at: row.created_at,
    }
}

fn category_view(row: CategoryRow) -> CategoryView {
    CategoryView {
        id: row.id,
        parent_id: row.parent_id,
        name: row.name,
        level: row.level,
        kind: row.kind,
        sort_order: row.sort_order,
        is_active: row.is_active,
    }
}

fn tag_view(row: TagRow) -> TagView {
    TagView {
        id: row.id,
        name: row.name,
    }
}

fn budget_view(row: BudgetRow) -> BudgetView {
    BudgetView {
        id: row.id,
        name: row.name,
        scope: row.scope,
        category_id: row.category_id,
        period_type: row.period_type,
        amount_cents: row.amount_cents,
        currency: row.currency,
        start_at: row.start_at,
        end_at: row.end_at,
        rollover: row.rollover,
        is_active: row.is_active,
    }
}

fn transaction_view(conn: &Connection, row: &TxnRow) -> CoreResult<TransactionView> {
    let settlements = repo::settlements_for_txn(conn, &row.id)?
        .into_iter()
        .map(|settlement: SettlementRow| SettlementView {
            id: settlement.id,
            account_id: settlement.account_id,
            account_name: settlement.account_name,
            currency: settlement.currency,
            amount_cents: settlement.amount_cents,
            role: settlement.role,
        })
        .collect();
    let tags = repo::tags_for_txn(conn, &row.id)?
        .into_iter()
        .map(tag_view)
        .collect();
    Ok(TransactionView {
        id: row.id.clone(),
        txn_type: row.txn_type,
        occurred_at: row.occurred_at,
        category_id: row.category_id.clone(),
        category_name: row.category_name.clone(),
        note: row.note.clone(),
        total_amount_cents: row.total_amount_cents,
        currency: row.currency.clone(),
        actual_amount_cents: row.actual_amount_cents,
        direction: row.direction,
        ref_txn_id: row.ref_txn_id.clone(),
        loan_id: row.loan_id.clone(),
        principal_part_cents: row.principal_part_cents,
        interest_part_cents: row.interest_part_cents,
        status: row.status,
        voided_at: row.voided_at,
        void_reason: row.void_reason.clone(),
        created_at: row.created_at,
        updated_at: row.updated_at,
        settlements,
        tags,
    })
}

fn loan_view(conn: &Connection, row: &LoanRow) -> CoreResult<LoanView> {
    let repaid = repo::loan_repaid_principal(conn, &row.id)?;
    let outstanding = checked_sub(row.principal_cents, repaid)?;
    Ok(LoanView {
        id: row.id.clone(),
        lender: row.lender.clone(),
        principal_cents: row.principal_cents,
        currency: row.currency.clone(),
        interest_rate_bp: row.interest_rate_bp,
        start_at: row.start_at,
        term_months: row.term_months,
        account_id: row.account_id.clone(),
        status: if outstanding <= 0 {
            LoanStatus::Settled
        } else {
            LoanStatus::Active
        },
        note: row.note.clone(),
        repaid_principal_cents: repaid,
        outstanding_cents: outstanding,
    })
}

fn require_category(
    conn: &Connection,
    id: &str,
    kind: CategoryKind,
    require_active: bool,
) -> CoreResult<CategoryRow> {
    let category = repo::get_category(conn, id)?.ok_or(CoreError::NotFound)?;
    if category.kind != kind {
        return Err(CoreError::validation("分类 kind 与交易类型不匹配"));
    }
    if require_active && !category.is_active {
        return Err(CoreError::validation("分类已停用，不能用于新交易"));
    }
    Ok(category)
}

fn require_active_account(conn: &Connection, id: &str) -> CoreResult<AccountRow> {
    let account = repo::get_account(conn, id)?.ok_or(CoreError::NotFound)?;
    if !account.is_active {
        return Err(CoreError::validation("账户已停用，不能用于新交易"));
    }
    Ok(account)
}

fn resolve_tag_ids(conn: &Connection, tag_ids: &[TagId]) -> CoreResult<Vec<TagId>> {
    let mut resolved = Vec::new();
    let mut seen = BTreeSet::new();
    for tag_id in tag_ids {
        if !seen.insert(tag_id.clone()) {
            continue;
        }
        repo::get_tag(conn, tag_id)?.ok_or(CoreError::NotFound)?;
        resolved.push(tag_id.clone());
    }
    Ok(resolved)
}

fn resolve_settlements(
    conn: &Connection,
    inputs: &[SettlementInput],
    txn_currency: &str,
    total_amount_cents: i64,
    actual_amount_cents: Option<i64>,
    role: SplitRole,
) -> CoreResult<(i64, Vec<ResolvedSettlement>)> {
    if inputs.is_empty() {
        return Err(CoreError::validation("至少需要一条结算行"));
    }
    let mut accounts = Vec::with_capacity(inputs.len());
    for input in inputs {
        ensure_positive(input.amount_cents, "结算行金额")?;
        let account = require_active_account(conn, &input.account_id)?;
        accounts.push(account);
    }
    let settlement_currency = accounts
        .first()
        .map(|account| account.currency.clone())
        .ok_or_else(|| CoreError::validation("至少需要一个结算账户"))?;
    if accounts
        .iter()
        .any(|account| account.currency != settlement_currency)
    {
        return Err(CoreError::MixedCurrencySettlement);
    }

    let actual = match actual_amount_cents {
        Some(actual) => actual,
        None if txn_currency == settlement_currency => total_amount_cents,
        None => {
            return Err(CoreError::validation(
                "花费币种与结算账户币种不同，必须显式填写 actual_amount_cents",
            ))
        }
    };

    let roles = vec![role; inputs.len()];
    let amounts: Vec<i64> = inputs.iter().map(|input| input.amount_cents).collect();
    let txn_type = match role {
        SplitRole::Payment => TxnType::Expense,
        SplitRole::Receipt => TxnType::Income,
        SplitRole::ReimburseSource => TxnType::Reimbursement,
    };
    // 形状校验使用实际交易类型；支出与还款同为 PAYMENT，这里仅用于角色/求和校验。
    validate_settlement_shape(txn_type, &roles, &amounts, actual)?;

    let settlements = inputs
        .iter()
        .map(|input| ResolvedSettlement {
            account_id: input.account_id.clone(),
            amount_cents: input.amount_cents,
            role,
        })
        .collect();
    Ok((actual, settlements))
}

#[allow(clippy::too_many_arguments)]
fn create_standard_flow(
    conn: &Connection,
    txn_type: TxnType,
    occurred_at: i64,
    category_id: &str,
    expected_kind: CategoryKind,
    note: Option<&str>,
    total_amount_cents: i64,
    currency: &str,
    actual_amount_cents: Option<i64>,
    settlements: &[SettlementInput],
    tag_ids: &[TagId],
    role: SplitRole,
) -> CoreResult<TransactionId> {
    ensure_positive(total_amount_cents, "花费金额")?;
    require_category(conn, category_id, expected_kind, true)?;
    validate_currency(currency)?;
    let tag_ids = resolve_tag_ids(conn, tag_ids)?;
    let (actual, settlements) = resolve_settlements(
        conn,
        settlements,
        currency,
        total_amount_cents,
        actual_amount_cents,
        role,
    )?;
    // 使用真实交易类型再校验一次方向与结算形状。
    let direction = expected_direction(txn_type);
    validate_direction(txn_type, direction)?;
    let roles: Vec<SplitRole> = settlements.iter().map(|s| s.role).collect();
    let amounts: Vec<i64> = settlements.iter().map(|s| s.amount_cents).collect();
    validate_settlement_shape(txn_type, &roles, &amounts, actual)?;
    insert_flow(
        conn,
        txn_type,
        occurred_at,
        Some(category_id),
        note,
        total_amount_cents,
        currency,
        actual,
        &settlements,
        &tag_ids,
        None,
        None,
        None,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn create_counterparty_flow(
    conn: &Connection,
    txn_type: TxnType,
    occurred_at: i64,
    ref_txn_id: &str,
    note: Option<&str>,
    total_amount_cents: i64,
    actual_amount_cents: Option<i64>,
    settlements: &[SettlementInput],
    tag_ids: &[TagId],
    role: SplitRole,
) -> CoreResult<TransactionId> {
    ensure_positive(total_amount_cents, "冲销金额")?;
    let original = repo::get_txn_row(conn, ref_txn_id)?.ok_or(CoreError::NotFound)?;
    if original.txn_type != TxnType::Expense || original.status != TxnStatus::Confirmed {
        return Err(CoreError::validation(
            "退款/报销必须指向一条未作废的 EXPENSE 交易",
        ));
    }
    let currency = original.currency.clone();
    if total_amount_cents > original.total_amount_cents {
        return Err(CoreError::validation("退款/报销金额不能超过原交易花费金额"));
    }
    let previous = repo::dependent_total_cents(conn, ref_txn_id, txn_type)?;
    let cumulative = checked_add(previous, total_amount_cents)?;
    if cumulative > original.total_amount_cents {
        return Err(CoreError::validation(
            "累计退款/报销金额不能超过原交易花费金额",
        ));
    }
    let tag_ids = resolve_tag_ids(conn, tag_ids)?;
    let (actual, settlements) = resolve_settlements(
        conn,
        settlements,
        &currency,
        total_amount_cents,
        actual_amount_cents,
        role,
    )?;
    let direction = expected_direction(txn_type);
    validate_direction(txn_type, direction)?;
    let roles: Vec<SplitRole> = settlements.iter().map(|s| s.role).collect();
    let amounts: Vec<i64> = settlements.iter().map(|s| s.amount_cents).collect();
    validate_settlement_shape(txn_type, &roles, &amounts, actual)?;
    insert_flow(
        conn,
        txn_type,
        occurred_at,
        original.category_id.as_deref(),
        note,
        total_amount_cents,
        &currency,
        actual,
        &settlements,
        &tag_ids,
        None,
        None,
        Some(ref_txn_id),
        None,
    )
}

#[allow(clippy::too_many_arguments)]
fn insert_flow(
    conn: &Connection,
    txn_type: TxnType,
    occurred_at: i64,
    category_id: Option<&str>,
    note: Option<&str>,
    total_amount_cents: i64,
    currency: &str,
    actual_amount_cents: i64,
    settlements: &[ResolvedSettlement],
    tag_ids: &[TagId],
    principal_part_cents: Option<i64>,
    interest_part_cents: Option<i64>,
    ref_txn_id: Option<&str>,
    loan_id: Option<&str>,
) -> CoreResult<TransactionId> {
    let id = new_id();
    let timestamp = now();
    repo::insert_transaction(
        conn,
        &TxnInsert {
            id: &id,
            txn_type,
            occurred_at,
            category_id,
            note,
            total_amount_cents,
            currency,
            actual_amount_cents,
            direction: expected_direction(txn_type),
            ref_txn_id,
            loan_id,
            principal_part_cents,
            interest_part_cents,
            created_at: timestamp,
            updated_at: timestamp,
        },
    )?;
    for settlement in settlements {
        repo::insert_settlement(
            conn,
            &new_id(),
            &id,
            &settlement.account_id,
            settlement.amount_cents,
            settlement.role,
        )?;
    }
    for tag_id in tag_ids {
        repo::link_txn_tag(conn, &id, tag_id)?;
    }
    Ok(id)
}

fn refresh_loan_status(conn: &Connection, loan_id: &str) -> CoreResult<()> {
    let loan = repo::get_loan(conn, loan_id)?.ok_or(CoreError::NotFound)?;
    let repaid = repo::loan_repaid_principal(conn, loan_id)?;
    let outstanding = checked_sub(loan.principal_cents, repaid)?;
    let status = if outstanding <= 0 {
        LoanStatus::Settled
    } else {
        LoanStatus::Active
    };
    repo::update_loan_status(conn, loan_id, status)
}

fn validate_budget_category(
    conn: &Connection,
    category_id: Option<&str>,
    expected_level: Option<i32>,
) -> CoreResult<()> {
    let Some(category_id) = category_id else {
        return Err(CoreError::validation("该预算 scope 必须指定 category_id"));
    };
    let category = repo::get_category(conn, category_id)?.ok_or(CoreError::NotFound)?;
    if category.kind != CategoryKind::Expense {
        return Err(CoreError::validation("预算只能关联 EXPENSE 分类"));
    }
    if let Some(level) = expected_level {
        if category.level != level {
            return Err(CoreError::validation("预算 scope 与分类层级不匹配"));
        }
    }
    Ok(())
}

fn account_balance_inner(conn: &Connection, account_id: &str, as_of: i64) -> CoreResult<i64> {
    let account = repo::get_account(conn, account_id)?.ok_or(CoreError::NotFound)?;
    checked_add(
        account.initial_balance_cents,
        repo::settlement_balance(conn, account_id, as_of)?,
    )
}

fn summary_entry<'a>(
    map: &'a mut BTreeMap<String, CurrencySummary>,
    currency: &str,
) -> &'a mut CurrencySummary {
    map.entry(currency.to_string())
        .or_insert_with(|| CurrencySummary {
            currency: currency.to_string(),
            total_income_cents: 0,
            total_expense_cents: 0,
            total_refund_cents: 0,
            total_reimbursement_cents: 0,
        })
}

fn scope_category_ids(conn: &Connection, category: &CategoryRow) -> CoreResult<BTreeSet<String>> {
    let mut ids = BTreeSet::new();
    ids.insert(category.id.clone());
    if category.level == 1 {
        for child in repo::category_children_ids(conn, &category.id)? {
            ids.insert(child);
        }
    }
    Ok(ids)
}

fn category_matches(category_ids: &BTreeSet<String>, category_id: Option<&str>) -> bool {
    category_id
        .map(|id| category_ids.contains(id))
        .unwrap_or(false)
}

fn budget_period(budget: &BudgetRow, as_of: i64) -> CoreResult<(TimeBucket, bool)> {
    match budget.period_type {
        PeriodType::Monthly => {
            let start_at = crate::domain::rules::bucket_start(as_of, Granularity::Monthly)?;
            let end_at = crate::domain::rules::next_bucket_start(start_at, Granularity::Monthly)?;
            Ok((TimeBucket { start_at, end_at }, true))
        }
        PeriodType::Yearly => {
            let dt = Utc
                .timestamp_opt(as_of, 0)
                .single()
                .ok_or_else(|| CoreError::validation("非法 UTC 时间戳"))?;
            let date = dt.date_naive();
            let start_date = date
                .with_month(1)
                .and_then(|value| value.with_day(1))
                .ok_or_else(|| CoreError::validation("年度起始日期计算失败"))?;
            let midnight = start_date
                .and_hms_opt(0, 0, 0)
                .ok_or_else(|| CoreError::validation("日期转时间失败"))?;
            let start_at = Utc.from_utc_datetime(&midnight).timestamp();
            let end_at = Utc
                .from_utc_datetime(&midnight)
                .checked_add_months(Months::new(12))
                .ok_or_else(|| CoreError::validation("年度结束日期计算失败"))?
                .timestamp();
            Ok((TimeBucket { start_at, end_at }, true))
        }
        PeriodType::Custom => {
            let start_at = budget
                .start_at
                .ok_or_else(|| CoreError::validation("CUSTOM 预算缺少 start_at"))?;
            let end_at = budget
                .end_at
                .ok_or_else(|| CoreError::validation("CUSTOM 预算缺少 end_at"))?;
            let in_period = as_of >= start_at && as_of < end_at;
            Ok((TimeBucket { start_at, end_at }, in_period))
        }
    }
}

fn compute_budget_usage(
    conn: &Connection,
    budget: &BudgetRow,
    period: TimeBucket,
) -> CoreResult<BudgetUsage> {
    let category_ids = match budget.scope {
        BudgetScope::Total => None,
        BudgetScope::CategoryL1 | BudgetScope::CategoryL2 => {
            let category_id = budget
                .category_id
                .as_deref()
                .ok_or_else(|| CoreError::validation("分类预算缺少 category_id"))?;
            let category = repo::get_category(conn, category_id)?.ok_or(CoreError::NotFound)?;
            Some(scope_category_ids(conn, &category)?)
        }
    };

    let rows = repo::flow_rows(conn, period.start_at, period.end_at, Some(&budget.currency))?;
    let mut used_cents = 0_i64;
    for row in rows {
        let matches = match budget.scope {
            BudgetScope::Total => true,
            BudgetScope::CategoryL1 | BudgetScope::CategoryL2 => category_ids
                .as_ref()
                .map(|ids| category_matches(ids, row.category_id.as_deref()))
                .unwrap_or(false),
        };
        if !matches {
            continue;
        }
        match row.txn_type {
            TxnType::Expense => {
                used_cents = checked_add(used_cents, row.total_amount_cents)?;
            }
            TxnType::Refund | TxnType::Reimbursement => {
                used_cents = checked_sub(used_cents, row.total_amount_cents)?;
            }
            TxnType::LoanRepay => {
                // 预算口径明确排除 LOAN_*（本金与利息都不计入）；利息只进入收支汇总与分类支出序列。
            }
            TxnType::Income | TxnType::Transfer | TxnType::LoanDisburse => {}
        }
    }

    Ok(BudgetUsage {
        budget_id: budget.id.clone(),
        name: budget.name.clone(),
        scope: budget.scope,
        category_id: budget.category_id.clone(),
        currency: budget.currency.clone(),
        amount_cents: budget.amount_cents,
        used_cents,
        remaining_cents: checked_sub(budget.amount_cents, used_cents)?,
        period_start_at: period.start_at,
        period_end_at: period.end_at,
        is_active: budget.is_active,
    })
}

fn budget_usage_inner(
    conn: &Connection,
    budget: &BudgetRow,
    as_of: i64,
) -> CoreResult<BudgetUsage> {
    let (period, in_period) = budget_period(budget, as_of)?;
    if in_period {
        compute_budget_usage(conn, budget, period)
    } else {
        Ok(BudgetUsage {
            budget_id: budget.id.clone(),
            name: budget.name.clone(),
            scope: budget.scope,
            category_id: budget.category_id.clone(),
            currency: budget.currency.clone(),
            amount_cents: budget.amount_cents,
            used_cents: 0,
            remaining_cents: budget.amount_cents,
            period_start_at: period.start_at,
            period_end_at: period.end_at,
            is_active: budget.is_active,
        })
    }
}
