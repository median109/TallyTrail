use std::fmt;
use std::str::FromStr;

use crate::domain::error::{CoreError, CoreResult};

pub type AccountId = String;
pub type CategoryId = String;
pub type TagId = String;
pub type TransactionId = String;
pub type LoanId = String;
pub type BudgetId = String;

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $db:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub fn as_db_str(self) -> &'static str {
                match self {
                    $($name::$variant => $db),+
                }
            }

            pub fn parse(value: &str) -> CoreResult<Self> {
                let normalized = value.trim().to_ascii_uppercase();
                match normalized.as_str() {
                    $($db => Ok($name::$variant),)+
                    _ => Err(CoreError::validation(format!(
                        "无法识别的 {} 值: {}",
                        stringify!($name),
                        value
                    ))),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_db_str())
            }
        }

        impl FromStr for $name {
            type Err = CoreError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                Self::parse(s)
            }
        }
    };
}

string_enum!(AccountType {
    Bank => "BANK",
    Cash => "CASH",
    EWallet => "E_WALLET",
    Credit => "CREDIT",
    ReimburseSource => "REIMBURSE_SOURCE",
    Other => "OTHER",
});

string_enum!(CategoryKind {
    Expense => "EXPENSE",
    Income => "INCOME",
});

string_enum!(TxnType {
    Expense => "EXPENSE",
    Income => "INCOME",
    Transfer => "TRANSFER",
    Refund => "REFUND",
    Reimbursement => "REIMBURSEMENT",
    LoanDisburse => "LOAN_DISBURSE",
    LoanRepay => "LOAN_REPAY",
});

string_enum!(TxnStatus {
    Confirmed => "CONFIRMED",
    Voided => "VOIDED",
});

string_enum!(Direction {
    Inflow => "INFLOW",
    Outflow => "OUTFLOW",
});

string_enum!(SplitRole {
    Payment => "PAYMENT",
    Receipt => "RECEIPT",
    ReimburseSource => "REIMBURSE_SOURCE",
});

string_enum!(BudgetScope {
    Total => "TOTAL",
    CategoryL1 => "CATEGORY_L1",
    CategoryL2 => "CATEGORY_L2",
});

string_enum!(PeriodType {
    Monthly => "MONTHLY",
    Yearly => "YEARLY",
    Custom => "CUSTOM",
});

string_enum!(Granularity {
    Daily => "DAILY",
    Weekly => "WEEKLY",
    Monthly => "MONTHLY",
});

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CategoryView {
    pub id: CategoryId,
    pub parent_id: Option<CategoryId>,
    pub name: String,
    pub level: i32,
    pub kind: CategoryKind,
    pub sort_order: i32,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagView {
    pub id: TagId,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountView {
    pub id: AccountId,
    pub name: String,
    pub account_type: AccountType,
    pub currency: String,
    pub initial_balance_cents: i64,
    pub is_active: bool,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementView {
    pub id: String,
    pub account_id: AccountId,
    pub account_name: String,
    pub currency: String,
    pub amount_cents: i64,
    pub role: SplitRole,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionView {
    pub id: TransactionId,
    pub txn_type: TxnType,
    pub occurred_at: i64,
    pub category_id: Option<CategoryId>,
    pub category_name: Option<String>,
    pub note: Option<String>,
    pub total_amount_cents: i64,
    pub currency: String,
    pub actual_amount_cents: i64,
    pub direction: Direction,
    pub ref_txn_id: Option<TransactionId>,
    pub loan_id: Option<LoanId>,
    pub principal_part_cents: Option<i64>,
    pub interest_part_cents: Option<i64>,
    pub status: TxnStatus,
    pub voided_at: Option<i64>,
    pub void_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub settlements: Vec<SettlementView>,
    pub tags: Vec<TagView>,
}

pub type TransactionDetail = TransactionView;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountBalance {
    pub account_id: AccountId,
    pub name: String,
    pub account_type: AccountType,
    pub currency: String,
    pub balance_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoanView {
    pub id: LoanId,
    pub lender: String,
    pub principal_cents: i64,
    pub currency: String,
    pub interest_rate_bp: i32,
    pub start_at: i64,
    pub term_months: Option<i32>,
    pub account_id: Option<AccountId>,
    pub status: LoanStatus,
    pub note: Option<String>,
    pub repaid_principal_cents: i64,
    pub outstanding_cents: i64,
}

/// 贷款状态在库内仅作缓存，对外展示以 `loan_outstanding` 的动态推导值为准。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoanStatus {
    Active,
    Settled,
}

impl LoanStatus {
    pub fn as_db_str(self) -> &'static str {
        match self {
            LoanStatus::Active => "ACTIVE",
            LoanStatus::Settled => "SETTLED",
        }
    }

    pub fn parse(value: &str) -> CoreResult<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "ACTIVE" => Ok(LoanStatus::Active),
            "SETTLED" => Ok(LoanStatus::Settled),
            other => Err(CoreError::validation(format!(
                "无法识别的贷款状态: {other}"
            ))),
        }
    }
}

impl std::str::FromStr for LoanStatus {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoanBalance {
    pub loan_id: LoanId,
    pub lender: String,
    pub currency: String,
    pub principal_cents: i64,
    pub repaid_principal_cents: i64,
    pub outstanding_cents: i64,
    pub status: LoanStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencySummary {
    pub currency: String,
    pub total_income_cents: i64,
    pub total_expense_cents: i64,
    pub total_refund_cents: i64,
    pub total_reimbursement_cents: i64,
}

impl CurrencySummary {
    /// 净支出 = 支出 − 退款 − 报销。仅为展示便利，Core 明细口径仍分别返回。
    pub fn net_expense_cents(&self) -> CoreResult<i64> {
        self.total_expense_cents
            .checked_sub(self.total_refund_cents)
            .and_then(|v| v.checked_sub(self.total_reimbursement_cents))
            .ok_or(CoreError::AmountOverflow)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    pub by_currency: Vec<CurrencySummary>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpendingPoint {
    pub period_start_at: i64,
    pub period_end_at: i64,
    pub currency: String,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrencyNetWorth {
    pub currency: String,
    pub total_assets_cents: i64,
    pub total_liabilities_cents: i64,
    pub net_worth_cents: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetWorthPoint {
    /// 桶的起始时间标签（UTC 秒）。余额截至该桶结束前一刻。
    pub at: i64,
    pub by_currency: Vec<CurrencyNetWorth>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetView {
    pub id: BudgetId,
    pub name: String,
    pub scope: BudgetScope,
    pub category_id: Option<CategoryId>,
    pub period_type: PeriodType,
    pub amount_cents: i64,
    pub currency: String,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub rollover: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BudgetUsage {
    pub budget_id: BudgetId,
    pub name: String,
    pub scope: BudgetScope,
    pub category_id: Option<CategoryId>,
    pub currency: String,
    pub amount_cents: i64,
    pub used_cents: i64,
    pub remaining_cents: i64,
    pub period_start_at: i64,
    pub period_end_at: i64,
    pub is_active: bool,
}

// ---------------------------------------------------------------------------
// 写入输入
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CreateCategoryInput {
    pub name: String,
    pub parent_id: Option<CategoryId>,
    pub kind: CategoryKind,
    pub sort_order: i32,
}

#[derive(Debug, Clone)]
pub struct CreateAccountInput {
    pub name: String,
    pub account_type: AccountType,
    pub currency: Option<String>,
    pub initial_balance_cents: i64,
}

#[derive(Debug, Clone)]
pub struct SettlementInput {
    pub account_id: AccountId,
    pub amount_cents: i64,
}

#[derive(Debug, Clone, Default)]
pub struct CreateExpenseInput {
    pub occurred_at: i64,
    pub category_id: CategoryId,
    pub note: Option<String>,
    pub total_amount_cents: i64,
    pub currency: Option<String>,
    pub actual_amount_cents: Option<i64>,
    pub settlements: Vec<SettlementInput>,
    pub tag_ids: Vec<TagId>,
}

#[derive(Debug, Clone, Default)]
pub struct CreateIncomeInput {
    pub occurred_at: i64,
    pub category_id: CategoryId,
    pub note: Option<String>,
    pub total_amount_cents: i64,
    pub currency: Option<String>,
    pub actual_amount_cents: Option<i64>,
    pub settlements: Vec<SettlementInput>,
    pub tag_ids: Vec<TagId>,
}

#[derive(Debug, Clone)]
pub struct CreateTransferInput {
    pub occurred_at: i64,
    pub from_account_id: AccountId,
    pub to_account_id: AccountId,
    pub amount_cents: i64,
    pub note: Option<String>,
    pub tag_ids: Vec<TagId>,
}

#[derive(Debug, Clone, Default)]
pub struct CreateRefundInput {
    pub occurred_at: i64,
    pub ref_txn_id: TransactionId,
    pub note: Option<String>,
    pub total_amount_cents: i64,
    pub actual_amount_cents: Option<i64>,
    pub settlements: Vec<SettlementInput>,
    pub tag_ids: Vec<TagId>,
}

#[derive(Debug, Clone, Default)]
pub struct CreateReimbursementInput {
    pub occurred_at: i64,
    pub ref_txn_id: TransactionId,
    pub note: Option<String>,
    pub total_amount_cents: i64,
    pub actual_amount_cents: Option<i64>,
    pub settlements: Vec<SettlementInput>,
    pub tag_ids: Vec<TagId>,
}

#[derive(Debug, Clone)]
pub struct CreateLoanInput {
    pub lender: String,
    pub principal_cents: i64,
    pub currency: Option<String>,
    pub interest_rate_bp: i32,
    pub start_at: i64,
    pub term_months: Option<i32>,
    pub account_id: Option<AccountId>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CreateLoanDisbursementInput {
    pub occurred_at: i64,
    pub loan_id: LoanId,
    pub amount_cents: i64,
    pub account_id: AccountId,
    pub note: Option<String>,
    pub tag_ids: Vec<TagId>,
}

#[derive(Debug, Clone)]
pub struct CreateLoanRepaymentInput {
    pub occurred_at: i64,
    pub loan_id: LoanId,
    pub actual_amount_cents: i64,
    pub principal_part_cents: i64,
    pub interest_part_cents: i64,
    pub account_id: AccountId,
    pub category_id: Option<CategoryId>,
    pub note: Option<String>,
    pub tag_ids: Vec<TagId>,
}

#[derive(Debug, Clone)]
pub struct CreateBudgetInput {
    pub name: String,
    pub scope: BudgetScope,
    pub category_id: Option<CategoryId>,
    pub period_type: PeriodType,
    pub amount_cents: i64,
    pub currency: Option<String>,
    pub start_at: Option<i64>,
    pub end_at: Option<i64>,
    pub rollover: bool,
}

#[derive(Debug, Clone, Default)]
pub struct TransactionFilter {
    pub from_at: Option<i64>,
    pub to_at: Option<i64>,
    pub types: Option<Vec<TxnType>>,
    pub category_id: Option<CategoryId>,
    pub account_id: Option<AccountId>,
    pub tag_ids: Option<Vec<TagId>>,
    pub currency: Option<String>,
    pub status: Option<TxnStatus>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeBucket {
    pub start_at: i64,
    pub end_at: i64,
}

impl TimeBucket {
    pub fn contains(self, at: i64) -> bool {
        at >= self.start_at && at < self.end_at
    }
}

// string_enum! 不支持在变体上加 #[default]，因此保留手写实现。
#[allow(clippy::derivable_impls)]
impl Default for Granularity {
    fn default() -> Self {
        Granularity::Monthly
    }
}
