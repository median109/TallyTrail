//! TallyTrail Core 快速上手示例。
//!
//! 运行：
//!
//! ```text
//! cargo run -p tallytrail-core --example quickstart
//! ```
//!
//! 示例会在系统临时目录创建 `tallytrail_quickstart.db`，每次运行前自动清空。

use chrono::{TimeZone, Utc};
use tallytrail_core::{
    AccountType, BudgetScope, CategoryKind, CoreHandle, CreateAccountInput, CreateBudgetInput,
    CreateCategoryInput, CreateExpenseInput, CreateIncomeInput, CreateLoanDisbursementInput,
    CreateLoanInput, CreateLoanRepaymentInput, CreateRefundInput, CreateReimbursementInput,
    CreateTransferInput, Granularity, PeriodType, SettlementInput,
};

fn at(year: i32, month: u32, day: u32) -> i64 {
    Utc.with_ymd_and_hms(year, month, day, 12, 0, 0)
        .unwrap()
        .timestamp()
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let db = std::env::temp_dir().join("tallytrail_quickstart.db");
    for suffix in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{}", db.display(), suffix));
    }

    // 1. 打开数据库：自动建表、启用外键与 WAL。
    let core = CoreHandle::open(&db)?;

    // 2. 建账户。金额单位是“分”，币种默认 CNY。
    let bank = core.create_account(CreateAccountInput {
        name: "招商银行".into(),
        account_type: AccountType::Bank,
        currency: Some("CNY".into()),
        initial_balance_cents: 1_000_000,
    })?;
    let wallet = core.create_account(CreateAccountInput {
        name: "支付宝".into(),
        account_type: AccountType::EWallet,
        currency: Some("CNY".into()),
        initial_balance_cents: 50_000,
    })?;
    let _usd_card = core.create_account(CreateAccountInput {
        name: "美元信用卡".into(),
        account_type: AccountType::Credit,
        currency: Some("USD".into()),
        initial_balance_cents: 0,
    })?;

    // 3. 建两级分类：一级“餐饮”下挂二级“午餐”。
    let food = core.create_category(CreateCategoryInput {
        name: "餐饮".into(),
        parent_id: None,
        kind: CategoryKind::Expense,
        sort_order: 0,
    })?;
    let lunch = core.create_category(CreateCategoryInput {
        name: "午餐".into(),
        parent_id: Some(food.clone()),
        kind: CategoryKind::Expense,
        sort_order: 0,
    })?;
    let salary = core.create_category(CreateCategoryInput {
        name: "工资".into(),
        parent_id: None,
        kind: CategoryKind::Income,
        sort_order: 0,
    })?;
    let tag = core.create_tag("出差".into())?;

    // 4. 普通支出：45.50 CNY，从支付宝支付。
    let lunch_txn = core.create_expense(CreateExpenseInput {
        occurred_at: at(2026, 10, 1),
        category_id: lunch,
        note: Some("工作日午餐".into()),
        total_amount_cents: 4_550,
        currency: None,
        actual_amount_cents: None,
        settlements: vec![SettlementInput {
            account_id: wallet.clone(),
            amount_cents: 4_550,
        }],
        tag_ids: vec![tag.clone()],
    })?;

    // 5. 境外消费：花费 100.00 USD，实际从人民币账户扣 730.00 CNY。
    let foreign = core.create_expense(CreateExpenseInput {
        occurred_at: at(2026, 10, 2),
        category_id: food.clone(),
        note: Some("境外餐饮".into()),
        total_amount_cents: 10_000,
        currency: Some("USD".into()),
        actual_amount_cents: Some(73_000),
        settlements: vec![SettlementInput {
            account_id: bank.clone(),
            amount_cents: 73_000,
        }],
        tag_ids: vec![tag.clone()],
    })?;

    // 6. 退款：对午餐退 20.00 CNY 回支付宝。
    core.create_refund(CreateRefundInput {
        occurred_at: at(2026, 10, 3),
        ref_txn_id: lunch_txn.clone(),
        note: Some("部分退款".into()),
        total_amount_cents: 2_000,
        actual_amount_cents: None,
        settlements: vec![SettlementInput {
            account_id: wallet.clone(),
            amount_cents: 2_000,
        }],
        tag_ids: vec![],
    })?;

    // 7. 报销：对境外消费报销 30.00 USD，进入报销来源账户。
    let reimburse_account = core.create_account(CreateAccountInput {
        name: "公司报销".into(),
        account_type: AccountType::ReimburseSource,
        currency: Some("USD".into()),
        initial_balance_cents: 0,
    })?;
    core.create_reimbursement(CreateReimbursementInput {
        occurred_at: at(2026, 10, 4),
        ref_txn_id: foreign,
        note: Some("公司报销".into()),
        total_amount_cents: 3_000,
        actual_amount_cents: None,
        settlements: vec![SettlementInput {
            account_id: reimburse_account,
            amount_cents: 3_000,
        }],
        tag_ids: vec![],
    })?;

    // 8. 收入与账户间转账。
    core.create_income(CreateIncomeInput {
        occurred_at: at(2026, 10, 5),
        category_id: salary,
        note: Some("十月工资".into()),
        total_amount_cents: 2_000_000,
        currency: None,
        actual_amount_cents: None,
        settlements: vec![SettlementInput {
            account_id: bank.clone(),
            amount_cents: 2_000_000,
        }],
        tag_ids: vec![],
    })?;
    core.create_transfer(CreateTransferInput {
        occurred_at: at(2026, 10, 6),
        from_account_id: bank.clone(),
        to_account_id: wallet.clone(),
        amount_cents: 100_000,
        note: Some("补充零花钱".into()),
        tag_ids: vec![],
    })?;

    // 9. 贷款：本金 5000.00 CNY，放款到银行卡，再还本金 1000.00 + 利息 30.00。
    let loan = core.create_loan(CreateLoanInput {
        lender: "某银行".into(),
        principal_cents: 500_000,
        currency: None,
        interest_rate_bp: 450,
        start_at: at(2026, 10, 7),
        term_months: Some(12),
        account_id: Some(bank.clone()),
        note: None,
    })?;
    core.create_loan_disbursement(CreateLoanDisbursementInput {
        occurred_at: at(2026, 10, 7),
        loan_id: loan.clone(),
        amount_cents: 500_000,
        account_id: bank.clone(),
        note: None,
        tag_ids: vec![],
    })?;
    core.create_loan_repayment(CreateLoanRepaymentInput {
        occurred_at: at(2026, 10, 8),
        loan_id: loan.clone(),
        actual_amount_cents: 103_000,
        principal_part_cents: 100_000,
        interest_part_cents: 3_000,
        account_id: bank.clone(),
        category_id: None,
        note: Some("第一期".into()),
        tag_ids: vec![],
    })?;

    // 10. 预算：2026 年 10 月餐饮预算 300.00 CNY。
    let budget = core.create_budget(CreateBudgetInput {
        name: "十月餐饮".into(),
        scope: BudgetScope::CategoryL1,
        category_id: Some(food.clone()),
        period_type: PeriodType::Monthly,
        amount_cents: 30_000,
        currency: None,
        start_at: None,
        end_at: None,
        rollover: false,
    })?;

    // 11. 查询。
    let month_start = at(2026, 10, 1) - 12 * 3600;
    let month_end = at(2026, 11, 1) - 12 * 3600;
    let summary = core.income_expense_summary(month_start, month_end)?;
    for row in &summary.by_currency {
        println!(
            "[{}] 收入 {:.2} / 支出 {:.2} / 退款 {:.2} / 报销 {:.2}",
            row.currency,
            row.total_income_cents as f64 / 100.0,
            row.total_expense_cents as f64 / 100.0,
            row.total_refund_cents as f64 / 100.0,
            row.total_reimbursement_cents as f64 / 100.0,
        );
    }

    let usage = core.budget_usage(&budget, at(2026, 10, 20))?;
    println!(
        "预算 {}：已用 {:.2} / {:.2} {}",
        usage.name,
        usage.used_cents as f64 / 100.0,
        usage.amount_cents as f64 / 100.0,
        usage.currency
    );

    let points = core.net_worth_series(month_start, at(2026, 10, 31), Granularity::Monthly)?;
    for point in points {
        for row in point.by_currency {
            println!(
                "净资产 [{}] 资产 {:.2} − 负债 {:.2} = {:.2}",
                row.currency,
                row.total_assets_cents as f64 / 100.0,
                row.total_liabilities_cents as f64 / 100.0,
                row.net_worth_cents as f64 / 100.0,
            );
        }
    }

    let balances = core.list_account_balances(month_end)?;
    for account in balances {
        println!(
            "账户 {}：{:.2} {}",
            account.name,
            account.balance_cents as f64 / 100.0,
            account.currency
        );
    }

    // 12. 关闭前 checkpoint，确保数据全部写回主 .db 文件。
    core.checkpoint()?;
    println!("数据库：{}", db.display());
    Ok(())
}
