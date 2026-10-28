use std::collections::BTreeMap;

use chrono::Utc;
use tallytrail_core::{
    AccountType, BudgetScope, CategoryKind, CreateAccountInput, CreateBudgetInput,
    CreateCategoryInput, CreateExpenseInput, CreateIncomeInput, CreateLoanDisbursementInput,
    CreateLoanInput, CreateLoanRepaymentInput, CreateRefundInput, CreateReimbursementInput,
    CreateTransferInput, Granularity, PeriodType, SettlementInput, TransactionFilter,
    TransactionView, TxnStatus, TxnType,
};

use crate::util::{
    format_at, format_cents, format_date, parse_as_of, parse_cents, parse_from, parse_input_time,
    parse_to, print_table, CliResult,
};
use crate::{
    AccountCommand, AccountTypeArg, AddDisbursementArgs, AddExpenseArgs, AddIncomeArgs,
    AddLoanArgs, AddRefundArgs, AddReimbursementArgs, AddRepaymentArgs, AddTransferArgs,
    BudgetCommand, CategoryCommand, Command, GranularityArg, KindArg, ListArgs, LoanCommand,
    PeriodArg, ScopeArg, StatusArg, TagCommand, TxnTypeArg,
};

pub fn dispatch(core: &tallytrail_core::CoreHandle, command: Command) -> CliResult<()> {
    match command {
        Command::Account { command } => account(core, command),
        Command::Category { command } => category(core, command),
        Command::Tag { command } => tag(core, command),
        Command::AddExpense(args) => add_expense(core, args),
        Command::AddIncome(args) => add_income(core, args),
        Command::AddTransfer(args) => add_transfer(core, args),
        Command::AddRefund(args) => add_refund(core, args),
        Command::AddReimbursement(args) => add_reimbursement(core, args),
        Command::AddLoan(args) => add_loan(core, args),
        Command::AddDisbursement(args) => add_disbursement(core, args),
        Command::AddRepayment(args) => add_repayment(core, args),
        Command::Void { id, reason } => {
            core.void_transaction(id.clone(), reason)?;
            println!("已作废交易: {id}");
            Ok(())
        }
        Command::List(args) => list(core, args),
        Command::Show { id } => show(core, &id),
        Command::Balance { as_of } => balance(core, as_of),
        Command::Summary { from, to } => summary(core, &from, &to),
        Command::Spending {
            category,
            from,
            to,
            granularity,
        } => spending(core, &category, &from, &to, granularity),
        Command::Networth {
            from,
            to,
            granularity,
        } => networth(core, &from, &to, granularity),
        Command::Budget { command } => budget(core, command),
        Command::Loan { command } => loan(core, command),
        Command::Checkpoint => {
            core.checkpoint()?;
            println!("WAL checkpoint 完成。现在可以安全拷贝 .db 文件。");
            Ok(())
        }
    }
}

fn account(core: &tallytrail_core::CoreHandle, command: AccountCommand) -> CliResult<()> {
    match command {
        AccountCommand::Add {
            name,
            account_type,
            currency,
            initial,
        } => {
            let id = core.create_account(CreateAccountInput {
                name,
                account_type: account_type.into(),
                currency,
                initial_balance_cents: parse_cents(&initial)?,
            })?;
            println!("账户已创建: {id}");
            Ok(())
        }
        AccountCommand::List { all } => {
            let accounts = core.list_accounts(all)?;
            let rows = accounts
                .iter()
                .map(|account| {
                    vec![
                        account.id.clone(),
                        account.name.clone(),
                        account.account_type.to_string(),
                        account.currency.clone(),
                        format_cents(account.initial_balance_cents),
                        yes_no(account.is_active),
                        format_at(account.created_at),
                    ]
                })
                .collect::<Vec<_>>();
            print_table(
                &["ID", "名称", "类型", "币种", "初始余额", "启用", "创建时间"],
                &rows,
            );
            Ok(())
        }
        AccountCommand::Rename { id, name } => {
            core.rename_account(id.clone(), name)?;
            println!("账户已重命名: {id}");
            Ok(())
        }
        AccountCommand::Enable { id } => {
            core.set_account_active(id.clone(), true)?;
            println!("账户已启用: {id}");
            Ok(())
        }
        AccountCommand::Disable { id } => {
            core.set_account_active(id.clone(), false)?;
            println!("账户已停用: {id}");
            Ok(())
        }
    }
}

fn category(core: &tallytrail_core::CoreHandle, command: CategoryCommand) -> CliResult<()> {
    match command {
        CategoryCommand::Add {
            name,
            kind,
            parent,
            sort,
        } => {
            let id = core.create_category(CreateCategoryInput {
                name,
                parent_id: parent,
                kind: kind.into(),
                sort_order: sort,
            })?;
            println!("分类已创建: {id}");
            Ok(())
        }
        CategoryCommand::List { kind, all } => {
            let categories = core.list_categories(kind.map(Into::into), all)?;
            let rows = categories
                .iter()
                .map(|category| {
                    let indent = if category.level == 2 { "  └─ " } else { "" };
                    vec![
                        category.id.clone(),
                        category.level.to_string(),
                        category.kind.to_string(),
                        category
                            .parent_id
                            .clone()
                            .unwrap_or_else(|| "-".to_string()),
                        format!("{indent}{}", category.name),
                        category.sort_order.to_string(),
                        yes_no(category.is_active),
                    ]
                })
                .collect::<Vec<_>>();
            print_table(
                &["ID", "层级", "Kind", "父分类", "名称", "排序", "启用"],
                &rows,
            );
            Ok(())
        }
        CategoryCommand::Rename { id, name } => {
            core.rename_category(id.clone(), name)?;
            println!("分类已重命名: {id}");
            Ok(())
        }
        CategoryCommand::Enable { id } => {
            core.set_category_active(id.clone(), true)?;
            println!("分类已启用: {id}");
            Ok(())
        }
        CategoryCommand::Disable { id } => {
            core.set_category_active(id.clone(), false)?;
            println!("分类已停用: {id}");
            Ok(())
        }
        CategoryCommand::Delete { id } => {
            core.delete_category(id.clone())?;
            println!("分类已删除: {id}");
            Ok(())
        }
        CategoryCommand::SetKind { id, kind } => {
            core.change_category_kind(id.clone(), kind.into())?;
            println!("分类 kind 已修改: {id}");
            Ok(())
        }
    }
}

fn tag(core: &tallytrail_core::CoreHandle, command: TagCommand) -> CliResult<()> {
    match command {
        TagCommand::Add { name } => {
            let id = core.create_tag(name)?;
            println!("标签已创建: {id}");
            Ok(())
        }
        TagCommand::List => {
            let tags = core.list_tags()?;
            let rows = tags
                .iter()
                .map(|tag| vec![tag.id.clone(), tag.name.clone()])
                .collect::<Vec<_>>();
            print_table(&["ID", "名称"], &rows);
            Ok(())
        }
        TagCommand::Rename { id, name } => {
            core.rename_tag(id.clone(), name)?;
            println!("标签已重命名: {id}");
            Ok(())
        }
        TagCommand::Delete { id } => {
            core.delete_tag(id.clone())?;
            println!("标签已删除，并已清理交易关联: {id}");
            Ok(())
        }
    }
}

fn add_expense(core: &tallytrail_core::CoreHandle, args: AddExpenseArgs) -> CliResult<()> {
    let settlements = parse_settlements(&args.pay, "--pay")?;
    let id = core.create_expense(CreateExpenseInput {
        occurred_at: parse_input_time(&args.at)?.at,
        category_id: args.category,
        note: args.note,
        total_amount_cents: parse_cents(&args.amount)?,
        currency: args.currency,
        actual_amount_cents: args.actual.as_deref().map(parse_cents).transpose()?,
        settlements,
        tag_ids: args.tags,
    })?;
    println!("已创建支出: {id}");
    Ok(())
}

fn add_income(core: &tallytrail_core::CoreHandle, args: AddIncomeArgs) -> CliResult<()> {
    let settlements = parse_settlements(&args.receive, "--receive")?;
    let id = core.create_income(CreateIncomeInput {
        occurred_at: parse_input_time(&args.at)?.at,
        category_id: args.category,
        note: args.note,
        total_amount_cents: parse_cents(&args.amount)?,
        currency: args.currency,
        actual_amount_cents: args.actual.as_deref().map(parse_cents).transpose()?,
        settlements,
        tag_ids: args.tags,
    })?;
    println!("已创建收入: {id}");
    Ok(())
}

fn add_transfer(core: &tallytrail_core::CoreHandle, args: AddTransferArgs) -> CliResult<()> {
    let id = core.create_transfer(CreateTransferInput {
        occurred_at: parse_input_time(&args.at)?.at,
        from_account_id: args.from,
        to_account_id: args.to,
        amount_cents: parse_cents(&args.amount)?,
        note: args.note,
        tag_ids: args.tags,
    })?;
    println!("已创建转账: {id}");
    Ok(())
}

fn add_refund(core: &tallytrail_core::CoreHandle, args: AddRefundArgs) -> CliResult<()> {
    let settlements = parse_settlements(&args.receive, "--receive")?;
    let id = core.create_refund(CreateRefundInput {
        occurred_at: parse_input_time(&args.at)?.at,
        ref_txn_id: args.reference,
        note: args.note,
        total_amount_cents: parse_cents(&args.amount)?,
        actual_amount_cents: args.actual.as_deref().map(parse_cents).transpose()?,
        settlements,
        tag_ids: args.tags,
    })?;
    println!("已创建退款: {id}");
    Ok(())
}

fn add_reimbursement(
    core: &tallytrail_core::CoreHandle,
    args: AddReimbursementArgs,
) -> CliResult<()> {
    let settlements = parse_settlements(&args.source, "--source")?;
    let id = core.create_reimbursement(CreateReimbursementInput {
        occurred_at: parse_input_time(&args.at)?.at,
        ref_txn_id: args.reference,
        note: args.note,
        total_amount_cents: parse_cents(&args.amount)?,
        actual_amount_cents: args.actual.as_deref().map(parse_cents).transpose()?,
        settlements,
        tag_ids: args.tags,
    })?;
    println!("已创建报销: {id}");
    Ok(())
}

fn add_loan(core: &tallytrail_core::CoreHandle, args: AddLoanArgs) -> CliResult<()> {
    let start_at = match args.start.as_deref() {
        Some(value) => parse_input_time(value)?.at,
        None => Utc::now().timestamp(),
    };
    let id = core.create_loan(CreateLoanInput {
        lender: args.lender,
        principal_cents: parse_cents(&args.principal)?,
        currency: args.currency,
        interest_rate_bp: args.rate_bp,
        start_at,
        term_months: args.term_months,
        account_id: args.account,
        note: args.note,
    })?;
    println!("贷款已创建: {id}");
    Ok(())
}

fn add_disbursement(
    core: &tallytrail_core::CoreHandle,
    args: AddDisbursementArgs,
) -> CliResult<()> {
    let id = core.create_loan_disbursement(CreateLoanDisbursementInput {
        occurred_at: parse_input_time(&args.at)?.at,
        loan_id: args.loan,
        amount_cents: parse_cents(&args.amount)?,
        account_id: args.to,
        note: args.note,
        tag_ids: args.tags,
    })?;
    println!("已创建放款交易: {id}");
    Ok(())
}

fn add_repayment(core: &tallytrail_core::CoreHandle, args: AddRepaymentArgs) -> CliResult<()> {
    let id = core.create_loan_repayment(CreateLoanRepaymentInput {
        occurred_at: parse_input_time(&args.at)?.at,
        loan_id: args.loan,
        actual_amount_cents: parse_cents(&args.amount)?,
        principal_part_cents: parse_cents(&args.principal)?,
        interest_part_cents: parse_cents(&args.interest)?,
        account_id: args.from,
        category_id: args.category,
        note: args.note,
        tag_ids: args.tags,
    })?;
    println!("已创建还款交易: {id}");
    Ok(())
}

fn list(core: &tallytrail_core::CoreHandle, args: ListArgs) -> CliResult<()> {
    let from_at = args.from.as_deref().map(parse_from).transpose()?;
    let to_at = args.to.as_deref().map(parse_to).transpose()?;
    let types = if args.types.is_empty() {
        None
    } else {
        Some(args.types.iter().map(|value| (*value).into()).collect())
    };
    let tag_ids = if args.tags.is_empty() {
        None
    } else {
        Some(args.tags)
    };
    let filter = TransactionFilter {
        from_at,
        to_at,
        types,
        category_id: args.category,
        account_id: args.account,
        tag_ids,
        currency: args.currency,
        status: args.status.map(Into::into),
        limit: args.limit,
        offset: args.offset,
    };
    let transactions = core.list_transactions(filter)?;
    let rows = transactions.iter().map(transaction_row).collect::<Vec<_>>();
    print_table(
        &[
            "ID", "时间", "类型", "方向", "分类", "花费", "实际", "结算", "标签", "状态", "备注",
        ],
        &rows,
    );
    Ok(())
}

fn show(core: &tallytrail_core::CoreHandle, id: &str) -> CliResult<()> {
    let transaction = core.get_transaction(id.to_string())?;
    println!("交易 ID   : {}", transaction.id);
    println!(
        "类型/状态 : {} / {}",
        transaction.txn_type, transaction.status
    );
    println!("时间      : {}", format_at(transaction.occurred_at));
    println!("方向      : {}", transaction.direction);
    println!(
        "分类      : {}",
        transaction
            .category_name
            .clone()
            .or_else(|| transaction.category_id.clone())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "花费金额  : {} {}",
        format_cents(transaction.total_amount_cents),
        transaction.currency
    );
    println!(
        "实际金额  : {}",
        format_cents(transaction.actual_amount_cents)
    );
    if let Some(reference) = &transaction.ref_txn_id {
        println!("原交易    : {reference}");
    }
    if let Some(loan_id) = &transaction.loan_id {
        println!("贷款      : {loan_id}");
    }
    if let Some(principal) = transaction.principal_part_cents {
        println!(
            "本金/利息 : {} / {}",
            format_cents(principal),
            format_cents(transaction.interest_part_cents.unwrap_or(0))
        );
    }
    if let Some(note) = &transaction.note {
        println!("备注      : {note}");
    }
    if let Some(voided_at) = transaction.voided_at {
        println!(
            "作废      : {} ({})",
            format_at(voided_at),
            transaction.void_reason.as_deref().unwrap_or("-")
        );
    }
    println!("结算行:");
    let rows = transaction
        .settlements
        .iter()
        .map(|settlement| {
            vec![
                settlement.account_name.clone(),
                settlement.account_id.clone(),
                settlement.role.to_string(),
                format_cents(settlement.amount_cents),
                settlement.currency.clone(),
            ]
        })
        .collect::<Vec<_>>();
    print_table(&["账户", "账户 ID", "角色", "金额", "币种"], &rows);
    if !transaction.tags.is_empty() {
        println!(
            "标签      : {}",
            transaction
                .tags
                .iter()
                .map(|tag| format!("{} ({})", tag.name, tag.id))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Ok(())
}

fn balance(core: &tallytrail_core::CoreHandle, as_of: Option<String>) -> CliResult<()> {
    let as_of_at = match as_of.as_deref() {
        Some(value) => parse_as_of(value)?,
        None => Utc::now().timestamp(),
    };
    let balances = core.list_account_balances(as_of_at)?;
    let rows = balances
        .iter()
        .map(|balance| {
            vec![
                balance.account_id.clone(),
                balance.name.clone(),
                balance.account_type.to_string(),
                balance.currency.clone(),
                format_cents(balance.balance_cents),
            ]
        })
        .collect::<Vec<_>>();
    print_table(&["账户 ID", "名称", "类型", "币种", "余额"], &rows);

    let mut totals: BTreeMap<String, i64> = BTreeMap::new();
    for balance in &balances {
        let entry = totals.entry(balance.currency.clone()).or_insert(0);
        *entry = entry
            .checked_add(balance.balance_cents)
            .ok_or("余额汇总溢出")?;
    }
    if !totals.is_empty() {
        println!();
        println!(
            "资产合计（按币种，不含贷款负债）: {}",
            totals
                .iter()
                .map(|(currency, cents)| format!("{currency} {}", format_cents(*cents)))
                .collect::<Vec<_>>()
                .join(" | ")
        );
    }
    Ok(())
}

fn summary(core: &tallytrail_core::CoreHandle, from: &str, to: &str) -> CliResult<()> {
    let summary = core.income_expense_summary(parse_from(from)?, parse_to(to)?)?;
    let rows = summary
        .by_currency
        .iter()
        .map(|item| {
            let net = item.net_expense_cents()?;
            Ok(vec![
                item.currency.clone(),
                format_cents(item.total_income_cents),
                format_cents(item.total_expense_cents),
                format_cents(item.total_refund_cents),
                format_cents(item.total_reimbursement_cents),
                format_cents(net),
            ])
        })
        .collect::<CliResult<Vec<_>>>()?;
    print_table(
        &["币种", "收入", "支出(毛)", "退款", "报销", "净支出"],
        &rows,
    );
    Ok(())
}

fn spending(
    core: &tallytrail_core::CoreHandle,
    category: &str,
    from: &str,
    to: &str,
    granularity: GranularityArg,
) -> CliResult<()> {
    let points = core.category_spending_series(
        category.to_string(),
        parse_from(from)?,
        parse_to(to)?,
        granularity.into(),
    )?;
    let rows = points
        .iter()
        .map(|point| {
            vec![
                format!(
                    "{} ~ {}",
                    format_date(point.period_start_at),
                    format_date(point.period_end_at)
                ),
                point.currency.clone(),
                format_cents(point.amount_cents),
            ]
        })
        .collect::<Vec<_>>();
    print_table(&["周期（左闭右开）", "币种", "支出净额"], &rows);
    Ok(())
}

fn networth(
    core: &tallytrail_core::CoreHandle,
    from: &str,
    to: &str,
    granularity: GranularityArg,
) -> CliResult<()> {
    let points = core.net_worth_series(parse_from(from)?, parse_to(to)?, granularity.into())?;
    let mut rows = Vec::new();
    for point in points {
        for item in point.by_currency {
            rows.push(vec![
                format_date(point.at),
                item.currency,
                format_cents(item.total_assets_cents),
                format_cents(item.total_liabilities_cents),
                format_cents(item.net_worth_cents),
            ]);
        }
    }
    print_table(
        &["周期起点", "币种", "总资产", "贷款未还本金", "净资产"],
        &rows,
    );
    Ok(())
}

fn budget(core: &tallytrail_core::CoreHandle, command: BudgetCommand) -> CliResult<()> {
    match command {
        BudgetCommand::Add {
            name,
            scope,
            category,
            period,
            amount,
            currency,
            start,
            end,
        } => {
            let start_at = start.as_deref().map(parse_from).transpose()?;
            let end_at = end.as_deref().map(parse_to).transpose()?;
            let id = core.create_budget(CreateBudgetInput {
                name,
                scope: scope.into(),
                category_id: category,
                period_type: period.into(),
                amount_cents: parse_cents(&amount)?,
                currency,
                start_at,
                end_at,
                rollover: false,
            })?;
            println!("预算已创建: {id}");
            Ok(())
        }
        BudgetCommand::List { as_of, all } => {
            let as_of_at = match as_of.as_deref() {
                Some(value) => parse_as_of(value)?,
                None => Utc::now().timestamp(),
            };
            if all {
                let budgets = core.list_budgets(true)?;
                let rows = budgets
                    .iter()
                    .map(|budget| {
                        vec![
                            budget.id.clone(),
                            budget.name.clone(),
                            budget.scope.to_string(),
                            budget
                                .category_id
                                .clone()
                                .unwrap_or_else(|| "-".to_string()),
                            budget.period_type.to_string(),
                            format!("{} {}", format_cents(budget.amount_cents), budget.currency),
                            budget
                                .start_at
                                .map(format_at)
                                .unwrap_or_else(|| "-".to_string()),
                            budget
                                .end_at
                                .map(format_at)
                                .unwrap_or_else(|| "-".to_string()),
                            yes_no(budget.is_active),
                        ]
                    })
                    .collect::<Vec<_>>();
                print_table(
                    &[
                        "ID", "名称", "Scope", "分类", "周期", "金额", "开始", "结束", "启用",
                    ],
                    &rows,
                );
            } else {
                let usages = core.list_budget_usage(as_of_at)?;
                let rows = usages
                    .iter()
                    .map(|usage| {
                        vec![
                            usage.budget_id.clone(),
                            usage.name.clone(),
                            usage.scope.to_string(),
                            usage.category_id.clone().unwrap_or_else(|| "-".to_string()),
                            format!("{} {}", format_cents(usage.amount_cents), usage.currency),
                            format_cents(usage.used_cents),
                            format_cents(usage.remaining_cents),
                            format!(
                                "{} ~ {}",
                                format_date(usage.period_start_at),
                                format_date(usage.period_end_at)
                            ),
                        ]
                    })
                    .collect::<Vec<_>>();
                print_table(
                    &[
                        "ID", "名称", "Scope", "分类", "预算", "已用", "剩余", "周期",
                    ],
                    &rows,
                );
            }
            Ok(())
        }
        BudgetCommand::Show { id, as_of } => {
            let as_of_at = match as_of.as_deref() {
                Some(value) => parse_as_of(value)?,
                None => Utc::now().timestamp(),
            };
            let usage = core.budget_usage(&id, as_of_at)?;
            println!("预算 ID   : {}", usage.budget_id);
            println!("名称      : {}", usage.name);
            println!("Scope     : {}", usage.scope);
            println!(
                "分类      : {}",
                usage.category_id.as_deref().unwrap_or("-")
            );
            println!(
                "周期      : {} ~ {}（左闭右开，UTC 周期）",
                format_date(usage.period_start_at),
                format_date(usage.period_end_at)
            );
            println!(
                "预算/已用 : {} / {} {}",
                format_cents(usage.amount_cents),
                format_cents(usage.used_cents),
                usage.currency
            );
            println!("剩余      : {}", format_cents(usage.remaining_cents));
            Ok(())
        }
        BudgetCommand::Enable { id } => {
            core.set_budget_active(id.clone(), true)?;
            println!("预算已启用: {id}");
            Ok(())
        }
        BudgetCommand::Disable { id } => {
            core.set_budget_active(id.clone(), false)?;
            println!("预算已停用: {id}");
            Ok(())
        }
        BudgetCommand::Delete { id } => {
            core.delete_budget(id.clone())?;
            println!("预算已删除: {id}");
            Ok(())
        }
    }
}

fn loan(core: &tallytrail_core::CoreHandle, command: LoanCommand) -> CliResult<()> {
    match command {
        LoanCommand::List => {
            let loans = core.list_loans()?;
            let rows = loans
                .iter()
                .map(|loan| {
                    vec![
                        loan.id.clone(),
                        loan.lender.clone(),
                        format!("{} {}", format_cents(loan.principal_cents), loan.currency),
                        format_rate(loan.interest_rate_bp),
                        format_at(loan.start_at),
                        loan.term_months
                            .map(|months| months.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        format_cents(loan.outstanding_cents),
                        loan.status.as_db_str().to_string(),
                    ]
                })
                .collect::<Vec<_>>();
            print_table(
                &[
                    "ID",
                    "贷款方",
                    "本金",
                    "年化",
                    "起始",
                    "期限(月)",
                    "未还本金",
                    "状态",
                ],
                &rows,
            );
            Ok(())
        }
        LoanCommand::Show { id } => {
            let balance = core.loan_outstanding(&id)?;
            println!("贷款 ID     : {}", balance.loan_id);
            println!("贷款方      : {}", balance.lender);
            println!(
                "本金/已还   : {} / {} {}",
                format_cents(balance.principal_cents),
                format_cents(balance.repaid_principal_cents),
                balance.currency
            );
            println!("未还本金    : {}", format_cents(balance.outstanding_cents));
            println!("状态        : {}", balance.status.as_db_str());
            Ok(())
        }
    }
}

fn transaction_row(transaction: &TransactionView) -> Vec<String> {
    let settlement_currency = transaction
        .settlements
        .first()
        .map(|settlement| settlement.currency.as_str())
        .unwrap_or(transaction.currency.as_str());
    let actual = if transaction.actual_amount_cents != transaction.total_amount_cents
        || settlement_currency != transaction.currency
    {
        format!(
            "{} {}",
            format_cents(transaction.actual_amount_cents),
            settlement_currency
        )
    } else {
        "-".to_string()
    };
    let settlements = transaction
        .settlements
        .iter()
        .map(|settlement| {
            format!(
                "{}:{} {}",
                settlement.account_name,
                format_cents(settlement.amount_cents),
                settlement.currency
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let tags = transaction
        .tags
        .iter()
        .map(|tag| tag.name.clone())
        .collect::<Vec<_>>()
        .join(",");
    vec![
        transaction.id.clone(),
        format_at(transaction.occurred_at),
        transaction.txn_type.to_string(),
        transaction.direction.to_string(),
        transaction
            .category_name
            .clone()
            .or_else(|| transaction.category_id.clone())
            .unwrap_or_else(|| "-".to_string()),
        format!(
            "{} {}",
            format_cents(transaction.total_amount_cents),
            transaction.currency
        ),
        actual,
        settlements,
        tags,
        transaction.status.to_string(),
        transaction.note.clone().unwrap_or_default(),
    ]
}

fn parse_settlements(values: &[String], flag: &str) -> CliResult<Vec<SettlementInput>> {
    if values.is_empty() {
        return Err(format!("至少需要一个 {flag}，格式为 account_id=金额").into());
    }
    values
        .iter()
        .map(|value| {
            let (account_id, amount) = value
                .split_once('=')
                .ok_or_else(|| format!("{flag} 参数格式必须为 account_id=金额: {value}"))?;
            if account_id.trim().is_empty() {
                return Err(format!("{flag} 的账户 ID 不能为空").into());
            }
            Ok(SettlementInput {
                account_id: account_id.trim().to_string(),
                amount_cents: parse_cents(amount)?,
            })
        })
        .collect()
}

fn yes_no(value: bool) -> String {
    if value { "是" } else { "否" }.to_string()
}

fn format_rate(basis_points: i32) -> String {
    format!("{}.{:02}%", basis_points / 100, basis_points.abs() % 100)
}

impl From<AccountTypeArg> for AccountType {
    fn from(value: AccountTypeArg) -> Self {
        match value {
            AccountTypeArg::Bank => AccountType::Bank,
            AccountTypeArg::Cash => AccountType::Cash,
            AccountTypeArg::EWallet => AccountType::EWallet,
            AccountTypeArg::Credit => AccountType::Credit,
            AccountTypeArg::ReimburseSource => AccountType::ReimburseSource,
            AccountTypeArg::Other => AccountType::Other,
        }
    }
}

impl From<KindArg> for CategoryKind {
    fn from(value: KindArg) -> Self {
        match value {
            KindArg::Expense => CategoryKind::Expense,
            KindArg::Income => CategoryKind::Income,
        }
    }
}

impl From<TxnTypeArg> for TxnType {
    fn from(value: TxnTypeArg) -> Self {
        match value {
            TxnTypeArg::Expense => TxnType::Expense,
            TxnTypeArg::Income => TxnType::Income,
            TxnTypeArg::Transfer => TxnType::Transfer,
            TxnTypeArg::Refund => TxnType::Refund,
            TxnTypeArg::Reimbursement => TxnType::Reimbursement,
            TxnTypeArg::LoanDisburse => TxnType::LoanDisburse,
            TxnTypeArg::LoanRepay => TxnType::LoanRepay,
        }
    }
}

impl From<StatusArg> for TxnStatus {
    fn from(value: StatusArg) -> Self {
        match value {
            StatusArg::Confirmed => TxnStatus::Confirmed,
            StatusArg::Voided => TxnStatus::Voided,
        }
    }
}

impl From<GranularityArg> for Granularity {
    fn from(value: GranularityArg) -> Self {
        match value {
            GranularityArg::Daily => Granularity::Daily,
            GranularityArg::Weekly => Granularity::Weekly,
            GranularityArg::Monthly => Granularity::Monthly,
        }
    }
}

impl From<ScopeArg> for BudgetScope {
    fn from(value: ScopeArg) -> Self {
        match value {
            ScopeArg::Total => BudgetScope::Total,
            ScopeArg::CategoryL1 => BudgetScope::CategoryL1,
            ScopeArg::CategoryL2 => BudgetScope::CategoryL2,
        }
    }
}

impl From<PeriodArg> for PeriodType {
    fn from(value: PeriodArg) -> Self {
        match value {
            PeriodArg::Monthly => PeriodType::Monthly,
            PeriodArg::Yearly => PeriodType::Yearly,
            PeriodArg::Custom => PeriodType::Custom,
        }
    }
}
