use chrono::{TimeZone, Utc};
use tallytrail_core::*;
use tempfile::TempDir;

struct Fixture {
    core: CoreHandle,
    _dir: TempDir,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let core = CoreHandle::open(dir.path().join("tallytrail.db")).expect("open core");
        Self { core, _dir: dir }
    }
}

fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
    Utc.with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .expect("valid utc time")
        .timestamp()
}

fn account(
    core: &CoreHandle,
    name: &str,
    account_type: AccountType,
    currency: &str,
    initial_balance_cents: i64,
) -> String {
    core.create_account(CreateAccountInput {
        name: name.to_string(),
        account_type,
        currency: Some(currency.to_string()),
        initial_balance_cents,
    })
    .expect("create account")
}

fn category(core: &CoreHandle, name: &str, kind: CategoryKind, parent_id: Option<&str>) -> String {
    core.create_category(CreateCategoryInput {
        name: name.to_string(),
        parent_id: parent_id.map(ToString::to_string),
        kind,
        sort_order: 0,
    })
    .expect("create category")
}

fn settle(account_id: &str, amount_cents: i64) -> SettlementInput {
    SettlementInput {
        account_id: account_id.to_string(),
        amount_cents,
    }
}

fn expense(
    core: &CoreHandle,
    occurred_at: i64,
    category_id: &str,
    total_amount_cents: i64,
    settlements: Vec<SettlementInput>,
) -> String {
    core.create_expense(CreateExpenseInput {
        occurred_at,
        category_id: category_id.to_string(),
        note: None,
        total_amount_cents,
        currency: None,
        actual_amount_cents: None,
        settlements,
        tag_ids: vec![],
    })
    .expect("create expense")
}

#[test]
fn combination_payment_updates_balances_and_statistics() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "工资卡", AccountType::Bank, "CNY", 100_000);
    let wallet = account(&fx.core, "钱包", AccountType::EWallet, "CNY", 5_000);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);

    let id = expense(
        &fx.core,
        at(2026, 10, 1, 12, 30),
        &food,
        12_000,
        vec![settle(&bank, 10_000), settle(&wallet, 2_000)],
    );

    let detail = fx.core.get_transaction(id.clone()).unwrap();
    assert_eq!(detail.actual_amount_cents, 12_000);
    assert_eq!(detail.total_amount_cents, 12_000);
    assert_eq!(detail.currency, "CNY");
    assert_eq!(detail.direction, Direction::Outflow);
    assert_eq!(detail.settlements.len(), 2);
    assert!(detail
        .settlements
        .iter()
        .all(|row| row.role == SplitRole::Payment));

    assert_eq!(
        fx.core
            .account_balance(&bank, at(2026, 10, 2, 0, 0))
            .unwrap(),
        90_000
    );
    assert_eq!(
        fx.core
            .account_balance(&wallet, at(2026, 10, 2, 0, 0))
            .unwrap(),
        3_000
    );

    let summary = fx
        .core
        .income_expense_summary(at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0))
        .unwrap();
    assert_eq!(summary.by_currency.len(), 1);
    assert_eq!(summary.by_currency[0].total_expense_cents, 12_000);
    assert_eq!(summary.by_currency[0].total_income_cents, 0);
}

#[test]
fn settlement_shape_and_mixed_currency_are_enforced() {
    let fx = Fixture::new();
    let cny = account(&fx.core, "CNY", AccountType::Bank, "CNY", 100_000);
    let usd = account(&fx.core, "USD", AccountType::Bank, "USD", 100_000);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);

    let error = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: food.clone(),
            note: None,
            total_amount_cents: 10_000,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&cny, 9_999)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let error = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: food.clone(),
            note: None,
            total_amount_cents: 10_000,
            currency: None,
            actual_amount_cents: Some(10_000),
            settlements: vec![settle(&cny, 5_000), settle(&usd, 5_000)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert_eq!(error, CoreError::MixedCurrencySettlement);

    let error = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: food.clone(),
            note: None,
            total_amount_cents: 10_000,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&cny, 0)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    // 结算行已写入前失败：整个事务必须回滚。
    let error = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: food.clone(),
            note: None,
            total_amount_cents: 10_000,
            currency: None,
            actual_amount_cents: Some(10_000),
            settlements: vec![settle(&cny, 10_000)],
            tag_ids: vec!["not-a-tag".to_string()],
        })
        .unwrap_err();
    assert_eq!(error, CoreError::NotFound);
    assert!(fx
        .core
        .list_transactions(TransactionFilter::default())
        .unwrap()
        .is_empty());
}

#[test]
fn foreign_currency_requires_dual_amounts_and_keeps_calibers_separate() {
    let fx = Fixture::new();
    let cny = account(&fx.core, "CNY", AccountType::Bank, "CNY", 100_000);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);

    let error = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 2, 20, 0),
            category_id: food.clone(),
            note: Some("境外消费".to_string()),
            total_amount_cents: 10_000,
            currency: Some("USD".to_string()),
            actual_amount_cents: None,
            settlements: vec![settle(&cny, 72_000)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let id = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 2, 20, 0),
            category_id: food.clone(),
            note: Some("境外消费".to_string()),
            total_amount_cents: 10_000,
            currency: Some("USD".to_string()),
            actual_amount_cents: Some(72_000),
            settlements: vec![settle(&cny, 72_000)],
            tag_ids: vec![],
        })
        .unwrap();

    let detail = fx.core.get_transaction(id).unwrap();
    assert_eq!(detail.total_amount_cents, 10_000);
    assert_eq!(detail.currency, "USD");
    assert_eq!(detail.actual_amount_cents, 72_000);
    assert_eq!(detail.settlements[0].currency, "CNY");
    assert_eq!(
        fx.core
            .account_balance(&cny, at(2026, 10, 3, 0, 0))
            .unwrap(),
        28_000
    );

    let summary = fx
        .core
        .income_expense_summary(at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0))
        .unwrap();
    assert_eq!(summary.by_currency.len(), 1);
    assert_eq!(summary.by_currency[0].currency, "USD");
    assert_eq!(summary.by_currency[0].total_expense_cents, 10_000);
}

#[test]
fn category_tree_rules_aggregation_and_lifecycle() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "卡", AccountType::Bank, "CNY", 100_000);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);
    let dining = category(&fx.core, "外出就餐", CategoryKind::Expense, Some(&food));
    let salary = category(&fx.core, "工资", CategoryKind::Income, None);

    let error = fx
        .core
        .create_category(CreateCategoryInput {
            name: "奖金".to_string(),
            parent_id: Some(salary.clone()),
            kind: CategoryKind::Expense,
            sort_order: 0,
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let error = fx
        .core
        .create_category(CreateCategoryInput {
            name: "三级".to_string(),
            parent_id: Some(dining.clone()),
            kind: CategoryKind::Expense,
            sort_order: 0,
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let error = fx
        .core
        .create_category(CreateCategoryInput {
            name: "餐饮".to_string(),
            parent_id: None,
            kind: CategoryKind::Expense,
            sort_order: 0,
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let error = fx
        .core
        .create_category(CreateCategoryInput {
            name: "餐饮子项".to_string(),
            parent_id: Some(food.clone()),
            kind: CategoryKind::Income,
            sort_order: 0,
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    // 一级分类与二级分类都是合法落点。
    expense(
        &fx.core,
        at(2026, 10, 5, 12, 0),
        &food,
        6_000,
        vec![settle(&bank, 6_000)],
    );
    expense(
        &fx.core,
        at(2026, 10, 6, 12, 0),
        &dining,
        4_000,
        vec![settle(&bank, 4_000)],
    );

    let l1 = fx
        .core
        .category_spending_series(
            food.clone(),
            at(2026, 10, 1, 0, 0),
            at(2026, 11, 1, 0, 0),
            Granularity::Monthly,
        )
        .unwrap();
    assert_eq!(l1.len(), 1);
    assert_eq!(l1[0].amount_cents, 10_000);

    let l2 = fx
        .core
        .category_spending_series(
            dining.clone(),
            at(2026, 10, 1, 0, 0),
            at(2026, 11, 1, 0, 0),
            Granularity::Monthly,
        )
        .unwrap();
    assert_eq!(l2.len(), 1);
    assert_eq!(l2[0].amount_cents, 4_000);

    let error = fx
        .core
        .change_category_kind(food.clone(), CategoryKind::Income)
        .unwrap_err();
    assert_eq!(error, CoreError::CategoryInUse);

    let error = fx.core.delete_category(dining.clone()).unwrap_err();
    assert_eq!(error, CoreError::CategoryInUse);

    fx.core.set_category_active(dining.clone(), false).unwrap();
    let error = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 7, 12, 0),
            category_id: dining.clone(),
            note: None,
            total_amount_cents: 100,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 100)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    // 历史交易仍参与统计。
    let l2 = fx
        .core
        .category_spending_series(
            dining.clone(),
            at(2026, 10, 1, 0, 0),
            at(2026, 11, 1, 0, 0),
            Granularity::Monthly,
        )
        .unwrap();
    assert_eq!(l2[0].amount_cents, 4_000);
    assert!(fx
        .core
        .list_categories(Some(CategoryKind::Expense), false)
        .unwrap()
        .iter()
        .all(|item| item.id != dining));
    assert!(fx
        .core
        .list_categories(Some(CategoryKind::Expense), true)
        .unwrap()
        .iter()
        .any(|item| item.id == dining));

    // 无引用分类可改 kind 并删除。
    let temp = category(&fx.core, "临时", CategoryKind::Expense, None);
    fx.core
        .change_category_kind(temp.clone(), CategoryKind::Income)
        .unwrap();
    fx.core.delete_category(temp).unwrap();
}

#[test]
fn refunds_and_reimbursements_inherit_and_offset() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "卡", AccountType::Bank, "CNY", 100_000);
    let source = account(&fx.core, "报销来源", AccountType::ReimburseSource, "CNY", 0);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);
    let original = expense(
        &fx.core,
        at(2026, 10, 1, 12, 0),
        &food,
        10_000,
        vec![settle(&bank, 10_000)],
    );

    let refund = fx
        .core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 5, 9, 0),
            ref_txn_id: original.clone(),
            note: Some("部分退款".to_string()),
            total_amount_cents: 3_000,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 3_000)],
            tag_ids: vec![],
        })
        .unwrap();
    let refund_detail = fx.core.get_transaction(refund.clone()).unwrap();
    assert_eq!(refund_detail.category_id, Some(food.clone()));
    assert_eq!(refund_detail.currency, "CNY");
    assert_eq!(refund_detail.direction, Direction::Inflow);
    assert_eq!(refund_detail.ref_txn_id, Some(original.clone()));

    let error = fx
        .core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 6, 9, 0),
            ref_txn_id: original.clone(),
            note: None,
            total_amount_cents: 8_000,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 8_000)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let reimbursement = fx
        .core
        .create_reimbursement(CreateReimbursementInput {
            occurred_at: at(2026, 10, 7, 9, 0),
            ref_txn_id: original.clone(),
            note: None,
            total_amount_cents: 4_000,
            actual_amount_cents: None,
            settlements: vec![settle(&source, 4_000)],
            tag_ids: vec![],
        })
        .unwrap();
    let reimbursement_detail = fx.core.get_transaction(reimbursement.clone()).unwrap();
    assert_eq!(reimbursement_detail.category_id, Some(food.clone()));
    assert_eq!(
        reimbursement_detail.settlements[0].role,
        SplitRole::ReimburseSource
    );

    let error = fx
        .core
        .create_reimbursement(CreateReimbursementInput {
            occurred_at: at(2026, 10, 8, 9, 0),
            ref_txn_id: original.clone(),
            note: None,
            total_amount_cents: 7_000,
            actual_amount_cents: None,
            settlements: vec![settle(&source, 7_000)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    assert_eq!(
        fx.core
            .account_balance(&source, at(2026, 10, 9, 0, 0))
            .unwrap(),
        4_000
    );
    assert_eq!(
        fx.core
            .account_balance(&bank, at(2026, 10, 9, 0, 0))
            .unwrap(),
        93_000
    );

    let summary = fx
        .core
        .income_expense_summary(at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0))
        .unwrap();
    assert_eq!(summary.by_currency[0].total_expense_cents, 10_000);
    assert_eq!(summary.by_currency[0].total_refund_cents, 3_000);
    assert_eq!(summary.by_currency[0].total_reimbursement_cents, 4_000);
    assert_eq!(summary.by_currency[0].net_expense_cents().unwrap(), 3_000);

    let series = fx
        .core
        .category_spending_series(
            food.clone(),
            at(2026, 10, 1, 0, 0),
            at(2026, 11, 1, 0, 0),
            Granularity::Monthly,
        )
        .unwrap();
    assert_eq!(series.len(), 1);
    assert_eq!(series[0].amount_cents, 3_000);

    // 有未作废关联时不能作废原消费。
    assert_eq!(
        fx.core
            .void_transaction(original.clone(), "想作废".to_string())
            .unwrap_err(),
        CoreError::HasActiveDependents
    );
    fx.core
        .void_transaction(refund.clone(), "退款录错".to_string())
        .unwrap();
    assert_eq!(
        fx.core
            .void_transaction(original.clone(), "再试".to_string())
            .unwrap_err(),
        CoreError::HasActiveDependents
    );
    fx.core
        .void_transaction(reimbursement, "报销录错".to_string())
        .unwrap();
    fx.core
        .void_transaction(original.clone(), "原消费录错".to_string())
        .unwrap();

    let error = fx
        .core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 10, 9, 0),
            ref_txn_id: original,
            note: None,
            total_amount_cents: 100,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 100)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let summary = fx
        .core
        .income_expense_summary(at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0))
        .unwrap();
    assert!(summary.by_currency.is_empty() || summary.by_currency[0].total_expense_cents == 0);
}

#[test]
fn foreign_refund_uses_original_currency_and_explicit_actual() {
    let fx = Fixture::new();
    let cny = account(&fx.core, "CNY", AccountType::Bank, "CNY", 100_000);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);
    let original = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: food,
            note: None,
            total_amount_cents: 10_000,
            currency: Some("USD".to_string()),
            actual_amount_cents: Some(72_000),
            settlements: vec![settle(&cny, 72_000)],
            tag_ids: vec![],
        })
        .unwrap();

    let error = fx
        .core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 2, 12, 0),
            ref_txn_id: original.clone(),
            note: None,
            total_amount_cents: 3_000,
            actual_amount_cents: None,
            settlements: vec![settle(&cny, 21_600)],
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let refund = fx
        .core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 2, 12, 0),
            ref_txn_id: original,
            note: None,
            total_amount_cents: 3_000,
            actual_amount_cents: Some(21_600),
            settlements: vec![settle(&cny, 21_600)],
            tag_ids: vec![],
        })
        .unwrap();
    let detail = fx.core.get_transaction(refund).unwrap();
    assert_eq!(detail.currency, "USD");
    assert_eq!(detail.total_amount_cents, 3_000);
    assert_eq!(detail.actual_amount_cents, 21_600);
    assert_eq!(
        fx.core
            .account_balance(&cny, at(2026, 10, 3, 0, 0))
            .unwrap(),
        49_600
    );
}

#[test]
fn loan_lifecycle_and_interest_caliber() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "卡", AccountType::Bank, "CNY", 0);
    let interest_category = category(&fx.core, "贷款利息", CategoryKind::Expense, None);
    let loan = fx
        .core
        .create_loan(CreateLoanInput {
            lender: "银行".to_string(),
            principal_cents: 100_000,
            currency: None,
            interest_rate_bp: 450,
            start_at: at(2026, 10, 1, 0, 0),
            term_months: Some(12),
            account_id: Some(bank.clone()),
            note: None,
        })
        .unwrap();

    let disburse = fx
        .core
        .create_loan_disbursement(CreateLoanDisbursementInput {
            occurred_at: at(2026, 10, 2, 9, 0),
            loan_id: loan.clone(),
            amount_cents: 100_000,
            account_id: bank.clone(),
            note: None,
            tag_ids: vec![],
        })
        .unwrap();
    let detail = fx.core.get_transaction(disburse.clone()).unwrap();
    assert_eq!(detail.direction, Direction::Inflow);
    assert_eq!(detail.loan_id, Some(loan.clone()));
    assert_eq!(detail.settlements[0].role, SplitRole::Receipt);

    let balance = fx.core.loan_outstanding(&loan).unwrap();
    assert_eq!(balance.outstanding_cents, 100_000);
    assert_eq!(balance.status, LoanStatus::Active);
    assert_eq!(
        fx.core
            .account_balance(&bank, at(2026, 10, 2, 9, 0))
            .unwrap(),
        100_000
    );

    let error = fx
        .core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 15, 9, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 21_000,
            principal_part_cents: 20_000,
            interest_part_cents: 500,
            account_id: bank.clone(),
            category_id: Some(interest_category.clone()),
            note: None,
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    fx.core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 15, 9, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 21_000,
            principal_part_cents: 20_000,
            interest_part_cents: 1_000,
            account_id: bank.clone(),
            category_id: Some(interest_category.clone()),
            note: None,
            tag_ids: vec![],
        })
        .unwrap();
    let balance = fx.core.loan_outstanding(&loan).unwrap();
    assert_eq!(balance.repaid_principal_cents, 20_000);
    assert_eq!(balance.outstanding_cents, 80_000);

    let summary = fx
        .core
        .income_expense_summary(at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0))
        .unwrap();
    assert_eq!(summary.by_currency[0].total_expense_cents, 1_000);
    assert_eq!(summary.by_currency[0].total_income_cents, 0);

    fx.core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 20, 9, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 80_500,
            principal_part_cents: 80_000,
            interest_part_cents: 500,
            account_id: bank.clone(),
            category_id: None,
            note: None,
            tag_ids: vec![],
        })
        .unwrap();
    let balance = fx.core.loan_outstanding(&loan).unwrap();
    assert_eq!(balance.outstanding_cents, 0);
    assert_eq!(balance.status, LoanStatus::Settled);

    let error = fx
        .core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 21, 9, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 100,
            principal_part_cents: 0,
            interest_part_cents: 100,
            account_id: bank.clone(),
            category_id: None,
            note: None,
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    assert_eq!(
        fx.core
            .void_transaction(disburse, "放款录错".to_string())
            .unwrap_err(),
        CoreError::HasActiveDependents
    );

    let repayments = fx
        .core
        .list_transactions(TransactionFilter {
            types: Some(vec![TxnType::LoanRepay]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(repayments.len(), 2);
    fx.core
        .void_transaction(repayments[0].id.clone(), "还款录错".to_string())
        .unwrap();
    let balance = fx.core.loan_outstanding(&loan).unwrap();
    assert_eq!(balance.outstanding_cents, 80_000);
    assert_eq!(balance.status, LoanStatus::Active);

    let error = fx
        .core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 22, 9, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 90_000,
            principal_part_cents: 90_000,
            interest_part_cents: 0,
            account_id: bank.clone(),
            category_id: None,
            note: None,
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));

    let usd = account(&fx.core, "USD", AccountType::Bank, "USD", 0);
    let error = fx
        .core
        .create_loan_disbursement(CreateLoanDisbursementInput {
            occurred_at: at(2026, 10, 23, 9, 0),
            loan_id: loan,
            amount_cents: 100,
            account_id: usd,
            note: None,
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));
}

#[test]
fn transfer_is_internal_and_does_not_change_net_worth() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "A", AccountType::Bank, "CNY", 10_000);
    let cash = account(&fx.core, "B", AccountType::Cash, "CNY", 0);
    let transfer = fx
        .core
        .create_transfer(CreateTransferInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            from_account_id: bank.clone(),
            to_account_id: cash.clone(),
            amount_cents: 3_000,
            note: Some("取现".to_string()),
            tag_ids: vec![],
        })
        .unwrap();

    let detail = fx.core.get_transaction(transfer).unwrap();
    assert_eq!(detail.actual_amount_cents, 3_000);
    assert_eq!(detail.total_amount_cents, 3_000);
    assert_eq!(detail.settlements.len(), 2);
    assert_eq!(detail.direction, Direction::Outflow);
    assert_eq!(
        fx.core
            .account_balance(&bank, at(2026, 10, 2, 0, 0))
            .unwrap(),
        7_000
    );
    assert_eq!(
        fx.core
            .account_balance(&cash, at(2026, 10, 2, 0, 0))
            .unwrap(),
        3_000
    );

    let summary = fx
        .core
        .income_expense_summary(at(2026, 10, 1, 0, 0), at(2026, 11, 1, 0, 0))
        .unwrap();
    assert!(summary.by_currency.is_empty());

    let networth = fx
        .core
        .net_worth_series(
            at(2026, 10, 1, 0, 0),
            at(2026, 11, 1, 0, 0),
            Granularity::Monthly,
        )
        .unwrap();
    assert_eq!(networth.len(), 1);
    assert_eq!(networth[0].by_currency[0].total_assets_cents, 10_000);
    assert_eq!(networth[0].by_currency[0].net_worth_cents, 10_000);

    let error = fx
        .core
        .create_transfer(CreateTransferInput {
            occurred_at: at(2026, 10, 2, 12, 0),
            from_account_id: bank.clone(),
            to_account_id: bank,
            amount_cents: 100,
            note: None,
            tag_ids: vec![],
        })
        .unwrap_err();
    assert!(matches!(error, CoreError::ValidationError(_)));
}

#[test]
fn tags_are_case_insensitive_unique_and_deletable() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "卡", AccountType::Bank, "CNY", 100_000);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);
    let tag = fx.core.create_tag("Trip".to_string()).unwrap();
    assert!(matches!(
        fx.core.create_tag("trip".to_string()).unwrap_err(),
        CoreError::ValidationError(_)
    ));
    fx.core
        .rename_tag(tag.clone(), "Travel".to_string())
        .unwrap();
    let other = fx.core.create_tag("Other".to_string()).unwrap();
    assert!(matches!(
        fx.core.rename_tag(other, "travel".to_string()).unwrap_err(),
        CoreError::ValidationError(_)
    ));

    let txn = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: food,
            note: None,
            total_amount_cents: 1_000,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 1_000)],
            tag_ids: vec![tag.clone()],
        })
        .unwrap();

    let filtered = fx
        .core
        .list_transactions(TransactionFilter {
            tag_ids: Some(vec![tag.clone()]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].tags[0].name, "Travel");

    fx.core.delete_tag(tag).unwrap();
    let detail = fx.core.get_transaction(txn).unwrap();
    assert!(detail.tags.is_empty());
}

#[test]
fn amount_overflow_is_reported() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "卡", AccountType::Bank, "CNY", i64::MAX);
    let salary = category(&fx.core, "收入", CategoryKind::Income, None);
    fx.core
        .create_income(CreateIncomeInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: salary,
            note: None,
            total_amount_cents: 1,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 1)],
            tag_ids: vec![],
        })
        .unwrap();
    assert_eq!(
        fx.core
            .account_balance(&bank, at(2026, 10, 2, 0, 0))
            .unwrap_err(),
        CoreError::AmountOverflow
    );

    // 结算行累加也必须在 Rust 侧做 checked_add，而不是依赖 SQLite 的 SUM。
    let huge = account(&fx.core, "大额", AccountType::Bank, "CNY", 0);
    let income = category(&fx.core, "收入二", CategoryKind::Income, None);
    fx.core
        .create_income(CreateIncomeInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: income.clone(),
            note: None,
            total_amount_cents: i64::MAX,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&huge, i64::MAX)],
            tag_ids: vec![],
        })
        .unwrap();
    fx.core
        .create_income(CreateIncomeInput {
            occurred_at: at(2026, 10, 1, 13, 0),
            category_id: income,
            note: None,
            total_amount_cents: 1,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&huge, 1)],
            tag_ids: vec![],
        })
        .unwrap();
    assert_eq!(
        fx.core
            .account_balance(&huge, at(2026, 10, 2, 0, 0))
            .unwrap_err(),
        CoreError::AmountOverflow
    );
    assert_eq!(
        fx.core
            .income_expense_summary(at(2026, 10, 1, 0, 0), at(2026, 10, 2, 0, 0))
            .unwrap_err(),
        CoreError::AmountOverflow
    );
}

#[test]
fn budgets_use_net_expense_by_category_and_currency() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "CNY", AccountType::Bank, "CNY", 1_000_000);
    let usd = account(&fx.core, "USD", AccountType::Bank, "USD", 1_000_000);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);
    let dining = category(&fx.core, "外出", CategoryKind::Expense, Some(&food));
    let takeout = category(&fx.core, "外卖", CategoryKind::Expense, Some(&food));
    let interest = category(&fx.core, "利息", CategoryKind::Expense, None);

    let d = expense(
        &fx.core,
        at(2026, 10, 5, 12, 0),
        &dining,
        10_000,
        vec![settle(&bank, 10_000)],
    );
    let t = expense(
        &fx.core,
        at(2026, 10, 6, 12, 0),
        &takeout,
        5_000,
        vec![settle(&bank, 5_000)],
    );
    expense(
        &fx.core,
        at(2026, 10, 7, 12, 0),
        &food,
        2_000,
        vec![settle(&bank, 2_000)],
    );
    // 外币交易不进入 CNY 预算。
    fx.core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 7, 12, 0),
            category_id: dining.clone(),
            note: None,
            total_amount_cents: 10_000,
            currency: Some("USD".to_string()),
            actual_amount_cents: None,
            settlements: vec![settle(&usd, 10_000)],
            tag_ids: vec![],
        })
        .unwrap();
    fx.core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 20, 12, 0),
            ref_txn_id: d,
            note: None,
            total_amount_cents: 3_000,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 3_000)],
            tag_ids: vec![],
        })
        .unwrap();
    fx.core
        .create_reimbursement(CreateReimbursementInput {
            occurred_at: at(2026, 10, 21, 12, 0),
            ref_txn_id: t,
            note: None,
            total_amount_cents: 1_000,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 1_000)],
            tag_ids: vec![],
        })
        .unwrap();
    // 贷款还款不计入预算（本金与利息都不计入）。
    let loan = fx
        .core
        .create_loan(CreateLoanInput {
            lender: "银行".to_string(),
            principal_cents: 100_000,
            currency: None,
            interest_rate_bp: 0,
            start_at: at(2026, 10, 1, 0, 0),
            term_months: None,
            account_id: None,
            note: None,
        })
        .unwrap();
    fx.core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 22, 12, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 500,
            principal_part_cents: 0,
            interest_part_cents: 500,
            account_id: bank.clone(),
            category_id: Some(interest.clone()),
            note: None,
            tag_ids: vec![],
        })
        .unwrap();
    fx.core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 23, 12, 0),
            loan_id: loan,
            actual_amount_cents: 300,
            principal_part_cents: 0,
            interest_part_cents: 300,
            account_id: bank.clone(),
            category_id: Some(takeout.clone()),
            note: None,
            tag_ids: vec![],
        })
        .unwrap();

    let l1_budget = fx
        .core
        .create_budget(CreateBudgetInput {
            name: "餐饮总预算".to_string(),
            scope: BudgetScope::CategoryL1,
            category_id: Some(food.clone()),
            period_type: PeriodType::Monthly,
            amount_cents: 20_000,
            currency: None,
            start_at: None,
            end_at: None,
            rollover: false,
        })
        .unwrap();
    let usage = fx
        .core
        .budget_usage(&l1_budget, at(2026, 10, 25, 0, 0))
        .unwrap();
    assert_eq!(usage.used_cents, 13_000);
    assert_eq!(usage.remaining_cents, 7_000);
    assert_eq!(usage.period_start_at, at(2026, 10, 1, 0, 0));
    assert_eq!(usage.period_end_at, at(2026, 11, 1, 0, 0));

    let l2_budget = fx
        .core
        .create_budget(CreateBudgetInput {
            name: "外卖预算".to_string(),
            scope: BudgetScope::CategoryL2,
            category_id: Some(takeout),
            period_type: PeriodType::Monthly,
            amount_cents: 6_000,
            currency: None,
            start_at: None,
            end_at: None,
            rollover: false,
        })
        .unwrap();
    let usage = fx
        .core
        .budget_usage(&l2_budget, at(2026, 10, 25, 0, 0))
        .unwrap();
    assert_eq!(usage.used_cents, 4_000);

    let total_budget = fx
        .core
        .create_budget(CreateBudgetInput {
            name: "总预算".to_string(),
            scope: BudgetScope::Total,
            category_id: None,
            period_type: PeriodType::Monthly,
            amount_cents: 100_000,
            currency: Some("CNY".to_string()),
            start_at: None,
            end_at: None,
            rollover: true,
        })
        .unwrap();
    let usage = fx
        .core
        .budget_usage(&total_budget, at(2026, 10, 25, 0, 0))
        .unwrap();
    assert_eq!(usage.used_cents, 13_000);

    // CUSTOM 左闭右开，且 as_of 在区间外时 used=0。
    let custom = fx
        .core
        .create_budget(CreateBudgetInput {
            name: "自定义".to_string(),
            scope: BudgetScope::CategoryL1,
            category_id: Some(food.clone()),
            period_type: PeriodType::Custom,
            amount_cents: 50_000,
            currency: None,
            start_at: Some(at(2026, 10, 1, 0, 0)),
            end_at: Some(at(2026, 10, 20, 0, 0)),
            rollover: false,
        })
        .unwrap();
    let usage = fx
        .core
        .budget_usage(&custom, at(2026, 10, 19, 23, 59))
        .unwrap();
    assert_eq!(usage.used_cents, 17_000);
    let usage = fx
        .core
        .budget_usage(&custom, at(2026, 10, 20, 0, 0))
        .unwrap();
    assert_eq!(usage.used_cents, 0);

    assert!(matches!(
        fx.core
            .create_budget(CreateBudgetInput {
                name: "层级错误".to_string(),
                scope: BudgetScope::CategoryL1,
                category_id: Some(dining),
                period_type: PeriodType::Monthly,
                amount_cents: 100,
                currency: None,
                start_at: None,
                end_at: None,
                rollover: false,
            })
            .unwrap_err(),
        CoreError::ValidationError(_)
    ));
    assert!(matches!(
        fx.core
            .create_budget(CreateBudgetInput {
                name: "TOTAL 带分类".to_string(),
                scope: BudgetScope::Total,
                category_id: Some(food),
                period_type: PeriodType::Monthly,
                amount_cents: 100,
                currency: None,
                start_at: None,
                end_at: None,
                rollover: false,
            })
            .unwrap_err(),
        CoreError::ValidationError(_)
    ));
    assert!(matches!(
        fx.core
            .create_budget(CreateBudgetInput {
                name: "自定义缺日期".to_string(),
                scope: BudgetScope::Total,
                category_id: None,
                period_type: PeriodType::Custom,
                amount_cents: 100,
                currency: None,
                start_at: None,
                end_at: None,
                rollover: false,
            })
            .unwrap_err(),
        CoreError::ValidationError(_)
    ));

    fx.core
        .set_budget_active(total_budget.clone(), false)
        .unwrap();
    let active = fx.core.list_budget_usage(at(2026, 10, 25, 0, 0)).unwrap();
    assert!(active.iter().all(|item| item.budget_id != total_budget));
    assert_eq!(fx.core.list_budgets(true).unwrap().len(), 4);
}

#[test]
fn net_worth_groups_by_currency_and_excludes_future_loans() {
    let fx = Fixture::new();
    account(&fx.core, "CNY", AccountType::Bank, "CNY", 100_000);
    account(&fx.core, "USD", AccountType::Bank, "USD", 10_000);
    fx.core
        .create_loan(CreateLoanInput {
            lender: "USD 银行".to_string(),
            principal_cents: 5_000,
            currency: Some("USD".to_string()),
            interest_rate_bp: 100,
            start_at: at(2026, 10, 1, 0, 0),
            term_months: Some(6),
            account_id: None,
            note: None,
        })
        .unwrap();

    let points = fx
        .core
        .net_worth_series(
            at(2026, 9, 1, 0, 0),
            at(2026, 11, 1, 0, 0),
            Granularity::Monthly,
        )
        .unwrap();
    assert_eq!(points.len(), 2);
    let september = &points[0].by_currency;
    let cny = september
        .iter()
        .find(|item| item.currency == "CNY")
        .unwrap();
    assert_eq!(cny.total_assets_cents, 100_000);
    assert_eq!(cny.total_liabilities_cents, 0);
    assert_eq!(cny.net_worth_cents, 100_000);
    let usd = september
        .iter()
        .find(|item| item.currency == "USD")
        .unwrap();
    assert_eq!(usd.total_assets_cents, 10_000);
    assert_eq!(usd.total_liabilities_cents, 0);

    let october = &points[1].by_currency;
    let usd = october.iter().find(|item| item.currency == "USD").unwrap();
    assert_eq!(usd.total_liabilities_cents, 5_000);
    assert_eq!(usd.net_worth_cents, 5_000);
}

#[test]
fn listing_filters_cover_category_tree_account_status_and_limit() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "卡", AccountType::Bank, "CNY", 100_000);
    let cash = account(&fx.core, "现金", AccountType::Cash, "CNY", 0);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);
    let dining = category(&fx.core, "外出", CategoryKind::Expense, Some(&food));
    let salary = category(&fx.core, "工资", CategoryKind::Income, None);

    let e1 = expense(
        &fx.core,
        at(2026, 10, 1, 12, 0),
        &dining,
        1_000,
        vec![settle(&bank, 1_000)],
    );
    expense(
        &fx.core,
        at(2026, 10, 2, 12, 0),
        &food,
        2_000,
        vec![settle(&cash, 2_000)],
    );
    fx.core
        .create_income(CreateIncomeInput {
            occurred_at: at(2026, 10, 3, 12, 0),
            category_id: salary,
            note: None,
            total_amount_cents: 50_000,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 50_000)],
            tag_ids: vec![],
        })
        .unwrap();
    fx.core
        .void_transaction(e1.clone(), "重复".to_string())
        .unwrap();

    let l1 = fx
        .core
        .list_transactions(TransactionFilter {
            category_id: Some(food),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(l1.len(), 2);

    let by_account = fx
        .core
        .list_transactions(TransactionFilter {
            account_id: Some(cash),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(by_account.len(), 1);

    let voided = fx
        .core
        .list_transactions(TransactionFilter {
            status: Some(TxnStatus::Voided),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(voided.len(), 1);
    assert_eq!(voided[0].id, e1);

    let income = fx
        .core
        .list_transactions(TransactionFilter {
            types: Some(vec![TxnType::Income]),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(income.len(), 1);

    let limited = fx
        .core
        .list_transactions(TransactionFilter {
            limit: Some(1),
            offset: Some(1),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(limited.len(), 1);
}

#[test]
fn required_full_workflow() {
    let fx = Fixture::new();
    let bank = account(&fx.core, "工资卡", AccountType::Bank, "CNY", 1_000_000);
    let wallet = account(&fx.core, "钱包", AccountType::EWallet, "CNY", 50_000);
    let source = account(&fx.core, "公司报销", AccountType::ReimburseSource, "CNY", 0);
    let food = category(&fx.core, "餐饮", CategoryKind::Expense, None);
    let travel = category(&fx.core, "差旅", CategoryKind::Expense, None);

    // 1. 组合支付（境内）
    let combo = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 1, 12, 0),
            category_id: food.clone(),
            note: Some("组合支付午餐".to_string()),
            total_amount_cents: 12_000,
            currency: None,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 10_000), settle(&wallet, 2_000)],
            tag_ids: vec![],
        })
        .unwrap();

    // 2. 境外消费（双金额）
    let foreign = fx
        .core
        .create_expense(CreateExpenseInput {
            occurred_at: at(2026, 10, 2, 20, 0),
            category_id: travel.clone(),
            note: Some("境外酒店".to_string()),
            total_amount_cents: 30_000,
            currency: Some("USD".to_string()),
            actual_amount_cents: Some(216_000),
            settlements: vec![settle(&bank, 216_000)],
            tag_ids: vec![],
        })
        .unwrap();

    // 3. 两次部分退款
    let refund1 = fx
        .core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 5, 10, 0),
            ref_txn_id: combo.clone(),
            note: Some("退款一".to_string()),
            total_amount_cents: 3_000,
            actual_amount_cents: None,
            settlements: vec![settle(&bank, 3_000)],
            tag_ids: vec![],
        })
        .unwrap();
    let refund2 = fx
        .core
        .create_refund(CreateRefundInput {
            occurred_at: at(2026, 10, 6, 10, 0),
            ref_txn_id: combo.clone(),
            note: Some("退款二".to_string()),
            total_amount_cents: 2_000,
            actual_amount_cents: None,
            settlements: vec![settle(&wallet, 2_000)],
            tag_ids: vec![],
        })
        .unwrap();

    // 4. 报销
    let reimbursement = fx
        .core
        .create_reimbursement(CreateReimbursementInput {
            occurred_at: at(2026, 10, 7, 10, 0),
            ref_txn_id: foreign.clone(),
            note: Some("差旅报销".to_string()),
            total_amount_cents: 30_000,
            actual_amount_cents: Some(200_000),
            settlements: vec![settle(&source, 200_000)],
            tag_ids: vec![],
        })
        .unwrap();

    // 5. 贷款放款 + 两次还款（含利息拆分）
    let loan = fx
        .core
        .create_loan(CreateLoanInput {
            lender: "招商银行".to_string(),
            principal_cents: 200_000,
            currency: None,
            interest_rate_bp: 450,
            start_at: at(2026, 10, 1, 0, 0),
            term_months: Some(12),
            account_id: Some(bank.clone()),
            note: None,
        })
        .unwrap();
    fx.core
        .create_loan_disbursement(CreateLoanDisbursementInput {
            occurred_at: at(2026, 10, 8, 10, 0),
            loan_id: loan.clone(),
            amount_cents: 200_000,
            account_id: bank.clone(),
            note: None,
            tag_ids: vec![],
        })
        .unwrap();
    fx.core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 15, 10, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 21_000,
            principal_part_cents: 20_000,
            interest_part_cents: 1_000,
            account_id: bank.clone(),
            category_id: Some(travel.clone()),
            note: None,
            tag_ids: vec![],
        })
        .unwrap();
    fx.core
        .create_loan_repayment(CreateLoanRepaymentInput {
            occurred_at: at(2026, 10, 25, 10, 0),
            loan_id: loan.clone(),
            actual_amount_cents: 20_500,
            principal_part_cents: 20_000,
            interest_part_cents: 500,
            account_id: bank.clone(),
            category_id: Some(travel.clone()),
            note: None,
            tag_ids: vec![],
        })
        .unwrap();

    // 6. 作废一条退款
    fx.core
        .void_transaction(refund2.clone(), "退款二录错".to_string())
        .unwrap();

    // 7. 再尝试作废原消费，应被拒绝
    assert_eq!(
        fx.core
            .void_transaction(combo.clone(), "原消费录错".to_string())
            .unwrap_err(),
        CoreError::HasActiveDependents
    );

    // 8. 净资产与预算
    let networth = fx
        .core
        .net_worth_series(
            at(2026, 10, 1, 0, 0),
            at(2026, 11, 1, 0, 0),
            Granularity::Monthly,
        )
        .unwrap();
    assert_eq!(networth.len(), 1);
    let cny = networth[0]
        .by_currency
        .iter()
        .find(|item| item.currency == "CNY")
        .unwrap();
    assert_eq!(
        cny.net_worth_cents,
        cny.total_assets_cents - cny.total_liabilities_cents
    );
    assert_eq!(cny.total_liabilities_cents, 160_000);

    let budget = fx
        .core
        .create_budget(CreateBudgetInput {
            name: "餐饮预算".to_string(),
            scope: BudgetScope::CategoryL1,
            category_id: Some(food),
            period_type: PeriodType::Monthly,
            amount_cents: 10_000,
            currency: None,
            start_at: None,
            end_at: None,
            rollover: false,
        })
        .unwrap();
    let usage = fx
        .core
        .budget_usage(&budget, at(2026, 10, 28, 0, 0))
        .unwrap();
    assert_eq!(usage.used_cents, 9_000);

    let detail = fx.core.get_transaction(refund1).unwrap();
    assert_eq!(detail.status, TxnStatus::Confirmed);
    assert_eq!(detail.ref_txn_id, Some(combo));
    let detail = fx.core.get_transaction(reimbursement).unwrap();
    assert_eq!(detail.currency, "USD");
    assert_eq!(detail.actual_amount_cents, 200_000);
}

#[test]
fn checkpoint_and_reopen_preserve_data() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("tallytrail.db");
    {
        let core = CoreHandle::open(&path).unwrap();
        let account = account(&core, "卡", AccountType::Bank, "CNY", 100);
        assert!(!account.is_empty());
        core.checkpoint().unwrap();
    }
    let core = CoreHandle::open(&path).unwrap();
    let accounts = core.list_accounts(true).unwrap();
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].initial_balance_cents, 100);
    core.checkpoint().unwrap();
}
